//! Spectrum cleanup steps: smoothing, normalization, binning, reverse, invert.

use crate::{BinMethod, BinParams, NormalizeMethod, SmoothMethod, Spectrum};
use num_complex::Complex64;

pub fn smooth(spec: &mut Spectrum, method: SmoothMethod) {
    match method {
        SmoothMethod::MovingAverage { window } => moving_average(&mut spec.values, window as usize),
        SmoothMethod::SavitzkyGolay { window, poly_order } => {
            savitzky_golay(&mut spec.values, window as usize, poly_order as usize)
        }
    }
}

fn moving_average(values: &mut Vec<Complex64>, window: usize) {
    let n = values.len();
    let w = (window.max(3) | 1).min(if n % 2 == 1 { n } else { n.saturating_sub(1) });
    if n < 3 || w < 3 {
        return;
    }
    let h = w / 2;
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let lo = i.saturating_sub(h);
        let hi = (i + h + 1).min(n);
        let sum: Complex64 = values[lo..hi].iter().sum();
        out.push(sum / (hi - lo) as f64);
    }
    *values = out;
}

/// Least-squares polynomial smoothing: each point is replaced by the value of a
/// degree-`order` polynomial fitted over an odd `window` around it. Edge points
/// reuse the boundary window, evaluated off-center, so a polynomial signal of
/// degree ≤ `order` is reproduced exactly everywhere.
fn savitzky_golay(values: &mut Vec<Complex64>, window: usize, order: usize) {
    let n = values.len();
    let w = (window.max(3) | 1).min(if n % 2 == 1 { n } else { n.saturating_sub(1) });
    if n < 3 || w < 3 {
        return;
    }
    let m = order.clamp(1, w - 1) + 1;
    let h = w / 2;
    let x = |i: usize| i as f64 - h as f64;

    let mut gram = vec![vec![0.0; m]; m];
    for i in 0..w {
        let mut powers = vec![1.0; m];
        for k in 1..m {
            powers[k] = powers[k - 1] * x(i);
        }
        for r in 0..m {
            for c in 0..m {
                gram[r][c] += powers[r] * powers[c];
            }
        }
    }
    let mut gram_inv = vec![vec![0.0; m]; m];
    for k in 0..m {
        let mut e = vec![0.0; m];
        e[k] = 1.0;
        let Some(col) = plotx_analysis::fit::solve_linear(&gram, &e) else {
            return;
        };
        for r in 0..m {
            gram_inv[r][k] = col[r];
        }
    }
    let sample_powers: Vec<Vec<f64>> = (0..w)
        .map(|i| {
            let mut powers = vec![1.0; m];
            for k in 1..m {
                powers[k] = powers[k - 1] * x(i);
            }
            powers
        })
        .collect();
    // projection[k][i]: coefficient k of the fitted polynomial from sample i.
    let projection: Vec<Vec<f64>> = (0..m)
        .map(|r| {
            sample_powers
                .iter()
                .map(|powers| (0..m).map(|k| gram_inv[r][k] * powers[k]).sum())
                .collect()
        })
        .collect();
    // weights[p][i]: smoothing weights when evaluating at offset p in the window.
    let weights: Vec<Vec<f64>> = (0..w)
        .map(|p| {
            let mut powers = vec![1.0; m];
            for k in 1..m {
                powers[k] = powers[k - 1] * x(p);
            }
            (0..w)
                .map(|i| (0..m).map(|k| powers[k] * projection[k][i]).sum())
                .collect()
        })
        .collect();

    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let (start, p) = if i < h {
            (0, i)
        } else if i + h >= n {
            (n - w, i - (n - w))
        } else {
            (i - h, h)
        };
        let mut acc = Complex64::new(0.0, 0.0);
        for (j, &weight) in weights[p].iter().enumerate() {
            acc += values[start + j] * weight;
        }
        out.push(acc);
    }
    *values = out;
}

pub fn normalize(spec: &mut Spectrum, method: NormalizeMethod) {
    let scale = match method {
        NormalizeMethod::MaxPeak => spec.values.iter().map(|c| c.norm()).fold(0.0, f64::max),
        NormalizeMethod::TotalArea => {
            spec.values.iter().map(|c| c.re.abs()).sum::<f64>() * axis_step(&spec.ppm)
        }
        NormalizeMethod::Constant { divisor } => divisor,
    };
    if scale.is_finite() && scale.abs() > f64::MIN_POSITIVE {
        for c in &mut spec.values {
            *c /= scale;
        }
    }
}

/// Aggregate runs of points into bins of `width` axis units. The axis becomes
/// the per-bin mean position and `hz_per_point` grows by the same factor, so
/// axis metadata stays consistent with the reduced point count.
pub fn bin(spec: &mut Spectrum, params: BinParams) {
    let n = spec.values.len();
    let step = axis_step(&spec.ppm);
    if n == 0 || !params.width.is_finite() || params.width <= 0.0 {
        return;
    }
    let per = ((params.width / step).round() as usize).max(1);
    if per <= 1 {
        return;
    }
    let bins = n.div_ceil(per);
    let mut values = Vec::with_capacity(bins);
    let mut ppm = Vec::with_capacity(bins);
    for start in (0..n).step_by(per) {
        let end = (start + per).min(n);
        let count = (end - start) as f64;
        let sum: Complex64 = spec.values[start..end].iter().sum();
        values.push(match params.method {
            BinMethod::Sum => sum,
            BinMethod::Mean => sum / count,
        });
        ppm.push(spec.ppm[start..end].iter().sum::<f64>() / count);
    }
    spec.values = values;
    spec.ppm = ppm;
    spec.hz_per_point *= per as f64;
}

/// Mirror the intensities along the axis; the axis itself keeps its ordering.
pub fn reverse(spec: &mut Spectrum) {
    spec.values.reverse();
}

pub fn invert(spec: &mut Spectrum) {
    for c in &mut spec.values {
        *c = -*c;
    }
}

fn axis_step(ppm: &[f64]) -> f64 {
    if ppm.len() < 2 {
        return 1.0;
    }
    let span = (ppm[ppm.len() - 1] - ppm[0]).abs();
    if span > 0.0 {
        span / (ppm.len() - 1) as f64
    } else {
        1.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec_from(real: Vec<f64>) -> Spectrum {
        let n = real.len();
        Spectrum {
            ppm: (0..n).map(|i| i as f64 * 0.01).collect(),
            values: real.into_iter().map(|v| Complex64::new(v, 0.0)).collect(),
            hz_per_point: 1.0,
            observe_freq_mhz: 400.0,
            nucleus: "1H".into(),
        }
    }

    #[test]
    fn moving_average_preserves_a_constant_and_averages_neighbors() {
        let mut s = spec_from(vec![4.0; 50]);
        smooth(&mut s, SmoothMethod::MovingAverage { window: 5 });
        for c in &s.values {
            assert!((c.re - 4.0).abs() < 1e-12);
        }
        let mut s = spec_from(vec![0.0, 0.0, 3.0, 0.0, 0.0]);
        smooth(&mut s, SmoothMethod::MovingAverage { window: 3 });
        assert!((s.values[1].re - 1.0).abs() < 1e-12);
        assert!((s.values[2].re - 1.0).abs() < 1e-12);
        assert!((s.values[3].re - 1.0).abs() < 1e-12);
    }

    #[test]
    fn savitzky_golay_reproduces_a_cubic_exactly_including_edges() {
        let cubic: Vec<f64> = (0..80)
            .map(|i| {
                let t = i as f64 * 0.1;
                2.0 + 3.0 * t - 1.5 * t * t + 0.25 * t * t * t
            })
            .collect();
        let mut s = spec_from(cubic.clone());
        smooth(
            &mut s,
            SmoothMethod::SavitzkyGolay {
                window: 9,
                poly_order: 3,
            },
        );
        for (c, expected) in s.values.iter().zip(&cubic) {
            assert!((c.re - expected).abs() < 1e-9, "{} vs {expected}", c.re);
        }
    }

    #[test]
    fn savitzky_golay_attenuates_noise() {
        let noisy: Vec<f64> = (0..200)
            .map(|i| if i % 2 == 0 { 1.0 } else { -1.0 })
            .collect();
        let mut s = spec_from(noisy);
        smooth(
            &mut s,
            SmoothMethod::SavitzkyGolay {
                window: 11,
                poly_order: 2,
            },
        );
        let rms: f64 =
            (s.values.iter().map(|c| c.re * c.re).sum::<f64>() / s.values.len() as f64).sqrt();
        assert!(rms < 0.5, "rms {rms}");
    }

    #[test]
    fn max_peak_normalization_scales_the_tallest_peak_to_one() {
        let mut s = spec_from(vec![1.0, -2.0, 8.0, 0.5]);
        normalize(&mut s, NormalizeMethod::MaxPeak);
        let max = s.values.iter().map(|c| c.norm()).fold(0.0, f64::max);
        assert!((max - 1.0).abs() < 1e-12);
        assert!((s.values[1].re + 0.25).abs() < 1e-12);
    }

    #[test]
    fn total_area_normalization_makes_the_integral_one() {
        let mut s = spec_from((0..100).map(|i| if i == 50 { 20.0 } else { 2.0 }).collect());
        normalize(&mut s, NormalizeMethod::TotalArea);
        let dx = 0.01;
        let area: f64 = s.values.iter().map(|c| c.re.abs()).sum::<f64>() * dx;
        assert!((area - 1.0).abs() < 1e-12, "area {area}");
    }

    #[test]
    fn constant_normalization_divides_and_ignores_zero() {
        let mut s = spec_from(vec![4.0, 6.0]);
        normalize(&mut s, NormalizeMethod::Constant { divisor: 2.0 });
        assert!((s.values[0].re - 2.0).abs() < 1e-12);
        normalize(&mut s, NormalizeMethod::Constant { divisor: 0.0 });
        assert!((s.values[0].re - 2.0).abs() < 1e-12);
    }

    #[test]
    fn binning_reduces_points_and_keeps_axis_and_metadata_consistent() {
        let mut s = spec_from((0..100).map(|i| i as f64).collect());
        bin(
            &mut s,
            BinParams {
                width: 0.05,
                method: BinMethod::Mean,
            },
        );
        assert_eq!(s.values.len(), 20);
        assert_eq!(s.ppm.len(), 20);
        assert!((s.values[0].re - 2.0).abs() < 1e-12);
        assert!((s.values[1].re - 7.0).abs() < 1e-12);
        assert!((s.ppm[0] - 0.02).abs() < 1e-12);
        assert!((s.hz_per_point - 5.0).abs() < 1e-12);

        let mut s = spec_from(vec![1.0; 10]);
        bin(
            &mut s,
            BinParams {
                width: 0.04,
                method: BinMethod::Sum,
            },
        );
        assert_eq!(s.values.len(), 3);
        assert!((s.values[0].re - 4.0).abs() < 1e-12);
        assert!((s.values[2].re - 2.0).abs() < 1e-12);
    }

    #[test]
    fn reverse_mirrors_intensities_and_keeps_the_axis() {
        let mut s = spec_from(vec![1.0, 2.0, 3.0]);
        let ppm = s.ppm.clone();
        reverse(&mut s);
        assert_eq!(s.ppm, ppm);
        assert!((s.values[0].re - 3.0).abs() < 1e-12);
        reverse(&mut s);
        assert!((s.values[0].re - 1.0).abs() < 1e-12);
    }

    #[test]
    fn invert_negates_intensities() {
        let mut s = spec_from(vec![1.0, -2.0]);
        invert(&mut s);
        assert!((s.values[0].re + 1.0).abs() < 1e-12);
        assert!((s.values[1].re - 2.0).abs() < 1e-12);
    }
}
