use super::*;
use plotx_processing::{ProjectionMode, Slice1D, SliceKind};
use std::sync::Arc;

impl NmrDataset {
    /// Explicit coordinates are retained for a standalone programmatic trace.
    pub fn from_slice(slice: Slice1D, source: String) -> Result<Self, String> {
        use nmr::axis::{AxisCoordinates, AxisDomain, AxisRole, AxisUnit, FrequencyEvidence};
        use nmr::processed::{
            ComponentBasis, ProcessedAxis, ProcessedDataset, ProcessedOrigin, ProcessedProvenance,
        };
        let fail = |error: &dyn std::fmt::Display| error.to_string();
        let (domain, unit) = match slice.domain {
            Domain::Time => (AxisDomain::Time, AxisUnit::Second),
            Domain::Frequency => (AxisDomain::Frequency, slice.unit),
        };
        let reference = (unit == AxisUnit::Ppm)
            .then_some(slice.reference_freq_mhz)
            .flatten();
        let coordinates = if let Some(frequency) = reference {
            slice
                .coordinates
                .into_iter()
                .map(|value| value * frequency)
                .collect()
        } else {
            slice.coordinates
        };
        let axis = ProcessedAxis::new(
            AxisRole::Signal,
            domain,
            Some(if reference.is_some() {
                AxisUnit::Hertz
            } else {
                unit
            }),
            slice.values.len(),
            AxisCoordinates::Explicit(coordinates),
            ComponentBasis::Cartesian,
        )
        .map_err(|error| fail(&error))?
        .with_nucleus((!slice.nucleus.is_empty()).then_some(slice.nucleus))
        .map_err(|error| fail(&error))?
        .with_frequency_evidence(Some(
            FrequencyEvidence::new(slice.observe_freq_mhz, None).map_err(|error| fail(&error))?,
        ))
        .map_err(|error| fail(&error))?;
        let data = ProcessedDataset::from_complex_trace(
            axis,
            slice.values,
            ProcessedProvenance::new(ProcessedOrigin::Unknown, Vec::new())
                .map_err(|error| fail(&error))?,
        )
        .map_err(|error| fail(&error))?;
        let data = if let Some(frequency) = reference {
            use nmr::processing::{
                FrequencyFrame, ProcessingOperation, ProcessingPlan, ReferenceSource,
            };
            ProcessingPlan::new(vec![ProcessingOperation::ResolveFrequencyFrame {
                axis: 0,
                frame: FrequencyFrame::Ppm(ReferenceSource::Explicit(
                    nmr::raw::ChemicalShiftReference::user_constructed(0.0, frequency)
                        .map_err(|error| fail(&error))?,
                )),
            }])
            .map_err(|error| fail(&error))?
            .apply(&data.into())
            .map_err(|error| fail(&error))?
        } else {
            data.into()
        };
        let input = plotx_io::nmr_view::NmrSource::new(Arc::new(data))
            .map_err(|error| fail(&error))?
            .with_display_label(source.clone());
        let mut dataset =
            Self::load_with_pipeline(input, Some(AxisPipeline { steps: Vec::new() }), Some(false))?;
        dataset.acquisition_identity = plotx_io::AcquisitionIdentity {
            source_label: source.clone(),
            subject: None,
            acquisition: None,
        };
        dataset.name = Some(source);
        Ok(dataset)
    }
}

impl PlotxApp {
    /// Materialize the current slice cursor as a new standalone 1D dataset and
    /// drop it into the workspace on its own page, as one undoable step.
    pub fn extract_slice_dataset(&mut self, dataset: usize) {
        let Some(cursor) = self.session.ui.slice.filter(|c| c.dataset == dataset) else {
            self.session.status =
                "Position a slice over the 2D plot (or pick an increment) first.".into();
            return;
        };
        let Some(d2) = self.doc.datasets.get(dataset).and_then(Dataset::as_nmr2d) else {
            return;
        };
        let parent = self.doc.datasets[dataset].display_name();
        let is_stack = matches!(d2.processed, Processed2D::Stack(_));
        let kind = if is_stack {
            SliceKind::Row
        } else {
            cursor.kind
        };
        let (source, slice) = match plotx_processing::slice::extract(
            &d2.native_processed,
            kind,
            plotx_processing::slice::Reduction::Slice(cursor.index),
        ) {
            Ok(output) => output,
            Err(error) => {
                self.session.status = format!("Slice extraction failed: {error}");
                return;
            }
        };
        let name = slice_name(&parent, &slice, kind, is_stack, cursor.index);
        self.insert_slice_dataset(source, name, dataset, DerivationKind::Slice);
    }

    /// Materialize a whole-axis projection of a true-2D spectrum as a new 1D
    /// dataset (the shared foundation the interactive slice reuses).
    pub fn extract_projection_dataset(
        &mut self,
        dataset: usize,
        kind: SliceKind,
        mode: ProjectionMode,
    ) {
        let Some(d2) = self.doc.datasets.get(dataset).and_then(Dataset::as_nmr2d) else {
            return;
        };
        let Processed2D::Ft(_) = &d2.processed else {
            self.session.status = "Projections are available for true-2D spectra.".into();
            return;
        };
        let parent = self.doc.datasets[dataset].display_name();
        let source = match plotx_processing::slice::extract(
            &d2.native_processed,
            kind,
            plotx_processing::slice::Reduction::Projection(mode),
        ) {
            Ok((source, _)) => source,
            Err(error) => {
                self.session.status = format!("Projection failed: {error}");
                return;
            }
        };
        let word = match mode {
            ProjectionMode::Sum => "sum",
            ProjectionMode::Skyline => "skyline",
        };
        let name = format!("{parent} — {} {word} projection", slice_axis_label(kind));
        self.insert_slice_dataset(source, name, dataset, DerivationKind::Projection);
    }

    fn insert_slice_dataset(
        &mut self,
        source_data: plotx_io::nmr_view::NmrSource,
        name: String,
        source: usize,
        kind: DerivationKind,
    ) {
        let dataset = match NmrDataset::load_with_pipeline(
            source_data,
            Some(AxisPipeline { steps: Vec::new() }),
            Some(false),
        ) {
            Ok(dataset) => dataset,
            Err(error) => {
                self.session.status = format!("Slice extraction failed: {error}");
                return;
            }
        };
        let mut dataset = dataset;
        dataset.name = Some(name.clone());
        let mut ds = Dataset::Nmr(Box::new(dataset));
        ds.set_lineage(Some(DatasetLineage::new(
            kind,
            [self.doc.datasets[source].resource_id()],
        )));
        let action = Action::insert_dataset_with_default_canvas(
            self,
            ds,
            format!("Canvas {} — {}", self.doc.canvases.len() + 1, name),
            DEFAULT_CANVAS_SIZE_MM,
        );
        self.execute_action(action);
        self.session.status = format!("Extracted {name}.");
    }
}

/// The axis a slice/projection of `kind` runs along (its trace's x-axis).
fn slice_axis_label(kind: SliceKind) -> &'static str {
    match kind {
        SliceKind::Row => "F2",
        SliceKind::Column => "F1",
    }
}

fn slice_name(
    parent: &str,
    slice: &Slice1D,
    kind: SliceKind,
    is_stack: bool,
    index: usize,
) -> String {
    if is_stack {
        return format!("{parent} — increment {index}");
    }
    match slice.position {
        Some(position) => format!(
            "{parent} — {} slice @ {position:.3} {}",
            slice_axis_label(kind),
            domain_unit(slice.position_domain)
        ),
        None => format!("{parent} — {} slice", slice_axis_label(kind)),
    }
}

fn domain_unit(domain: plotx_io::Domain) -> &'static str {
    match domain {
        plotx_io::Domain::Time => "s",
        plotx_io::Domain::Frequency => "ppm",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use num_complex::Complex64;

    fn slice() -> Slice1D {
        Slice1D {
            coordinates: vec![2.0, 1.0],
            domain: plotx_io::Domain::Frequency,
            values: vec![Complex64::new(1.0, 0.0), Complex64::new(0.5, 0.0)],
            nucleus: "1H".to_owned(),
            observe_freq_mhz: Some(400.0),
            reference_freq_mhz: Some(400.0),
            unit: nmr::axis::AxisUnit::Ppm,
            position: Some(3.0),
            position_domain: plotx_io::Domain::Frequency,
        }
    }

    #[test]
    fn slice_and_projection_insertions_record_the_source() {
        let mut app = PlotxApp::new();
        app.doc.datasets.push(Dataset::Nmr(Box::new(
            NmrDataset::from_slice(slice(), "source".to_owned()).unwrap(),
        )));

        app.insert_slice_dataset(
            app.doc.datasets[0]
                .as_nmr()
                .unwrap()
                .native_processed
                .clone(),
            "slice".to_owned(),
            0,
            DerivationKind::Slice,
        );
        app.insert_slice_dataset(
            app.doc.datasets[0]
                .as_nmr()
                .unwrap()
                .native_processed
                .clone(),
            "projection".to_owned(),
            0,
            DerivationKind::Projection,
        );

        assert_eq!(
            app.doc.datasets[1].lineage(),
            Some(&DatasetLineage::new(
                DerivationKind::Slice,
                [app.doc.datasets[0].resource_id()]
            ))
        );
        assert_eq!(
            app.doc.datasets[2].lineage(),
            Some(&DatasetLineage::new(
                DerivationKind::Projection,
                [app.doc.datasets[0].resource_id()]
            ))
        );
    }

    #[test]
    fn frequency_domain_slices_share_the_factory_group_delay_default() {
        let dataset = NmrDataset::from_slice(slice(), "slice".to_owned()).unwrap();
        assert!(!dataset.group_delay_correct);
        assert_eq!(
            dataset.group_delay_correct,
            default_group_delay_correct(&dataset.data)
        );
    }

    #[test]
    fn time_domain_slice_stays_a_time_trace() {
        let mut time = slice();
        time.coordinates = vec![0.0, 0.002];
        time.domain = plotx_io::Domain::Time;
        let dataset = NmrDataset::from_slice(time, "FID slice".to_owned()).unwrap();
        assert_eq!(dataset.input_domain(), plotx_io::Domain::Time);
        assert_eq!(dataset.output_domain(), plotx_io::Domain::Time);
        assert_eq!(dataset.time_trace().unwrap().time_s, vec![0.0, 0.002]);
    }
}
