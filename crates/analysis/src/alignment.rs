//! Reference-feature detection used to align multiple spectra.

use crate::peaks::{DetectParams, detect_peaks, estimate_noise};

const MIN_PROMINENCE_SIGMA: f64 = 5.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PeakPolarity {
    Positive,
    Negative,
    Magnitude,
}

/// The tallest significant peak of `ys` with `lo <= x <= hi`.
pub fn reference_peak(x: &[f64], ys: &[f64], lo: f64, hi: f64) -> Option<f64> {
    let (lo, hi) = (lo.min(hi), lo.max(hi));
    let mut xs_window = Vec::new();
    let mut ys_window = Vec::new();
    for (&x, &y) in x.iter().zip(ys) {
        if x.is_finite() && y.is_finite() && x >= lo && x <= hi {
            xs_window.push(x);
            ys_window.push(y);
        }
    }
    let floor = MIN_PROMINENCE_SIGMA * estimate_noise(ys);
    let params = DetectParams {
        min_height: Some(floor),
        min_prominence: floor,
        min_spacing: None,
        max_count: None,
    };
    detect_peaks(&xs_window, &ys_window, &params)
        .into_iter()
        .max_by(|a, b| a.y.total_cmp(&b.y))
        .map(|peak| peak.x)
}

/// Most prominent significant feature of a generic trace in a displayed x window.
/// Magnitude compares upward and downward prominence rather than `abs(y)`, so a
/// non-zero baseline does not become a false feature.
pub fn trace_peak_anchor(
    x: &[f64],
    ys: &[f64],
    lo: f64,
    hi: f64,
    polarity: PeakPolarity,
) -> Option<f64> {
    let (lo, hi) = (lo.min(hi), lo.max(hi));
    let mut xs = Vec::new();
    let mut values = Vec::new();
    for (&x, &y) in x.iter().zip(ys) {
        if x.is_finite() && y.is_finite() && x >= lo && x <= hi {
            xs.push(x);
            values.push(y);
        }
    }
    let scale = values
        .iter()
        .fold(0.0_f64, |max, value| max.max(value.abs()));
    let floor = (MIN_PROMINENCE_SIGMA * estimate_noise(&values)).max(f64::EPSILON * scale.max(1.0));
    let params = DetectParams {
        min_height: None,
        min_prominence: floor,
        min_spacing: None,
        max_count: Some(1),
    };
    let strongest = |values: &[f64]| detect_peaks(&xs, values, &params).into_iter().next();
    match polarity {
        PeakPolarity::Positive => strongest(&values).map(|peak| peak.x),
        PeakPolarity::Negative => {
            let inverted: Vec<_> = values.iter().map(|value| -*value).collect();
            strongest(&inverted).map(|peak| peak.x)
        }
        PeakPolarity::Magnitude => {
            let positive = strongest(&values);
            let inverted: Vec<_> = values.iter().map(|value| -*value).collect();
            let negative = strongest(&inverted);
            match (positive, negative) {
                (Some(up), Some(down)) => {
                    let mut sorted = values.clone();
                    sorted.sort_by(f64::total_cmp);
                    let baseline = sorted[sorted.len() / 2];
                    let up_excursion = (values[up.index] - baseline).abs();
                    let down_excursion = (values[down.index] - baseline).abs();
                    if down.prominence > up.prominence
                        || (down.prominence == up.prominence && down_excursion > up_excursion)
                    {
                        Some(down.x)
                    } else {
                        Some(up.x)
                    }
                }
                (Some(peak), None) | (None, Some(peak)) => Some(peak.x),
                (None, None) => None,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lorentzian(x: &[f64], center: f64, width: f64) -> Vec<f64> {
        x.iter()
            .map(|&value| 1.0 / (1.0 + ((value - center) / width).powi(2)))
            .collect()
    }

    fn axis(n: usize, lo: f64, hi: f64) -> Vec<f64> {
        (0..n)
            .map(|i| lo + (hi - lo) * i as f64 / (n - 1) as f64)
            .collect()
    }

    #[test]
    fn tallest_peak_in_window_wins() {
        let xs = axis(2048, 0.0, 10.0);
        let mut ys = lorentzian(&xs, 3.0, 0.05);
        for (y, extra) in ys.iter_mut().zip(lorentzian(&xs, 7.0, 0.05)) {
            *y += 2.0 * extra;
        }
        assert!((reference_peak(&xs, &ys, 0.0, 10.0).unwrap() - 7.0).abs() < 0.02);
        assert!((reference_peak(&xs, &ys, 2.0, 5.0).unwrap() - 3.0).abs() < 0.02);
    }

    #[test]
    fn flat_or_noise_only_window_yields_none() {
        let xs = axis(1024, 0.0, 10.0);
        let ys = lorentzian(&xs, 2.0, 0.05);
        assert_eq!(reference_peak(&xs, &ys, 6.0, 9.0), None);

        let mut seed = 1u64;
        let mut rand = || {
            seed = seed
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            (seed >> 33) as f64 / (1u64 << 31) as f64 - 1.0
        };
        let noisy: Vec<f64> = ys.iter().map(|&y| y + 0.01 * rand()).collect();
        assert_eq!(reference_peak(&xs, &noisy, 6.0, 9.0), None);
        assert!((reference_peak(&xs, &noisy, 0.0, 10.0).unwrap() - 2.0).abs() < 0.05);
    }

    #[test]
    fn magnitude_compares_prominence_on_nonzero_baselines() {
        let x: Vec<_> = (0..9).map(|i| i as f64).collect();
        let y = vec![10.0, 10.0, 12.0, 10.0, 10.0, 4.0, 10.0, 10.0, 10.0];
        assert_eq!(
            trace_peak_anchor(&x, &y, 0.0, 8.0, PeakPolarity::Positive),
            Some(2.0)
        );
        assert_eq!(
            trace_peak_anchor(&x, &y, 0.0, 8.0, PeakPolarity::Negative),
            Some(5.0)
        );
        assert_eq!(
            trace_peak_anchor(&x, &y, 0.0, 8.0, PeakPolarity::Magnitude),
            Some(5.0)
        );
    }
}
