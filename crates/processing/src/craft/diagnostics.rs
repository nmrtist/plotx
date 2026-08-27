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
    /// One entry per protocol-owned modeling window. These are independent of
    /// user-visible signal-region identities.
    pub modeling_windows: Vec<CraftModelingWindowDiagnostic>,
    pub warnings: Vec<CraftWarning>,
    pub stability: CraftStabilityDiagnostics,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CraftStabilityDiagnostics {
    pub delta_ppm: f64,
    pub regions: Vec<CraftStabilityRegion>,
    pub ratio: Option<CraftStabilityMetric>,
    pub passed: bool,
    pub skipped: Vec<String>,
}

impl Default for CraftStabilityDiagnostics {
    fn default() -> Self {
        Self {
            delta_ppm: 0.0,
            regions: Vec::new(),
            ratio: None,
            passed: false,
            skipped: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CraftStabilityRegion {
    pub region: CraftRegionId,
    pub metric: CraftStabilityMetric,
    pub component_count_min: usize,
    pub component_count_max: usize,
    pub model_order_min: usize,
    pub model_order_max: usize,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct CraftStabilityMetric {
    pub median: f64,
    pub minimum: f64,
    pub maximum: f64,
    pub relative_dispersion: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CraftWarningKind {
    ModelingWindowFailure,
    ModelOrderLimit,
    EmptyRegion,
    LinewidthAtBound,
    UnboundedUncertainty,
    IllConditionedFit,
    StabilityFailure,
    LowAmplitudeThreshold,
    SsfpQuantitation,
    InputAssessment,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CraftWarning {
    pub kind: CraftWarningKind,
    pub region: Option<CraftRegionId>,
    pub modeling_window: Option<usize>,
    pub message: String,
}

impl CraftWarning {
    pub fn blocks_quantitation(&self) -> bool {
        matches!(
            self.kind,
            CraftWarningKind::ModelingWindowFailure
                | CraftWarningKind::ModelOrderLimit
                | CraftWarningKind::EmptyRegion
                | CraftWarningKind::LinewidthAtBound
                | CraftWarningKind::UnboundedUncertainty
                | CraftWarningKind::IllConditionedFit
                | CraftWarningKind::StabilityFailure
        )
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CraftModelingWindowDiagnostic {
    pub retention_band_hz: (f64, f64),
    pub modeling_band_hz: (f64, f64),
    pub decimation_factor: usize,
    pub modeled_sample_count: usize,
    pub evaluated_model_orders: usize,
    pub selected_model_order: usize,
    pub training_bic: Option<f64>,
    pub condition_number: Option<f64>,
    pub modeled_duration_s: f64,
    pub training_normalized_residual: f64,
    pub validation_normalized_residual: f64,
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
