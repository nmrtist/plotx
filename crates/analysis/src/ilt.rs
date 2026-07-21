//! Regularized inverse Laplace transform for DOSY: recover a non-negative
//! diffusion-coefficient distribution from a single intensity decay, or a full
//! ppm × D map from a pseudo-2D stack. With kernel `K[i][j] = exp(−b_i·d_j)`
//! this solves `min ‖K a − y‖² + λ‖a‖²  s.t.  a ≥ 0` by fast non-negative
//! least squares (Bro & De Jong) on the Tikhonov normal equations.

use crate::{SpectrumStack, fit::solve_linear};

/// Columns below this fraction of the global peak magnitude are treated as
/// noise and skipped, matching the DOSY-map convention in `pseudo2d`.
const SNR_FRAC: f64 = 0.02;

/// A non-negative diffusion distribution over a shared ppm and D grid.
/// `amp[c]` is the D distribution for ppm column `c`, aligned to `d_grid`.
#[derive(Debug, Clone)]
pub struct IltResult {
    pub ppm: Vec<f64>,
    pub d_grid: Vec<f64>,
    pub amp: Vec<Vec<f64>>,
}

/// Log-spaced diffusion-coefficient grid of `n` points spanning `[d_min, d_max]`
/// (m²·s⁻¹). A geometric axis matches the multi-decade span of real DOSY data.
pub fn log_grid(d_min: f64, d_max: f64, n: usize) -> Vec<f64> {
    if n == 0 {
        return Vec::new();
    }
    if n == 1 || d_min <= 0.0 || d_max <= 0.0 {
        return vec![d_min; n];
    }
    let (lo, hi) = (d_min.ln(), d_max.ln());
    (0..n)
        .map(|i| (lo + (hi - lo) * i as f64 / (n - 1) as f64).exp())
        .collect()
}

fn kernel(b: &[f64], d_grid: &[f64]) -> Vec<Vec<f64>> {
    b.iter()
        .map(|&bi| d_grid.iter().map(|&dj| (-bi * dj).exp()).collect())
        .collect()
}

/// Recover the non-negative D distribution for one decay. `x` is the Stejskal–
/// Tanner b-factor of each increment (s·m⁻², exactly what
/// [`plotx_io::DiffusionMeta::b_factor`] returns — the caller converts the raw
/// gradient ruler once, keeping this a pure numeric kernel); `y` is the
/// per-increment intensity; `d_grid` the target D axis (m²·s⁻¹); `lambda` the
/// Tikhonov weight. The returned amplitudes are ≥ 0 and aligned to `d_grid`.
pub fn fit_ilt(x: &[f64], y: &[f64], d_grid: &[f64], lambda: f64) -> Vec<f64> {
    fit_ilt_cancellable(x, y, d_grid, lambda, &|| false).expect("non-cancelling ILT fit")
}

pub fn fit_ilt_cancellable(
    x: &[f64],
    y: &[f64],
    d_grid: &[f64],
    lambda: f64,
    cancelled: &impl Fn() -> bool,
) -> Option<Vec<f64>> {
    let n = x.len().min(y.len());
    let m = d_grid.len();
    if n == 0 || m == 0 {
        return Some(vec![0.0; m]);
    }
    let k = kernel(&x[..n], d_grid);

    // Normal equations of the augmented Tikhonov system [K; √λ·I] a = [y; 0]:
    // G = KᵀK + λ·I, h = Kᵀy. (L = identity ridge, keeping G positive definite.)
    let mut g = vec![vec![0.0; m]; m];
    for a in 0..m {
        if cancelled() {
            return None;
        }
        for b in a..m {
            let mut s = 0.0;
            for row in k.iter().take(n) {
                s += row[a] * row[b];
            }
            g[a][b] = s;
            g[b][a] = s;
        }
        g[a][a] += lambda;
    }
    let mut h = vec![0.0; m];
    for (a, ha) in h.iter_mut().enumerate() {
        *ha = (0..n).map(|i| k[i][a] * y[i]).sum();
    }
    fnnls(&g, &h, cancelled)
}

/// Full-spectrum ILT: invert each ppm column of `stack` against `b_factors`
/// onto `d_grid`, skipping columns whose peak magnitude is below the noise
/// floor. Column intensities are the per-increment magnitude, matching
/// `pseudo2d::diffusion_map`. `b_factors` is one Stejskal–Tanner b per
/// increment (SI), pre-converted from the gradient ruler by the caller.
pub fn ilt_map(
    stack: &impl SpectrumStack,
    b_factors: &[f64],
    d_grid: &[f64],
    lambda: f64,
) -> IltResult {
    ilt_map_cancellable(stack, b_factors, d_grid, lambda, &|| false)
        .expect("non-cancelling ILT map")
}

pub fn ilt_map_cancellable(
    stack: &impl SpectrumStack,
    b_factors: &[f64],
    d_grid: &[f64],
    lambda: f64,
    cancelled: &impl Fn() -> bool,
) -> Option<IltResult> {
    let coordinates = stack.coordinates();
    let traces = stack.traces();
    let cols = coordinates.len();
    let n = stack.increments().min(b_factors.len());
    let m = d_grid.len();
    let mut amp = vec![vec![0.0; m]; cols];
    if n < 3 || cols == 0 || m == 0 {
        return Some(IltResult {
            ppm: coordinates.to_vec(),
            d_grid: d_grid.to_vec(),
            amp,
        });
    }
    let global_peak = stack.max_magnitude().max(f64::MIN_POSITIVE);
    let threshold = global_peak * SNR_FRAC;
    let b = &b_factors[..n];
    for (c, slot) in amp.iter_mut().enumerate().take(cols) {
        if cancelled() {
            return None;
        }
        let y: Vec<f64> = (0..n).map(|i| traces[i][c].norm()).collect();
        let peak = y.iter().cloned().fold(0.0, f64::max);
        if peak < threshold {
            continue;
        }
        *slot = fit_ilt_cancellable(b, &y, d_grid, lambda, cancelled)?;
    }
    Some(IltResult {
        ppm: coordinates.to_vec(),
        d_grid: d_grid.to_vec(),
        amp,
    })
}

/// Fast non-negative least squares on precomputed normal equations `G a = h`:
/// minimize `½aᵀG a − hᵀa` with `a ≥ 0` by the active-set method of Lawson &
/// Hanson, reformulated on `G`/`h` (Bro & De Jong) so the inner solves are on
/// the small passive submatrix via [`solve_linear`].
fn fnnls(g: &[Vec<f64>], h: &[f64], cancelled: &impl Fn() -> bool) -> Option<Vec<f64>> {
    let m = h.len();
    let mut x = vec![0.0; m];
    let mut passive = vec![false; m];
    let g_scale = g
        .iter()
        .flat_map(|r| r.iter())
        .fold(0.0_f64, |a, &v| a.max(v.abs()))
        .max(1.0);
    let tol = 1e-12 * g_scale * m as f64;
    let mut w = h.to_vec(); // gradient h − G·x at x = 0

    let max_outer = 3 * m + 10;
    for _ in 0..max_outer {
        if cancelled() {
            return None;
        }
        // Bring the most-violated inactive index into the passive set.
        let mut t = None;
        let mut best = tol;
        for j in 0..m {
            if !passive[j] && w[j] > best {
                best = w[j];
                t = Some(j);
            }
        }
        let t = match t {
            Some(t) => t,
            None => break,
        };
        passive[t] = true;

        let mut inner_ok = true;
        for _ in 0..(2 * m + 10) {
            if cancelled() {
                return None;
            }
            let idx: Vec<usize> = (0..m).filter(|&j| passive[j]).collect();
            let s_p = match solve_passive(g, h, &idx) {
                Some(s) => s,
                None => {
                    inner_ok = false;
                    break;
                }
            };
            let mut s = vec![0.0; m];
            let mut min_s = f64::INFINITY;
            for (k, &j) in idx.iter().enumerate() {
                s[j] = s_p[k];
                min_s = min_s.min(s[j]);
            }
            if min_s > 0.0 {
                x = s;
                break;
            }
            // Move partway to `s`, stopping where the first passive component
            // reaches zero; drop the now-inactive components.
            let mut alpha = f64::INFINITY;
            for &j in &idx {
                if s[j] <= 0.0 {
                    let denom = x[j] - s[j];
                    if denom > 0.0 {
                        alpha = alpha.min(x[j] / denom);
                    }
                }
            }
            if !alpha.is_finite() {
                x = s;
                break;
            }
            for j in 0..m {
                x[j] += alpha * (s[j] - x[j]);
            }
            for j in 0..m {
                if passive[j] && x[j] <= tol {
                    passive[j] = false;
                    x[j] = 0.0;
                }
            }
        }
        if !inner_ok {
            break;
        }

        for j in 0..m {
            let gx: f64 = (0..m).map(|k| g[j][k] * x[k]).sum();
            w[j] = h[j] - gx;
        }
    }
    for v in &mut x {
        if *v < 0.0 {
            *v = 0.0;
        }
    }
    Some(x)
}

fn solve_passive(g: &[Vec<f64>], h: &[f64], idx: &[usize]) -> Option<Vec<f64>> {
    let p = idx.len();
    if p == 0 {
        return Some(Vec::new());
    }
    let mut sub = vec![vec![0.0; p]; p];
    let mut rhs = vec![0.0; p];
    for (a, &ia) in idx.iter().enumerate() {
        rhs[a] = h[ia];
        for (b, &ib) in idx.iter().enumerate() {
            sub[a][b] = g[ia][ib];
        }
    }
    solve_linear(&sub, &rhs)
}

#[cfg(test)]
mod tests {
    use super::*;

    // Realistic Stejskal–Tanner b-factors (s·m⁻²) for a 16-step gradient ramp.
    fn b_ramp() -> Vec<f64> {
        (0..16).map(|i| i as f64 * 2.0e9 / 15.0).collect()
    }

    fn argmax(v: &[f64]) -> usize {
        (0..v.len())
            .max_by(|&a, &b| v[a].partial_cmp(&v[b]).unwrap())
            .unwrap_or(0)
    }

    // Local maxima above a fraction of the global peak.
    fn peaks(a: &[f64], frac: f64) -> Vec<usize> {
        let max = a.iter().cloned().fold(0.0, f64::max);
        let thr = max * frac;
        (0..a.len())
            .filter(|&i| {
                a[i] > thr && (i == 0 || a[i] >= a[i - 1]) && (i + 1 == a.len() || a[i] >= a[i + 1])
            })
            .collect()
    }

    #[test]
    fn single_component_peaks_at_true_d() {
        let b = b_ramp();
        let d_true = 1e-9;
        let d_grid = log_grid(1e-10, 1e-8, 21); // d_true lands on index 10
        let y: Vec<f64> = b.iter().map(|&bi| 100.0 * (-bi * d_true).exp()).collect();
        let amp = fit_ilt(&b, &y, &d_grid, 1e-2);
        let peak = argmax(&amp);
        let true_i = 10;
        assert!(
            peak.abs_diff(true_i) <= 1,
            "peak at index {peak} (D={:.3e}), expected near {:.3e}",
            d_grid[peak],
            d_true
        );
    }

    #[test]
    fn two_components_resolve() {
        let b = b_ramp();
        let (d1, d2) = (3e-10, 3e-9); // one decade apart
        let d_grid = log_grid(1e-10, 1e-8, 41);
        let y: Vec<f64> = b
            .iter()
            .map(|&bi| 50.0 * (-bi * d1).exp() + 50.0 * (-bi * d2).exp())
            .collect();
        let amp = fit_ilt(&b, &y, &d_grid, 1e-2);

        let found = peaks(&amp, 0.1);
        assert!(found.len() >= 2, "expected ≥2 peaks, got {found:?}");

        let total: f64 = amp.iter().sum();
        let low: f64 = d_grid
            .iter()
            .zip(&amp)
            .filter(|&(&d, _)| d < 1e-9)
            .map(|(_, &a)| a)
            .sum();
        let high: f64 = d_grid
            .iter()
            .zip(&amp)
            .filter(|&(&d, _)| d > 1e-9)
            .map(|(_, &a)| a)
            .sum();
        assert!(
            low > 0.1 * total && high > 0.1 * total,
            "mass not split across both D regions: low={low:.3}, high={high:.3}, total={total:.3}"
        );
    }

    #[test]
    fn amplitudes_are_non_negative() {
        let b = b_ramp();
        let d_grid = log_grid(1e-10, 1e-8, 25);
        let y: Vec<f64> = b
            .iter()
            .map(|&bi| 80.0 * (-bi * 7e-10).exp() + 20.0 * (-bi * 2e-9).exp())
            .collect();
        let amp = fit_ilt(&b, &y, &d_grid, 5e-2);
        assert!(amp.iter().all(|&a| a >= 0.0), "found negative amplitude");
    }

    #[test]
    fn cancellation_stops_fnnls_iterations() {
        let b = b_ramp();
        let d_grid = log_grid(1e-10, 1e-8, 64);
        let y: Vec<f64> = b.iter().map(|&bi| 100.0 * (-bi * 1e-9).exp()).collect();
        let checks = std::cell::Cell::new(0usize);
        let cancelled = || {
            checks.set(checks.get() + 1);
            checks.get() > 4
        };

        assert!(fit_ilt_cancellable(&b, &y, &d_grid, 0.01, &cancelled).is_none());
        assert!(checks.get() <= 6);
    }
}
