//! Non-UI glue: building figures from processed spectra and picking peaks.

pub mod actions;
pub mod automation;
pub mod data_export;
pub mod delimited;
pub mod export;
pub mod fit_model_library;
pub mod layout;
pub mod operation;
pub mod project;
pub mod settings;
pub mod state;
pub mod templates;
pub mod theme;
pub mod update;
pub mod workflow;
pub mod xlsx;

/// Stable typed-table API shared by PlotX core, CLI, analysis and UI layers.
/// Backend arrays remain private to `plotx-data`.
pub use plotx_data as data;

mod figures;
pub use figures::*;

use plotx_analysis::series::DecaySeries;
use plotx_io::{PseudoAxis, PseudoKind};
use plotx_processing::{DisplayMode, Spectrum, StackSpectrum};
use serde::{Deserialize, Serialize};

pub use plotx_analysis::integrate_2d::BaselineMode;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct IntegralResult {
    #[serde(default)]
    pub id: u64,
    pub start_ppm: f64,
    pub end_ppm: f64,
    pub area: f64,
    pub normalized_area: f64,
    pub mode: DisplayModeLabel,
    /// `Some(value)` marks this band as the normalization reference and sets its
    /// displayed target value. `None` is an ordinary integral.
    #[serde(default)]
    pub reference_value: Option<f64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DisplayModeLabel {
    Real,
    Magnitude,
}

/// Numerical method used for a stored true-2D rectangular integral.
///
/// The enum is persisted even though the first version supports only direct
/// area summation, leaving project files forward-compatible with later methods.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IntegralMethod {
    #[default]
    Sum,
}

/// A persistent rectangular volume measurement on a true-2D spectrum.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Integral2D {
    pub id: u64,
    pub name: String,
    /// Ordered direct-axis bounds `(lo, hi)` in ppm.
    pub f2: (f64, f64),
    /// Ordered indirect-axis bounds `(lo, hi)` in ppm.
    pub f1: (f64, f64),
    /// Raw signed volume in intensity·ppm².
    pub volume: f64,
    pub normalized_volume: Option<f64>,
    /// `Some(value)` marks this rectangle as the normalization reference and
    /// sets its displayed target value.
    #[serde(default)]
    pub reference_value: Option<f64>,
    pub mode: DisplayModeLabel,
    pub method: IntegralMethod,
    pub baseline: BaselineMode,
}

impl DisplayModeLabel {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Real => "real",
            Self::Magnitude => "magnitude",
        }
    }
}

impl From<DisplayMode> for DisplayModeLabel {
    fn from(value: DisplayMode) -> Self {
        match value {
            DisplayMode::Real => Self::Real,
            DisplayMode::Magnitude => Self::Magnitude,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PseudoDisplay {
    Stack,
    DosyMap,
}

/// Parameters for a regularized inverse-Laplace (CONTIN-style) DOSY inversion:
/// the Tikhonov weight and the log-spaced diffusion grid (m²·s⁻¹) to solve onto.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct IltParams {
    pub lambda: f64,
    pub d_min: f64,
    pub d_max: f64,
    pub n_grid: usize,
}

impl Default for IltParams {
    fn default() -> Self {
        Self {
            lambda: 1e-2,
            d_min: 1e-11,
            d_max: 1e-8,
            n_grid: 128,
        }
    }
}

/// How a DOSY map is computed: an independent per-column mono-exponential fit, or
/// a full regularized inverse-Laplace inversion over one shared D grid. Selects
/// which cached map [`crate::state::Nmr2DDataset::figure`] renders for `DosyMap`.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub enum DosyMethod {
    #[default]
    MonoExp,
    Ilt(IltParams),
}

pub use plotx_analysis::series::ReduceOp;

pub fn extract_region_series(
    stack: &StackSpectrum,
    axis: &PseudoAxis,
    range: (f64, f64),
    op: ReduceOp,
) -> DecaySeries {
    plotx_analysis::series::extract_region_series(stack, &axis.values, range, op)
}

pub fn pseudo_axis_is_gradient(axis: &PseudoAxis) -> bool {
    axis.kind == PseudoKind::Gradient
}

pub fn integrate_region(
    spec: &Spectrum,
    mode: DisplayMode,
    ppm_range: (f64, f64),
) -> Option<IntegralResult> {
    let range = normalize_range(ppm_range);
    let points: Vec<(f64, f64)> = spec
        .ppm
        .iter()
        .zip(&spec.values)
        .filter_map(|(&ppm, c)| in_range(ppm, Some(range)).then_some((ppm, mode.reduce(c))))
        .collect();
    if points.len() < 2 {
        return None;
    }

    let area = trapezoid_area(&points);
    let full_points: Vec<(f64, f64)> = spec
        .ppm
        .iter()
        .zip(&spec.values)
        .map(|(&ppm, c)| (ppm, mode.reduce(c)))
        .collect();
    let total_abs_area = trapezoid_abs_area(&full_points).max(f64::MIN_POSITIVE);

    Some(IntegralResult {
        id: 0,
        start_ppm: range.0,
        end_ppm: range.1,
        area,
        normalized_area: area / total_abs_area,
        mode: mode.into(),
        reference_value: None,
    })
}

fn normalize_range((a, b): (f64, f64)) -> (f64, f64) {
    if a <= b { (a, b) } else { (b, a) }
}

fn in_range(value: f64, range: Option<(f64, f64)>) -> bool {
    range
        .map(|(lo, hi)| lo <= value && value <= hi)
        .unwrap_or(true)
}

fn trapezoid_area(points: &[(f64, f64)]) -> f64 {
    points
        .windows(2)
        .map(|pair| {
            let dx = (pair[1].0 - pair[0].0).abs();
            0.5 * (pair[0].1 + pair[1].1) * dx
        })
        .sum()
}

fn trapezoid_abs_area(points: &[(f64, f64)]) -> f64 {
    points
        .windows(2)
        .map(|pair| {
            let dx = (pair[1].0 - pair[0].0).abs();
            0.5 * (pair[0].1.abs() + pair[1].1.abs()) * dx
        })
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;
    use num_complex::Complex64;

    fn spectrum() -> Spectrum {
        Spectrum {
            ppm: vec![0.0, 1.0, 2.0, 3.0, 4.0],
            values: vec![0.0, 10.0, 1.0, 6.0, 0.0]
                .into_iter()
                .map(|v| Complex64::new(v, 0.0))
                .collect(),
            hz_per_point: 1.0,
            observe_freq_mhz: 400.0,
            nucleus: "1H".to_owned(),
        }
    }

    #[test]
    fn integration_is_stable_with_reversed_range_input() {
        let integral = integrate_region(&spectrum(), DisplayMode::Real, (3.0, 1.0)).unwrap();
        assert_eq!(integral.start_ppm, 1.0);
        assert_eq!(integral.end_ppm, 3.0);
        assert!((integral.area - 9.0).abs() < 1e-9);
    }
}
