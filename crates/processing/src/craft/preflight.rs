use plotx_analysis::peaks::{DetectParams, detect_peaks, estimate_noise};
use plotx_io::{Domain, NmrData};
use rustfft::FftPlanner;
use serde::{Deserialize, Serialize};
use std::f64::consts::PI;

use super::{CraftDerivedPlan, CraftParams, CraftReference, CraftRegionId};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CraftIssueSeverity {
    Error,
    Warning,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CraftIssueCode {
    IncompatibleDomain,
    EmptyInput,
    NonFiniteSamples,
    InvalidAcquisition,
    InvalidReference,
    InvalidParameters,
    InvalidRegions,
    TooFewEffectivePoints,
    ShortEffectiveRecord,
    NoClearSignal,
    RegionWithoutClearSignal,
    DenseSignalWindow,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CraftIssueAction {
    SelectTimeDomainFid,
    CheckImport,
    CheckAcquisitionMetadata,
    CorrectReference,
    ResetModelingSettings,
    AdjustRegions,
    ReduceSkippedPoints,
    ReviewAcquisition,
    ConfirmWithIndependentEvidence,
    IncreaseModelLimit,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CraftAssessmentIssue {
    pub code: CraftIssueCode,
    pub severity: CraftIssueSeverity,
    pub region: Option<CraftRegionId>,
    pub message: String,
    pub action: CraftIssueAction,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct CraftSignalSuggestion {
    pub chemical_shift_ppm: f64,
    pub height_sigma: f64,
    pub prominence_sigma: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CraftInputAssessment {
    pub point_count: usize,
    pub effective_point_count: usize,
    pub acquisition_duration_s: Option<f64>,
    pub modeling_window_count: usize,
    pub clear_signals: Vec<CraftSignalSuggestion>,
    pub issues: Vec<CraftAssessmentIssue>,
}

impl CraftInputAssessment {
    pub(super) fn assess(
        data: &NmrData,
        reference: CraftReference,
        params: &CraftParams,
        plan: &CraftDerivedPlan,
    ) -> Self {
        let mut issues = Vec::new();
        {
            let mut error = |code, message: &str, action| {
                issues.push(CraftAssessmentIssue {
                    code,
                    severity: CraftIssueSeverity::Error,
                    region: None,
                    message: message.to_owned(),
                    action,
                })
            };
            if data.domain != Domain::Time {
                error(
                    CraftIssueCode::IncompatibleDomain,
                    "Select a one-dimensional time-domain FID.",
                    CraftIssueAction::SelectTimeDomainFid,
                );
            }
            if data.points.is_empty() {
                error(
                    CraftIssueCode::EmptyInput,
                    "The FID contains no samples; check the import.",
                    CraftIssueAction::CheckImport,
                );
            } else if data
                .points
                .iter()
                .any(|point| !point.re.is_finite() || !point.im.is_finite())
            {
                error(
                    CraftIssueCode::NonFiniteSamples,
                    "The FID contains NaN or infinite samples; check the import.",
                    CraftIssueAction::CheckImport,
                );
            }
            if !data.spectral_width_hz.is_finite()
                || data.spectral_width_hz <= 0.0
                || !data.observe_freq_mhz.is_finite()
                || data.observe_freq_mhz <= 0.0
                || !data.carrier_ppm.is_finite()
                || !data.group_delay.is_finite()
                || data.group_delay < 0.0
            {
                error(
                    CraftIssueCode::InvalidAcquisition,
                    "Spectral width, observe frequency, carrier, or group delay is invalid.",
                    CraftIssueAction::CheckAcquisitionMetadata,
                );
            }
            if reference.validate(data).is_err() {
                error(
                    CraftIssueCode::InvalidReference,
                    "The chemical-shift reference does not match this acquisition.",
                    CraftIssueAction::CorrectReference,
                );
            }
            if params.validate().is_err() {
                error(
                    CraftIssueCode::InvalidParameters,
                    "One or more explicit component or acquisition settings are invalid.",
                    CraftIssueAction::ResetModelingSettings,
                );
            }
            if regions_outside_bandwidth_or_overlap(data, reference, params) {
                error(
                    CraftIssueCode::InvalidRegions,
                    "Spectral regions overlap or extend outside the acquired bandwidth.",
                    CraftIssueAction::AdjustRegions,
                );
            }
            if plan.available_points < 16 {
                error(
                    CraftIssueCode::TooFewEffectivePoints,
                    "Fewer than 16 samples remain after the effective skip.",
                    CraftIssueAction::ReduceSkippedPoints,
                );
            }
            if plan
                .modeling_windows
                .iter()
                .any(|window| window.planned_modeled_sample_count < 16)
            {
                error(
                    CraftIssueCode::TooFewEffectivePoints,
                    "Fewer than 16 samples remain in one or more modeling windows after FIR filtering.",
                    CraftIssueAction::ResetModelingSettings,
                );
            }
        }

        if plan.available_points >= 16 && plan.available_points < 64 {
            issues.push(warning(CraftIssueCode::ShortEffectiveRecord, None, "Fewer than 64 effective samples remain; inspect the residual and confirm the result independently.", CraftIssueAction::ReviewAcquisition));
        }
        let clear_signals = if issues
            .iter()
            .any(|issue| issue.severity == CraftIssueSeverity::Error)
        {
            Vec::new()
        } else {
            detect_clear_signals(data, reference, plan.effective_skip_points)
        };
        if plan.available_points >= 16 && clear_signals.is_empty() {
            issues.push(warning(CraftIssueCode::NoClearSignal, None, "No clear signal reached the 6σ height and 5σ prominence thresholds; the calculation can run but needs review.", CraftIssueAction::ConfirmWithIndependentEvidence));
        }
        for region in &params.regions {
            let normalized = region.normalized();
            let count = clear_signals
                .iter()
                .filter(|signal| {
                    signal.chemical_shift_ppm >= normalized.start_ppm
                        && signal.chemical_shift_ppm <= normalized.end_ppm
                })
                .count();
            if count == 0 && !clear_signals.is_empty() {
                issues.push(warning(CraftIssueCode::RegionWithoutClearSignal, Some(region.id), "A selected region contains no clear signal; adjust the region or confirm it with independent evidence.", CraftIssueAction::AdjustRegions));
            }
            // Peak-picking is only a preflight hint.  The FFT can contain many
            // transition-band extrema for one physical multiplet, so raw peak
            // count must not be used as a model-capacity warning.  Capacity is
            // assessed after the bounded time-domain fit has selected a model.
        }
        Self {
            point_count: data.points.len(),
            effective_point_count: plan.available_points,
            acquisition_duration_s: (data.spectral_width_hz.is_finite()
                && data.spectral_width_hz > 0.0)
                .then(|| data.points.len() as f64 / data.spectral_width_hz),
            modeling_window_count: plan.modeling_windows.len(),
            clear_signals,
            issues,
        }
    }

    pub fn can_run(&self) -> bool {
        !self
            .issues
            .iter()
            .any(|issue| issue.severity == CraftIssueSeverity::Error)
    }

    pub fn has_warnings(&self) -> bool {
        self.issues
            .iter()
            .any(|issue| issue.severity == CraftIssueSeverity::Warning)
    }

    pub fn first_blocking_message(&self) -> Option<&str> {
        self.issues
            .iter()
            .find(|issue| issue.severity == CraftIssueSeverity::Error)
            .map(|issue| issue.message.as_str())
    }
}

fn warning(
    code: CraftIssueCode,
    region: Option<CraftRegionId>,
    message: &str,
    action: CraftIssueAction,
) -> CraftAssessmentIssue {
    CraftAssessmentIssue {
        code,
        severity: CraftIssueSeverity::Warning,
        region,
        message: message.to_owned(),
        action,
    }
}

fn regions_outside_bandwidth_or_overlap(
    data: &NmrData,
    reference: CraftReference,
    params: &CraftParams,
) -> bool {
    if !data.spectral_width_hz.is_finite()
        || !data.observe_freq_mhz.is_finite()
        || data.observe_freq_mhz <= 0.0
    {
        return false;
    }
    let half_ppm = data.spectral_width_hz / (2.0 * data.observe_freq_mhz);
    let carrier = reference.effective_carrier_ppm();
    let lower = carrier - half_ppm;
    let upper = carrier + half_ppm;
    let mut regions = params
        .regions
        .iter()
        .copied()
        .map(|region| region.normalized())
        .collect::<Vec<_>>();
    regions.sort_by(|left, right| left.start_ppm.total_cmp(&right.start_ppm));
    regions.iter().any(|region| {
        !region.start_ppm.is_finite()
            || !region.end_ppm.is_finite()
            || region.start_ppm < lower
            || region.end_ppm > upper
            || region.start_ppm >= region.end_ppm
    }) || regions
        .windows(2)
        .any(|pair| pair[0].end_ppm > pair[1].start_ppm)
}

pub(super) fn detect_clear_signals(
    data: &NmrData,
    reference: CraftReference,
    skip: usize,
) -> Vec<CraftSignalSuggestion> {
    let input = &data.points[skip..];
    if input.len() < 3 {
        return Vec::new();
    }
    let fft_len = input.len().next_power_of_two();
    let mut spectrum = vec![num_complex::Complex64::new(0.0, 0.0); fft_len];
    let duration_s = input.len() as f64 / data.spectral_width_hz;
    let matched_line_broadening_hz = 1.0 / duration_s.max(f64::MIN_POSITIVE);
    for (index, (&sample, output)) in input.iter().zip(&mut spectrum).enumerate() {
        let time_s = index as f64 / data.spectral_width_hz;
        *output = sample * (-PI * matched_line_broadening_hz * time_s).exp();
    }
    FftPlanner::<f64>::new()
        .plan_fft_forward(fft_len)
        .process(&mut spectrum);
    let magnitudes = spectrum
        .iter()
        .map(|value| value.norm())
        .collect::<Vec<_>>();
    let shifted = (0..fft_len)
        .map(|index| magnitudes[(index + fft_len / 2) % fft_len])
        .collect::<Vec<_>>();
    let sigma = estimate_noise(&shifted).max(f64::MIN_POSITIVE);
    let xs = (0..fft_len).map(|index| index as f64).collect::<Vec<_>>();
    let peaks = detect_peaks(
        &xs,
        &shifted,
        &DetectParams {
            min_height: Some(6.0 * sigma),
            min_prominence: 5.0 * sigma,
            // Merge FFT extrema closer than one acquired spectral
            // resolution element (1/acquisition time).  Matched exponential
            // apodization broadens a line and otherwise creates several
            // equally significant extrema for a single resonance.
            // `xs` is expressed in zero-padded FFT-bin indices.  One acquired
            // spectral-resolution element spans `fft_len / input.len()` of
            // those bins when the FFT is zero-padded.
            min_spacing: Some(fft_len as f64 / input.len() as f64),
            max_count: Some(64),
        },
    );
    peaks
        .into_iter()
        .map(|peak| {
            let frequency_hz = (peak.index as f64 / fft_len as f64 - 0.5) * data.spectral_width_hz;
            CraftSignalSuggestion {
                chemical_shift_ppm: reference.effective_carrier_ppm()
                    + frequency_hz / data.observe_freq_mhz,
                height_sigma: shifted[peak.index] / sigma,
                prominence_sigma: peak.prominence / sigma,
            }
        })
        .collect()
}
