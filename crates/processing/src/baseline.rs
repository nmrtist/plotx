use crate::{BaselineMethod, Spectrum};

/// Subtract a baseline from the real channel in place, per `method`.
pub fn apply(spec: &mut Spectrum, method: BaselineMethod) {
    match method {
        BaselineMethod::Offset => correct_offset(spec),
        BaselineMethod::Polynomial { order } => subtract_polynomial(spec, order as usize),
        BaselineMethod::AsymmetricLeastSquares {
            smoothness,
            asymmetry,
            iterations,
        } => subtract_asymmetric_least_squares(spec, smoothness, asymmetry, iterations as usize),
    }
}

/// Estimate a smooth baseline with Eilers' asymmetric least-squares method and
/// subtract it from the real channel. Peaks receive the small asymmetric weight,
/// while points at or below the estimate anchor the baseline. The linear system
/// is symmetric positive definite and pentadiagonal, so each iteration is O(n).
fn subtract_asymmetric_least_squares(
    spec: &mut Spectrum,
    smoothness: f64,
    asymmetry: f64,
    iterations: usize,
) {
    let n = spec.values.len();
    if n < 3 {
        correct_offset(spec);
        return;
    }
    let y = spec.real();
    let lambda = smoothness.clamp(1.0, 1.0e12);
    let p = asymmetry.clamp(1.0e-6, 0.5);
    let mut weights = vec![1.0; n];
    let mut baseline = vec![0.0; n];
    for _ in 0..iterations.clamp(1, 100) {
        let main_penalty = |i: usize| match i {
            0 if n == 3 => 1.0,
            1 if n == 3 => 4.0,
            2 if n == 3 => 1.0,
            0 => 1.0,
            1 => 5.0,
            i if i + 2 == n => 5.0,
            i if i + 1 == n => 1.0,
            _ => 6.0,
        };
        let main: Vec<f64> = weights
            .iter()
            .enumerate()
            .map(|(i, weight)| weight + lambda * main_penalty(i))
            .collect();
        let first: Vec<f64> = (0..n - 1)
            .map(|i| {
                if i == 0 || i + 2 == n {
                    -2.0 * lambda
                } else {
                    -4.0 * lambda
                }
            })
            .collect();
        let second = vec![lambda; n - 2];
        let rhs: Vec<f64> = weights.iter().zip(&y).map(|(w, value)| w * value).collect();
        let Some(solution) = solve_symmetric_pentadiagonal(&main, &first, &second, &rhs) else {
            correct_offset(spec);
            return;
        };
        baseline = solution;
        for i in 0..n {
            weights[i] = if y[i] > baseline[i] { p } else { 1.0 - p };
        }
    }
    for (value, base) in spec.values.iter_mut().zip(baseline) {
        value.re -= base;
    }
}

/// Banded Cholesky solve for a symmetric positive-definite matrix with two
/// populated off-diagonals. `first[i]` is A[i,i+1], `second[i]` is A[i,i+2].
fn solve_symmetric_pentadiagonal(
    main: &[f64],
    first: &[f64],
    second: &[f64],
    rhs: &[f64],
) -> Option<Vec<f64>> {
    let n = main.len();
    if rhs.len() != n || first.len() + 1 != n || second.len() + 2 != n {
        return None;
    }
    let mut diagonal = vec![0.0; n];
    let mut lower1 = vec![0.0; n];
    let mut lower2 = vec![0.0; n];
    for i in 0..n {
        if i >= 2 {
            lower2[i] = second[i - 2] / diagonal[i - 2];
        }
        if i >= 1 {
            let cross = if i >= 2 {
                lower2[i] * lower1[i - 1]
            } else {
                0.0
            };
            lower1[i] = (first[i - 1] - cross) / diagonal[i - 1];
        }
        let remainder = main[i] - lower1[i] * lower1[i] - lower2[i] * lower2[i];
        if !remainder.is_finite() || remainder <= f64::MIN_POSITIVE {
            return None;
        }
        diagonal[i] = remainder.sqrt();
    }
    let mut solution = vec![0.0; n];
    for i in 0..n {
        let mut value = rhs[i];
        if i >= 1 {
            value -= lower1[i] * solution[i - 1];
        }
        if i >= 2 {
            value -= lower2[i] * solution[i - 2];
        }
        solution[i] = value / diagonal[i];
    }
    for i in (0..n).rev() {
        let mut value = solution[i];
        if i + 1 < n {
            value -= lower1[i + 1] * solution[i + 1];
        }
        if i + 2 < n {
            value -= lower2[i + 2] * solution[i + 2];
        }
        solution[i] = value / diagonal[i];
    }
    Some(solution)
}

/// Subtract a constant offset, estimated from the quietest region of the
/// spectrum, from the real channel in place.
pub fn correct_offset(spec: &mut Spectrum) {
    if spec.values.is_empty() {
        return;
    }
    let offset = estimate_offset(&spec.real());
    for c in &mut spec.values {
        c.re -= offset;
    }
}

// Fit a polynomial to the low-lying points of the real channel and subtract it,
// so a rolling or sloped baseline is flattened without peaks (which ride above
// the baseline) dragging the fit up. The fit index is mapped to `[-1, 1]` to
// keep the normal equations well-conditioned for higher orders.
fn subtract_polynomial(spec: &mut Spectrum, order: usize) {
    let n = spec.values.len();
    let m = order + 1;
    if n <= m {
        correct_offset(spec);
        return;
    }
    let real = spec.real();
    let mut sorted = real.clone();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let threshold = sorted[n / 2];

    let t_of = |i: usize| 2.0 * i as f64 / (n - 1) as f64 - 1.0;
    let mut ata = vec![vec![0.0; m]; m];
    let mut atb = vec![0.0; m];
    let mut anchors = 0usize;
    let mut powers = vec![0.0; m];
    for (i, &value) in real.iter().enumerate() {
        if value > threshold {
            continue;
        }
        let t = t_of(i);
        powers[0] = 1.0;
        for k in 1..m {
            powers[k] = powers[k - 1] * t;
        }
        for a in 0..m {
            atb[a] += powers[a] * value;
            for b in 0..m {
                ata[a][b] += powers[a] * powers[b];
            }
        }
        anchors += 1;
    }
    if anchors < m {
        correct_offset(spec);
        return;
    }
    let coeffs = match plotx_analysis::fit::solve_linear(&ata, &atb) {
        Some(c) => c,
        None => {
            correct_offset(spec);
            return;
        }
    };
    for i in 0..n {
        let t = t_of(i);
        let mut tp = 1.0;
        let mut base = 0.0;
        for &c in &coeffs {
            base += c * tp;
            tp *= t;
        }
        spec.values[i].re -= base;
    }
}

fn estimate_offset(real: &[f64]) -> f64 {
    if real.is_empty() {
        return 0.0;
    }
    let mut sorted: Vec<f64> = real.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    // Robust centre + scale of the whole channel: with sparse (positive) peaks the
    // median sits on the baseline noise, and MAD/0.6745 estimates its σ.
    let median = median_sorted(&sorted);
    let mut dev: Vec<f64> = sorted.iter().map(|&v| (v - median).abs()).collect();
    dev.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let sigma = median_sorted(&dev) / 0.674_489_75;
    if sigma <= f64::MIN_POSITIVE {
        return median;
    }
    // Average the points inside a ±3σ band about the median: peaks are excluded and
    // the retained noise is symmetric, so the mean is an unbiased estimate of the
    // baseline centre — unlike the lowest decile, whose median sat ~1.6σ too low.
    let (lo, hi) = (median - 3.0 * sigma, median + 3.0 * sigma);
    let (mut sum, mut count) = (0.0, 0usize);
    for &v in &sorted {
        if v >= lo && v <= hi {
            sum += v;
            count += 1;
        }
    }
    if count == 0 {
        median
    } else {
        sum / count as f64
    }
}

fn median_sorted(sorted: &[f64]) -> f64 {
    let n = sorted.len();
    if n == 0 {
        0.0
    } else if n % 2 == 1 {
        sorted[n / 2]
    } else {
        0.5 * (sorted[n / 2 - 1] + sorted[n / 2])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use num_complex::Complex64;

    #[test]
    fn removes_constant_offset() {
        let mut values: Vec<Complex64> = (0..100).map(|_| Complex64::new(5.0, 0.0)).collect();
        values[50] = Complex64::new(105.0, 0.0);
        let mut s = Spectrum {
            ppm: (0..100).map(|i| i as f64).collect(),
            values,
            hz_per_point: 1.0,
            observe_freq_mhz: 400.0,
            nucleus: "1H".into(),
        };
        correct_offset(&mut s);
        assert!(s.values[0].re.abs() < 1e-9);
        assert!((s.values[50].re - 100.0).abs() < 1e-9);
    }

    #[test]
    fn offset_estimate_is_unbiased_for_noisy_baseline() {
        let offset = 10.0;
        let real: Vec<f64> = (0..400)
            .map(|i| {
                let noise = (i as f64 * 0.7).sin() * 2.0;
                let peak = if i % 137 == 0 { 100.0 } else { 0.0 };
                offset + noise + peak
            })
            .collect();
        let est = estimate_offset(&real);
        assert!(
            (est - offset).abs() < 0.3,
            "offset estimate {est} vs {offset}"
        );
    }

    #[test]
    fn polynomial_flattens_a_sloped_baseline() {
        let n = 200;
        let values: Vec<Complex64> = (0..n)
            .map(|i| {
                let ramp = 3.0 + 0.05 * i as f64;
                let peak = if i == 150 { 500.0 } else { 0.0 };
                Complex64::new(ramp + peak, 0.0)
            })
            .collect();
        let mut s = Spectrum {
            ppm: (0..n).map(|i| i as f64).collect(),
            values,
            hz_per_point: 1.0,
            observe_freq_mhz: 400.0,
            nucleus: "1H".into(),
        };
        apply(&mut s, BaselineMethod::Polynomial { order: 1 });
        for i in 0..n {
            if i == 150 {
                continue;
            }
            assert!(
                s.values[i].re.abs() < 1e-6,
                "baseline at {i} = {}",
                s.values[i].re
            );
        }
        assert!((s.values[150].re - 500.0).abs() < 1e-6);
    }

    #[test]
    fn asymmetric_least_squares_removes_curved_baseline_without_erasing_peaks() {
        let n = 600;
        let values: Vec<Complex64> = (0..n)
            .map(|i| {
                let x = 2.0 * i as f64 / (n - 1) as f64 - 1.0;
                let baseline = 8.0 + 4.0 * x + 7.0 * x * x;
                let peak = 120.0 * (-((i as f64 - 190.0) / 8.0).powi(2)).exp()
                    + 75.0 * (-((i as f64 - 430.0) / 13.0).powi(2)).exp();
                Complex64::new(baseline + peak, 0.0)
            })
            .collect();
        let mut spectrum = Spectrum {
            ppm: (0..n).map(|i| i as f64).collect(),
            values,
            hz_per_point: 1.0,
            observe_freq_mhz: 400.0,
            nucleus: "1H".into(),
        };
        // This baseline is deliberately steep, so it exercises the solver with a
        // smoothness looser than the `AUTO` preset — which is tuned for the gentle
        // baselines and broad peaks covered by the crate's quality-contract tests.
        apply(
            &mut spectrum,
            BaselineMethod::AsymmetricLeastSquares {
                smoothness: 1.0e4,
                asymmetry: 0.001,
                iterations: 20,
            },
        );
        assert!(spectrum.values[20].re.abs() < 1.0);
        assert!(spectrum.values[570].re.abs() < 1.0);
        assert!(spectrum.values[190].re > 100.0);
        assert!(spectrum.values[430].re > 60.0);
    }
}
