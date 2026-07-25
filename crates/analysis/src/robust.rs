//! Robust scale estimators shared by scientific field providers.

/// Stable identity and implementation version for the 2D NMR noise estimator.
pub const ROBUST_DIFFERENCE_MAD_ID: &str = "robust_difference_mad";
pub const ROBUST_DIFFERENCE_MAD_VERSION: u32 = 1;

/// Stable identity and implementation version for the de-planed background
/// estimator used by scalar height-like fields.
pub const DEPLANED_LOCATION_SCALE_ID: &str = "deplaned_location_scale";
pub const DEPLANED_LOCATION_SCALE_VERSION: u32 = 1;

/// Estimate white-noise scale from horizontal and vertical first differences.
/// Differences never cross a row boundary; combining both axes avoids giving a
/// preferred direction to a regular 2D spectrum.
pub fn robust_difference_mad(values: &[f32], rows: usize, cols: usize) -> f64 {
    let mut deviations = Vec::new();
    for row in 0..rows {
        let Some(offset) = row.checked_mul(cols) else {
            return 0.0;
        };
        for col in 0..cols.saturating_sub(1) {
            let Some(left) = values.get(offset + col) else {
                return 0.0;
            };
            let Some(right) = values.get(offset + col + 1) else {
                return 0.0;
            };
            if left.is_finite() && right.is_finite() {
                deviations.push(f64::from(*right - *left));
            }
        }
    }
    for row in 0..rows.saturating_sub(1) {
        let Some(next_row) = row.checked_add(1).and_then(|next| next.checked_mul(cols)) else {
            return 0.0;
        };
        let Some(offset) = row.checked_mul(cols) else {
            return 0.0;
        };
        for col in 0..cols {
            let Some(top) = values.get(offset + col) else {
                return 0.0;
            };
            let Some(bottom) = values.get(next_row + col) else {
                return 0.0;
            };
            if top.is_finite() && bottom.is_finite() {
                deviations.push(f64::from(*bottom - *top));
            }
        }
    }
    mad_scale(&mut deviations).map_or(0.0, |scale| scale / std::f64::consts::SQRT_2)
}

/// Estimate an AFM-style background after subtracting its least-squares plane.
/// The reported location is that plane at the grid centre while the spread is a
/// MAD of residuals, so a sample tilt cannot turn into contour noise.
pub fn deplaned_location_scale(values: &[f32], rows: usize, cols: usize) -> (f64, f64) {
    let Some((offset, slope_x, slope_y)) = least_squares_plane(values, rows, cols) else {
        let mut finite = values
            .iter()
            .copied()
            .filter(|value| value.is_finite())
            .map(f64::from)
            .collect::<Vec<_>>();
        let location = median(&mut finite).unwrap_or(0.0);
        let scale = mad_scale(&mut finite).unwrap_or(0.0);
        return (location, scale);
    };
    let mut residuals = Vec::new();
    for row in 0..rows {
        for col in 0..cols {
            let Some(index) = row
                .checked_mul(cols)
                .and_then(|offset| offset.checked_add(col))
            else {
                return (0.0, 0.0);
            };
            let Some(value) = values.get(index).filter(|value| value.is_finite()) else {
                continue;
            };
            let plane = offset + slope_x * col as f64 + slope_y * row as f64;
            residuals.push(f64::from(*value) - plane);
        }
    }
    let location = offset
        + slope_x * cols.saturating_sub(1) as f64 / 2.0
        + slope_y * rows.saturating_sub(1) as f64 / 2.0;
    (location, mad_scale(&mut residuals).unwrap_or(0.0))
}

fn least_squares_plane(values: &[f32], rows: usize, cols: usize) -> Option<(f64, f64, f64)> {
    let mut normal = [[0.0; 3]; 3];
    let mut rhs = [0.0; 3];
    for row in 0..rows {
        for col in 0..cols {
            let index = row.checked_mul(cols)?.checked_add(col)?;
            let value = f64::from(*values.get(index)?);
            if !value.is_finite() {
                continue;
            }
            let point = [1.0, col as f64, row as f64];
            for i in 0..3 {
                rhs[i] += point[i] * value;
                for j in 0..3 {
                    normal[i][j] += point[i] * point[j];
                }
            }
        }
    }
    for pivot in 0..3 {
        let best = (pivot..3).max_by(|&left, &right| {
            normal[left][pivot]
                .abs()
                .total_cmp(&normal[right][pivot].abs())
        })?;
        if normal[best][pivot].abs() <= f64::EPSILON {
            return None;
        }
        if best != pivot {
            normal.swap(pivot, best);
            rhs.swap(pivot, best);
        }
        let divisor = normal[pivot][pivot];
        for value in &mut normal[pivot][pivot..] {
            *value /= divisor;
        }
        rhs[pivot] /= divisor;
        let pivot_row = normal[pivot];
        for row in 0..3 {
            if row == pivot {
                continue;
            }
            let factor = normal[row][pivot];
            for (value, pivot_value) in normal[row][pivot..]
                .iter_mut()
                .zip(pivot_row[pivot..].iter())
            {
                *value -= factor * *pivot_value;
            }
            rhs[row] -= factor * rhs[pivot];
        }
    }
    Some((rhs[0], rhs[1], rhs[2]))
}

fn mad_scale(values: &mut [f64]) -> Option<f64> {
    let center = median(values)?;
    for value in &mut *values {
        *value = (*value - center).abs();
    }
    median(values).map(|mad| mad / 0.674_489_750_196_081_7)
}

fn median(values: &mut [f64]) -> Option<f64> {
    if values.is_empty() {
        return None;
    }
    let middle = values.len() / 2;
    let (_, upper, _) = values.select_nth_unstable_by(middle, f64::total_cmp);
    let upper = *upper;
    if values.len().is_multiple_of(2) {
        let lower = values[..middle].iter().copied().max_by(f64::total_cmp)?;
        Some((lower + upper) / 2.0)
    } else {
        Some(upper)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn white_noise_estimate_is_close_to_its_known_sigma() {
        let mut state = 0x4d59_5df4_d0f3_3173_u64;
        let mut uniform = || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            (state as f64 / u64::MAX as f64).max(f64::MIN_POSITIVE)
        };
        let values = (0..128 * 128)
            .map(|_| {
                let radius = (-2.0 * uniform().ln()).sqrt();
                (radius * (std::f64::consts::TAU * uniform()).cos()) as f32
            })
            .collect::<Vec<_>>();
        let sigma = robust_difference_mad(&values, 128, 128);
        assert!((sigma - 1.0).abs() < 0.08, "estimated sigma was {sigma}");
    }

    #[test]
    fn correlated_noise_has_a_smaller_difference_scale() {
        let values = (0..32)
            .flat_map(|row| (0..32).map(move |col| row as f32 + col as f32 * 0.01))
            .collect::<Vec<_>>();
        let sigma = robust_difference_mad(&values, 32, 32);
        assert!(
            sigma < 0.6,
            "correlation should reduce the difference scale"
        );
    }

    #[test]
    fn degenerate_grids_have_zero_scale() {
        assert_eq!(robust_difference_mad(&[0.0], 1, 1), 0.0);
        assert_eq!(robust_difference_mad(&[0.0, 0.0], 1, 2), 0.0);
        assert_eq!(robust_difference_mad(&[0.0, 0.0], 2, 1), 0.0);
    }
}
