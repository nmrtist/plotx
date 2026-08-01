use super::*;
use crate::actions::Action;
use plotx_io::FunctionId;

impl PlotxApp {
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
        let function = self.doc.datasets[dataset_index]
            .as_mass_spec()
            .ok_or_else(|| "The selected dataset is not LC–MS data.".to_owned())?
            .active_function;
        let (before, after, extraction_id, field, title) = {
            let dataset = self.doc.datasets[dataset_index]
                .as_mass_spec()
                .expect("dataset kind checked above");
            let before = (
                dataset.extracted_spectra.clone(),
                dataset.next_extraction_id,
            );
            let extraction =
                dataset.plan_extraction(function, start_time_min, end_time_min, method)?;
            let extraction_id = extraction.id;
            let mut planned_catalog = dataset.field_catalog.clone();
            let field = planned_catalog.ensure_key(extracted_spectrum_key(extraction_id));
            let title = extraction_title(&extraction);
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

    pub fn select_mass_spec_function(
        &mut self,
        dataset_id: DatasetId,
        function: FunctionId,
    ) -> bool {
        let Some(index) = self.doc.dataset_index(dataset_id) else {
            return false;
        };
        let old_function = match self.doc.datasets.get(index) {
            Some(Dataset::MassSpec(dataset))
                if dataset.supported_ms_functions().any(|id| id == function) =>
            {
                dataset.active_function
            }
            _ => return false,
        };
        if old_function == function {
            return false;
        }
        self.execute_action(Action::SetMassSpecFunction {
            dataset: dataset_id,
            before: old_function,
            after: function,
        });
        true
    }

    pub(crate) fn set_mass_spec_function_value(&mut self, index: usize, function: FunctionId) {
        let (old_function, changed) = match self.doc.datasets.get_mut(index) {
            Some(Dataset::MassSpec(dataset)) => {
                let old = dataset.active_function;
                (old, dataset.select_function(function))
            }
            _ => return,
        };
        if changed {
            let dataset_id = self.doc.datasets[index].resource_id();
            self.retarget_mass_spec_bindings(dataset_id, old_function, function);
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

    pub fn select_mass_spec_scan_near(
        &mut self,
        dataset_id: DatasetId,
        function: FunctionId,
        retention_time_min: f64,
    ) -> bool {
        let Some(index) = self.doc.dataset_index(dataset_id) else {
            return false;
        };
        let (old_function, changed) = match self.doc.datasets.get_mut(index) {
            Some(Dataset::MassSpec(dataset)) => {
                let old = dataset.active_function;
                (
                    old,
                    dataset.select_nearest_scan(function, retention_time_min),
                )
            }
            _ => return false,
        };
        if changed {
            self.retarget_mass_spec_bindings(dataset_id, old_function, function);
            self.rebuild_canvases_for(index);
        }
        changed
    }

    fn retarget_mass_spec_bindings(
        &mut self,
        dataset_id: DatasetId,
        old_function: FunctionId,
        new_function: FunctionId,
    ) {
        let Some(index) = self.doc.dataset_index(dataset_id) else {
            return;
        };
        let Some(dataset) = self.doc.datasets[index].as_mass_spec() else {
            return;
        };
        let mappings = [
            (tic_key(old_function), tic_key(new_function)),
            (bpi_key(old_function), bpi_key(new_function)),
            (spectrum_key(old_function), spectrum_key(new_function)),
        ]
        .map(|(old, new)| {
            (
                dataset.field_catalog.id_for_key(&old),
                dataset.field_catalog.id_for_key(&new),
            )
        });
        let new_tic = dataset.field_catalog.id_for_key(&tic_key(new_function));
        let new_bpi = dataset.field_catalog.id_for_key(&bpi_key(new_function));
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
