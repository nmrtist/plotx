//! Automatic phase determination beyond the single tallest-peak rule.

use crate::AutoPhaseMethod;
use crate::phase;
use num_complex::Complex64;
use plotx_analysis::peaks::{DetectParams, detect_peaks, estimate_noise};
use std::f64::consts::{PI, TAU};

/// Cap on the point count fed to the iterative optimizers.
const SEARCH_POINTS: usize = 1024;

/// Weight on the negative-intensity penalty in the ACME entropy cost, applied to
/// a spectrum normalized to unit peak magnitude. Large enough to break the 180°
/// sign ambiguity the derivative entropy cannot see, following Chen et al.
const ACME_PENALTY: f64 = 1000.0;

pub fn compute(values: &[Complex64], method: AutoPhaseMethod) -> (f64, f64, f64) {
    match method {
        AutoPhaseMethod::RobustConsensus => robust_consensus(values),
        AutoPhaseMethod::AbsorptivePeak => absorptive_peak(values),
        AutoPhaseMethod::Entropy => optimized(values, acme_cost),
        AutoPhaseMethod::NegativeMinimization => optimized(values, negative_cost),
        AutoPhaseMethod::PeakRegression => peak_regression(values),
    }
}

/// Build candidates with deliberately different failure modes and select among
/// them using a scale-independent objective. The winning candidate is refined
/// once more against that common objective. Testing each candidate's pi-shifted
/// counterpart makes the sign decision explicit.
fn robust_consensus(values: &[Complex64]) -> (f64, f64, f64) {
    let (mut dec, frac) = decimate(values, SEARCH_POINTS);
    if dec.len() < 4 {
        return absorptive_peak(values);
    }
    let scale = dec.iter().map(|c| c.norm()).fold(0.0_f64, f64::max);
    if !scale.is_finite() || scale <= f64::MIN_POSITIVE {
        return (0.0, 0.0, phase::peak_pivot_frac(values));
    }
    for value in &mut dec {
        *value /= scale;
    }
    let strategies = [
        absorptive_peak(values),
        optimized(values, acme_cost),
        optimized(values, negative_cost),
        peak_regression(values),
    ];
    // Every candidate is judged with its zero-order phase pinned so the tallest
    // peak is absorptive, leaving `p1` (the ramp) as the only free variable. This
    // resolves the isolated-peak case — where the dominant bin stays positive at
    // any orientation, so entropy and negative power alone are degenerate — while
    // still letting negative power rank the ramp on overlapping spectra.
    let objective = |p1: f64| consensus_cost(&dec, &frac, snap_zero_order_to_peak(values, p1), p1);
    let mut best_p1 = 0.0;
    let mut best_cost = f64::INFINITY;
    for (_, phase1, _) in strategies {
        let cost = objective(phase1);
        if cost < best_cost {
            best_p1 = phase1;
            best_cost = cost;
        }
    }
    let p1 = pattern_search_1d(&objective, best_p1);
    to_pivoted(values, snap_zero_order_to_peak(values, p1), p1)
}

/// Zero-order phase that lands the tallest bin on the positive real axis given
/// the ramp `p1`. Referencing the consensus ramp to the peak's own argument keeps
/// a clean isolated peak exactly absorptive, which the global objective — flat to
/// a degree or so near its optimum — cannot pin on its own.
fn snap_zero_order_to_peak(values: &[Complex64], p1: f64) -> f64 {
    let Some((index, peak)) = values
        .iter()
        .enumerate()
        .max_by(|a, b| a.1.norm().total_cmp(&b.1.norm()))
    else {
        return 0.0;
    };
    if peak.norm() <= f64::MIN_POSITIVE {
        return 0.0;
    }
    let peak_frac = index as f64 / (values.len() - 1).max(1) as f64;
    peak.arg() - p1 * peak_frac
}

/// Normalized derivative entropy rewards sharp absorptive lines while negative
/// power resolves the remaining sign and ramp ambiguity. With the caller pinning
/// the tallest peak absorptive, negative power is what separates a correct ramp
/// (every peak upright) from a wrong one. Both terms are dimensionless, so ranking
/// is invariant under spectrum intensity scaling.
fn consensus_cost(values: &[Complex64], frac: &[f64], p0: f64, p1: f64) -> f64 {
    let real = phased_real(values, frac, p0, p1);
    let derivatives: Vec<f64> = real.windows(2).map(|w| (w[1] - w[0]).abs()).collect();
    let derivative_sum: f64 = derivatives.iter().sum();
    if derivative_sum <= f64::MIN_POSITIVE {
        return f64::INFINITY;
    }
    let entropy = derivatives.iter().fold(0.0, |acc, derivative| {
        let probability = derivative / derivative_sum;
        if probability > 0.0 {
            acc - probability * probability.ln()
        } else {
            acc
        }
    });
    let max_entropy = (derivatives.len().max(2) as f64).ln();
    entropy / max_entropy + 4.0 * negative_cost(values, frac, p0, p1)
}

/// Zeroth-order only: rotate the tallest peak onto the positive real axis.
fn absorptive_peak(values: &[Complex64]) -> (f64, f64, f64) {
    let pivot = phase::peak_pivot_frac(values);
    let peak = values
        .iter()
        .max_by(|a, b| a.norm().total_cmp(&b.norm()))
        .copied()
        .unwrap_or(Complex64::new(0.0, 0.0));
    (peak.arg(), 0.0, pivot)
}

/// Grid-seeded pattern search over `(phase0, phase1)` on a decimated, unit-peak
/// spectrum, minimizing `cost`. Falls back to the zero-order rule when there are
/// too few points to fit a ramp.
fn optimized(
    values: &[Complex64],
    cost: fn(&[Complex64], &[f64], f64, f64) -> f64,
) -> (f64, f64, f64) {
    let (mut dec, frac) = decimate(values, SEARCH_POINTS);
    if dec.len() < 4 {
        return absorptive_peak(values);
    }
    let m = dec.iter().map(|c| c.norm()).fold(0.0_f64, f64::max);
    if m <= 0.0 {
        return (0.0, 0.0, phase::peak_pivot_frac(values));
    }
    for c in &mut dec {
        *c /= m;
    }
    let obj = |p0: f64, p1: f64| cost(&dec, &frac, p0, p1);
    let (p0, p1) = coarse_grid(&obj);
    let (p0, p1) = pattern_search(&obj, p0, p1);
    to_pivoted(values, p0, p1)
}

/// Detect peaks on the magnitude spectrum, read each one's dispersive angle, and
/// least-squares fit a phase ramp `arg = phase0 + phase1·frac` through them
/// (weighted by peak height). The classic multi-peak linear phasing; needs at
/// least two resolved peaks, else it defers to the zero-order rule.
fn peak_regression(values: &[Complex64]) -> (f64, f64, f64) {
    let n = values.len();
    if n < 3 {
        return absorptive_peak(values);
    }
    let mag: Vec<f64> = values.iter().map(|c| c.norm()).collect();
    let xs: Vec<f64> = (0..n).map(|i| i as f64).collect();
    let sigma = estimate_noise(&mag);
    let params = DetectParams {
        min_height: Some(6.0 * sigma),
        min_prominence: 5.0 * sigma,
        min_spacing: None,
        max_count: Some(32),
    };
    let mut peaks = detect_peaks(&xs, &mag, &params);
    if peaks.len() < 2 {
        return absorptive_peak(values);
    }
    peaks.sort_by_key(|a| a.index);

    let denom = (n - 1).max(1) as f64;
    // Unwrap successive peak angles so a ramp within ±π per gap fits cleanly.
    let mut angles = Vec::with_capacity(peaks.len());
    let mut prev = 0.0;
    for (k, p) in peaks.iter().enumerate() {
        let raw = values[p.index].arg();
        let a = if k == 0 {
            raw
        } else {
            prev + wrap_to_pi(raw - prev)
        };
        angles.push(a);
        prev = a;
    }

    let (mut sw, mut swx, mut swy, mut swxx, mut swxy) = (0.0, 0.0, 0.0, 0.0, 0.0);
    for (p, &y) in peaks.iter().zip(&angles) {
        let w = p.y;
        let x = p.index as f64 / denom;
        sw += w;
        swx += w * x;
        swy += w * y;
        swxx += w * x * x;
        swxy += w * x * y;
    }
    let det = sw * swxx - swx * swx;
    if det.abs() <= f64::MIN_POSITIVE {
        return absorptive_peak(values);
    }
    let p0 = (swxx * swy - swx * swxy) / det;
    let p1 = (sw * swxy - swx * swy) / det;
    to_pivoted(values, p0, p1)
}

/// ACME (Chen et al. 2002): Shannon entropy of the normalized absolute first
/// derivative of the real spectrum, plus a penalty for negative intensity.
fn acme_cost(values: &[Complex64], frac: &[f64], p0: f64, p1: f64) -> f64 {
    let re: Vec<f64> = phased_real(values, frac, p0, p1);
    let mut deriv: Vec<f64> = re.windows(2).map(|w| (w[1] - w[0]).abs()).collect();
    let sum: f64 = deriv.iter().sum();
    if sum <= 0.0 {
        return f64::INFINITY;
    }
    let mut entropy = 0.0;
    for d in &mut deriv {
        let p = *d / sum;
        if p > 0.0 {
            entropy -= p * p.ln();
        }
    }
    let penalty: f64 = re.iter().filter(|&&y| y < 0.0).map(|y| y * y).sum();
    entropy + ACME_PENALTY * penalty
}

/// Fraction of the real spectrum's power carried by its negative parts; zero when
/// every point is non-negative (a purely absorptive, upright spectrum).
fn negative_cost(values: &[Complex64], frac: &[f64], p0: f64, p1: f64) -> f64 {
    let mut neg = 0.0;
    let mut total = 0.0;
    for (c, &fr) in values.iter().zip(frac) {
        let (s, co) = (p0 + p1 * fr).sin_cos();
        let r = c.re * co + c.im * s;
        total += r * r;
        if r < 0.0 {
            neg += r * r;
        }
    }
    if total <= 0.0 {
        f64::INFINITY
    } else {
        neg / total
    }
}

fn phased_real(values: &[Complex64], frac: &[f64], p0: f64, p1: f64) -> Vec<f64> {
    values
        .iter()
        .zip(frac)
        .map(|(c, &fr)| {
            let (s, co) = (p0 + p1 * fr).sin_cos();
            c.re * co + c.im * s
        })
        .collect()
}

/// Coarse scan over `phase0 ∈ [-π, π)` and `phase1 ∈ [-2π, 2π]` for a robust
/// starting point that dodges the local minima the refinement would fall into.
fn coarse_grid(obj: &impl Fn(f64, f64) -> f64) -> (f64, f64) {
    const N0: usize = 48;
    const N1: usize = 25;
    let mut best = (0.0, 0.0);
    let mut best_cost = f64::INFINITY;
    for i in 0..N0 {
        let p0 = -PI + TAU * i as f64 / N0 as f64;
        for j in 0..N1 {
            let p1 = -2.0 * PI + 4.0 * PI * j as f64 / (N1 - 1) as f64;
            let c = obj(p0, p1);
            if c < best_cost {
                best_cost = c;
                best = (p0, p1);
            }
        }
    }
    best
}

/// Hooke–Jeeves pattern search: probe ±step on each axis, step toward any
/// improvement, halve the step when stuck. Refines the grid seed to < 0.01°.
fn pattern_search(obj: &impl Fn(f64, f64) -> f64, mut p0: f64, mut p1: f64) -> (f64, f64) {
    let mut step = PI / 18.0;
    let mut best = obj(p0, p1);
    for _ in 0..80 {
        let mut improved = false;
        for &(d0, d1) in &[(step, 0.0), (-step, 0.0), (0.0, step), (0.0, -step)] {
            let c = obj(p0 + d0, p1 + d1);
            if c < best {
                best = c;
                p0 += d0;
                p1 += d1;
                improved = true;
            }
        }
        if !improved {
            step *= 0.5;
            if step < 1e-4 {
                break;
            }
        }
    }
    (p0, p1)
}

/// One-dimensional Hooke–Jeeves search over the ramp `p1` alone, used when the
/// zero-order phase is a pinned function of `p1`. Same probe-and-halve schedule.
fn pattern_search_1d(obj: &impl Fn(f64) -> f64, mut p1: f64) -> f64 {
    let mut step = PI / 18.0;
    let mut best = obj(p1);
    for _ in 0..80 {
        let mut improved = false;
        for &delta in &[step, -step] {
            let cost = obj(p1 + delta);
            if cost < best {
                best = cost;
                p1 += delta;
                improved = true;
            }
        }
        if !improved {
            step *= 0.5;
            if step < 1e-4 {
                break;
            }
        }
    }
    p1
}

/// Re-express a pivot-at-origin phase `φ(frac) = p0 + p1·frac` about the tallest
/// peak, so the returned pivot matches the on-plot handle of the other methods.
fn to_pivoted(values: &[Complex64], p0: f64, p1: f64) -> (f64, f64, f64) {
    let pivot = phase::peak_pivot_frac(values);
    (p0 + p1 * pivot, p1, pivot)
}

/// Downsample to at most `max` points by max-magnitude pooling: each stride-wide
/// block contributes its tallest point. Plain stride sampling would step over the
/// narrow peaks of a large spectrum (a 160k-point spectrum decimates with stride
/// 160) and feed the optimizer mostly noise, so phasing would minimize the entropy
/// of noise. Keeping each block's peak preserves the lineshape the cost functions
/// need while holding the working length bounded.
fn decimate(values: &[Complex64], max: usize) -> (Vec<Complex64>, Vec<f64>) {
    let n = values.len();
    if n == 0 {
        return (Vec::new(), Vec::new());
    }
    let denom = (n - 1).max(1) as f64;
    let stride = n.div_ceil(max).max(1);
    let mut vals = Vec::new();
    let mut fracs = Vec::new();
    let mut i = 0;
    while i < n {
        let end = (i + stride).min(n);
        let j = (i..end)
            .max_by(|&a, &b| values[a].norm().total_cmp(&values[b].norm()))
            .unwrap_or(i);
        vals.push(values[j]);
        fracs.push(j as f64 / denom);
        i = end;
    }
    (vals, fracs)
}

fn wrap_to_pi(mut a: f64) -> f64 {
    while a > PI {
        a -= TAU;
    }
    while a < -PI {
        a += TAU;
    }
    a
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lorentzian(n: usize, center: usize, width: f64) -> Vec<Complex64> {
        (0..n)
            .map(|i| {
                let d = (i as f64 - center as f64) / width;
                // Absorption + i·dispersion of a Lorentzian.
                Complex64::new(1.0 / (1.0 + d * d), -d / (1.0 + d * d))
            })
            .collect()
    }

    fn scramble(values: &[Complex64], p0: f64, p1: f64) -> Vec<Complex64> {
        let denom = (values.len() - 1).max(1) as f64;
        values
            .iter()
            .enumerate()
            .map(|(i, c)| {
                let phi = p0 + p1 * (i as f64 / denom);
                c * Complex64::from_polar(1.0, phi)
            })
            .collect()
    }

    fn apply(values: &[Complex64], p: (f64, f64, f64)) -> Vec<f64> {
        let denom = (values.len() - 1).max(1) as f64;
        values
            .iter()
            .enumerate()
            .map(|(i, c)| {
                let phi = p.0 + p.1 * (i as f64 / denom - p.2);
                (c * Complex64::from_polar(1.0, -phi)).re
            })
            .collect()
    }

    fn upright(re: &[f64]) -> bool {
        let (imin, &min) = re
            .iter()
            .enumerate()
            .min_by(|a, b| a.1.total_cmp(b.1))
            .unwrap();
        let max = re.iter().cloned().fold(f64::MIN, f64::max);
        // Absorptive peak dominates; no deep negative lobe.
        max > 0.5 && min > -0.15 * max && imin != re.len() / 2
    }

    #[test]
    fn entropy_recovers_scrambled_phase() {
        let clean = lorentzian(512, 200, 4.0);
        let bad = scramble(&clean, 1.1, 0.7);
        let p = compute(&bad, AutoPhaseMethod::Entropy);
        assert!(upright(&apply(&bad, p)));
    }

    #[test]
    fn negative_minimization_recovers_scrambled_phase() {
        let clean = lorentzian(512, 300, 5.0);
        let bad = scramble(&clean, -0.9, 0.5);
        let p = compute(&bad, AutoPhaseMethod::NegativeMinimization);
        assert!(upright(&apply(&bad, p)));
    }

    #[test]
    fn peak_regression_fits_two_peaks() {
        let mut clean = lorentzian(1024, 250, 4.0);
        for (i, c) in lorentzian(1024, 750, 4.0).into_iter().enumerate() {
            clean[i] += c;
        }
        let bad = scramble(&clean, 0.4, 1.2);
        let p = compute(&bad, AutoPhaseMethod::PeakRegression);
        assert!(upright(&apply(&bad, p)));
    }

    #[test]
    fn methods_are_stable_on_degenerate_input() {
        for m in [
            AutoPhaseMethod::RobustConsensus,
            AutoPhaseMethod::Entropy,
            AutoPhaseMethod::NegativeMinimization,
            AutoPhaseMethod::PeakRegression,
        ] {
            let (p0, p1, piv) = compute(&[], m);
            assert!(p0.is_finite() && p1.is_finite() && piv.is_finite());
        }
    }

    #[test]
    fn robust_consensus_handles_scaled_overlapping_peaks() {
        let mut clean = lorentzian(768, 330, 6.0);
        for (i, value) in lorentzian(768, 342, 8.0).into_iter().enumerate() {
            clean[i] += value * 0.65;
        }
        let bad: Vec<_> = scramble(&clean, -1.2, 1.4)
            .into_iter()
            .map(|value| value * 2.5e5)
            .collect();
        let p = compute(&bad, AutoPhaseMethod::RobustConsensus);
        let corrected = apply(&bad, p);
        let max = corrected.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        let min = corrected.iter().copied().fold(f64::INFINITY, f64::min);
        assert!(max > 1.0e5);
        assert!(min > -0.2 * max, "negative residual {min} vs peak {max}");
    }
}
