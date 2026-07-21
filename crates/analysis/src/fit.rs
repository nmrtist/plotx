/// Coefficient of determination of `pred` against `y`.
pub fn r_squared(x: &[f64], y: &[f64], pred: impl Fn(f64) -> f64) -> f64 {
    let n = x.len();
    if n == 0 {
        return 0.0;
    }
    let mean = y.iter().sum::<f64>() / n as f64;
    let mut ss_res = 0.0;
    let mut ss_tot = 0.0;
    for (&xi, &yi) in x.iter().zip(y) {
        let r = yi - pred(xi);
        ss_res += r * r;
        ss_tot += (yi - mean) * (yi - mean);
    }
    if ss_tot <= f64::MIN_POSITIVE {
        return 0.0;
    }
    1.0 - ss_res / ss_tot
}

const REL_COST_TOL: f64 = 1e-6;

/// A least-squares problem the Levenberg–Marquardt core can drive: the sum of
/// squared residuals at `p`, and the normal-equation pieces `JᵀJ` (full m×m)
/// and `Jᵀ·r` at `p`. Implementations may keep reusable scratch state.
pub trait LmProblem {
    fn cost(&mut self, p: &[f64]) -> f64;
    fn normal_equations(&mut self, p: &[f64], jtj: &mut [Vec<f64>], jtr: &mut [f64]);
}

/// Levenberg–Marquardt with a numerical (central-difference) Jacobian. Fits
/// `params` so `model(params, x_i) ≈ y_i`. Returns the converged parameters, or
/// `None` if the normal equations stayed singular.
pub fn levenberg_marquardt(
    x: &[f64],
    y: &[f64],
    params0: &[f64],
    model: impl Fn(&[f64], f64) -> f64,
    max_iter: usize,
) -> Option<Vec<f64>> {
    levenberg_marquardt_cancellable(x, y, params0, model, max_iter, &|| false)
}

/// Cooperative-cancellation variant for fits run by the desktop task service.
/// Cancellation is checked between LM iterations and damped trial steps.
pub fn levenberg_marquardt_cancellable(
    x: &[f64],
    y: &[f64],
    params0: &[f64],
    model: impl Fn(&[f64], f64) -> f64,
    max_iter: usize,
    cancelled: &impl Fn() -> bool,
) -> Option<Vec<f64>> {
    let m = params0.len();
    if x.len() < m {
        return None;
    }
    let mut problem = NumericalProblem {
        x,
        y,
        model,
        grad: vec![0.0; m],
        pj: vec![0.0; m],
    };
    // A zero tolerance keeps the historical exit (max_iter or a stalled step).
    lm_core(&mut problem, params0, max_iter, 0.0, cancelled).map(|(p, _)| p)
}

/// Levenberg–Marquardt over a caller-supplied [`LmProblem`] (typically with an
/// analytic Jacobian). Besides the historical exits it stops once two
/// consecutive accepted steps each improve the cost by less than a small
/// relative tolerance. Returns the converged parameters and the number of
/// iterations consumed.
pub fn levenberg_marquardt_problem(
    problem: &mut impl LmProblem,
    params0: &[f64],
    max_iter: usize,
) -> Option<(Vec<f64>, usize)> {
    levenberg_marquardt_problem_cancellable(problem, params0, max_iter, &|| false)
}

pub fn levenberg_marquardt_problem_cancellable(
    problem: &mut impl LmProblem,
    params0: &[f64],
    max_iter: usize,
    cancelled: &impl Fn() -> bool,
) -> Option<(Vec<f64>, usize)> {
    lm_core(problem, params0, max_iter, REL_COST_TOL, cancelled)
}

struct NumericalProblem<'a, F> {
    x: &'a [f64],
    y: &'a [f64],
    model: F,
    grad: Vec<f64>,
    pj: Vec<f64>,
}

impl<F: Fn(&[f64], f64) -> f64> LmProblem for NumericalProblem<'_, F> {
    fn cost(&mut self, p: &[f64]) -> f64 {
        sum_sq_residual(self.x, self.y, p, &self.model)
    }

    #[allow(clippy::needless_range_loop)]
    fn normal_equations(&mut self, p: &[f64], jtj: &mut [Vec<f64>], jtr: &mut [f64]) {
        let m = p.len();
        for row in jtj.iter_mut() {
            row.fill(0.0);
        }
        jtr.fill(0.0);
        for (&xk, &yk) in self.x.iter().zip(self.y) {
            let rk = yk - (self.model)(p, xk);
            self.pj.copy_from_slice(p);
            for j in 0..m {
                let h = jac_step(p[j]);
                self.pj[j] = p[j] + h;
                let fp = (self.model)(&self.pj, xk);
                self.pj[j] = p[j] - h;
                let fm = (self.model)(&self.pj, xk);
                self.pj[j] = p[j];
                self.grad[j] = (fp - fm) / (2.0 * h);
            }
            for a in 0..m {
                let ja = self.grad[a];
                jtr[a] += ja * rk;
                for b in a..m {
                    jtj[a][b] += ja * self.grad[b];
                }
            }
        }
        mirror_upper(jtj);
    }
}

/// Copy the upper triangle onto the lower one.
#[allow(clippy::needless_range_loop)]
pub(crate) fn mirror_upper(jtj: &mut [Vec<f64>]) {
    for a in 1..jtj.len() {
        for b in 0..a {
            jtj[a][b] = jtj[b][a];
        }
    }
}

fn lm_core(
    problem: &mut impl LmProblem,
    params0: &[f64],
    max_iter: usize,
    rel_tol: f64,
    cancelled: &impl Fn() -> bool,
) -> Option<(Vec<f64>, usize)> {
    let m = params0.len();
    let mut p = params0.to_vec();
    let mut lambda = 1e-3;
    let mut cost = problem.cost(&p);

    let mut jtj = vec![vec![0.0; m]; m];
    let mut jtr = vec![0.0; m];
    let mut trial = vec![0.0; m];
    let mut aug = vec![vec![0.0; m]; m];
    let mut rhs = vec![0.0; m];
    let mut small_steps = 0;
    let mut iters = 0;

    for it in 0..max_iter {
        if cancelled() {
            return None;
        }
        iters = it + 1;
        problem.normal_equations(&p, &mut jtj, &mut jtr);

        // Try damped steps, growing lambda until the cost drops.
        let mut improved = false;
        for _ in 0..12 {
            if cancelled() {
                return None;
            }
            for (dst, src) in aug.iter_mut().zip(&jtj) {
                dst.copy_from_slice(src);
            }
            for i in 0..m {
                aug[i][i] += lambda * jtj[i][i].max(1e-12);
            }
            rhs.copy_from_slice(&jtr);
            if solve_in_place(&mut aug, &mut rhs) {
                trial.copy_from_slice(&p);
                for i in 0..m {
                    trial[i] += rhs[i];
                }
                let new_cost = problem.cost(&trial);
                if new_cost < cost {
                    let rel = (cost - new_cost) / cost.max(f64::MIN_POSITIVE);
                    std::mem::swap(&mut p, &mut trial);
                    cost = new_cost;
                    lambda = (lambda * 0.5).max(1e-9);
                    improved = true;
                    if rel < rel_tol {
                        small_steps += 1;
                    } else {
                        small_steps = 0;
                    }
                    break;
                }
            }
            lambda *= 4.0;
            if lambda > 1e12 {
                break;
            }
        }
        if !improved || small_steps >= 2 {
            break;
        }
    }
    Some((p, iters))
}

pub fn sum_sq_residual(
    x: &[f64],
    y: &[f64],
    p: &[f64],
    model: &impl Fn(&[f64], f64) -> f64,
) -> f64 {
    x.iter()
        .zip(y)
        .map(|(&xi, &yi)| {
            let r = yi - model(p, xi);
            r * r
        })
        .sum()
}

#[inline]
pub fn jac_step(v: f64) -> f64 {
    let a = v.abs();
    if a > 1e-8 { a * 1e-6 } else { 1e-8 }
}

/// Weighted least-squares line `ln(y) = intercept + slope*x`, weighting each
/// point by `y^2` so high-signal points dominate.
pub(crate) fn weighted_log_line(x: &[f64], y: &[f64]) -> Option<(f64, f64)> {
    let (mut sw, mut swx, mut swy, mut swxx, mut swxy) = (0.0, 0.0, 0.0, 0.0, 0.0);
    let mut count = 0;
    for (&xi, &yi) in x.iter().zip(y) {
        if yi > 0.0 && xi.is_finite() {
            let w = yi * yi;
            let ly = yi.ln();
            sw += w;
            swx += w * xi;
            swy += w * ly;
            swxx += w * xi * xi;
            swxy += w * xi * ly;
            count += 1;
        }
    }
    if count < 2 {
        return None;
    }
    let denom = sw * swxx - swx * swx;
    if denom.abs() < f64::MIN_POSITIVE {
        return None;
    }
    let slope = (sw * swxy - swx * swy) / denom;
    let intercept = (swy - slope * swx) / sw;
    Some((intercept, slope))
}

/// One-standard-deviation uncertainty for parameter `pi`, estimated from the
/// residual variance and the inverse numerical-Jacobian normal matrix.
pub(crate) fn param_sigma(
    x: &[f64],
    y: &[f64],
    p: &[f64],
    model: impl Fn(&[f64], f64) -> f64,
    pi: usize,
) -> f64 {
    let m = p.len();
    let n = x.len();
    if n <= m || pi >= m {
        return f64::NAN;
    }
    let scale: Vec<f64> = p
        .iter()
        .map(|&value| if value.abs() > 0.0 { value.abs() } else { 1.0 })
        .collect();
    let mut jtj = vec![vec![0.0; m]; m];
    let mut ss = 0.0;
    for k in 0..n {
        let residual = y[k] - model(p, x[k]);
        ss += residual * residual;
        let mut gradient = vec![0.0; m];
        for j in 0..m {
            let h = jac_step(p[j]);
            let mut pj = p.to_vec();
            pj[j] = p[j] + h;
            let fp = model(&pj, x[k]);
            pj[j] = p[j] - h;
            let fm = model(&pj, x[k]);
            gradient[j] = (fp - fm) / (2.0 * h) * scale[j];
        }
        for a in 0..m {
            for b in 0..m {
                jtj[a][b] += gradient[a] * gradient[b];
            }
        }
    }
    let variance = ss / (n - m) as f64;
    let mut unit = vec![0.0; m];
    unit[pi] = 1.0;
    match solve_linear(&jtj, &unit) {
        Some(inv_col) => (variance * inv_col[pi]).max(0.0).sqrt() * scale[pi],
        None => f64::NAN,
    }
}

/// Solve `a·x = b` for a small symmetric system by Gauss–Jordan with partial
/// pivoting. Returns `None` if singular.
pub fn solve_linear(a: &[Vec<f64>], b: &[f64]) -> Option<Vec<f64>> {
    let mut m = a.to_vec();
    let mut r = b.to_vec();
    solve_in_place(&mut m, &mut r).then_some(r)
}

/// In-place Gauss–Jordan with partial pivoting: on success `r` holds the
/// solution; `m` is destroyed either way. Returns `false` if singular.
#[allow(clippy::needless_range_loop)]
fn solve_in_place(m: &mut [Vec<f64>], r: &mut [f64]) -> bool {
    let n = r.len();
    for col in 0..n {
        let mut pivot = col;
        for row in (col + 1)..n {
            if m[row][col].abs() > m[pivot][col].abs() {
                pivot = row;
            }
        }
        if m[pivot][col].abs() < 1e-15 {
            return false;
        }
        m.swap(col, pivot);
        r.swap(col, pivot);
        let d = m[col][col];
        for j in col..n {
            m[col][j] /= d;
        }
        r[col] /= d;
        for row in 0..n {
            if row != col {
                let factor = m[row][col];
                for j in col..n {
                    m[row][j] -= factor * m[col][j];
                }
                r[row] -= factor * r[col];
            }
        }
    }
    true
}
