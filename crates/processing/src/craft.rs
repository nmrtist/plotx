//! Complete Reduction to Amplitude Frequency Table (CRAFT) for one-dimensional FIDs.

use num_complex::Complex64;
use plotx_analysis::craft::CraftFitError;
use plotx_io::{Domain, NmrData};
use serde::{Deserialize, Serialize};

mod diagnostics;
mod fitting;
mod preflight;
mod reconstruction;
mod regions;
mod report;
mod resolution;
mod stability;
pub use diagnostics::{
    CraftDiagnostics, CraftModelingWindowDiagnostic, CraftRegionRatio, CraftRegionSummary,
    CraftRunStatus, CraftStabilityDiagnostics, CraftStabilityMetric, CraftStabilityRegion,
    CraftWarning, CraftWarningKind,
};
use fitting::{CraftModelingContext, fit_modeling_window};
pub use preflight::{
    CraftAssessmentIssue, CraftInputAssessment, CraftIssueAction, CraftIssueCode,
    CraftIssueSeverity, CraftSignalSuggestion,
};
use reconstruction::model_at;
pub use reconstruction::{synthesize_craft_fid, synthesize_craft_samples};
use regions::{build_modeling_windows, region_ratio, selections_are_valid, summarize_regions};
pub use report::{
    CraftAmplitudeReport, CraftReportDefinition, CraftReportError, CraftReportSegment,
    calculate_craft_report,
};
pub use resolution::{
    CraftDerivedModelingWindow, CraftDerivedPlan, CraftModelingPolicy, CraftParamOverrides,
    CraftParamSource, CraftParameterSources, resolve_craft_invocation,
};
use stability::{components_for_regions, stability_diagnostics};

pub const CRAFT_ALGORITHM: &str = "plotx-craft-matrix-pencil-validation";
pub const CRAFT_ALGORITHM_VERSION: u32 = 1;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CraftProfile {
    #[default]
    Conventional,
    Ssfp,
}

impl CraftProfile {
    pub const fn modeling_bandwidth_hz(self) -> f64 {
        match self {
            Self::Conventional => 250.0,
            Self::Ssfp => 2_000.0,
        }
    }

    pub const fn modeling_duration_s(self) -> f64 {
        match self {
            Self::Conventional => 1.0,
            Self::Ssfp => 1.2,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct CraftRegion {
    pub id: CraftRegionId,
    pub start_ppm: f64,
    pub end_ppm: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct CraftRegionId(pub u64);

/// Chemical-shift calibration applied to a CRAFT invocation.
///
/// CRAFT always fits frequencies relative to the acquisition carrier. This
/// value carries the independent, user-visible ppm translation from the
/// processing pipeline without mutating the original FID metadata.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct CraftReference {
    pub acquisition_carrier_ppm: f64,
    pub offset_ppm: f64,
}

impl CraftReference {
    pub const fn new(acquisition_carrier_ppm: f64, offset_ppm: f64) -> Self {
        Self {
            acquisition_carrier_ppm,
            offset_ppm,
        }
    }

    pub fn acquisition(data: &NmrData) -> Self {
        Self::new(data.carrier_ppm, 0.0)
    }

    pub fn effective_carrier_ppm(self) -> f64 {
        self.acquisition_carrier_ppm + self.offset_ppm
    }

    pub fn validate(self, data: &NmrData) -> Result<(), CraftError> {
        if self.acquisition_carrier_ppm.is_finite()
            && self.offset_ppm.is_finite()
            && self.effective_carrier_ppm().is_finite()
            && self.acquisition_carrier_ppm == data.carrier_ppm
        {
            Ok(())
        } else {
            Err(CraftError::InvalidReference)
        }
    }
}

impl CraftRegion {
    pub const fn new(id: CraftRegionId, start_ppm: f64, end_ppm: f64) -> Self {
        Self {
            id,
            start_ppm,
            end_ppm,
        }
    }

    pub fn normalized(self) -> Self {
        Self {
            id: self.id,
            start_ppm: self.start_ppm.min(self.end_ppm),
            end_ppm: self.start_ppm.max(self.end_ppm),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CraftParams {
    pub profile: CraftProfile,
    /// Empty means the complete acquired spectral width.
    pub regions: Vec<CraftRegion>,
    pub maximum_model_order: usize,
    pub minimum_amplitude_to_noise: f64,
    pub component_linewidth_bounds_hz: (f64, f64),
    pub fir_filter_taps: usize,
    pub maximum_modeled_sample_count: usize,
    pub skip_duration_s: f64,
    pub reconstruction_duration_s: Option<f64>,
}

impl CraftParams {
    pub fn conventional() -> Self {
        Self {
            profile: CraftProfile::Conventional,
            regions: Vec::new(),
            maximum_model_order: 15,
            minimum_amplitude_to_noise: 3.3,
            component_linewidth_bounds_hz: (0.05, 20.0),
            fir_filter_taps: 499,
            maximum_modeled_sample_count: 8192,
            skip_duration_s: 0.0,
            reconstruction_duration_s: None,
        }
    }

    pub fn ssfp() -> Self {
        Self {
            profile: CraftProfile::Ssfp,
            skip_duration_s: 0.0005,
            reconstruction_duration_s: Some(1.2),
            ..Self::conventional()
        }
    }

    pub fn discovery() -> Self {
        Self {
            minimum_amplitude_to_noise: 2.5,
            ..Self::conventional()
        }
    }

    pub fn validate(&self) -> Result<(), CraftError> {
        if self.maximum_model_order == 0
            || self.maximum_model_order > 64
            || !self.minimum_amplitude_to_noise.is_finite()
            || self.minimum_amplitude_to_noise <= 0.0
            || !self.component_linewidth_bounds_hz.0.is_finite()
            || !self.component_linewidth_bounds_hz.1.is_finite()
            || self.component_linewidth_bounds_hz.0 <= 0.0
            || self.component_linewidth_bounds_hz.0 >= self.component_linewidth_bounds_hz.1
            || self.fir_filter_taps < 3
            || self.fir_filter_taps.is_multiple_of(2)
            || self.maximum_modeled_sample_count < 64
            || !self.skip_duration_s.is_finite()
            || self.skip_duration_s < 0.0
            || self
                .reconstruction_duration_s
                .is_some_and(|value| !value.is_finite() || value <= 0.0)
            || !selections_are_valid(&self.regions)
        {
            return Err(CraftError::InvalidParameters);
        }
        Ok(())
    }
}

impl Default for CraftParams {
    fn default() -> Self {
        Self::conventional()
    }
}

/// Complete, immutable input contract for one CRAFT run.
///
/// Parameters and chemical-shift calibration travel together so asynchronous
/// execution and persisted provenance cannot describe a different invocation
/// from the one that produced the result.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CraftInvocation {
    pub params: CraftParams,
    pub sources: CraftParameterSources,
    pub reference: CraftReference,
    pub derived_plan: CraftDerivedPlan,
    pub assessment: CraftInputAssessment,
    pub modeling_policy: CraftModelingPolicy,
}

impl CraftInvocation {
    pub fn acquisition(data: &NmrData, params: CraftParams) -> Self {
        let overrides = CraftParamOverrides::from_params(params);
        resolve_craft_invocation(data, CraftReference::acquisition(data), &overrides, None)
    }

    pub fn validate(&self, data: &NmrData) -> Result<(), CraftError> {
        self.params.validate()?;
        self.reference.validate(data)?;
        if self.modeling_policy != CraftModelingPolicy::for_params(&self.params) {
            return Err(CraftError::InvalidParameters);
        }
        if self.assessment.can_run() {
            Ok(())
        } else {
            Err(CraftError::Preflight(
                self.assessment
                    .first_blocking_message()
                    .unwrap_or("CRAFT input cannot be analyzed")
                    .to_owned(),
            ))
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct CraftComponentId(pub u64);

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CraftComponent {
    pub id: CraftComponentId,
    pub region: CraftRegionId,
    pub frequency_hz: f64,
    pub chemical_shift_ppm: f64,
    pub amplitude_t0: f64,
    pub phase_rad: f64,
    pub decay_rate_s_inv: f64,
    pub linewidth_hz: f64,
    pub amplitude_to_noise: f64,
    pub amplitude_std: Option<f64>,
    pub frequency_std_hz: Option<f64>,
    pub linewidth_std_hz: Option<f64>,
    pub phase_std_rad: Option<f64>,
}

#[derive(Clone, Debug)]
pub struct CraftResult {
    pub components: Vec<CraftComponent>,
    pub region_summaries: Vec<CraftRegionSummary>,
    pub region_ratio: Option<CraftRegionRatio>,
    pub diagnostics: CraftDiagnostics,
    pub synthetic_fid: Vec<Complex64>,
    pub residual_fid: Vec<Complex64>,
}

#[derive(Debug, thiserror::Error)]
pub enum CraftError {
    #[error("CRAFT requires a non-empty complex time-domain FID")]
    InvalidInput,
    #[error("CRAFT parameters are invalid")]
    InvalidParameters,
    #[error("CRAFT preflight failed: {0}")]
    Preflight(String),
    #[error("CRAFT chemical-shift reference is invalid")]
    InvalidReference,
    #[error("CRAFT analysis was cancelled")]
    Cancelled,
    #[error("CRAFT could not fit a modeling window: {0}")]
    Fit(#[from] CraftFitError),
}

pub fn process_craft_cancellable(
    data: &NmrData,
    invocation: &CraftInvocation,
    cancelled: &impl Fn() -> bool,
) -> Result<CraftResult, CraftError> {
    invocation.validate(data)?;
    let params = &invocation.params;
    let reference = invocation.reference;
    if data.domain != Domain::Time
        || data.points.is_empty()
        || data
            .points
            .iter()
            .any(|point| !point.re.is_finite() || !point.im.is_finite())
        || !data.spectral_width_hz.is_finite()
        || data.spectral_width_hz <= 0.0
        || !data.observe_freq_mhz.is_finite()
        || data.observe_freq_mhz <= 0.0
    {
        return Err(CraftError::InvalidInput);
    }
    if cancelled() {
        return Err(CraftError::Cancelled);
    }

    let sw = data.spectral_width_hz;
    let skip = invocation.derived_plan.effective_skip_points;
    if data.points.len().saturating_sub(skip) < 16 {
        return Err(CraftError::InvalidInput);
    }
    let input = &data.points[skip..];
    let modeling_context = CraftModelingContext {
        input,
        skipped_points: skip,
        group_delay_points: data.group_delay,
        spectral_width_hz: sw,
        params,
        policy: invocation.modeling_policy,
    };
    let noise_sigma = estimate_complex_noise(input).max(f64::MIN_POSITIVE);
    let modeling_windows = build_modeling_windows(
        data,
        params,
        reference,
        &invocation.assessment.clear_signals,
    )?;
    let mut fitted = Vec::new();
    let mut warnings = Vec::new();
    let mut window_diagnostics = Vec::with_capacity(modeling_windows.len());
    let mut max_condition = 1.0_f64;
    let selected_frequency_bands = params
        .regions
        .iter()
        .map(|region| {
            let region = region.normalized();
            (
                (region.start_ppm - reference.effective_carrier_ppm()) * data.observe_freq_mhz,
                (region.end_ppm - reference.effective_carrier_ppm()) * data.observe_freq_mhz,
            )
        })
        .collect::<Vec<_>>();

    for (index, window) in modeling_windows.iter().copied().enumerate() {
        if cancelled() {
            return Err(CraftError::Cancelled);
        }
        let result = fit_modeling_window(&modeling_context, window, cancelled)?;
        let contributes_to_selection = selected_frequency_bands.is_empty()
            || selected_frequency_bands.iter().any(|&(start, end)| {
                window.retention_band_hz.0 <= end && window.retention_band_hz.1 >= start
            });
        if let Some((kind, message)) = result.warning
            && contributes_to_selection
        {
            warnings.push(CraftWarning {
                kind,
                region: None,
                modeling_window: Some(index),
                message: format!("Modeling window {}: {message}", index + 1),
            });
        }
        window_diagnostics.push(CraftModelingWindowDiagnostic {
            retention_band_hz: window.retention_band_hz,
            modeling_band_hz: window.modeling_band_hz,
            decimation_factor: result.decimation,
            modeled_sample_count: result.modeled_sample_count,
            evaluated_model_orders: result.evaluated_model_orders,
            selected_model_order: result.components.len(),
            training_bic: result.training_bic,
            condition_number: result
                .condition_number
                .is_finite()
                .then_some(result.condition_number),
            modeled_duration_s: result.modeled_duration_s,
            training_normalized_residual: result.training_normalized_residual,
            validation_normalized_residual: result.validation_normalized_residual,
        });
        max_condition = max_condition.max(result.condition_number);
        for component in result.components {
            let frequency_hz = component.frequency_hz + result.center_hz;
            let is_last_window = index + 1 == modeling_windows.len();
            if frequency_hz >= window.retention_band_hz.0
                && (frequency_hz < window.retention_band_hz.1
                    || (is_last_window && frequency_hz <= window.retention_band_hz.1))
            {
                // Padded modeling bands may overlap. Retention bands assign a
                // model to exactly one window before the sub-tables are joined.
                fitted.push((frequency_hz, component));
            }
        }
    }

    fitted.sort_by(|left, right| left.0.total_cmp(&right.0));
    let all_components: Vec<CraftComponent> = fitted
        .into_iter()
        .enumerate()
        .map(|(id, (frequency_hz, component))| CraftComponent {
            id: CraftComponentId(id as u64),
            region: CraftRegionId(0),
            frequency_hz,
            chemical_shift_ppm: reference.effective_carrier_ppm()
                + frequency_hz / data.observe_freq_mhz,
            amplitude_t0: component.amplitude,
            phase_rad: component.phase_rad,
            decay_rate_s_inv: component.decay_rate_s_inv,
            linewidth_hz: component.linewidth_hz,
            amplitude_to_noise: component
                .amplitude_std
                .filter(|value| *value > 0.0)
                .map_or(0.0, |value| component.amplitude / value),
            amplitude_std: component.amplitude_std,
            frequency_std_hz: component.frequency_std_hz,
            linewidth_std_hz: component.linewidth_std_hz,
            phase_std_rad: component.phase_std_rad,
        })
        .collect();
    let selections = if params.regions.is_empty() {
        let half_width_ppm = sw / (2.0 * data.observe_freq_mhz);
        vec![CraftRegion::new(
            CraftRegionId(0),
            reference.effective_carrier_ppm() - half_width_ppm,
            reference.effective_carrier_ppm() + half_width_ppm,
        )]
    } else {
        params.regions.clone()
    };
    let components = components_for_regions(&all_components, &selections);
    if params.minimum_amplitude_to_noise < 3.3 {
        warnings.push(CraftWarning {
            kind: CraftWarningKind::LowAmplitudeThreshold,
            region: None,
            modeling_window: None,
            message: "Discovery threshold is below the strict 3.3 amplitude/noise threshold; confirm weak components independently."
                .to_owned(),
        });
    }
    if params.profile == CraftProfile::Ssfp {
        warnings.push(CraftWarning {
            kind: CraftWarningKind::SsfpQuantitation,
            region: None,
            modeling_window: None,
            message: "SSFP response is relaxation-dependent; use this result for screening or relative comparison, not absolute qNMR."
                .to_owned(),
        });
    }
    warnings.extend(
        invocation
            .assessment
            .issues
            .iter()
            .filter(|issue| issue.severity == CraftIssueSeverity::Warning)
            .map(|issue| CraftWarning {
                kind: CraftWarningKind::InputAssessment,
                region: issue.region,
                modeling_window: None,
                message: issue.message.clone(),
            }),
    );
    if max_condition > 1e8 {
        warnings.push(CraftWarning {
            kind: CraftWarningKind::IllConditionedFit,
            region: None,
            modeling_window: None,
            message: "One or more fits are ill-conditioned; inspect overlapping components and uncertainties.".to_owned(),
        });
    }
    if components.iter().any(|component| {
        (component.linewidth_hz - params.component_linewidth_bounds_hz.0).abs() < 1e-6
            || (component.linewidth_hz - params.component_linewidth_bounds_hz.1).abs() < 1e-6
    }) {
        warnings.push(CraftWarning {
            kind: CraftWarningKind::LinewidthAtBound,
            region: None,
            modeling_window: None,
            message: "One or more linewidths reached a configured fit bound.".to_owned(),
        });
    }
    if components.iter().any(|component| {
        component.amplitude_std.is_none()
            || component.frequency_std_hz.is_none()
            || component.linewidth_std_hz.is_none()
            || component.phase_std_rad.is_none()
    }) {
        warnings.push(CraftWarning {
            kind: CraftWarningKind::UnboundedUncertainty,
            region: None,
            modeling_window: None,
            message: "One or more components have unbounded uncertainties.".to_owned(),
        });
    }

    let synthetic_len = invocation.derived_plan.reconstruction_points.max(1);
    let synthetic_fid = synthesize_craft_fid(&components, synthetic_len, sw);
    let residual_fid: Vec<Complex64> = data
        .points
        .iter()
        .enumerate()
        .map(|(index, &sample)| {
            let time = (index as f64 - data.group_delay) / sw;
            sample - model_at(&components, time)
        })
        .collect();
    let residual_rss: f64 = residual_fid[skip..].iter().map(Complex64::norm_sqr).sum();
    let input_rss: f64 = input.iter().map(Complex64::norm_sqr).sum();
    let normalized_residual = (residual_rss / input_rss.max(f64::MIN_POSITIVE)).sqrt();
    let region_summaries = summarize_regions(&components, &selections);
    for (position, summary) in region_summaries.iter().enumerate() {
        if summary.component_count == 0 {
            warnings.push(CraftWarning {
                kind: CraftWarningKind::EmptyRegion,
                region: Some(summary.region),
                modeling_window: None,
                message: format!(
                    "Region {} contains no retained signal components.",
                    position + 1
                ),
            });
        }
    }
    let stability = stability_diagnostics(
        &all_components,
        &selections,
        &window_diagnostics,
        invocation.modeling_policy,
        reference,
        data,
    );
    if !stability.passed {
        warnings.push(CraftWarning {
            kind: CraftWarningKind::StabilityFailure,
            region: None,
            modeling_window: None,
            message: "Boundary perturbation exceeded the 1% stability tolerance; retain the full fit for review, but do not use it for quantitative reporting.".to_owned(),
        });
    }
    let status = if invocation.assessment.has_warnings()
        || warnings.iter().any(CraftWarning::blocks_quantitation)
    {
        CraftRunStatus::Partial
    } else {
        CraftRunStatus::Complete
    };
    let region_ratio = region_ratio(&region_summaries);
    Ok(CraftResult {
        components,
        region_summaries,
        region_ratio,
        diagnostics: CraftDiagnostics {
            status,
            noise_sigma,
            residual_rss,
            normalized_residual,
            maximum_condition_number: max_condition.is_finite().then_some(max_condition),
            modeling_windows: window_diagnostics,
            warnings,
            stability,
        },
        synthetic_fid,
        residual_fid,
    })
}

fn estimate_complex_noise(values: &[Complex64]) -> f64 {
    let start = values.len().saturating_sub((values.len() / 4).max(64));
    let tail = &values[start..];
    let mut differences = Vec::with_capacity(tail.len().saturating_sub(1) * 2);
    for pair in tail.windows(2) {
        let difference = pair[1] - pair[0];
        differences.push(difference.re);
        differences.push(difference.im);
    }
    if differences.is_empty() {
        return 0.0;
    }
    let center = median(&mut differences);
    for value in &mut differences {
        *value = (*value - center).abs();
    }
    median(&mut differences) / (0.674_489_750_196_081_7 * 2.0_f64.sqrt())
}

fn median(values: &mut [f64]) -> f64 {
    values.sort_by(f64::total_cmp);
    let middle = values.len() / 2;
    if values.len().is_multiple_of(2) {
        (values[middle - 1] + values[middle]) * 0.5
    } else {
        values[middle]
    }
}

#[cfg(test)]
#[path = "craft_tests.rs"]
mod tests;
