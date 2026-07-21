//! The domain-neutral window reducer behind the Regions tool: collapse one
//! x-window of a series stack to a single value per increment. Shares the signed
//! reference-phasor projection of [`crate::series::extract_series`] (so values
//! are absorptive and comparable across increments) but offers a choice of
//! reducers, so future spectral / XPS / cytometry series can reuse it.

use crate::SpectrumStack;
use crate::pseudo2d_impl::{DecaySeries, window_indices};

/// How a window collapses to one value per increment. `Height`/`Area` mirror
/// [`crate::series::IntensityMode`]; the rest generalise it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReduceOp {
    Height,
    Area,
    Max,
    Min,
    Mean,
}

/// Extract a per-increment series by reducing the ppm `range` with `op`.
pub fn extract_region_series(
    stack: &impl SpectrumStack,
    axis_values: &[f64],
    range: (f64, f64),
    op: ReduceOp,
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
    let Some((start, end)) = window_indices(coordinates, Some(range)) else {
        return DecaySeries {
            x: Vec::new(),
            y: Vec::new(),
        };
    };
    let x = axis_values[..n].to_vec();

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
    let project = |i: usize, c: usize| {
        let r = traces[strongest][c];
        let m = r.norm();
        if m <= f64::MIN_POSITIVE {
            0.0
        } else {
            (traces[i][c] * r.conj()).re / m
        }
    };
    let peak_col = (start..end)
        .max_by(|&a, &b| {
            traces[strongest][a]
                .norm()
                .partial_cmp(&traces[strongest][b].norm())
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .unwrap_or(start);
    let width = (end - start).max(1) as f64;

    let mut y: Vec<f64> = (0..n)
        .map(|i| match op {
            ReduceOp::Height => project(i, peak_col),
            ReduceOp::Area => (start..end).map(|c| project(i, c)).sum(),
            ReduceOp::Mean => (start..end).map(|c| project(i, c)).sum::<f64>() / width,
            ReduceOp::Max => (start..end)
                .map(|c| project(i, c))
                .fold(f64::NEG_INFINITY, f64::max),
            ReduceOp::Min => (start..end)
                .map(|c| project(i, c))
                .fold(f64::INFINITY, f64::min),
        })
        .collect();

    let tail = (n - n.div_ceil(3)).min(n.saturating_sub(1));
    let tail_mean = y[tail..].iter().sum::<f64>() / (n - tail).max(1) as f64;
    if tail_mean < 0.0 {
        for v in &mut y {
            *v = -*v;
        }
    }
    DecaySeries { x, y }
}
