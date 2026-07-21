//! Pseudo-2D analysis: turn an arrayed direct-dimension stack (DOSY gradients,
//! T1/T2 delays) into fitted diffusion coefficients or relaxation times. The
//! indirect axis is a parameter ruler, not a frequency, so nothing is Fourier
//! transformed here — we extract an intensity-vs-ruler decay and fit a model.

use crate::SpectrumStack;
use crate::fit::{
    levenberg_marquardt, levenberg_marquardt_cancellable, param_sigma, r_squared, sum_sq_residual,
    weighted_log_line,
};
use plotx_io::DiffusionMeta;

/// One intensity value per increment against its ruler value (gradient in T/m,
/// delay in s). The independent variable `x` is in SI units.
#[derive(Debug, Clone)]
pub struct DecaySeries {
    pub x: Vec<f64>,
    pub y: Vec<f64>,
}

impl DecaySeries {
    #[inline]
    pub fn len(&self) -> usize {
        self.x.len().min(self.y.len())
    }
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// How a ppm window collapses to one intensity per increment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IntensityMode {
    /// Magnitude at the single ppm index where the reference trace peaks.
    PeakHeight,
    /// Summed magnitude across the window (∝ integrated area).
    Integral,
}

/// Map a ppm `range` to a half-open index window `[start, end)`. `None` means
/// "no selection" → the whole spectrum. A `Some(range)` overlapping the spectrum
/// returns the covered indices; a `Some(range)` lying entirely outside returns
/// `None` (selected but empty), so callers refuse rather than silently reducing
/// over the whole spectrum.
pub(crate) fn window_indices(ppm: &[f64], range: Option<(f64, f64)>) -> Option<(usize, usize)> {
    match range {
        None => Some((0, ppm.len())),
        Some((a, b)) => {
            let (lo, hi) = if a <= b { (a, b) } else { (b, a) };
            let mut start = ppm.len();
            let mut end = 0;
            for (i, &p) in ppm.iter().enumerate() {
                if p >= lo && p <= hi {
                    start = start.min(i);
                    end = end.max(i + 1);
                }
            }
            (start < end).then_some((start, end))
        }
    }
}

/// Extract an intensity-vs-ruler decay from a pseudo-2D stack over a ppm window.
/// `axis_values` is the indirect ruler (one SI value per increment).
///
/// Intensities are **signed** (real, absorptive part), recovered by projecting
/// each increment's complex spectrum onto the phase direction of the strongest
/// (reference) increment column-by-column:
///
/// ```text
/// S_i(c) = I_i(c)·e^{iφ(c)},   y_i(c) = Re(S_i(c)·conj(S_ref(c))) / |S_ref(c)| = I_i(c)
/// ```
///
/// Because the acquisition phase `φ(c)` is constant across increments, this
/// cancels arbitrary zero- and first-order phase and returns the signed
/// intensity `I_i(c)` — negative near τ→0 for inversion recovery, which is what
/// the relaxation fit needs. It reduces to the (positive) absorptive height for
/// diffusion/T2 decays, where every increment shares the reference's sign.
/// For `PeakHeight` the peak column is fixed from the reference increment so the
/// same resonance is tracked across the array.
pub fn extract_series(
    stack: &impl SpectrumStack,
    axis_values: &[f64],
    ppm_range: Option<(f64, f64)>,
    mode: IntensityMode,
) -> DecaySeries {
    let coordinates = stack.coordinates();
    let traces = stack.traces();
    let n = stack.increments().min(axis_values.len());
    if n == 0 || coordinates.is_empty() {
        return DecaySeries {
            x: Vec::new(),
            y: Vec::new(),
        };
    }
    let Some((start, end)) = window_indices(coordinates, ppm_range) else {
        return DecaySeries {
            x: Vec::new(),
            y: Vec::new(),
        };
    };
    let x = axis_values[..n].to_vec();

    // Reference increment: the one carrying the most signal in the window. For an
    // inversion-recovery array this is the fully-recovered (longest-τ) trace,
    // whose peaks are absorptive and positive — an ideal phase reference.
    let window_energy = |i: usize| {
        traces[i][start..end]
            .iter()
            .map(|c| c.norm_sqr())
            .sum::<f64>()
    };
    let strongest = (0..n)
        .max_by(|&a, &b| {
            window_energy(a)
                .partial_cmp(&window_energy(b))
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .unwrap_or(0);

    // Project S_i(c) onto the reference column's phasor: Re(S_i·conj(S_ref))/|S_ref|.
    let project = |i: usize, c: usize| {
        let r = traces[strongest][c];
        let m = r.norm();
        if m <= f64::MIN_POSITIVE {
            0.0
        } else {
            (traces[i][c] * r.conj()).re / m
        }
    };

    let mut y: Vec<f64> = match mode {
        IntensityMode::Integral => (0..n)
            .map(|i| (start..end).map(|c| project(i, c)).sum())
            .collect(),
        IntensityMode::PeakHeight => {
            let peak_col = (start..end)
                .max_by(|&a, &b| {
                    traces[strongest][a]
                        .norm()
                        .partial_cmp(&traces[strongest][b].norm())
                        .unwrap_or(std::cmp::Ordering::Equal)
                })
                .unwrap_or(start);
            (0..n).map(|i| project(i, peak_col)).collect()
        }
    };

    // Orient for natural display: arrays are stored in ascending-ruler order, so
    // the high-ruler tail is the recovered plateau (relaxation) or the residual
    // signal (diffusion) — positive by convention. If the phase reference landed
    // on a fully-inverted increment the whole series comes out flipped; the sign
    // is irrelevant to the fit but an upside-down inversion-recovery curve reads
    // wrong. Flip on the sign of the tail mean, which avoids the noisy null.
    let tail = (n - n.div_ceil(3)).min(n.saturating_sub(1));
    let tail_mean = y[tail..].iter().sum::<f64>() / (n - tail).max(1) as f64;
    if tail_mean < 0.0 {
        for v in &mut y {
            *v = -*v;
        }
    }
    DecaySeries { x, y }
}

/// Result of a Stejskal–Tanner diffusion fit: I(g) = I0·exp(−D·b(g)).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DiffusionFit {
    /// Diffusion coefficient, m²·s⁻¹.
    pub d: f64,
    pub i0: f64,
    pub r2: f64,
    /// 1σ standard error on `d`.
    pub sigma_d: f64,
}

/// Fit a monoexponential diffusion decay. The ruler `series.x` is gradient
/// strength (T/m); `meta` converts each to a b-factor. Returns `None` if the
/// data is degenerate.
pub fn fit_diffusion(series: &DecaySeries, meta: &DiffusionMeta) -> Option<DiffusionFit> {
    fit_diffusion_cancellable(series, meta, &|| false)
}

pub fn fit_diffusion_cancellable(
    series: &DecaySeries,
    meta: &DiffusionMeta,
    cancelled: &impl Fn() -> bool,
) -> Option<DiffusionFit> {
    let n = series.len();
    if n < 3 {
        return None;
    }
    let b: Vec<f64> = series.x[..n].iter().map(|&g| meta.b_factor(g)).collect();
    let y: Vec<f64> = series.y[..n].to_vec();

    // Log-linear initial guess: ln I = ln I0 − D·b, weighted by I² so bright
    // points (small log noise) dominate.
    let (ln_i0, slope) = weighted_log_line(&b, &y)?;
    let mut d = -slope;
    let mut i0 = ln_i0.exp();

    // Refine on the true nonlinear residual.
    let model = |p: &[f64], x: f64| p[0] * (-p[1] * x).exp();
    if let Some(p) = levenberg_marquardt_cancellable(&b, &y, &[i0, d], model, 60, cancelled) {
        i0 = p[0];
        d = p[1];
    }
    if cancelled() {
        return None;
    }
    if !d.is_finite() || d <= 0.0 || !i0.is_finite() {
        return None;
    }

    let pred = |x: f64| model(&[i0, d], x);
    let r2 = r_squared(&b, &y, pred);
    let sigma_d = param_sigma(&b, &y, &[i0, d], model, 1);
    Some(DiffusionFit { d, i0, r2, sigma_d })
}

/// Relaxation model for a delay array.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RelaxModel {
    /// I = a + b·exp(−τ/T1), b ≈ −2a for a perfect inversion.
    InversionRecovery,
    /// I = a·(1 − exp(−τ/T1)); modelled with the same 3-parameter form.
    SaturationRecovery,
    /// I = a·exp(−τ/T2).
    T2Decay,
}

impl RelaxModel {
    pub fn all() -> &'static [Self] {
        &[
            RelaxModel::InversionRecovery,
            RelaxModel::SaturationRecovery,
            RelaxModel::T2Decay,
        ]
    }
    pub fn label(self) -> &'static str {
        match self {
            RelaxModel::InversionRecovery => "T1 (inversion recovery)",
            RelaxModel::SaturationRecovery => "T1 (saturation recovery)",
            RelaxModel::T2Decay => "T2 (exponential decay)",
        }
    }
    /// Whether the fitted time constant is a T1 or T2, for labelling.
    pub fn is_t1(self) -> bool {
        !matches!(self, RelaxModel::T2Decay)
    }
}

/// Result of a relaxation fit: the time constant plus the recovery/decay shape.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RelaxFit {
    /// Fitted T1 or T2, seconds.
    pub t: f64,
    pub a: f64,
    pub b: f64,
    pub r2: f64,
    pub sigma_t: f64,
}

/// Fit a relaxation delay array. Intensities are the signed real part (see
/// [`extract_series`]), so inversion recovery is fitted with the unfolded
/// 3-parameter model `a + b·exp(−τ/T)` — `b` runs negative and the curve passes
/// smoothly through zero at the null, which pins T1 far more tightly than the
/// folded magnitude ever could.
pub fn fit_relaxation(series: &DecaySeries, model: RelaxModel) -> Option<RelaxFit> {
    let n = series.len();
    if n < 3 {
        return None;
    }
    let x = &series.x[..n];
    let y = &series.y[..n];

    match model {
        RelaxModel::T2Decay => {
            // ln I = ln a − x/T2.
            let (ln_a, slope) = weighted_log_line(x, y)?;
            let mut a = ln_a.exp();
            let mut t = if slope < 0.0 {
                -1.0 / slope
            } else {
                return None;
            };
            let f = |p: &[f64], xx: f64| p[0] * (-xx / p[1]).exp();
            if let Some(p) = levenberg_marquardt(x, y, &[a, t], f, 60) {
                a = p[0];
                t = p[1];
            }
            if !t.is_finite() || t <= 0.0 {
                return None;
            }
            let r2 = r_squared(x, y, |xx| f(&[a, t], xx));
            let sigma_t = param_sigma(x, y, &[a, t], f, 1);
            Some(RelaxFit {
                t,
                a,
                b: 0.0,
                r2,
                sigma_t,
            })
        }
        RelaxModel::InversionRecovery | RelaxModel::SaturationRecovery => {
            // Signed recovery: I(τ) = a + b·exp(−τ/T), rising from a+b (b<0 for
            // inversion, giving I(0)≈−a) up to the plateau a. Fit the model
            // directly — no folding — which also covers saturation recovery
            // (b<0 but I(0)≈0, monotonic rise).
            let plateau = *y.last().unwrap();
            let first = y[0];
            // Null index: the delay whose signed intensity is nearest zero.
            // For inversion recovery τ_null ≈ ln2·T1.
            let null_i = (0..n)
                .min_by(|&a, &b| {
                    y[a].abs()
                        .partial_cmp(&y[b].abs())
                        .unwrap_or(std::cmp::Ordering::Equal)
                })
                .unwrap_or(0);
            let a0 = plateau;
            let b0 = first - plateau;
            let t0 = if null_i > 0 {
                (x[null_i] / std::f64::consts::LN_2).max(x[1])
            } else {
                x[(n / 2).max(1)]
            }
            .max(f64::MIN_POSITIVE);

            let f = |p: &[f64], xx: f64| p[0] + p[1] * (-xx / p[2]).exp();
            // Try several T seeds around the estimate and keep the lowest-residual
            // fit, so a poor null estimate can't strand the optimizer.
            let seeds = [0.5, 1.0, 2.0, 4.0];
            let (a, b, t) = seeds
                .iter()
                .filter_map(|&s| {
                    let p = levenberg_marquardt(x, y, &[a0, b0, t0 * s], f, 200)?;
                    (p[2].is_finite() && p[2] > 0.0)
                        .then(|| (sum_sq_residual(x, y, &p, &f), p[0], p[1], p[2]))
                })
                .min_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal))
                .map(|(_, a, b, t)| (a, b, t))?;
            if !t.is_finite() || t <= 0.0 {
                return None;
            }
            let r2 = r_squared(x, y, |xx| f(&[a, b, t], xx));
            let sigma_t = param_sigma(x, y, &[a, b, t], f, 2);
            Some(RelaxFit {
                t,
                a,
                b,
                r2,
                sigma_t,
            })
        }
    }
}

/// A per-column diffusion fit across the whole spectrum: the classic DOSY map.
/// Columns whose peak amplitude is below `snr_frac`·(global peak) are dropped as
/// noise (`d`/`amp` are `NaN`/`0` there).
#[derive(Debug, Clone)]
pub struct DiffusionMap {
    pub ppm: Vec<f64>,
    pub d: Vec<f64>,
    pub amp: Vec<f64>,
}

pub fn diffusion_map(
    stack: &impl SpectrumStack,
    axis_values: &[f64],
    meta: &DiffusionMeta,
    snr_frac: f64,
) -> DiffusionMap {
    diffusion_map_cancellable(stack, axis_values, meta, snr_frac, &|| false)
        .expect("non-cancelling diffusion map")
}

pub fn diffusion_map_cancellable(
    stack: &impl SpectrumStack,
    axis_values: &[f64],
    meta: &DiffusionMeta,
    snr_frac: f64,
    cancelled: &impl Fn() -> bool,
) -> Option<DiffusionMap> {
    let coordinates = stack.coordinates();
    let traces = stack.traces();
    let cols = coordinates.len();
    let n = stack.increments().min(axis_values.len());
    let mut d = vec![f64::NAN; cols];
    let mut amp = vec![0.0; cols];
    if n < 3 || cols == 0 {
        return Some(DiffusionMap {
            ppm: coordinates.to_vec(),
            d,
            amp,
        });
    }
    let global_peak = stack.max_magnitude().max(f64::MIN_POSITIVE);
    let threshold = global_peak * snr_frac.clamp(0.0, 1.0);

    for c in 0..cols {
        if cancelled() {
            return None;
        }
        let y: Vec<f64> = (0..n).map(|i| traces[i][c].norm()).collect();
        let peak = y.iter().cloned().fold(0.0, f64::max);
        if peak < threshold {
            continue;
        }
        let series = DecaySeries {
            x: axis_values[..n].to_vec(),
            y,
        };
        if let Some(fit) = fit_diffusion_cancellable(&series, meta, cancelled) {
            // Require a sane fit to keep noise from smearing the map.
            if fit.r2 > 0.8 && fit.d.is_finite() && fit.d > 0.0 {
                d[c] = fit.d;
                amp[c] = fit.i0.max(peak);
            }
        }
    }
    Some(DiffusionMap {
        ppm: coordinates.to_vec(),
        d,
        amp,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SpectrumStack;
    use num_complex::Complex64;
    use plotx_io::DiffusionMeta;

    struct TestStack {
        ppm: Vec<f64>,
        traces: Vec<Vec<Complex64>>,
    }

    impl SpectrumStack for TestStack {
        fn coordinates(&self) -> &[f64] {
            &self.ppm
        }

        fn traces(&self) -> &[Vec<Complex64>] {
            &self.traces
        }
    }

    // A single-peak inversion-recovery stack with an arbitrary constant phase and
    // a linear phase ramp across the peak, so the real part alone is meaningless
    // but the reference-phasor projection must still recover signed intensities.
    fn ir_stack(t1: f64, taus: &[f64]) -> TestStack {
        let cols = 8;
        let peak = 3usize;
        let phi0 = 0.7; // constant zero-order phase
        let phi1 = 0.35; // per-column first-order phase
        let traces = taus
            .iter()
            .map(|&tau| {
                let signed = 1.0 - 2.0 * (-tau / t1).exp(); // −1 → +1
                (0..cols)
                    .map(|c| {
                        let amp = if c == peak { 100.0 * signed } else { 0.0 };
                        let phase = phi0 + phi1 * (c as f64 - peak as f64);
                        Complex64::from_polar(amp.abs(), phase) * if amp < 0.0 { -1.0 } else { 1.0 }
                    })
                    .collect()
            })
            .collect();
        TestStack {
            ppm: (0..cols).map(|c| c as f64).collect(),
            traces,
        }
    }

    #[test]
    fn projection_recovers_signed_intensities() {
        let t1 = 0.8;
        let taus: Vec<f64> = (0..16)
            .map(|i| 1e-3 * 5000f64.powf(i as f64 / 15.0))
            .collect();
        let stack = ir_stack(t1, &taus);
        let series = extract_series(&stack, &taus, None, IntensityMode::PeakHeight);
        // Early increments must be negative (below the null), late ones positive.
        assert!(
            series.y[0] < 0.0,
            "I(0) should be negative, got {}",
            series.y[0]
        );
        assert!(*series.y.last().unwrap() > 0.0);
        let fit = fit_relaxation(&series, RelaxModel::InversionRecovery).expect("fit");
        assert!((fit.t - t1).abs() / t1 < 0.02, "T1={} vs {}", fit.t, t1);
        assert!(fit.r2 > 0.999);
    }

    fn diff_meta() -> DiffusionMeta {
        DiffusionMeta {
            gamma: 2.675_222e8,
            delta: 2e-3,
            big_delta: 0.1,
            tau: 0.0,
            shape_factor: 1.0 / 3.0,
        }
    }

    #[test]
    fn window_outside_spectrum_yields_empty_series() {
        // A region dragged entirely off the spectrum must return an empty series,
        // not silently reduce over the whole spectrum.
        let stack = ir_stack(0.8, &[0.1, 0.5, 1.0, 2.0]);
        let taus = [0.1, 0.5, 1.0, 2.0];
        let off = extract_series(&stack, &taus, Some((100.0, 200.0)), IntensityMode::Integral);
        assert!(off.is_empty());
        let on = extract_series(&stack, &taus, Some((0.0, 7.0)), IntensityMode::Integral);
        assert!(!on.is_empty());
    }

    #[test]
    fn recovers_known_diffusion_coefficient() {
        let meta = diff_meta();
        let d_true = 1.2e-9;
        let i0 = 100.0;
        let g: Vec<f64> = (0..16)
            .map(|i| 0.02 + i as f64 * (0.28 - 0.02) / 15.0)
            .collect();
        let y: Vec<f64> = g
            .iter()
            .map(|&gi| i0 * (-d_true * meta.b_factor(gi)).exp())
            .collect();
        let fit = fit_diffusion(&DecaySeries { x: g, y }, &meta).expect("fit");
        assert!(
            (fit.d - d_true).abs() / d_true < 0.01,
            "D={} vs {}",
            fit.d,
            d_true
        );
        assert!(fit.r2 > 0.999);
    }

    #[test]
    fn recovers_known_t1_inversion_recovery() {
        let t1_true = 0.8;
        let a = 50.0;
        // Log-spaced delays 1 ms → 5 s, as a real inversion-recovery array.
        let x: Vec<f64> = (0..16)
            .map(|i| 1e-3 * 5000f64.powf(i as f64 / 15.0))
            .collect();
        // Signed (real-part) data: I(0) ≈ −a, passing through zero at the null.
        let y: Vec<f64> = x
            .iter()
            .map(|&tau| a * (1.0 - 2.0 * (-tau / t1_true).exp()))
            .collect();
        let fit =
            fit_relaxation(&DecaySeries { x, y }, RelaxModel::InversionRecovery).expect("fit");
        assert!(
            (fit.t - t1_true).abs() / t1_true < 0.02,
            "T1={} vs {}",
            fit.t,
            t1_true
        );
        assert!(fit.r2 > 0.999);
    }

    #[test]
    fn recovers_known_t2_decay() {
        let t2_true = 0.35;
        let a = 80.0;
        let x: Vec<f64> = (0..16).map(|i| 0.01 + i as f64 * 0.05).collect();
        let y: Vec<f64> = x.iter().map(|&tau| a * (-tau / t2_true).exp()).collect();
        let fit = fit_relaxation(&DecaySeries { x, y }, RelaxModel::T2Decay).expect("fit");
        assert!(
            (fit.t - t2_true).abs() / t2_true < 0.01,
            "T2={} vs {}",
            fit.t,
            t2_true
        );
    }
}
