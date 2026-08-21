use num_complex::Complex64;
use std::f64::consts::TAU;

use super::CraftComponent;

/// Rebuild a uniformly sampled complex FID from a fitted CRAFT component table.
pub fn synthesize_craft_fid(
    components: &[CraftComponent],
    count: usize,
    spectral_width_hz: f64,
) -> Vec<Complex64> {
    (0..count)
        .map(|index| model_at(components, index as f64 / spectral_width_hz))
        .collect()
}

/// Rebuild selected samples without evaluating or allocating intervening points.
pub fn synthesize_craft_samples(
    components: &[CraftComponent],
    sample_indices: &[usize],
    spectral_width_hz: f64,
) -> Vec<Complex64> {
    sample_indices
        .iter()
        .map(|&index| model_at(components, index as f64 / spectral_width_hz))
        .collect()
}

pub(super) fn model_at(components: &[CraftComponent], time_s: f64) -> Complex64 {
    components
        .iter()
        .fold(Complex64::new(0.0, 0.0), |sum, component| {
            sum + Complex64::from_polar(
                component.amplitude_t0 * (-component.decay_rate_s_inv * time_s).exp(),
                component.phase_rad + TAU * component.frequency_hz * time_s,
            )
        })
}
