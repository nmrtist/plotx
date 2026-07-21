use super::*;

/// A validated row-major grid that can integrate many rectangles without
/// repeatedly scanning the complete matrix and axes.
pub struct IntegrationGrid2D<'a> {
    f2_ppm: &'a [f64],
    f1_ppm: &'a [f64],
    grid: &'a [f64],
    f2_size: usize,
    f1_size: usize,
}

impl<'a> IntegrationGrid2D<'a> {
    pub fn new(
        f2_ppm: &'a [f64],
        f1_ppm: &'a [f64],
        grid: &'a [f64],
        f2_size: usize,
        f1_size: usize,
    ) -> Result<Self, IntegrateError> {
        validate_grid(f2_ppm, f1_ppm, grid, f2_size, f1_size)?;
        Ok(Self {
            f2_ppm,
            f1_ppm,
            grid,
            f2_size,
            f1_size,
        })
    }

    pub fn integrate(
        &self,
        f2_bounds: (f64, f64),
        f1_bounds: (f64, f64),
        baseline: BaselineMode,
    ) -> Result<f64, IntegrateError> {
        validate_bounds(f2_bounds, f1_bounds)?;
        integrate_validated(
            self.f2_ppm,
            self.f1_ppm,
            self.grid,
            self.f2_size,
            self.f1_size,
            f2_bounds,
            f1_bounds,
            baseline,
        )
    }
}
