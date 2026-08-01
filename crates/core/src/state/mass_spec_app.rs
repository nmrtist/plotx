use super::*;
use crate::actions::Action;
use plotx_io::AcquisitionStreamId;

const LCMS_CHROMATOGRAM_GESTURES: &[PlotInteractionGesture] = &[
    PlotInteractionGesture::Cursor,
    PlotInteractionGesture::Range,
];

impl PlotxApp {
    /// Returns the explicit semantic boundary for a plot that currently accepts
    /// LC–MS chromatogram cursor/range input.  The descriptor itself is domain
    /// neutral; the field's owner decides whether it exposes one.
    pub fn plot_interaction_descriptor(
        &self,
        canvas_index: usize,
        object: ObjectId,
    ) -> Option<PlotInteractionDescriptor> {
        let canvas = self.doc.canvases.get(canvas_index)?;
        let plot = canvas.object(object)?.plot()?;
        let source = plot.binding.series.first()?.source;
        let dataset = self.doc.dataset_by_id(source.resource)?;
        let mass_spec = dataset.as_mass_spec()?;
        mass_spec.chromatogram_stream_for_field(source.field)?;
        let field = dataset.field_descriptor(source.field)?;
        let unit = field.units.first()?.clone();
        // A selection describes the whole overlaid plot.  Do not silently let
        // its first trace stand in for an incompatible secondary trace.
        if plot.binding.series.iter().any(|series| {
            series.source.resource != source.resource
                || self
                    .doc
                    .dataset_by_id(series.source.resource)
                    .and_then(Dataset::as_mass_spec)
                    .and_then(|dataset| dataset.chromatogram_stream_for_field(series.source.field))
                    .is_none()
                || self
                    .doc
                    .dataset_by_id(series.source.resource)
                    .and_then(|dataset| dataset.field_descriptor(series.source.field))
                    .and_then(|field| field.units.into_iter().next())
                    .as_deref()
                    != Some(unit.as_str())
        }) {
            return None;
        }
        let axis = &plot.figure().x;
        (axis.categories.is_none() && axis.min.is_finite() && axis.max.is_finite()).then_some(
            PlotInteractionDescriptor {
                dataset: source.resource,
                canvas: canvas.resource_id,
                object,
                field: source.field,
                axis: PlotInteractionAxis::X,
                gestures: LCMS_CHROMATOGRAM_GESTURES,
                unit,
            },
        )
    }

    /// Mass-spectrometry plots reserve range input for their declared semantic
    /// interaction.  Other plots retain the legacy analysis-selection path.
    pub fn plot_rejects_legacy_selection(&self, canvas: usize, object: ObjectId) -> bool {
        self.doc
            .canvases
            .get(canvas)
            .and_then(|canvas| canvas.object(object))
            .and_then(|object| object.plot())
            .and_then(|plot| plot.binding.primary_dataset())
            .and_then(|id| self.doc.dataset_by_id(id))
            .is_some_and(|dataset| dataset.as_mass_spec().is_some())
    }

    /// Route presentation intent to the bound domain.  Stale transient input is
    /// deliberately dropped; no fallback lookup is attempted.
    pub fn dispatch_plot_interaction(&mut self, request: PlotInteractionRequest) -> bool {
        let target = match &request {
            PlotInteractionRequest::Cursor { target, .. }
            | PlotInteractionRequest::Range { target, .. } => target,
        };
        let Some(canvas_index) = self
            .doc
            .canvases
            .iter()
            .position(|canvas| canvas.resource_id == target.canvas)
        else {
            return false;
        };
        let Some(current) = self.plot_interaction_descriptor(canvas_index, target.object) else {
            return false;
        };
        if current != *target {
            return false;
        }
        let Some(dataset_index) = self.doc.dataset_index(target.dataset) else {
            return false;
        };
        let Some(dataset) = self
            .doc
            .datasets
            .get(dataset_index)
            .and_then(Dataset::as_mass_spec)
        else {
            return false;
        };
        let Some(stream) = dataset.chromatogram_stream_for_field(target.field) else {
            return false;
        };
        let stream_label = stream_display_label_for_id(&dataset.run, stream);
        match request {
            PlotInteractionRequest::Cursor { value, .. } => {
                let selected = self.select_mass_spec_spectrum_near(target.dataset, stream, value);
                if selected {
                    self.session.status =
                        format!("Selected the nearest scan in {stream_label} at {value:.3} min.");
                }
                selected
            }
            PlotInteractionRequest::Range { range, .. } => {
                if target.unit != "min" || !range.is_valid() {
                    return false;
                }
                self.session.ui.analysis_selection = Some(AnalysisSelection {
                    dataset: target.dataset,
                    canvas: target.canvas,
                    object: target.object,
                    x_range: range,
                    y_range: None,
                });
                self.session.status = format!("Selected {:.3}-{:.3} min.", range.min, range.max);
                true
            }
        }
    }
    pub fn pin_mass_spectrum_extraction(
        &mut self,
        dataset_id: DatasetId,
        start_time_min: f64,
        end_time_min: f64,
        method: MassSpectrumExtractionMethod,
    ) -> Result<ExtractionId, String> {
        let dataset_index = self
            .doc
            .dataset_index(dataset_id)
            .ok_or_else(|| "The LC–MS dataset is no longer available.".to_owned())?;
        let canvas_index = self
            .mass_spec_canvas_index(dataset_id)
            .ok_or_else(|| "No canvas currently displays this LC–MS dataset.".to_owned())?;
        let stream = self.doc.datasets[dataset_index]
            .as_mass_spec()
            .ok_or_else(|| "The selected dataset is not LC–MS data.".to_owned())?
            .active_stream;
        let (before, after, extraction_id, field, title) = {
            let dataset = self.doc.datasets[dataset_index]
                .as_mass_spec()
                .expect("dataset kind checked above");
            let before = (
                dataset.extracted_spectra.clone(),
                dataset.next_extraction_id,
            );
            let extraction =
                dataset.plan_extraction(stream, start_time_min, end_time_min, method)?;
            let extraction_id = extraction.id;
            let mut planned_catalog = dataset.field_catalog.clone();
            let field = planned_catalog.ensure_key(extracted_stream_spectrum_key(extraction_id));
            let title = extraction_title(&dataset.run, &extraction);
            let next_id = extraction_id
                .get()
                .checked_add(1)
                .map(ExtractionId::new)
                .ok_or_else(|| "LC–MS extraction identity overflow".to_owned())?;
            let mut after_extractions = before.0.clone();
            after_extractions.push(extraction);
            let after = (after_extractions, next_id);
            (before, after, extraction_id, field, title)
        };
        let [width, height] = self.doc.canvases[canvas_index].size_pt();
        let object_id = self.doc.canvases[canvas_index].allocate_object_id();
        let mut object = self.build_plot_object(
            dataset_index,
            ObjectFrame::new(0.0, 0.0, width, height),
            object_id,
            "Extracted Mass Spectrum".to_owned(),
        );
        if let Some(plot) = object.plot_mut() {
            plot.chart.type_id = "mass_spectrum".to_owned();
            if let Some(series) = plot.binding.series.first_mut() {
                series.source.field = field;
                series.encoding = plotx_figure::SeriesEncoding::default();
            }
            plot.panel.user_note = title;
        }
        let selection_before = self.session.ui.selection.clone();
        let before_frames = self.doc.canvases[canvas_index]
            .objects
            .iter()
            .filter_map(|object| {
                object.plot().and_then(|plot| {
                    plot.binding
                        .dataset_ids()
                        .contains(&dataset_id)
                        .then_some((object.id, object.frame))
                })
            })
            .collect::<Vec<_>>();
        let row_height = height / (before_frames.len() + 1) as f32;
        let mut after_frames = before_frames
            .iter()
            .enumerate()
            .map(|(row, (id, _))| {
                (
                    *id,
                    ObjectFrame::new(0.0, row as f32 * row_height, width, row_height),
                )
            })
            .collect::<Vec<_>>();
        after_frames.push((
            object_id,
            ObjectFrame::new(
                0.0,
                before_frames.len() as f32 * row_height,
                width,
                row_height,
            ),
        ));
        self.execute_action(Action::Composite(vec![
            Action::insert_object(canvas_index, object, selection_before),
            Action::SetMassSpectrumExtractions {
                dataset: dataset_id,
                before,
                after,
            },
            Action::SetObjectFrames {
                canvas: canvas_index,
                before: before_frames,
                after: after_frames,
            },
        ]));
        self.session.ui.analysis_selection = None;
        Ok(extraction_id)
    }

    fn mass_spec_canvas_index(&self, dataset_id: DatasetId) -> Option<usize> {
        self.session
            .active_canvas
            .filter(|&index| self.canvas_contains_dataset(index, dataset_id))
            .or_else(|| {
                (0..self.doc.canvases.len())
                    .find(|&index| self.canvas_contains_dataset(index, dataset_id))
            })
    }

    fn canvas_contains_dataset(&self, canvas: usize, dataset_id: DatasetId) -> bool {
        self.doc.canvases.get(canvas).is_some_and(|canvas| {
            canvas.objects.iter().any(|object| {
                object
                    .plot()
                    .is_some_and(|plot| plot.binding.dataset_ids().contains(&dataset_id))
            })
        })
    }

    pub fn select_mass_spec_stream(
        &mut self,
        dataset_id: DatasetId,
        stream: AcquisitionStreamId,
    ) -> bool {
        let Some(index) = self.doc.dataset_index(dataset_id) else {
            return false;
        };
        let old_stream = match self.doc.datasets.get(index) {
            Some(Dataset::MassSpec(dataset))
                if dataset.supported_ms_streams().any(|id| id == stream) =>
            {
                dataset.active_stream
            }
            _ => return false,
        };
        if old_stream == stream {
            return false;
        }
        self.execute_action(Action::SetMassSpecStream {
            dataset: dataset_id,
            before: old_stream,
            after: stream,
        });
        true
    }

    pub(crate) fn set_mass_spec_stream_value(&mut self, index: usize, stream: AcquisitionStreamId) {
        let (old_stream, changed) = match self.doc.datasets.get_mut(index) {
            Some(Dataset::MassSpec(dataset)) => {
                let old = dataset.active_stream;
                (old, dataset.select_stream(stream))
            }
            _ => return,
        };
        if changed {
            let dataset_id = self.doc.datasets[index].resource_id();
            self.retarget_mass_spec_bindings(dataset_id, old_stream, stream);
            self.rebuild_canvases_for(index);
        }
    }

    pub(crate) fn set_mass_spectrum_extractions_value(
        &mut self,
        index: usize,
        extractions: Vec<ExtractedMassSpectrum>,
        next_extraction_id: ExtractionId,
    ) {
        let Some(dataset) = self
            .doc
            .datasets
            .get_mut(index)
            .and_then(Dataset::as_mass_spec_mut)
        else {
            return;
        };
        dataset
            .replace_extractions(extractions, next_extraction_id)
            .expect("validated mass-spectrum extraction action");
        self.rebuild_canvases_for(index);
    }

    pub fn select_mass_spec_spectrum_near(
        &mut self,
        dataset_id: DatasetId,
        stream: AcquisitionStreamId,
        retention_time_min: f64,
    ) -> bool {
        if !retention_time_min.is_finite() {
            return false;
        }
        let Some(index) = self.doc.dataset_index(dataset_id) else {
            return false;
        };
        let active_stream = match self.doc.datasets.get(index) {
            Some(Dataset::MassSpec(dataset))
                if dataset.supported_ms_streams().any(|id| id == stream) =>
            {
                dataset.active_stream
            }
            _ => return false,
        };
        if active_stream != stream && !self.select_mass_spec_stream(dataset_id, stream) {
            return false;
        }
        let changed = self.doc.datasets[index]
            .as_mass_spec_mut()
            .is_some_and(|dataset| dataset.select_nearest_spectrum(stream, retention_time_min));
        if changed {
            self.rebuild_canvases_for(index);
        }
        changed
    }

    fn retarget_mass_spec_bindings(
        &mut self,
        dataset_id: DatasetId,
        old_stream: AcquisitionStreamId,
        new_stream: AcquisitionStreamId,
    ) {
        let Some(index) = self.doc.dataset_index(dataset_id) else {
            return;
        };
        let Some(dataset) = self.doc.datasets[index].as_mass_spec() else {
            return;
        };
        let mappings = [
            (stream_tic_key(old_stream), stream_tic_key(new_stream)),
            (stream_bpi_key(old_stream), stream_bpi_key(new_stream)),
            (
                stream_spectrum_key(old_stream),
                stream_spectrum_key(new_stream),
            ),
        ]
        .map(|(old, new)| {
            (
                dataset.field_catalog.id_for_key(&old),
                dataset.field_catalog.id_for_key(&new),
            )
        });
        let new_tic = dataset
            .field_catalog
            .id_for_key(&stream_tic_key(new_stream));
        let new_bpi = dataset
            .field_catalog
            .id_for_key(&stream_bpi_key(new_stream));
        let tic_note = dataset.tic_panel_note();
        for canvas in &mut self.doc.canvases {
            for object in &mut canvas.objects {
                let Some(plot) = object.plot_mut() else {
                    continue;
                };
                for series in &mut plot.binding.series {
                    if series.source.resource != dataset_id {
                        continue;
                    }
                    for (old, new) in mappings {
                        if old == Some(series.source.field)
                            && let Some(new) = new
                        {
                            series.source.field = new;
                        }
                    }
                }
                if plot.binding.series.iter().any(|series| {
                    series.source.resource == dataset_id
                        && (Some(series.source.field) == new_tic
                            || Some(series.source.field) == new_bpi)
                }) {
                    plot.panel.user_note = tic_note.clone();
                }
            }
        }
    }
}
