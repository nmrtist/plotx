//! Multi-peak lineshape deconvolution: a sum of Lorentzian / Gaussian /
//! pseudo-Voigt components plus a constant offset, refined through the shared
//! Levenberg–Marquardt core with an analytic Jacobian.

#[cfg(test)]
use crate::fit::levenberg_marquardt_problem;
use crate::fit::{
    LmProblem, levenberg_marquardt_problem_cancellable, mirror_upper, r_squared, solve_linear,
};
use std::f64::consts::{FRAC_PI_2, LN_2, PI};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineShape {
    Lorentzian,
    Gaussian,
    /// `eta·Lorentzian + (1−eta)·Gaussian`, both height-normalized so the
    /// component value at the peak position is the height for any `eta`.
    PseudoVoigt,
}

impl LineShape {
    /// Flat parameter count per peak: `[position, height, fwhm]` plus `eta`
    /// for [`LineShape::PseudoVoigt`]. The full fit vector is that block
    /// repeated per peak, followed by one trailing constant offset.
    pub fn params_per_peak(self) -> usize {
        match self {
            LineShape::PseudoVoigt => 4,
            LineShape::Lorentzian | LineShape::Gaussian => 3,
        }
    }

    fn unit(self, dx: f64, w: f64, eta: f64) -> f64 {
        match self {
            LineShape::Lorentzian => lorentz_unit(dx, w),
            LineShape::Gaussian => gauss_unit(dx, w),
            LineShape::PseudoVoigt => eta * lorentz_unit(dx, w) + (1.0 - eta) * gauss_unit(dx, w),
        }
    }

    /// Analytic area of a unit-height, unit-FWHM peak; area = h·w·factor.
    fn area_factor(self, eta: f64) -> f64 {
        match self {
            LineShape::Lorentzian => FRAC_PI_2,
            LineShape::Gaussian => gauss_area_factor(),
            LineShape::PseudoVoigt => eta * FRAC_PI_2 + (1.0 - eta) * gauss_area_factor(),
        }
    }
}

fn lorentz_unit(dx: f64, w: f64) -> f64 {
    let hw2 = 0.25 * w * w;
    let denom = dx * dx + hw2;
    if denom > 0.0 { hw2 / denom } else { 1.0 }
}

fn gauss_unit(dx: f64, w: f64) -> f64 {
    let w2 = w * w;
    if w2 > 0.0 {
        (-4.0 * LN_2 * dx * dx / w2).exp()
    } else if dx == 0.0 {
        1.0
    } else {
        0.0
    }
}

fn gauss_area_factor() -> f64 {
    (PI / (4.0 * LN_2)).sqrt()
}

fn eval_model(shape: LineShape, n_peaks: usize, p: &[f64], x: f64) -> f64 {
    let k = shape.params_per_peak();
    let mut y = p[n_peaks * k];
    for i in 0..n_peaks {
        let b = i * k;
        let eta = if k == 4 {
            p[b + 3].clamp(0.0, 1.0)
        } else {
            0.0
        };
        y += p[b + 1] * shape.unit(x - p[b], p[b + 2].abs(), eta);
    }
    y
}

/// Analytic partials of one peak's contribution `h·unit(x−x0, |w|, clamp(eta))`
/// w.r.t. its block `[x0, h, w(, eta)]`. Chains through `w.abs()` (sign
/// factor, zero at `w = 0`) and through the `eta` clamp — the `eta` partial is
/// identically zero when the stored value sits at or beyond a clamp boundary,
/// so a pinned `eta` stays pinned.
fn peak_partials(shape: LineShape, dx: f64, h: f64, w: f64, eta_raw: f64) -> [f64; 4] {
    let aw = w.abs();
    let sw = if w > 0.0 {
        1.0
    } else if w < 0.0 {
        -1.0
    } else {
        0.0
    };

    let q = 0.25 * aw * aw;
    let denom = dx * dx + q;
    let (lu, dl_dx0, dl_dw) = if denom > 0.0 {
        let inv2 = 1.0 / (denom * denom);
        (
            q / denom,
            2.0 * dx * q * inv2,
            0.5 * aw * dx * dx * inv2 * sw,
        )
    } else {
        (1.0, 0.0, 0.0)
    };

    let w2 = aw * aw;
    let (gu, dg_dx0, dg_dw) = if w2 > 0.0 {
        let g = (-4.0 * LN_2 * dx * dx / w2).exp();
        let c = 8.0 * LN_2 * g * dx / w2;
        (g, c, c * dx / aw * sw)
    } else {
        (if dx == 0.0 { 1.0 } else { 0.0 }, 0.0, 0.0)
    };

    let (unit, du_dx0, du_dw) = match shape {
        LineShape::Lorentzian => (lu, dl_dx0, dl_dw),
        LineShape::Gaussian => (gu, dg_dx0, dg_dw),
        LineShape::PseudoVoigt => {
            let eta = eta_raw.clamp(0.0, 1.0);
            (
                eta * lu + (1.0 - eta) * gu,
                eta * dl_dx0 + (1.0 - eta) * dg_dx0,
                eta * dl_dw + (1.0 - eta) * dg_dw,
            )
        }
    };
    let d_eta = if shape == LineShape::PseudoVoigt && eta_raw > 0.0 && eta_raw < 1.0 {
        h * (lu - gu)
    } else {
        0.0
    };
    [h * du_dx0, unit, h * du_dw, d_eta]
}

/// Analytic Jacobian row of [`eval_model`] at `x`: `row[j] = ∂model/∂p_j`.
#[cfg(test)]
fn fill_jacobian_row(shape: LineShape, n_peaks: usize, p: &[f64], x: f64, row: &mut [f64]) {
    let k = shape.params_per_peak();
    for i in 0..n_peaks {
        let b = i * k;
        let g = peak_partials(
            shape,
            x - p[b],
            p[b + 1],
            p[b + 2],
            if k == 4 { p[b + 3] } else { 0.0 },
        );
        row[b..b + k].copy_from_slice(&g[..k]);
    }
    row[n_peaks * k] = 1.0;
}

// Beyond this many FWHMs a peak's value and partials are ≤ ~6e-5 of its
// height (1/(4·64²) for the Lorentzian tail; the Gaussian underflows far
// sooner), so the fit treats the peak as exactly local to this window.
fn window_halfwidth(shape: LineShape, w: f64) -> f64 {
    match shape {
        LineShape::Gaussian => 4.0 * w.abs(),
        LineShape::Lorentzian | LineShape::PseudoVoigt => 64.0 * w.abs(),
    }
}

/// The multi-peak fit as an [`LmProblem`]: residuals and normal equations are
/// evaluated on a compact support window per peak (the model is a sum of
/// local components), which keeps the cost per iteration proportional to the
/// covered points instead of `points × params²`.
struct LineshapeProblem<'a> {
    shape: LineShape,
    n_peaks: usize,
    xs: &'a [f64],
    ys: &'a [f64],
    /// `Some(true)` ascending, `Some(false)` descending, `None` unsorted
    /// (falls back to whole-range windows).
    ascending: Option<bool>,
    resid: Vec<f64>,
    active: Vec<(usize, [f64; 4])>,
}

impl<'a> LineshapeProblem<'a> {
    fn new(shape: LineShape, n_peaks: usize, xs: &'a [f64], ys: &'a [f64]) -> Self {
        let ascending = if xs.windows(2).all(|w| w[0] <= w[1]) {
            Some(true)
        } else if xs.windows(2).all(|w| w[0] >= w[1]) {
            Some(false)
        } else {
            None
        };
        Self {
            shape,
            n_peaks,
            xs,
            ys,
            ascending,
            resid: vec![0.0; xs.len()],
            active: Vec::with_capacity(n_peaks),
        }
    }

    fn point_range(&self, x0: f64, hw: f64) -> (usize, usize) {
        let (lo, hi) = (x0 - hw, x0 + hw);
        match self.ascending {
            Some(true) => (
                self.xs.partition_point(|&x| x < lo),
                self.xs.partition_point(|&x| x <= hi),
            ),
            Some(false) => (
                self.xs.partition_point(|&x| x > hi),
                self.xs.partition_point(|&x| x >= lo),
            ),
            None => (0, self.xs.len()),
        }
    }

    /// `resid[i] = ys[i] − model(p, xs[i])` under the windowed model.
    fn fill_resid(&mut self, p: &[f64]) {
        let k = self.shape.params_per_peak();
        let offset = p[self.n_peaks * k];
        for (r, &y) in self.resid.iter_mut().zip(self.ys) {
            *r = y - offset;
        }
        for i in 0..self.n_peaks {
            let b = i * k;
            let (x0, h, w) = (p[b], p[b + 1], p[b + 2]);
            let eta = if k == 4 {
                p[b + 3].clamp(0.0, 1.0)
            } else {
                0.0
            };
            let (lo, hi) = self.point_range(x0, window_halfwidth(self.shape, w));
            let aw = w.abs();
            for idx in lo..hi {
                self.resid[idx] -= h * self.shape.unit(self.xs[idx] - x0, aw, eta);
            }
        }
    }
}

impl LmProblem for LineshapeProblem<'_> {
    fn cost(&mut self, p: &[f64]) -> f64 {
        self.fill_resid(p);
        self.resid.iter().map(|r| r * r).sum()
    }

    fn normal_equations(&mut self, p: &[f64], jtj: &mut [Vec<f64>], jtr: &mut [f64]) {
        self.fill_resid(p);
        for row in jtj.iter_mut() {
            row.fill(0.0);
        }
        jtr.fill(0.0);
        let k = self.shape.params_per_peak();
        let off = self.n_peaks * k;
        jtj[off][off] = self.xs.len() as f64;
        for idx in 0..self.xs.len() {
            let x = self.xs[idx];
            let rk = self.resid[idx];
            jtr[off] += rk;
            self.active.clear();
            for i in 0..self.n_peaks {
                let b = i * k;
                let w = p[b + 2];
                if (x - p[b]).abs() <= window_halfwidth(self.shape, w) {
                    let eta = if k == 4 { p[b + 3] } else { 0.0 };
                    self.active
                        .push((b, peak_partials(self.shape, x - p[b], p[b + 1], w, eta)));
                }
            }
            for (ai, &(ba, ga)) in self.active.iter().enumerate() {
                for la in 0..k {
                    let g1 = ga[la];
                    jtr[ba + la] += g1 * rk;
                    jtj[ba + la][off] += g1;
                    for lb in la..k {
                        jtj[ba + la][ba + lb] += g1 * ga[lb];
                    }
                    for &(bb, gb) in &self.active[ai + 1..] {
                        for lb in 0..k {
                            jtj[ba + la][bb + lb] += g1 * gb[lb];
                        }
                    }
                }
            }
        }
        mirror_upper(jtj);
    }
}

/// Initial estimate for one peak, in the same units as the data window.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PeakSeed {
    pub position: f64,
    pub height: f64,
    pub fwhm: f64,
}

/// One converged component. `eta` and `eta_sigma` are `Some` only for
/// [`LineShape::PseudoVoigt`]; each `*_sigma` is the 1σ standard error when
/// the normal equations allowed one.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FittedPeak {
    pub position: f64,
    pub height: f64,
    pub fwhm: f64,
    pub eta: Option<f64>,
    pub area: f64,
    pub position_sigma: Option<f64>,
    pub height_sigma: Option<f64>,
    pub fwhm_sigma: Option<f64>,
    pub eta_sigma: Option<f64>,
    pub area_sigma: Option<f64>,
}

/// A converged deconvolution: components sorted by position, plus the fitted
/// constant offset and the coefficient of determination over the window.
#[derive(Debug, Clone, PartialEq)]
pub struct LineFit {
    pub shape: LineShape,
    pub peaks: Vec<FittedPeak>,
    pub offset: f64,
    pub offset_sigma: Option<f64>,
    pub r2: f64,
}

impl LineFit {
    pub fn eval_total(&self, x: f64) -> f64 {
        self.offset
            + (0..self.peaks.len())
                .map(|i| self.eval_component(i, x))
                .sum::<f64>()
    }

    /// Component `i` alone, without the fitted offset.
    pub fn eval_component(&self, i: usize, x: f64) -> f64 {
        self.peaks.get(i).map_or(0.0, |pk| {
            pk.height
                * self
                    .shape
                    .unit(x - pk.position, pk.fwhm, pk.eta.unwrap_or(0.0))
        })
    }

    pub fn sample_residual(&self, xs: &[f64], ys: &[f64]) -> Vec<f64> {
        xs.iter()
            .zip(ys)
            .map(|(&x, &y)| y - self.eval_total(x))
            .collect()
    }
}

/// Estimate seeds at the supplied x positions: height above an edge-median
/// baseline at the strongest sample near each position, FWHM by half-height
/// crossing search, with a `span/(4·n)` fallback when no crossing exists.
pub fn seed_peaks(xs: &[f64], ys: &[f64], positions: &[f64]) -> Vec<PeakSeed> {
    let n = xs.len().min(ys.len());
    if n == 0 {
        return Vec::new();
    }
    let (xs, ys) = (&xs[..n], &ys[..n]);
    let base = edge_baseline(ys);
    let fallback = fallback_fwhm(xs, positions.len().max(1));
    positions
        .iter()
        .map(|&x0| {
            let apex = apex_near(xs, ys, base, x0);
            let height = ys[apex] - base;
            let fwhm = half_height_fwhm(xs, ys, apex, base, height).unwrap_or(fallback);
            PeakSeed {
                position: xs[apex],
                height,
                fwhm,
            }
        })
        .collect()
}

fn apex_near(xs: &[f64], ys: &[f64], base: f64, x0: f64) -> usize {
    let nearest = (0..xs.len())
        .min_by(|&a, &b| (xs[a] - x0).abs().total_cmp(&(xs[b] - x0).abs()))
        .unwrap_or(0);
    let lo = nearest.saturating_sub(3);
    let hi = (nearest + 3).min(xs.len() - 1);
    (lo..=hi)
        .max_by(|&a, &b| (ys[a] - base).abs().total_cmp(&(ys[b] - base).abs()))
        .unwrap_or(nearest)
}

fn edge_baseline(ys: &[f64]) -> f64 {
    let k = ys.len().min(3);
    let mut edges: Vec<f64> = ys[..k].iter().chain(&ys[ys.len() - k..]).copied().collect();
    edges.sort_by(f64::total_cmp);
    let m = edges.len() / 2;
    if edges.len() % 2 == 1 {
        edges[m]
    } else {
        (edges[m - 1] + edges[m]) / 2.0
    }
}

fn fallback_fwhm(xs: &[f64], n_peaks: usize) -> f64 {
    let span = (xs[xs.len() - 1] - xs[0]).abs();
    if span > 0.0 {
        span / (4.0 * n_peaks as f64)
    } else {
        1.0
    }
}

fn half_height_fwhm(xs: &[f64], ys: &[f64], apex: usize, base: f64, height: f64) -> Option<f64> {
    if height == 0.0 {
        return None;
    }
    let excess = |i: usize| (ys[i] - base) / height;
    let cross = |above: usize, below: usize| -> f64 {
        let (ea, eb) = (excess(above), excess(below));
        let t = if (ea - eb).abs() > 0.0 {
            (ea - 0.5) / (ea - eb)
        } else {
            0.0
        };
        xs[above] + t * (xs[below] - xs[above])
    };
    let left = (0..apex)
        .rev()
        .find(|&j| excess(j) < 0.5)
        .map(|j| cross(j + 1, j));
    let right = (apex + 1..xs.len())
        .find(|&j| excess(j) < 0.5)
        .map(|j| cross(j - 1, j));
    let xa = xs[apex];
    match (left, right) {
        (Some(l), Some(r)) => Some((xa - l).abs() + (r - xa).abs()),
        (Some(l), None) => Some(2.0 * (xa - l).abs()),
        (None, Some(r)) => Some(2.0 * (r - xa).abs()),
        (None, None) => None,
    }
}

/// Fit `seeds.len()` peaks of `shape` plus a constant offset to the window.
/// Returns `None` for empty seeds, mismatched or non-finite data, fewer points
/// than parameters, or a diverged solve; every value in a `Some` is finite.
pub fn fit_lineshapes(
    xs: &[f64],
    ys: &[f64],
    shape: LineShape,
    seeds: &[PeakSeed],
) -> Option<LineFit> {
    fit_lineshapes_cancellable(xs, ys, shape, seeds, &|| false)
}

pub fn fit_lineshapes_cancellable(
    xs: &[f64],
    ys: &[f64],
    shape: LineShape,
    seeds: &[PeakSeed],
    cancelled: &impl Fn() -> bool,
) -> Option<LineFit> {
    if cancelled() {
        return None;
    }
    let n = xs.len();
    if n == 0 || ys.len() != n || seeds.is_empty() {
        return None;
    }
    if xs.iter().chain(ys).any(|v| !v.is_finite()) {
        return None;
    }
    if seeds
        .iter()
        .any(|s| !(s.position.is_finite() && s.height.is_finite() && s.fwhm.is_finite()))
    {
        return None;
    }
    let k = shape.params_per_peak();
    let n_peaks = seeds.len();
    let m = n_peaks * k + 1;
    if n < m {
        return None;
    }

    let fallback = fallback_fwhm(xs, n_peaks);
    let mut p0 = Vec::with_capacity(m);
    for s in seeds {
        p0.push(s.position);
        p0.push(s.height);
        p0.push(if s.fwhm.abs() > 0.0 {
            s.fwhm.abs()
        } else {
            fallback
        });
        if k == 4 {
            p0.push(0.5);
        }
    }
    p0.push(edge_baseline(ys));

    let model = move |p: &[f64], x: f64| eval_model(shape, n_peaks, p, x);
    let mut problem = LineshapeProblem::new(shape, n_peaks, xs, ys);
    let (p, _) = levenberg_marquardt_problem_cancellable(&mut problem, &p0, 300, cancelled)?;
    if p.iter().any(|v| !v.is_finite()) {
        return None;
    }
    let (lo, hi) = xs
        .iter()
        .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), &x| {
            (lo.min(x), hi.max(x))
        });
    let span = hi - lo;
    for i in 0..n_peaks {
        let b = i * k;
        if p[b] < lo - span || p[b] > hi + span || p[b + 2].abs() > 10.0 * span {
            return None;
        }
    }
    let r2 = r_squared(xs, ys, |x| model(&p, x));
    if !r2.is_finite() {
        return None;
    }
    if cancelled() {
        return None;
    }
    let sigmas = param_sigmas(&mut problem, &p);
    if cancelled() {
        return None;
    }
    let sig = |i: usize| sigmas[i];

    let mut peaks: Vec<FittedPeak> = (0..n_peaks)
        .map(|i| {
            let b = i * k;
            let height = p[b + 1];
            let fwhm = p[b + 2].abs();
            let eta = (k == 4).then(|| p[b + 3].clamp(0.0, 1.0));
            let factor = shape.area_factor(eta.unwrap_or(0.0));
            let height_sigma = sig(b + 1);
            let fwhm_sigma = sig(b + 2);
            let eta_sigma = if k == 4 { sig(b + 3) } else { None };
            let area_sigma = match (height_sigma, fwhm_sigma) {
                (Some(hs), Some(ws)) => {
                    let mut v = (factor * fwhm * hs).powi(2) + (factor * height * ws).powi(2);
                    if let Some(es) = eta_sigma {
                        v += ((FRAC_PI_2 - gauss_area_factor()) * height * fwhm * es).powi(2);
                    }
                    Some(v.sqrt())
                }
                _ => None,
            };
            FittedPeak {
                position: p[b],
                height,
                fwhm,
                eta,
                area: height * fwhm * factor,
                position_sigma: sig(b),
                height_sigma,
                fwhm_sigma,
                eta_sigma,
                area_sigma,
            }
        })
        .collect();
    if peaks.iter().any(|pk| !pk.area.is_finite()) {
        return None;
    }
    peaks.sort_by(|a, b| a.position.total_cmp(&b.position));

    Some(LineFit {
        shape,
        peaks,
        offset: p[m - 1],
        offset_sigma: sig(m - 1),
        r2,
    })
}

/// All 1σ standard errors in one pass: one analytic Jacobian, one JᵀJ.
/// Identically-zero columns (pinned parameters, e.g. a clamped `eta`) are
/// dropped before inversion so they cannot poison the remaining sigmas; the
/// rest are equilibrated by column norm for conditioning.
fn param_sigmas(problem: &mut LineshapeProblem, p: &[f64]) -> Vec<Option<f64>> {
    let m = p.len();
    let n = problem.xs.len();
    let mut jtj = vec![vec![0.0; m]; m];
    let mut jtr = vec![0.0; m];
    problem.normal_equations(p, &mut jtj, &mut jtr);
    let ss: f64 = problem.resid.iter().map(|r| r * r).sum();
    let mut out = vec![None; m];
    let active: Vec<usize> = (0..m).filter(|&j| jtj[j][j] > 0.0).collect();
    let ma = active.len();
    if ma == 0 || n <= ma {
        return out;
    }
    let d: Vec<f64> = active.iter().map(|&j| 1.0 / jtj[j][j].sqrt()).collect();
    let scaled: Vec<Vec<f64>> = (0..ma)
        .map(|a| {
            (0..ma)
                .map(|b| jtj[active[a]][active[b]] * d[a] * d[b])
                .collect()
        })
        .collect();
    let variance = ss / (n - ma) as f64;
    for (a, &j) in active.iter().enumerate() {
        let mut unit = vec![0.0; ma];
        unit[a] = 1.0;
        if let Some(inv_col) = solve_linear(&scaled, &unit)
            && inv_col[a] > 0.0
        {
            let s = d[a] * (variance * inv_col[a]).sqrt();
            if s.is_finite() {
                out[j] = Some(s);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests;
