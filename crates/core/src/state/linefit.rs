//! Serde mirror of a multi-peak lineshape deconvolution, stored on the source
//! 1D dataset so fits persist in projects and paint as figure overlays.

use plotx_analysis::lineshape::{FittedPeak, LineFit, LineShape};
use plotx_figure::{Color, Figure, Series};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum LineShapeKind {
    Lorentzian,
    Gaussian,
    PseudoVoigt,
}

impl LineShapeKind {
    pub fn all() -> &'static [Self] {
        &[Self::Lorentzian, Self::Gaussian, Self::PseudoVoigt]
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Lorentzian => "Lorentzian",
            Self::Gaussian => "Gaussian",
            Self::PseudoVoigt => "Pseudo-Voigt",
        }
    }
}

impl From<LineShapeKind> for LineShape {
    fn from(kind: LineShapeKind) -> Self {
        match kind {
            LineShapeKind::Lorentzian => LineShape::Lorentzian,
            LineShapeKind::Gaussian => LineShape::Gaussian,
            LineShapeKind::PseudoVoigt => LineShape::PseudoVoigt,
        }
    }
}

impl From<LineShape> for LineShapeKind {
    fn from(shape: LineShape) -> Self {
        match shape {
            LineShape::Lorentzian => LineShapeKind::Lorentzian,
            LineShape::Gaussian => LineShapeKind::Gaussian,
            LineShape::PseudoVoigt => LineShapeKind::PseudoVoigt,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct StoredFittedPeak {
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

fn finite_sigma(sigma: Option<f64>) -> Option<f64> {
    sigma.filter(|v| v.is_finite())
}

impl From<FittedPeak> for StoredFittedPeak {
    fn from(p: FittedPeak) -> Self {
        Self {
            position: p.position,
            height: p.height,
            fwhm: p.fwhm,
            eta: p.eta,
            area: p.area,
            position_sigma: finite_sigma(p.position_sigma),
            height_sigma: finite_sigma(p.height_sigma),
            fwhm_sigma: finite_sigma(p.fwhm_sigma),
            eta_sigma: finite_sigma(p.eta_sigma),
            area_sigma: finite_sigma(p.area_sigma),
        }
    }
}

impl From<StoredFittedPeak> for FittedPeak {
    fn from(p: StoredFittedPeak) -> Self {
        Self {
            position: p.position,
            height: p.height,
            fwhm: p.fwhm,
            eta: p.eta,
            area: p.area,
            position_sigma: p.position_sigma,
            height_sigma: p.height_sigma,
            fwhm_sigma: p.fwhm_sigma,
            eta_sigma: p.eta_sigma,
            area_sigma: p.area_sigma,
        }
    }
}

/// One converged deconvolution over the x-window `[lo, hi]` of its dataset.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct StoredLineFit {
    pub id: u64,
    pub lo: f64,
    pub hi: f64,
    pub shape: LineShapeKind,
    pub peaks: Vec<StoredFittedPeak>,
    pub offset: f64,
    pub offset_sigma: Option<f64>,
    pub r2: f64,
}

impl StoredLineFit {
    pub fn from_fit(id: u64, lo: f64, hi: f64, fit: &LineFit) -> Self {
        Self {
            id,
            lo,
            hi,
            shape: fit.shape.into(),
            peaks: fit.peaks.iter().copied().map(Into::into).collect(),
            offset: fit.offset,
            offset_sigma: finite_sigma(fit.offset_sigma),
            r2: fit.r2,
        }
    }

    /// Rebuild the processing-layer fit, the single home of the evaluation math.
    pub fn to_fit(&self) -> LineFit {
        LineFit {
            shape: self.shape.into(),
            peaks: self.peaks.iter().copied().map(Into::into).collect(),
            offset: self.offset,
            offset_sigma: self.offset_sigma,
            r2: self.r2,
        }
    }
}

const FIT_SAMPLES: usize = 201;
const FIT_TOTAL_COLOR: Color = Color::rgb(0xd1, 0x24, 0x2a);
const FIT_COMPONENT_COLOR: Color = Color::rgb(0x1f, 0x6f, 0xeb);

/// Append each fit's smooth total curve plus one thinner curve per component
/// (drawn on the fitted baseline), sampled over the fit's own window. Called at
/// figure-rebuild time only, never per frame.
pub fn apply_line_fit_overlays(mut fig: Figure, fits: &[StoredLineFit]) -> Figure {
    let (ax_lo, ax_hi) = (fig.x.min.min(fig.x.max), fig.x.min.max(fig.x.max));
    for stored in fits {
        if stored.lo >= stored.hi || stored.hi < ax_lo || stored.lo > ax_hi {
            continue;
        }
        let fit = stored.to_fit();
        let xs: Vec<f64> = (0..FIT_SAMPLES)
            .map(|k| stored.lo + (stored.hi - stored.lo) * k as f64 / (FIT_SAMPLES - 1) as f64)
            .collect();
        for i in 0..fit.peaks.len() {
            let pts: Vec<[f64; 2]> = xs
                .iter()
                .map(|&x| [x, fit.offset + fit.eval_component(i, x)])
                .collect();
            let mut series =
                Series::line(format!("peak {}", i + 1), pts).colored(FIT_COMPONENT_COLOR);
            series.width = 0.7;
            fig = fig.with_series(series);
        }
        let total: Vec<[f64; 2]> = xs.iter().map(|&x| [x, fit.eval_total(x)]).collect();
        fig = fig.with_series(Series::line("fit", total).colored(FIT_TOTAL_COLOR));
    }
    fig
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_fit() -> StoredLineFit {
        StoredLineFit {
            id: 3,
            lo: 1.0,
            hi: 4.0,
            shape: LineShapeKind::PseudoVoigt,
            peaks: vec![StoredFittedPeak {
                position: 2.5,
                height: 5.0,
                fwhm: 0.4,
                eta: Some(0.3),
                area: 2.4,
                position_sigma: Some(0.01),
                height_sigma: None,
                fwhm_sigma: Some(0.02),
                eta_sigma: None,
                area_sigma: None,
            }],
            offset: 0.2,
            offset_sigma: Some(0.005),
            r2: 0.999,
        }
    }

    #[test]
    fn stored_fit_survives_json_round_trip() {
        let fit = sample_fit();
        let json = serde_json::to_string(&fit).unwrap();
        let back: StoredLineFit = serde_json::from_str(&json).unwrap();
        assert_eq!(back, fit);
    }

    #[test]
    fn to_fit_evaluates_peak_apex_at_offset_plus_height() {
        let fit = sample_fit().to_fit();
        assert!((fit.eval_total(2.5) - 5.2).abs() < 1e-9);
        assert!((fit.eval_component(0, 2.5) - 5.0).abs() < 1e-9);
    }

    #[test]
    fn overlays_add_total_and_component_series_inside_axis_range() {
        let fig = Figure::new(
            "t",
            plotx_figure::Axis::new("x", 0.0, 10.0).reversed(true),
            plotx_figure::Axis::new("y", 0.0, 10.0),
        );
        let fig = apply_line_fit_overlays(fig, &[sample_fit()]);
        let names: Vec<&str> = fig.series.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, vec!["peak 1", "fit"]);
        assert!(fig.series.iter().all(|s| s.points.len() == 201));

        let outside = StoredLineFit {
            lo: 50.0,
            hi: 60.0,
            ..sample_fit()
        };
        let fig2 = apply_line_fit_overlays(
            Figure::new(
                "t",
                plotx_figure::Axis::new("x", 0.0, 10.0),
                plotx_figure::Axis::new("y", 0.0, 10.0),
            ),
            &[outside],
        );
        assert!(fig2.series.is_empty());
    }
}
