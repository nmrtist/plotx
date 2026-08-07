use super::{AxisSampling, ScalarGrid2D};
use plotx_analysis::diffusion::DiffusionMap;
use std::sync::Arc;

pub(crate) const DOSY_GRID_COLS: usize = 512;
pub(crate) const DOSY_GRID_ROWS: usize = 300;

/// Materialize the scalar grid represented by the mono-exponential DOSY field.
/// This matches the Gaussian deposition used by the figure builder.
pub(crate) fn dosy_scalar_grid(map: &DiffusionMap) -> ScalarGrid2D {
    let fitted = map
        .ppm
        .iter()
        .zip(&map.d)
        .zip(&map.amp)
        .filter_map(|((&ppm, &d), &amp)| {
            (d.is_finite() && d > 0.0).then_some((ppm, d.log10(), amp))
        })
        .collect::<Vec<_>>();
    let (ppm_lo, ppm_hi) = map
        .ppm
        .iter()
        .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), &ppm| {
            (lo.min(ppm), hi.max(ppm))
        });
    let (mut logd_lo, mut logd_hi) = fitted.iter().fold(
        (f64::INFINITY, f64::NEG_INFINITY),
        |(lo, hi), &(_, logd, _)| (lo.min(logd), hi.max(logd)),
    );
    if fitted.is_empty() {
        (logd_lo, logd_hi) = (-10.5, -8.5);
    } else {
        logd_lo -= 0.5;
        logd_hi += 0.5;
    }
    let x_span = (ppm_hi - ppm_lo).max(f64::MIN_POSITIVE);
    let y_span = (logd_hi - logd_lo).max(f64::MIN_POSITIVE);
    let mut values = vec![0.0_f32; DOSY_GRID_COLS * DOSY_GRID_ROWS];
    for (ppm, logd, amplitude) in fitted {
        let cx = ((ppm - ppm_lo) / x_span * (DOSY_GRID_COLS - 1) as f64).round() as isize;
        let cy = ((logd - logd_lo) / y_span * (DOSY_GRID_ROWS - 1) as f64).round() as isize;
        for dy in -9..=9 {
            let row = cy + dy;
            if !(0..DOSY_GRID_ROWS as isize).contains(&row) {
                continue;
            }
            for dx in -4..=4 {
                let col = cx + dx;
                if !(0..DOSY_GRID_COLS as isize).contains(&col) {
                    continue;
                }
                let weight = (-(dx as f64).powi(2) / (2.0 * 1.5_f64.powi(2))
                    - (dy as f64).powi(2) / (2.0 * 3.0_f64.powi(2)))
                .exp();
                values[row as usize * DOSY_GRID_COLS + col as usize] += (amplitude * weight) as f32;
            }
        }
    }
    ScalarGrid2D {
        values: Arc::from(values),
        rows: DOSY_GRID_ROWS,
        cols: DOSY_GRID_COLS,
        x: AxisSampling::Linear {
            start: ppm_lo,
            end: ppm_hi,
        },
        y: AxisSampling::Linear {
            start: logd_lo,
            end: logd_hi,
        },
    }
}
