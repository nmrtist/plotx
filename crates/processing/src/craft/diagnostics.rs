use serde::{Deserialize, Serialize};

use super::CraftRegionId;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CraftRunStatus {
    Complete,
    Partial,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CraftDiagnostics {
    pub status: CraftRunStatus,
    pub noise_sigma: f64,
    pub residual_rss: f64,
    pub normalized_residual: f64,
    /// `None` means at least one fitted design was rank deficient or unbounded.
    pub maximum_condition_number: Option<f64>,
    /// One entry per internal fit window. A user region can require several
    /// windows, but those windows never become user-visible region identities.
    pub fit_windows: Vec<CraftFitWindowDiagnostic>,
    pub warnings: Vec<CraftWarning>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CraftWarningKind {
    FitWindowFailure,
    ModelOrderLimit,
    EmptyRegion,
    LinewidthAtBound,
    UnboundedUncertainty,
    IllConditionedFit,
    LowAmplitudeThreshold,
    SsfpQuantitation,
    InputAssessment,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CraftWarning {
    pub kind: CraftWarningKind,
    pub region: Option<CraftRegionId>,
    pub fit_window: Option<usize>,
    pub message: String,
}

impl CraftWarning {
    pub fn blocks_quantitation(&self) -> bool {
        matches!(
            self.kind,
            CraftWarningKind::FitWindowFailure
                | CraftWarningKind::ModelOrderLimit
                | CraftWarningKind::EmptyRegion
                | CraftWarningKind::LinewidthAtBound
                | CraftWarningKind::UnboundedUncertainty
                | CraftWarningKind::IllConditionedFit
        )
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CraftFitWindowDiagnostic {
    pub region: CraftRegionId,
    pub core_hz: (f64, f64),
    pub padded_hz: (f64, f64),
    pub actual_decimation: usize,
    pub retained_samples: usize,
    pub evaluated_model_orders: usize,
    pub selected_model_order: usize,
    pub bic: Option<f64>,
    pub condition_number: Option<f64>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CraftRegionSummary {
    pub region: CraftRegionId,
    pub start_ppm: f64,
    pub end_ppm: f64,
    pub component_count: usize,
    pub scalar_amplitude_sum_t0: f64,
    pub coherent_amplitude_t0: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct CraftRegionRatio {
    pub numerator: CraftRegionId,
    pub denominator: CraftRegionId,
    pub value: f64,
}
