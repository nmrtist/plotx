//! Real-valued XPS intensity processing in binding-energy coordinates.
use crate::{NormalizeMethod, SmoothMethod};

/// Gaussian smoothing for real-valued detection helpers that need a stable,
/// symmetric kernel but are not persisted processing steps.
pub fn gaussian_smooth_real(values: &[f64], sigma: f64) -> Option<Vec<f64>> {
    if values.is_empty()
        || !sigma.is_finite()
        || sigma <= 0.0
        || values.iter().any(|value| !value.is_finite())
    {
        return None;
    }
    let radius = (3.0 * sigma).ceil() as isize;
    Some(
        (0..values.len())
            .map(|index| {
                let mut weighted = 0.0;
                let mut total = 0.0;
                for offset in -radius..=radius {
                    let source =
                        (index as isize + offset).clamp(0, values.len() as isize - 1) as usize;
                    let weight = (-0.5 * (offset as f64 / sigma).powi(2)).exp();
                    weighted += values[source] * weight;
                    total += weight;
                }
                weighted / total
            })
            .collect(),
    )
}

fn moving_average(values: &mut Vec<f64>, window: usize) {
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
        let sum: f64 = values[lo..hi].iter().sum();
        out.push(sum / (hi - lo) as f64);
    }
    *values = out;
}

/// Least-squares polynomial smoothing: each point is replaced by the value of a
/// degree-`order` polynomial fitted over an odd `window` around it. Edge points
/// reuse the boundary window, evaluated off-center, so a polynomial signal of
/// degree ≤ `order` is reproduced exactly everywhere.
fn savitzky_golay(values: &mut Vec<f64>, window: usize, order: usize) {
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
        let mut acc = 0.0;
        for (j, &weight) in weights[p].iter().enumerate() {
            acc += values[start + j] * weight;
        }
        out.push(acc);
    }
    *values = out;
}

pub fn normalize(axis: &[f64], values: &mut [f64], method: NormalizeMethod) {
    let scale = match method {
        NormalizeMethod::MaxPeak => values.iter().map(|c| c.abs()).fold(0.0, f64::max),
        NormalizeMethod::TotalArea => values.iter().map(|c| c.abs()).sum::<f64>() * axis_step(axis),
        NormalizeMethod::Constant { divisor } => divisor,
    };
    if scale.is_finite() && scale.abs() > f64::MIN_POSITIVE {
        for c in values {
            *c /= scale;
        }
    }
}

pub fn axis_step(ppm: &[f64]) -> f64 {
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

pub fn smooth(values: &[f64], method: SmoothMethod) -> Vec<f64> {
    let mut values = values.to_vec();
    match method {
        SmoothMethod::MovingAverage { window } => moving_average(&mut values, window as usize),
        SmoothMethod::SavitzkyGolay { window, poly_order } => {
            savitzky_golay(&mut values, window as usize, poly_order as usize)
        }
    }
    values
}
