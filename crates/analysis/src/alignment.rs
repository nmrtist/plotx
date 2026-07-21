//! Reference-feature detection used to align multiple spectra.

use crate::peaks::{DetectParams, detect_peaks, estimate_noise};

const MIN_PROMINENCE_SIGMA: f64 = 5.0;

/// The tallest significant peak of `ys` with `lo <= x <= hi`.
pub fn reference_peak(x: &[f64], ys: &[f64], lo: f64, hi: f64) -> Option<f64> {
    let (lo, hi) = (lo.min(hi), lo.max(hi));
    let mut xs_window = Vec::new();
    let mut ys_window = Vec::new();
    for (&x, &y) in x.iter().zip(ys) {
        if x >= lo && x <= hi {
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
}
