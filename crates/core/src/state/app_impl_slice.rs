use super::*;
use plotx_processing::{ProcessingStep, ProjectionMode, Slice1D, SliceKind, StepKind, StepSource};

impl NmrDataset {
    /// Build a standalone 1D spectrum from a slice/projection lifted out of a 2D
    /// dataset, bypassing the FID→FFT pipeline: the extracted trace becomes both
    /// the cached base and the displayed spectrum. A synthetic frequency-domain
    /// `data` reproduces it exactly through [`fft::transform_base`], so a later
    /// reprocess or a save/reload round-trips the shown values.
    pub fn from_slice(slice: Slice1D, source: String) -> Self {
        let Slice1D {
            ppm,
            values,
            nucleus,
            observe_freq_mhz,
            ..
        } = slice;
        let (sw, carrier) = linear_axis_params(&ppm, observe_freq_mhz);
        let n = ppm.len().max(1);
        let spectrum = Spectrum {
            ppm,
            values: values.clone(),
            hz_per_point: (sw / n as f64).abs(),
            observe_freq_mhz,
            nucleus: nucleus.clone(),
        };
        let data = NmrData {
            points: values,
            domain: plotx_io::Domain::Frequency,
            spectral_width_hz: sw,
            observe_freq_mhz,
            carrier_ppm: carrier,
            nucleus,
            source: source.clone(),
            group_delay: 0.0,
        };
        // The trace is already a phased spectrum: the pipeline is the bare FFT
        // anchor, so the transform reproduces the values with no further steps.
        let pipeline = AxisPipeline {
            steps: vec![ProcessingStep::new(StepKind::Fft, StepSource::Default)],
        };
        Self {
            resource_id: uuid::Uuid::new_v4().to_string(),
            data,
            base: spectrum.clone(),
            pipeline,
            group_delay_correct: true,
            has_imaginary: true,
            spectrum,
            name: Some(source),
            lineage: None,
            peaks: PeakSet::default(),
            integrals: Vec::new(),
            next_integral_id: 0,
            line_fits: Vec::new(),
            next_line_fit_id: 0,
            multiplets: Vec::new(),
            next_multiplet_id: 0,
        }
    }
}

/// Spectral width and carrier (ppm) that make [`fft::transform_base`] reproduce a
/// linear ppm axis `p`: `ppm[i] = carrier + (i − n/2)·sw/(n·obs)`.
fn linear_axis_params(ppm: &[f64], obs: f64) -> (f64, f64) {
    let n = ppm.len();
    if n < 2 {
        return (
            obs.max(f64::MIN_POSITIVE),
            ppm.first().copied().unwrap_or(0.0),
        );
    }
    let dp = (ppm[n - 1] - ppm[0]) / (n - 1) as f64;
    let sw = dp * n as f64 * obs;
    let carrier = ppm[0] + (n as f64 / 2.0) * dp;
    (sw, carrier)
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
        let (slice, is_stack) = match &d2.processed {
            Processed2D::Ft(s) => (s.slice(cursor.kind, cursor.index), false),
            Processed2D::Stack(s) => (s.slice(cursor.index), true),
        };
        let name = slice_name(&parent, &slice, cursor.kind, is_stack, cursor.index);
        self.insert_slice_dataset(slice, name, dataset, DerivationKind::Slice);
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
        let Processed2D::Ft(s) = &d2.processed else {
            self.session.status = "Projections are available for true-2D spectra.".into();
            return;
        };
        let parent = self.doc.datasets[dataset].display_name();
        let slice = s.project(kind, mode);
        let word = match mode {
            ProjectionMode::Sum => "sum",
            ProjectionMode::Skyline => "skyline",
        };
        let name = format!("{parent} — {} {word} projection", slice_axis_label(kind));
        self.insert_slice_dataset(slice, name, dataset, DerivationKind::Projection);
    }

    fn insert_slice_dataset(
        &mut self,
        slice: Slice1D,
        name: String,
        source: usize,
        kind: DerivationKind,
    ) {
        let mut ds = Dataset::Nmr(Box::new(NmrDataset::from_slice(slice, name.clone())));
        ds.set_lineage(Some(DatasetLineage::new(kind, [source])));
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
    match slice.position_ppm {
        Some(p) => format!("{parent} — {} slice @ {p:.3} ppm", slice_axis_label(kind)),
        None => format!("{parent} — {} slice", slice_axis_label(kind)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use num_complex::Complex64;

    fn slice() -> Slice1D {
        Slice1D {
            ppm: vec![2.0, 1.0],
            values: vec![Complex64::new(1.0, 0.0), Complex64::new(0.5, 0.0)],
            nucleus: "1H".to_owned(),
            observe_freq_mhz: 400.0,
            position_ppm: Some(3.0),
        }
    }

    #[test]
    fn slice_and_projection_insertions_record_the_source() {
        let mut app = PlotxApp::new();
        app.doc
            .datasets
            .push(Dataset::Nmr(Box::new(NmrDataset::from_slice(
                slice(),
                "source".to_owned(),
            ))));

        app.insert_slice_dataset(slice(), "slice".to_owned(), 0, DerivationKind::Slice);
        app.insert_slice_dataset(
            slice(),
            "projection".to_owned(),
            0,
            DerivationKind::Projection,
        );

        assert_eq!(
            app.doc.datasets[1].lineage(),
            Some(&DatasetLineage::new(DerivationKind::Slice, [0]))
        );
        assert_eq!(
            app.doc.datasets[2].lineage(),
            Some(&DatasetLineage::new(DerivationKind::Projection, [0]))
        );
    }
}
