use super::*;
use plotx_processing::{Processed1D, ProjectionMode, Slice1D, SliceKind};

impl NmrDataset {
    /// Build a standalone 1D trace from a slice/projection lifted out of a 2D
    /// dataset without changing its scientific domain.
    pub fn from_slice(slice: Slice1D, source: String) -> Self {
        let Slice1D {
            coordinates,
            domain,
            values,
            nucleus,
            observe_freq_mhz,
            ..
        } = slice;
        let (spectral_width_hz, carrier_ppm) = match domain {
            plotx_io::Domain::Frequency => linear_axis_params(&coordinates, observe_freq_mhz),
            plotx_io::Domain::Time => (time_axis_spectral_width(&coordinates), 0.0),
        };
        let data = NmrData {
            points: values.clone(),
            domain,
            spectral_width_hz,
            observe_freq_mhz,
            carrier_ppm,
            nucleus: nucleus.clone(),
            source: source.clone(),
            group_delay: 0.0,
        };
        let group_delay_correct = super::default_group_delay_correct(data.domain);
        let pipeline = AxisPipeline { steps: Vec::new() };
        let processed = match domain {
            plotx_io::Domain::Frequency => {
                let n = coordinates.len().max(1);
                Processed1D::Frequency(Spectrum {
                    ppm: coordinates,
                    values,
                    hz_per_point: (spectral_width_hz / n as f64).abs(),
                    observe_freq_mhz,
                    nucleus,
                })
            }
            plotx_io::Domain::Time => Processed1D::Time(plotx_processing::TimeTrace {
                time_s: coordinates,
                values,
                nucleus,
                source: source.clone(),
            }),
        };
        let mut field_catalog = nmr_field_catalog();
        field_catalog.attach_provenance(&data.source, None);
        Self {
            resource_id: DatasetId::new(),
            field_catalog,
            data,
            base: processed.clone(),
            pipeline,
            next_step_id: 0,
            group_delay_correct,
            has_imaginary: true,
            processed,
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

fn time_axis_spectral_width(time_s: &[f64]) -> f64 {
    let Some((&first, &last)) = time_s.first().zip(time_s.last()) else {
        return 1.0;
    };
    if time_s.len() < 2 {
        return 1.0;
    }
    let dwell = (last - first).abs() / (time_s.len() - 1) as f64;
    if dwell.is_finite() && dwell > f64::MIN_POSITIVE {
        1.0 / dwell
    } else {
        1.0
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
            observe_freq_mhz: 400.0,
            position: Some(3.0),
            position_domain: plotx_io::Domain::Frequency,
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
        let dataset = NmrDataset::from_slice(slice(), "slice".to_owned());
        assert!(!dataset.group_delay_correct);
        assert_eq!(
            dataset.group_delay_correct,
            default_group_delay_correct(dataset.data.domain)
        );
    }

    #[test]
    fn time_domain_slice_stays_a_time_trace() {
        let mut time = slice();
        time.coordinates = vec![0.0, 0.002];
        time.domain = plotx_io::Domain::Time;
        let dataset = NmrDataset::from_slice(time, "FID slice".to_owned());
        assert_eq!(dataset.data.domain, plotx_io::Domain::Time);
        assert_eq!(dataset.output_domain(), plotx_io::Domain::Time);
        assert_eq!(dataset.time_trace().unwrap().time_s, vec![0.0, 0.002]);
    }
}
