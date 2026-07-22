//! True-2D integral collection and reference-normalization helpers.

use super::*;

impl Nmr2DDataset {
    /// Reduction currently drawn by the contour view.
    pub fn display_mode(&self) -> DisplayMode {
        let magnitude = [&self.params.f2, &self.params.f1]
            .into_iter()
            .any(|pipeline| {
                pipeline
                    .steps
                    .iter()
                    .any(|step| step.enabled && matches!(step.kind, StepKind::Magnitude))
            });
        if magnitude {
            DisplayMode::Magnitude
        } else {
            DisplayMode::Real
        }
    }

    /// Refresh raw volumes from the displayed true-2D reduction. A malformed
    /// rectangle remains stored with its previous value while valid peers update.
    pub fn recompute_integrals(
        &mut self,
    ) -> Result<(), plotx_analysis::integrate_2d::IntegrateError> {
        let mode = self.display_mode();
        let Processed2D::Ft(spectrum) = &self.processed else {
            self.integral_error = None;
            return Ok(());
        };
        if self.integrals.is_empty() {
            self.integral_error = None;
            return Ok(());
        }
        let grid: Vec<f64> = spectrum
            .data
            .iter()
            .map(|value| mode.reduce(value))
            .collect();
        let prepared = plotx_analysis::integrate_2d::IntegrationGrid2D::new(
            &spectrum.f2_ppm,
            &spectrum.f1_ppm,
            &grid,
            spectrum.f2_size,
            spectrum.f1_size,
        );
        let prepared = match prepared {
            Ok(prepared) => prepared,
            Err(error) => {
                self.integral_error = Some(error.to_string());
                return Err(error);
            }
        };
        let results: Vec<_> = self
            .integrals
            .iter()
            .map(|integral| prepared.integrate(integral.f2, integral.f1, integral.baseline))
            .collect();
        let mut first_error = None;
        for (integral, result) in self.integrals.iter_mut().zip(results) {
            match result {
                Ok(volume) => {
                    integral.volume = volume;
                    integral.mode = mode.into();
                }
                Err(error) if first_error.is_none() => first_error = Some(error),
                Err(_) => {}
            }
        }
        self.renormalize_integrals();
        self.integral_error = first_error.as_ref().map(ToString::to_string);
        match first_error {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }

    /// Rebuild the runtime id source without changing persisted ids.
    pub fn reseed_integral_ids(&mut self) {
        self.next_integral_id = self
            .integrals
            .iter()
            .map(|integral| integral.id.saturating_add(1))
            .max()
            .unwrap_or(0);
    }

    /// Allocate an id unique within this dataset for a newly created integral.
    pub fn next_integral_id(&mut self) -> u64 {
        let id = self.next_integral_id;
        self.next_integral_id = self.next_integral_id.saturating_add(1);
        id
    }

    /// Refresh normalized values from the optional user-selected reference.
    pub fn renormalize_integrals(&mut self) {
        let reference = self.integrals.iter().find_map(|integral| {
            integral
                .reference_value
                .map(|value| (integral.volume, value))
        });
        let max_volume = self
            .integrals
            .iter()
            .map(|integral| integral.volume.abs())
            .fold(0.0, f64::max);
        let usable = reference.is_some_and(|(volume, value)| {
            volume.is_finite()
                && value.is_finite()
                && max_volume.is_finite()
                && max_volume > 0.0
                && volume.abs() >= 1e-12 * max_volume
        });

        for integral in &mut self.integrals {
            integral.normalized_volume = usable
                .then(|| {
                    let (reference_volume, reference_value) = reference.unwrap();
                    integral.volume / reference_volume * reference_value
                })
                .filter(|value| value.is_finite());
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{BaselineMode, DisplayModeLabel, IntegralMethod};

    use super::*;

    fn integral(id: u64, volume: f64, reference_value: Option<f64>) -> Integral2D {
        Integral2D {
            id,
            name: format!("I{id}"),
            f2: (1.0, 2.0),
            f1: (3.0, 4.0),
            volume,
            normalized_volume: None,
            reference_value,
            mode: DisplayModeLabel::Real,
            method: IntegralMethod::Sum,
            baseline: BaselineMode::None,
        }
    }

    #[test]
    fn normalization_supports_signed_reference_weight() {
        let values = vec![integral(7, -2.0, Some(2.0)), integral(11, 6.0, None)];
        let mut dataset = test_dataset();
        dataset.integrals = values;
        dataset.renormalize_integrals();

        assert_eq!(dataset.integrals[0].normalized_volume, Some(2.0));
        assert_eq!(dataset.integrals[1].normalized_volume, Some(-6.0));
    }

    #[test]
    fn near_zero_reference_is_unusable_and_delete_leaves_no_reference() {
        let mut dataset = test_dataset();
        dataset.integrals = vec![integral(5, 1e-13, Some(1.0)), integral(9, 1.0, None)];
        dataset.renormalize_integrals();
        assert!(
            dataset
                .integrals
                .iter()
                .all(|integral| integral.normalized_volume.is_none())
        );

        dataset.integrals.retain(|integral| integral.id != 5);
        dataset.renormalize_integrals();
        assert_eq!(dataset.integrals[0].reference_value, None);
        assert_eq!(dataset.integrals[0].normalized_volume, None);
    }

    #[test]
    fn reseeding_preserves_sparse_stable_ids() {
        let mut dataset = test_dataset();
        dataset.integrals = vec![integral(3, 1.0, Some(1.0)), integral(41, 2.0, None)];
        dataset.reseed_integral_ids();
        assert_eq!(dataset.integrals[0].id, 3);
        assert_eq!(dataset.integrals[1].id, 41);
        assert_eq!(dataset.next_integral_id(), 42);
    }

    #[test]
    fn processing_preview_defers_volume_recompute_until_commit() {
        let mut dataset = test_dataset();
        dataset.integrals = vec![integral(0, 123.0, Some(1.0))];

        dataset.rebuild();
        assert_eq!(dataset.integrals[0].volume, 123.0);

        dataset.recompute_integrals().unwrap();
        assert_ne!(dataset.integrals[0].volume, 123.0);
    }

    #[test]
    fn committed_phase_change_recomputes_signed_volume() {
        use crate::actions::DatasetProcessingState;
        use std::f64::consts::PI;

        let mut dataset = test_dataset();
        dataset.integrals = vec![integral(0, 0.0, Some(1.0))];
        dataset.recompute_integrals().unwrap();
        assert!(dataset.integrals[0].volume > 0.0);

        let mut app = PlotxApp::new();
        app.doc.datasets.push(Dataset::Nmr2D(Box::new(dataset)));
        let mut state = DatasetProcessingState::from_dataset(&app.doc.datasets[0]);
        let DatasetProcessingState::Nmr2D { params, .. } = &mut state else {
            unreachable!();
        };
        let StepKind::Phase(phase) = &mut params.f2.steps[0].kind else {
            unreachable!();
        };
        phase.auto = None;
        phase.phase0 = PI;

        app.set_dataset_processing_state(0, &state);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        while app.compute_busy() && std::time::Instant::now() < deadline {
            app.poll_compute();
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert!(!app.compute_busy(), "2D phase task did not finish");
        let volume = app.doc.datasets[0].as_nmr2d().unwrap().integrals[0].volume;
        assert!(volume < 0.0);
    }

    fn test_dataset() -> Nmr2DDataset {
        use num_complex::Complex64;
        use plotx_io::{Dim, Domain, NmrData2D, QuadMode};

        let dim = Dim {
            spectral_width_hz: 1000.0,
            observe_freq_mhz: 100.0,
            carrier_ppm: 5.0,
            nucleus: "X".to_owned(),
            group_delay: 0.0,
        };
        Nmr2DDataset::load(NmrData2D {
            data: vec![Complex64::new(1.0, 0.0); 4],
            rows: 2,
            cols: 2,
            domain: Domain::Frequency,
            direct: dim.clone(),
            indirect: dim,
            quad: QuadMode::Complex,
            indirect_conjugate: false,
            experiment: None,
            pseudo_axis: None,
            diffusion: None,
            nus: None,
            source: "test".to_owned(),
        })
    }
}
