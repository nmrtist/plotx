//! `NmrDataset` interactive-integral and pivot helpers.

use super::*;

impl NmrDataset {
    /// Reassign contiguous ids to the loaded integrals and reseed the id source, so
    /// bands stay individually addressable after a project round-trip.
    pub fn normalize_integral_ids(&mut self) {
        for (i, integ) in self.integrals.iter_mut().enumerate() {
            integ.id = i as u64;
        }
        self.next_integral_id = self.integrals.len() as u64;
    }

    /// Refresh every integral's area from the current spectrum (after a band moved
    /// or resized) and renormalize: with a reference band, values are `area /
    /// reference-area` (the reference reads 1.000); without one, each keeps its
    /// total-spectrum fraction.
    pub fn recompute_integrals(&mut self) {
        let refreshed: Vec<Option<(f64, f64)>> = self
            .integrals
            .iter()
            .map(|integ| {
                crate::integrate_region(
                    &self.spectrum,
                    DisplayMode::Real,
                    (integ.start_ppm, integ.end_ppm),
                )
                .map(|r| (r.area, r.normalized_area))
            })
            .collect();
        for (integ, res) in self.integrals.iter_mut().zip(refreshed) {
            if let Some((area, norm)) = res {
                integ.area = area;
                integ.normalized_area = norm;
            }
        }
        if let Some(ref_area) = self
            .integrals
            .iter()
            .find(|integ| integ.is_reference)
            .map(|integ| integ.area)
        {
            let ref_area = if ref_area.abs() < f64::MIN_POSITIVE {
                f64::MIN_POSITIVE
            } else {
                ref_area
            };
            for integ in &mut self.integrals {
                integ.normalized_area = integ.area / ref_area;
            }
        }
    }

    fn ppm_ends(&self) -> (f64, f64) {
        let p = &self.base.ppm;
        match (p.first(), p.last()) {
            (Some(&a), Some(&b)) => (a, b),
            _ => (0.0, 1.0),
        }
    }

    pub fn pivot_ppm(&self) -> f64 {
        let (lo, hi) = self.ppm_ends();
        let frac = self
            .pipeline
            .steps
            .iter()
            .filter(|s| s.enabled)
            .find_map(|s| match &s.kind {
                // While a step still auto-phases, its stored pivot is a placeholder;
                // show the peak the pass actually rotates about so the on-plot handle
                // sits where the user expects instead of pinned to an edge.
                StepKind::Phase(p) => Some(match p.auto {
                    Some(_) => plotx_processing::phase::peak_pivot_frac(&self.base.values),
                    None => p.pivot_frac,
                }),
                _ => None,
            })
            .unwrap_or(0.0);
        lo + (hi - lo) * frac
    }

    pub fn set_pivot_ppm(&mut self, ppm: f64) {
        let (lo, hi) = self.ppm_ends();
        let span = hi - lo;
        let frac = if span.abs() < f64::EPSILON {
            0.0
        } else {
            ((ppm - lo) / span).clamp(0.0, 1.0)
        };
        for step in self.pipeline.steps.iter_mut().filter(|s| s.enabled) {
            if let StepKind::Phase(p) = &mut step.kind {
                p.pivot_frac = frac;
                return;
            }
        }
    }
}
