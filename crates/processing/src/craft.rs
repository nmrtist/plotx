//! Complete Reduction to Amplitude Frequency Table (CRAFT) for one-dimensional FIDs.

use num_complex::Complex64;
use plotx_analysis::craft::{
    CraftFitBounds, CraftFitError, DampedSinusoid, evaluate_damped_sinusoids_cancellable,
    matrix_pencil_estimates,
};
use plotx_io::{Domain, NmrData};
use serde::{Deserialize, Serialize};
use std::f64::consts::{PI, TAU};

mod diagnostics;
mod preflight;
mod reconstruction;
mod regions;
mod resolution;
pub use diagnostics::{
    CraftDiagnostics, CraftFitWindowDiagnostic, CraftRegionRatio, CraftRegionSummary,
    CraftRunStatus, CraftWarning, CraftWarningKind,
};
pub use preflight::{
    CraftAssessmentIssue, CraftInputAssessment, CraftIssueAction, CraftIssueCode,
    CraftIssueSeverity, CraftSignalSuggestion,
};
use reconstruction::model_at;
pub use reconstruction::{synthesize_craft_fid, synthesize_craft_samples};
use regions::{HzRegion, build_regions, region_ratio, selections_are_valid, summarize_regions};
pub use resolution::{
    CraftDerivedPlan, CraftDerivedWindow, CraftParamOverrides, CraftParamSource,
    CraftParameterSources, resolve_craft_invocation,
};

pub const CRAFT_ALGORITHM: &str = "plotx-craft-matrix-pencil-bic";
pub const CRAFT_ALGORITHM_VERSION: u32 = 1;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CraftProfile {
    #[default]
    Conventional,
    Ssfp,
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
    pub max_components_per_fit_window: usize,
    pub min_amplitude_to_noise: f64,
    pub linewidth_hz: (f64, f64),
    pub filter_taps: usize,
    pub padding_fraction: f64,
    pub max_fit_window_width_hz: f64,
    pub max_downsampled_points: usize,
    pub skip_duration_s: f64,
    pub reconstruction_duration_s: Option<f64>,
}

impl CraftParams {
    pub fn conventional() -> Self {
        Self {
            profile: CraftProfile::Conventional,
            regions: Vec::new(),
            max_components_per_fit_window: 15,
            min_amplitude_to_noise: 3.3,
            linewidth_hz: (0.05, 10.0),
            filter_taps: 499,
            padding_fraction: 0.2,
            max_fit_window_width_hz: 500.0,
            max_downsampled_points: 8192,
            skip_duration_s: 0.0,
            reconstruction_duration_s: None,
        }
    }

    pub fn ssfp() -> Self {
        Self {
            profile: CraftProfile::Ssfp,
            skip_duration_s: 0.0005,
            reconstruction_duration_s: Some(1.2),
            max_fit_window_width_hz: 2_000.0,
            ..Self::conventional()
        }
    }

    pub fn discovery() -> Self {
        Self {
            min_amplitude_to_noise: 2.5,
            ..Self::conventional()
        }
    }

    pub fn validate(&self) -> Result<(), CraftError> {
        if self.max_components_per_fit_window == 0
            || self.max_components_per_fit_window > 64
            || !self.min_amplitude_to_noise.is_finite()
            || self.min_amplitude_to_noise <= 0.0
            || !self.linewidth_hz.0.is_finite()
            || !self.linewidth_hz.1.is_finite()
            || self.linewidth_hz.0 <= 0.0
            || self.linewidth_hz.0 >= self.linewidth_hz.1
            || self.filter_taps < 3
            || self.filter_taps.is_multiple_of(2)
            || !self.padding_fraction.is_finite()
            || !(0.0..=1.0).contains(&self.padding_fraction)
            || !self.max_fit_window_width_hz.is_finite()
            || self.max_fit_window_width_hz <= 0.0
            || self.max_downsampled_points < 64
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
}

impl CraftInvocation {
    pub fn acquisition(data: &NmrData, params: CraftParams) -> Self {
        let overrides = CraftParamOverrides::from_params(params);
        resolve_craft_invocation(data, CraftReference::acquisition(data), &overrides, None)
    }

    pub fn validate(&self, data: &NmrData) -> Result<(), CraftError> {
        self.params.validate()?;
        self.reference.validate(data)?;
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
    #[error("CRAFT could not fit a requested region: {0}")]
    Fit(#[from] CraftFitError),
}

struct RegionResult {
    components: Vec<DampedSinusoid>,
    center_hz: f64,
    bic: Option<f64>,
    condition_number: f64,
    decimation: usize,
    retained_samples: usize,
    evaluated_model_orders: usize,
    warning: Option<(CraftWarningKind, String)>,
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
    let noise_sigma = estimate_complex_noise(input).max(f64::MIN_POSITIVE);
    let regions = build_regions(data, params, reference)?;
    let mut fitted = Vec::new();
    let mut warnings = Vec::new();
    let mut fit_windows = Vec::with_capacity(regions.len());
    let mut max_condition = 1.0_f64;

    for (index, region) in regions.iter().copied().enumerate() {
        if cancelled() {
            return Err(CraftError::Cancelled);
        }
        let result = fit_region(input, skip, data.group_delay, sw, region, params, cancelled)?;
        if let Some((kind, message)) = result.warning {
            warnings.push(CraftWarning {
                kind,
                region: Some(region.selection.id),
                fit_window: Some(index),
                message: format!("Fit window {}: {message}", index + 1),
            });
        }
        fit_windows.push(CraftFitWindowDiagnostic {
            region: region.selection.id,
            core_hz: region.core,
            padded_hz: region.padded,
            actual_decimation: result.decimation,
            retained_samples: result.retained_samples,
            evaluated_model_orders: result.evaluated_model_orders,
            selected_model_order: result.components.len(),
            bic: result.bic,
            condition_number: result
                .condition_number
                .is_finite()
                .then_some(result.condition_number),
        });
        max_condition = max_condition.max(result.condition_number);
        for component in result.components {
            let frequency_hz = component.frequency_hz + result.center_hz;
            if frequency_hz >= region.core.0
                && frequency_hz < region.core.1
                && component.amplitude / noise_sigma >= params.min_amplitude_to_noise
            {
                fitted.push((region.selection.id, frequency_hz, component));
            }
        }
    }

    fitted.sort_by(|left, right| left.1.total_cmp(&right.1));
    let components: Vec<CraftComponent> = fitted
        .into_iter()
        .enumerate()
        .map(|(id, (region, frequency_hz, component))| CraftComponent {
            id: CraftComponentId(id as u64),
            region,
            frequency_hz,
            chemical_shift_ppm: reference.effective_carrier_ppm()
                + frequency_hz / data.observe_freq_mhz,
            amplitude_t0: component.amplitude,
            phase_rad: component.phase_rad,
            decay_rate_s_inv: component.decay_rate_s_inv,
            linewidth_hz: component.linewidth_hz,
            amplitude_to_noise: component.amplitude / noise_sigma,
            amplitude_std: component.amplitude_std,
            frequency_std_hz: component.frequency_std_hz,
            linewidth_std_hz: component.linewidth_std_hz,
            phase_std_rad: component.phase_std_rad,
        })
        .collect();
    if params.min_amplitude_to_noise < 3.3 {
        warnings.push(CraftWarning {
            kind: CraftWarningKind::LowAmplitudeThreshold,
            region: None,
            fit_window: None,
            message: "Discovery threshold is below the strict 3.3 amplitude/noise threshold; confirm weak components independently."
                .to_owned(),
        });
    }
    if params.profile == CraftProfile::Ssfp {
        warnings.push(CraftWarning {
            kind: CraftWarningKind::SsfpQuantitation,
            region: None,
            fit_window: None,
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
                fit_window: None,
                message: issue.message.clone(),
            }),
    );
    if max_condition > 1e8 {
        warnings.push(CraftWarning {
            kind: CraftWarningKind::IllConditionedFit,
            region: None,
            fit_window: None,
            message: "One or more fits are ill-conditioned; inspect overlapping components and uncertainties.".to_owned(),
        });
    }
    if components.iter().any(|component| {
        (component.linewidth_hz - params.linewidth_hz.0).abs() < 1e-6
            || (component.linewidth_hz - params.linewidth_hz.1).abs() < 1e-6
    }) {
        warnings.push(CraftWarning {
            kind: CraftWarningKind::LinewidthAtBound,
            region: None,
            fit_window: None,
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
            fit_window: None,
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
    let selections = if params.regions.is_empty() {
        vec![regions[0].selection]
    } else {
        params
            .regions
            .iter()
            .map(|requested| {
                regions
                    .iter()
                    .find(|window| window.selection.id == requested.id)
                    .map(|window| window.selection)
                    .expect("validated CRAFT region has at least one fit window")
            })
            .collect()
    };
    let region_summaries = summarize_regions(&components, &selections);
    for (position, summary) in region_summaries.iter().enumerate() {
        if summary.component_count == 0 {
            warnings.push(CraftWarning {
                kind: CraftWarningKind::EmptyRegion,
                region: Some(summary.region),
                fit_window: None,
                message: format!(
                    "Region {} contains no retained signal components.",
                    position + 1
                ),
            });
        }
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
            fit_windows,
            warnings,
        },
        synthetic_fid,
        residual_fid,
    })
}

fn fit_region(
    input: &[Complex64],
    skipped_points: usize,
    group_delay_points: f64,
    sw: f64,
    region: HzRegion,
    params: &CraftParams,
    cancelled: &impl Fn() -> bool,
) -> Result<RegionResult, CraftError> {
    let center_hz = (region.padded.0 + region.padded.1) * 0.5;
    let padded_width = region.padded.1 - region.padded.0;
    // Only the early signal-bearing record is modeled. Include one filter
    // length of guard samples so the centered FIR has a fully observed window
    // for every retained point.
    let filter_input_len = input
        .len()
        .min(6_000_usize.saturating_add(params.filter_taps));
    let mixed: Vec<Complex64> = input[..filter_input_len]
        .iter()
        .enumerate()
        .map(|(index, &value)| {
            // Bruker points after the digital-filter transient are already
            // samples of the FID starting at `ceil(delay) - delay`. Keeping the
            // raw point number here would reintroduce the removed delay as an
            // amplitude extrapolation and a first-order phase ramp.
            let time = (skipped_points as f64 + index as f64 - group_delay_points) / sw;
            value * Complex64::from_polar(1.0, -TAU * center_hz * time)
        })
        .collect();
    let filtered = low_pass_fir(
        &mixed,
        sw,
        padded_width * 0.5,
        params.filter_taps,
        cancelled,
    )?;
    // Retain two samples per padded-bandwidth interval so the FIR transition
    // band remains below the downsampled Nyquist limit.
    let mut decimation = (sw / (2.0 * padded_width).max(f64::MIN_POSITIVE))
        .floor()
        .max(1.0) as usize;
    decimation = decimation.max(filtered.len().div_ceil(params.max_downsampled_points));
    let filter_half = effective_filter_taps(params.filter_taps, mixed.len()) / 2;
    let valid_end = filtered.len().saturating_sub(filter_half);
    let phase_search_end = valid_end.min(filter_half.saturating_add(params.filter_taps));
    let phase_start = filtered[filter_half..phase_search_end]
        .iter()
        .enumerate()
        .max_by(|left, right| left.1.norm_sqr().total_cmp(&right.1.norm_sqr()))
        .map(|(index, _)| filter_half + index)
        .unwrap_or(filter_half);
    let useful_end = phase_start.saturating_add(6_000).min(valid_end);
    let samples: Vec<Complex64> = filtered[phase_start..useful_end]
        .iter()
        .step_by(decimation)
        .copied()
        .collect();
    let times: Vec<f64> = (0..samples.len())
        .map(|index| {
            (skipped_points as f64 + phase_start as f64 + (index * decimation) as f64
                - group_delay_points)
                / sw
        })
        .collect();
    if samples.len() < 16 {
        return Ok(RegionResult {
            components: Vec::new(),
            center_hz,
            bic: None,
            condition_number: 1.0,
            decimation,
            retained_samples: samples.len(),
            evaluated_model_orders: 0,
            warning: Some((
                CraftWarningKind::FitWindowFailure,
                "too few samples remained after filtering".to_owned(),
            )),
        });
    }
    let relative_bounds = (region.padded.0 - center_hz, region.padded.1 - center_hz);
    let n_observations = (samples.len() * 2) as f64;
    let initial_rss = samples.iter().map(Complex64::norm_sqr).sum();
    let mut best_bic = bic(initial_rss, n_observations, 1.0);
    let mut best_components = Vec::new();
    let mut best_condition = 1.0;
    let mut warning = None;

    let fit_bounds = CraftFitBounds {
        frequency_hz: relative_bounds,
        linewidth_hz: params.linewidth_hz,
    };
    let dwell_s = decimation as f64 / sw;
    let merge_hz = sw / input.len() as f64;
    let max_order = params
        .max_components_per_fit_window
        .min(samples.len() / 2 - 1);
    let mut evaluated_model_orders = 0;
    // Matrix-pencil cost grows cubically with its Hankel dimension. The early
    // 256 uniformly sampled points contain the same frequency/decay poles and
    // keep full-width, long acquisitions bounded; the final LM still uses all
    // retained samples.
    let pencil_samples = &samples[..samples.len().min(256)];
    for order in 1..=max_order {
        evaluated_model_orders += 1;
        let Ok(candidate) = matrix_pencil_estimates(pencil_samples, dwell_s, order, fit_bounds)
        else {
            continue;
        };
        if candidate.components.len() != order {
            continue;
        }
        let fit = match evaluate_damped_sinusoids_cancellable(
            &samples,
            &times,
            &candidate.components,
            fit_bounds,
            cancelled,
        ) {
            Ok(fit) => Some(fit),
            Err(CraftFitError::Cancelled) => return Err(CraftError::Cancelled),
            Err(error) => {
                warning = Some((CraftWarningKind::FitWindowFailure, error.to_string()));
                None
            }
        };
        if let Some(fit) = fit {
            let candidate_bic = bic(
                fit.rss,
                n_observations,
                (fit.components.len() * 4 + 1) as f64,
            );
            let separated = fit
                .components
                .windows(2)
                .all(|pair| (pair[1].frequency_hz - pair[0].frequency_hz).abs() >= merge_hz);
            if candidate_bic < best_bic && separated && fit.condition_number <= 1e8 {
                best_bic = candidate_bic;
                best_condition = fit.condition_number;
                best_components = fit.components;
            }
        }
    }
    if !best_components.is_empty()
        && warning
            .as_ref()
            .is_some_and(|(kind, _)| *kind == CraftWarningKind::FitWindowFailure)
    {
        warning = None;
    }
    if best_components.len() == max_order {
        warning = Some((
            CraftWarningKind::ModelOrderLimit,
            "model order reached the fit-window limit; inspect the residual before quantitation"
                .to_owned(),
        ));
    }
    Ok(RegionResult {
        components: best_components,
        center_hz,
        bic: Some(best_bic),
        condition_number: best_condition,
        decimation,
        retained_samples: samples.len(),
        evaluated_model_orders,
        warning,
    })
}

fn low_pass_fir(
    input: &[Complex64],
    sample_rate_hz: f64,
    cutoff_hz: f64,
    requested_taps: usize,
    cancelled: &impl Fn() -> bool,
) -> Result<Vec<Complex64>, CraftError> {
    if cutoff_hz * 2.0 >= sample_rate_hz * 0.999 {
        return Ok(input.to_vec());
    }
    let taps = effective_filter_taps(requested_taps, input.len());
    if taps < 3 {
        return Ok(input.to_vec());
    }
    let half = taps / 2;
    let normalized = cutoff_hz / sample_rate_hz;
    let mut kernel = Vec::with_capacity(taps);
    for index in 0..taps {
        let x = index as isize - half as isize;
        let sinc = if x == 0 {
            2.0 * normalized
        } else {
            (TAU * normalized * x as f64).sin() / (PI * x as f64)
        };
        let window = 0.42 - 0.5 * (TAU * index as f64 / (taps - 1) as f64).cos()
            + 0.08 * (2.0 * TAU * index as f64 / (taps - 1) as f64).cos();
        kernel.push(sinc * window);
    }
    let sum: f64 = kernel.iter().sum();
    for coefficient in &mut kernel {
        *coefficient /= sum;
    }
    let mut output = vec![Complex64::new(0.0, 0.0); input.len()];
    for (center, filtered) in output
        .iter_mut()
        .enumerate()
        .take(input.len().saturating_sub(half))
        .skip(half)
    {
        if center % 64 == 0 && cancelled() {
            return Err(CraftError::Cancelled);
        }
        let start = center - half;
        *filtered = input[start..start + taps]
            .iter()
            .zip(&kernel)
            .fold(Complex64::new(0.0, 0.0), |sum, (&sample, &coefficient)| {
                sum + sample * coefficient
            });
    }
    Ok(output)
}

fn effective_filter_taps(requested_taps: usize, input_len: usize) -> usize {
    let taps = requested_taps.min(input_len.saturating_sub(1));
    if taps.is_multiple_of(2) {
        taps.saturating_sub(1)
    } else {
        taps
    }
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

fn bic(rss: f64, observations: f64, parameters: f64) -> f64 {
    observations * (rss.max(f64::MIN_POSITIVE) / observations).ln() + parameters * observations.ln()
}

#[cfg(test)]
#[path = "craft_tests.rs"]
mod tests;
