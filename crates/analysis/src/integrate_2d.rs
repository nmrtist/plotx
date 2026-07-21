//! Rectangular volume integration over a reduced two-dimensional spectrum.

use serde::{Deserialize, Serialize};
use std::{error::Error, fmt};

/// Local baseline correction applied before integrating a rectangle.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BaselineMode {
    /// Integrate the reduced surface without changing it.
    #[default]
    None,
    /// Subtract the median intensity along the rectangle perimeter.
    Constant,
    /// Subtract a least-squares plane fitted to the rectangle perimeter.
    Plane,
}

/// Invalid input supplied to [`integrate_rectangular_volume`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IntegrateError {
    /// An axis length does not agree with its declared dimension.
    AxisSizeMismatch {
        axis: &'static str,
        expected: usize,
        actual: usize,
    },
    /// The grid length does not equal `f2_size * f1_size`.
    GridSizeMismatch { expected: usize, actual: usize },
    /// Multiplying the declared dimensions overflowed `usize`.
    DimensionOverflow,
    /// An input contains a NaN or infinity.
    NonFiniteValue { input: &'static str, index: usize },
    /// A rectangle boundary is a NaN or infinity.
    NonFiniteBoundary,
    /// An axis is not strictly monotonic.
    NonMonotonicAxis { axis: &'static str },
    /// The local plane fit could not be solved reliably.
    SingularBaselinePlane,
}

impl fmt::Display for IntegrateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AxisSizeMismatch {
                axis,
                expected,
                actual,
            } => write!(
                f,
                "{axis} axis has {actual} values, but its declared size is {expected}"
            ),
            Self::GridSizeMismatch { expected, actual } => write!(
                f,
                "2D grid has {actual} values, but its dimensions require {expected}"
            ),
            Self::DimensionOverflow => f.write_str("2D grid dimensions overflow usize"),
            Self::NonFiniteValue { input, index } => {
                write!(f, "{input} contains a non-finite value at index {index}")
            }
            Self::NonFiniteBoundary => f.write_str("integration bounds must be finite"),
            Self::NonMonotonicAxis { axis } => {
                write!(f, "{axis} axis must be strictly monotonic")
            }
            Self::SingularBaselinePlane => {
                f.write_str("could not fit a baseline plane to the rectangle perimeter")
            }
        }
    }
}

impl Error for IntegrateError {}

mod prepared;
pub use prepared::IntegrationGrid2D;

/// Integrate a rectangle over a row-major reduced 2D spectrum.
///
/// `grid` contains `f1_size` rows of `f2_size` values. The bounds and axes are
/// in ppm. Bounds may be supplied in either order and are clipped to the axis
/// extents. The returned unit is intensity·ppm².
#[allow(clippy::too_many_arguments)]
pub fn integrate_rectangular_volume(
    f2_ppm: &[f64],
    f1_ppm: &[f64],
    grid: &[f64],
    f2_size: usize,
    f1_size: usize,
    f2_bounds: (f64, f64),
    f1_bounds: (f64, f64),
    baseline: BaselineMode,
) -> Result<f64, IntegrateError> {
    IntegrationGrid2D::new(f2_ppm, f1_ppm, grid, f2_size, f1_size)?
        .integrate(f2_bounds, f1_bounds, baseline)
}

#[allow(clippy::too_many_arguments)]
fn integrate_validated(
    f2_ppm: &[f64],
    f1_ppm: &[f64],
    grid: &[f64],
    f2_size: usize,
    f1_size: usize,
    f2_bounds: (f64, f64),
    f1_bounds: (f64, f64),
    baseline: BaselineMode,
) -> Result<f64, IntegrateError> {
    if f2_size < 2 || f1_size < 2 {
        return Ok(0.0);
    }

    let f2 = Axis::new(f2_ppm);
    let f1 = Axis::new(f1_ppm);
    let Some((f2_lo, f2_hi)) = clipped_bounds(f2_bounds, f2.min(), f2.max()) else {
        return Ok(0.0);
    };
    let Some((f1_lo, f1_hi)) = clipped_bounds(f1_bounds, f1.min(), f1.max()) else {
        return Ok(0.0);
    };

    let xs = integration_coordinates(f2, f2_lo, f2_hi);
    let ys = integration_coordinates(f1, f1_lo, f1_hi);
    if xs.len() < 2 || ys.len() < 2 {
        return Ok(0.0);
    }

    let baseline = fit_baseline(baseline, &xs, &ys, f2, f1, grid, f2_size)?;
    let mut volume = 0.0;
    for y_pair in ys.windows(2) {
        let y0 = y_pair[0];
        let y1 = y_pair[1];
        for x_pair in xs.windows(2) {
            let x0 = x_pair[0];
            let x1 = x_pair[1];
            let cell_sum = corrected_value(x0, y0, baseline, f2, f1, grid, f2_size)
                + corrected_value(x1, y0, baseline, f2, f1, grid, f2_size)
                + corrected_value(x0, y1, baseline, f2, f1, grid, f2_size)
                + corrected_value(x1, y1, baseline, f2, f1, grid, f2_size);
            volume += cell_sum * (x1 - x0).abs() * (y1 - y0).abs() * 0.25;
        }
    }
    Ok(volume)
}

#[allow(clippy::too_many_arguments)]
fn validate_grid(
    f2_ppm: &[f64],
    f1_ppm: &[f64],
    grid: &[f64],
    f2_size: usize,
    f1_size: usize,
) -> Result<(), IntegrateError> {
    if f2_ppm.len() != f2_size {
        return Err(IntegrateError::AxisSizeMismatch {
            axis: "F2",
            expected: f2_size,
            actual: f2_ppm.len(),
        });
    }
    if f1_ppm.len() != f1_size {
        return Err(IntegrateError::AxisSizeMismatch {
            axis: "F1",
            expected: f1_size,
            actual: f1_ppm.len(),
        });
    }
    let expected = f2_size
        .checked_mul(f1_size)
        .ok_or(IntegrateError::DimensionOverflow)?;
    if grid.len() != expected {
        return Err(IntegrateError::GridSizeMismatch {
            expected,
            actual: grid.len(),
        });
    }
    validate_finite("F2 axis", f2_ppm)?;
    validate_finite("F1 axis", f1_ppm)?;
    validate_finite("2D grid", grid)?;
    validate_monotonic("F2", f2_ppm)?;
    validate_monotonic("F1", f1_ppm)?;
    Ok(())
}

fn validate_bounds(f2_bounds: (f64, f64), f1_bounds: (f64, f64)) -> Result<(), IntegrateError> {
    if [f2_bounds.0, f2_bounds.1, f1_bounds.0, f1_bounds.1]
        .into_iter()
        .all(f64::is_finite)
    {
        Ok(())
    } else {
        Err(IntegrateError::NonFiniteBoundary)
    }
}

fn validate_finite(input: &'static str, values: &[f64]) -> Result<(), IntegrateError> {
    if let Some(index) = values.iter().position(|value| !value.is_finite()) {
        return Err(IntegrateError::NonFiniteValue { input, index });
    }
    Ok(())
}

fn validate_monotonic(axis: &'static str, values: &[f64]) -> Result<(), IntegrateError> {
    if values.len() < 2 {
        return Ok(());
    }
    let ascending = values[1] > values[0];
    if values[1] == values[0]
        || values.windows(2).any(|pair| {
            if ascending {
                pair[1] <= pair[0]
            } else {
                pair[1] >= pair[0]
            }
        })
    {
        return Err(IntegrateError::NonMonotonicAxis { axis });
    }
    Ok(())
}

fn clipped_bounds(bounds: (f64, f64), min: f64, max: f64) -> Option<(f64, f64)> {
    let (lo, hi) = if bounds.0 <= bounds.1 {
        bounds
    } else {
        (bounds.1, bounds.0)
    };
    let lo = lo.max(min);
    let hi = hi.min(max);
    (hi > lo).then_some((lo, hi))
}

#[derive(Clone, Copy)]
struct Axis<'a> {
    values: &'a [f64],
    ascending: bool,
}

impl<'a> Axis<'a> {
    fn new(values: &'a [f64]) -> Self {
        Self {
            values,
            ascending: values.len() < 2 || values[1] > values[0],
        }
    }

    fn get(self, ascending_index: usize) -> f64 {
        self.values[self.source_index(ascending_index)]
    }

    fn source_index(self, ascending_index: usize) -> usize {
        if self.ascending {
            ascending_index
        } else {
            self.values.len() - 1 - ascending_index
        }
    }

    fn min(self) -> f64 {
        self.get(0)
    }

    fn max(self) -> f64 {
        self.get(self.values.len() - 1)
    }

    fn lower_bound(self, needle: f64) -> usize {
        let mut lo = 0;
        let mut hi = self.values.len();
        while lo < hi {
            let mid = lo + (hi - lo) / 2;
            if self.get(mid) < needle {
                lo = mid + 1;
            } else {
                hi = mid;
            }
        }
        lo
    }

    fn bracket(self, value: f64) -> (usize, usize, f64) {
        let upper = self.lower_bound(value).clamp(1, self.values.len() - 1);
        let lower = upper - 1;
        let fraction = (value - self.get(lower)) / (self.get(upper) - self.get(lower));
        (lower, upper, fraction.clamp(0.0, 1.0))
    }
}

fn integration_coordinates(axis: Axis<'_>, lo: f64, hi: f64) -> Vec<f64> {
    let start = axis.lower_bound(lo);
    let end = axis.lower_bound(hi);
    let mut coordinates = Vec::with_capacity(end.saturating_sub(start) + 2);
    coordinates.push(lo);
    for index in start..end {
        let value = axis.get(index);
        if value > lo && value < hi {
            coordinates.push(value);
        }
    }
    coordinates.push(hi);
    coordinates
}

#[derive(Clone, Copy)]
struct Plane {
    intercept: f64,
    x_slope: f64,
    y_slope: f64,
}

impl Plane {
    fn value(self, x: f64, y: f64) -> f64 {
        self.intercept + self.x_slope * x + self.y_slope * y
    }
}

fn fit_baseline(
    mode: BaselineMode,
    xs: &[f64],
    ys: &[f64],
    f2: Axis<'_>,
    f1: Axis<'_>,
    grid: &[f64],
    f2_size: usize,
) -> Result<Plane, IntegrateError> {
    if mode == BaselineMode::None {
        return Ok(Plane {
            intercept: 0.0,
            x_slope: 0.0,
            y_slope: 0.0,
        });
    }
    let mut perimeter = Vec::with_capacity(2 * xs.len() + 2 * ys.len());
    for &x in xs {
        perimeter.push((x, ys[0], interpolate(x, ys[0], f2, f1, grid, f2_size)));
        perimeter.push((
            x,
            ys[ys.len() - 1],
            interpolate(x, ys[ys.len() - 1], f2, f1, grid, f2_size),
        ));
    }
    for &y in &ys[1..ys.len() - 1] {
        perimeter.push((xs[0], y, interpolate(xs[0], y, f2, f1, grid, f2_size)));
        perimeter.push((
            xs[xs.len() - 1],
            y,
            interpolate(xs[xs.len() - 1], y, f2, f1, grid, f2_size),
        ));
    }

    if mode == BaselineMode::Constant {
        let mut values: Vec<_> = perimeter.iter().map(|point| point.2).collect();
        values.sort_by(f64::total_cmp);
        let middle = values.len() / 2;
        let median = if values.len() % 2 == 0 {
            values[middle - 1] * 0.5 + values[middle] * 0.5
        } else {
            values[middle]
        };
        return Ok(Plane {
            intercept: median,
            x_slope: 0.0,
            y_slope: 0.0,
        });
    }

    least_squares_plane(&perimeter)
}

fn least_squares_plane(points: &[(f64, f64, f64)]) -> Result<Plane, IntegrateError> {
    let count = points.len() as f64;
    let mean_x = points.iter().map(|point| point.0).sum::<f64>() / count;
    let mean_y = points.iter().map(|point| point.1).sum::<f64>() / count;
    let mean_z = points.iter().map(|point| point.2).sum::<f64>() / count;
    let (mut xx, mut xy, mut yy, mut xz, mut yz) = (0.0, 0.0, 0.0, 0.0, 0.0);
    for &(x, y, z) in points {
        let x = x - mean_x;
        let y = y - mean_y;
        let z = z - mean_z;
        xx += x * x;
        xy += x * y;
        yy += y * y;
        xz += x * z;
        yz += y * z;
    }
    let determinant = xx * yy - xy * xy;
    if !determinant.is_finite() || determinant <= f64::EPSILON * (xx * yy).abs() {
        return Err(IntegrateError::SingularBaselinePlane);
    }
    let x_slope = (xz * yy - yz * xy) / determinant;
    let y_slope = (yz * xx - xz * xy) / determinant;
    Ok(Plane {
        intercept: mean_z - x_slope * mean_x - y_slope * mean_y,
        x_slope,
        y_slope,
    })
}

fn corrected_value(
    x: f64,
    y: f64,
    baseline: Plane,
    f2: Axis<'_>,
    f1: Axis<'_>,
    grid: &[f64],
    f2_size: usize,
) -> f64 {
    interpolate(x, y, f2, f1, grid, f2_size) - baseline.value(x, y)
}

fn interpolate(x: f64, y: f64, f2: Axis<'_>, f1: Axis<'_>, grid: &[f64], f2_size: usize) -> f64 {
    let (x0, x1, tx) = f2.bracket(x);
    let (y0, y1, ty) = f1.bracket(y);
    let x0 = f2.source_index(x0);
    let x1 = f2.source_index(x1);
    let y0 = f1.source_index(y0);
    let y1 = f1.source_index(y1);
    let z00 = grid[y0 * f2_size + x0];
    let z10 = grid[y0 * f2_size + x1];
    let z01 = grid[y1 * f2_size + x0];
    let z11 = grid[y1 * f2_size + x1];
    let lower = z00 + tx * (z10 - z00);
    let upper = z01 + tx * (z11 - z01);
    lower + ty * (upper - lower)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grid(f2: &[f64], f1: &[f64], surface: impl Fn(f64, f64) -> f64) -> Vec<f64> {
        let mut values = Vec::with_capacity(f2.len() * f1.len());
        for &y in f1 {
            for &x in f2 {
                values.push(surface(x, y));
            }
        }
        values
    }

    fn integrate(
        f2: &[f64],
        f1: &[f64],
        values: &[f64],
        f2_bounds: (f64, f64),
        f1_bounds: (f64, f64),
        baseline: BaselineMode,
    ) -> Result<f64, IntegrateError> {
        integrate_rectangular_volume(
            f2,
            f1,
            values,
            f2.len(),
            f1.len(),
            f2_bounds,
            f1_bounds,
            baseline,
        )
    }

    fn assert_close(actual: f64, expected: f64) {
        let tolerance = 1e-11 * expected.abs().max(1.0);
        assert!(
            (actual - expected).abs() <= tolerance,
            "{actual} != {expected}"
        );
    }

    #[test]
    fn constant_surface_with_partial_edges_is_exact() {
        let f2 = [0.0, 1.0, 3.0, 6.0];
        let f1 = [-2.0, 0.5, 4.0];
        let values = grid(&f2, &f1, |_, _| 3.5);
        let result = integrate(
            &f2,
            &f1,
            &values,
            (0.25, 4.5),
            (-1.0, 2.0),
            BaselineMode::None,
        )
        .unwrap();
        assert_close(result, 3.5 * 4.25 * 3.0);
    }

    #[test]
    fn bilinear_surface_on_non_uniform_axes_is_exact() {
        let f2 = [-1.0, 0.2, 2.5, 5.0];
        let f1 = [-3.0, -0.5, 1.0, 4.0];
        let values = grid(&f2, &f1, |x, y| 2.0 + 3.0 * x - 0.5 * y + 4.0 * x * y);
        let (xa, xb) = (-0.4, 3.7);
        let (ya, yb) = (-2.2, 2.8);
        let antiderivative =
            |x: f64, y: f64| 2.0 * x * y + 1.5 * x * x * y - 0.25 * x * y * y + x * x * y * y;
        let expected = antiderivative(xb, yb) - antiderivative(xa, yb) - antiderivative(xb, ya)
            + antiderivative(xa, ya);
        let result = integrate(&f2, &f1, &values, (xa, xb), (ya, yb), BaselineMode::None).unwrap();
        assert_close(result, expected);
    }

    #[test]
    fn descending_axes_do_not_change_volume() {
        let ascending_f2 = [0.0, 1.0, 2.5, 5.0];
        let ascending_f1 = [-2.0, 0.0, 3.0];
        let descending_f2 = [5.0, 2.5, 1.0, 0.0];
        let descending_f1 = [3.0, 0.0, -2.0];
        let surface = |x, y| 1.0 + x * 2.0 - y + x * y * 0.25;
        let ascending = grid(&ascending_f2, &ascending_f1, surface);
        let descending = grid(&descending_f2, &descending_f1, surface);
        let a = integrate(
            &ascending_f2,
            &ascending_f1,
            &ascending,
            (0.3, 4.1),
            (-1.0, 2.0),
            BaselineMode::None,
        )
        .unwrap();
        let d = integrate(
            &descending_f2,
            &descending_f1,
            &descending,
            (0.3, 4.1),
            (-1.0, 2.0),
            BaselineMode::None,
        )
        .unwrap();
        assert_close(a, d);
    }

    #[test]
    fn rectangle_bound_order_does_not_change_volume() {
        let f2 = [0.0, 1.0, 3.0];
        let f1 = [-2.0, 0.0, 4.0];
        let values = grid(&f2, &f1, |x, y| 2.0 + x - y);
        let ordered = integrate(
            &f2,
            &f1,
            &values,
            (0.25, 2.0),
            (-1.0, 3.0),
            BaselineMode::None,
        )
        .unwrap();
        let reversed = integrate(
            &f2,
            &f1,
            &values,
            (2.0, 0.25),
            (3.0, -1.0),
            BaselineMode::None,
        )
        .unwrap();
        assert_close(ordered, reversed);
    }

    #[test]
    fn preserves_surface_sign() {
        let f2 = [0.0, 1.0, 2.0];
        let f1 = [0.0, 1.0, 2.0];
        let positive = vec![2.0; 9];
        let negative = vec![-2.0; 9];
        assert_close(
            integrate(
                &f2,
                &f1,
                &positive,
                (0.0, 2.0),
                (0.0, 2.0),
                BaselineMode::None,
            )
            .unwrap(),
            8.0,
        );
        assert_close(
            integrate(
                &f2,
                &f1,
                &negative,
                (0.0, 2.0),
                (0.0, 2.0),
                BaselineMode::None,
            )
            .unwrap(),
            -8.0,
        );
    }

    #[test]
    fn clips_partial_bounds_and_rejects_disjoint_bounds() {
        let f2 = [0.0, 1.0, 2.0];
        let f1 = [10.0, 11.0, 13.0];
        let values = vec![1.0; 9];
        assert_close(
            integrate(
                &f2,
                &f1,
                &values,
                (-5.0, 1.5),
                (9.0, 12.0),
                BaselineMode::None,
            )
            .unwrap(),
            3.0,
        );
        assert_eq!(
            integrate(
                &f2,
                &f1,
                &values,
                (3.0, 4.0),
                (10.0, 12.0),
                BaselineMode::None
            )
            .unwrap(),
            0.0
        );
    }

    #[test]
    fn degenerate_and_single_sample_axes_return_zero() {
        assert_eq!(
            integrate(
                &[1.0],
                &[2.0, 3.0],
                &[4.0, 5.0],
                (0.0, 2.0),
                (2.0, 3.0),
                BaselineMode::None
            )
            .unwrap(),
            0.0
        );
        assert_eq!(
            integrate(
                &[0.0, 1.0],
                &[0.0, 1.0],
                &[1.0; 4],
                (0.5, 0.5),
                (0.0, 1.0),
                BaselineMode::None,
            )
            .unwrap(),
            0.0
        );
    }

    #[test]
    fn constant_baseline_removes_dc_offset() {
        let f2 = [0.0, 1.0, 2.0, 3.0, 4.0];
        let f1 = [0.0, 1.0, 2.0, 3.0, 4.0];
        let values = grid(&f2, &f1, |x, y| {
            10.0 + if x == 2.0 && y == 2.0 { 8.0 } else { 0.0 }
        });
        let uncorrected = integrate(
            &f2,
            &f1,
            &values,
            (0.0, 4.0),
            (0.0, 4.0),
            BaselineMode::None,
        )
        .unwrap();
        let corrected = integrate(
            &f2,
            &f1,
            &values,
            (0.0, 4.0),
            (0.0, 4.0),
            BaselineMode::Constant,
        )
        .unwrap();
        assert_close(uncorrected, 168.0);
        assert_close(corrected, 8.0);
    }

    #[test]
    fn plane_baseline_removes_tilt() {
        let f2 = [-2.0, -0.25, 1.0, 3.0];
        let f1 = [-1.0, 0.5, 2.0, 5.0];
        let values = grid(&f2, &f1, |x, y| 7.0 + 2.5 * x - 0.75 * y);
        let corrected = integrate(
            &f2,
            &f1,
            &values,
            (-1.5, 2.5),
            (-0.5, 4.0),
            BaselineMode::Plane,
        )
        .unwrap();
        assert_close(corrected, 0.0);
    }

    #[test]
    fn uniform_grid_matches_cell_area_weighted_trapezoid_sum() {
        let f2 = [0.0, 0.5, 1.0];
        let f1 = [0.0, 2.0, 4.0];
        let values = grid(&f2, &f1, |x, y| 1.0 + x + y);
        let result = integrate(
            &f2,
            &f1,
            &values,
            (0.0, 1.0),
            (0.0, 4.0),
            BaselineMode::None,
        )
        .unwrap();
        let trapezoid_weights = [0.25, 0.5, 0.25, 0.5, 1.0, 0.5, 0.25, 0.5, 0.25];
        let expected = values
            .iter()
            .zip(trapezoid_weights)
            .map(|(value, weight)| value * weight)
            .sum::<f64>();
        assert_close(result, expected);
    }

    #[test]
    fn validated_grid_is_reusable_across_rectangles() {
        let axes = [0.0, 1.0, 2.0];
        let values = grid(&axes, &axes, |x, y| x + y);
        let prepared = IntegrationGrid2D::new(&axes, &axes, &values, 3, 3).unwrap();
        let first = prepared
            .integrate((0.0, 1.0), (0.0, 1.0), BaselineMode::None)
            .unwrap();
        let second = prepared
            .integrate((1.0, 2.0), (1.0, 2.0), BaselineMode::None)
            .unwrap();
        assert_close(first, 1.0);
        assert_close(second, 3.0);
    }

    #[test]
    fn malformed_inputs_are_errors() {
        assert!(matches!(
            integrate_rectangular_volume(
                &[0.0, 1.0],
                &[0.0, 1.0],
                &[0.0; 3],
                2,
                2,
                (0.0, 1.0),
                (0.0, 1.0),
                BaselineMode::None,
            ),
            Err(IntegrateError::GridSizeMismatch { .. })
        ));
        assert!(matches!(
            integrate(
                &[0.0, 1.0, 0.5],
                &[0.0, 1.0],
                &[0.0; 6],
                (0.0, 1.0),
                (0.0, 1.0),
                BaselineMode::None,
            ),
            Err(IntegrateError::NonMonotonicAxis { axis: "F2" })
        ));
        assert!(matches!(
            integrate(
                &[0.0, 1.0],
                &[0.0, 1.0],
                &[0.0, f64::NAN, 0.0, 0.0],
                (0.0, 1.0),
                (0.0, 1.0),
                BaselineMode::None,
            ),
            Err(IntegrateError::NonFiniteValue {
                input: "2D grid",
                index: 1
            })
        ));
        assert_eq!(
            integrate(
                &[0.0, 1.0],
                &[0.0, 1.0],
                &[0.0; 4],
                (f64::NAN, 1.0),
                (0.0, 1.0),
                BaselineMode::None,
            ),
            Err(IntegrateError::NonFiniteBoundary)
        );
    }
}
