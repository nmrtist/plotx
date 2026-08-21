use plotx_io::NmrData;
use serde::{Deserialize, Serialize};

use super::{
    CraftInputAssessment, CraftInvocation, CraftParams, CraftProfile, CraftReference, CraftRegion,
    CraftRegionId,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CraftParamSource {
    ExplicitInput,
    ResultProvenance,
    StableDefault,
    InputDerived,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct CraftParamOverrides {
    pub profile: Option<CraftProfile>,
    pub regions: Option<Vec<CraftRegion>>,
    pub max_components_per_fit_window: Option<usize>,
    pub min_amplitude_to_noise: Option<f64>,
    pub linewidth_hz: Option<(f64, f64)>,
    pub filter_taps: Option<usize>,
    pub padding_fraction: Option<f64>,
    pub max_fit_window_width_hz: Option<f64>,
    pub max_downsampled_points: Option<usize>,
    pub skip_duration_s: Option<f64>,
    pub reconstruction_duration_s: Option<Option<f64>>,
}

impl CraftParamOverrides {
    /// Treat a complete externally supplied parameter object as explicit input.
    pub fn from_params(params: CraftParams) -> Self {
        Self {
            profile: Some(params.profile),
            regions: Some(params.regions),
            max_components_per_fit_window: Some(params.max_components_per_fit_window),
            min_amplitude_to_noise: Some(params.min_amplitude_to_noise),
            linewidth_hz: Some(params.linewidth_hz),
            filter_taps: Some(params.filter_taps),
            padding_fraction: Some(params.padding_fraction),
            max_fit_window_width_hz: Some(params.max_fit_window_width_hz),
            max_downsampled_points: Some(params.max_downsampled_points),
            skip_duration_s: Some(params.skip_duration_s),
            reconstruction_duration_s: Some(params.reconstruction_duration_s),
        }
    }

    /// Select a profile explicitly while allowing all profile-owned fields to
    /// fall back to that profile's stable defaults. Region intent is retained.
    pub fn select_profile(&mut self, profile: CraftProfile) {
        let regions = self.regions.take();
        *self = Self {
            profile: Some(profile),
            regions,
            ..Self::default()
        };
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CraftParameterSources {
    pub profile: CraftParamSource,
    pub regions: CraftParamSource,
    pub max_components_per_fit_window: CraftParamSource,
    pub min_amplitude_to_noise: CraftParamSource,
    pub linewidth_hz: CraftParamSource,
    pub filter_taps: CraftParamSource,
    pub padding_fraction: CraftParamSource,
    pub max_fit_window_width_hz: CraftParamSource,
    pub max_downsampled_points: CraftParamSource,
    pub skip_duration_s: CraftParamSource,
    pub reconstruction_duration_s: CraftParamSource,
}

impl CraftParameterSources {
    pub fn uses_result_provenance(&self) -> bool {
        [
            self.profile,
            self.regions,
            self.max_components_per_fit_window,
            self.min_amplitude_to_noise,
            self.linewidth_hz,
            self.filter_taps,
            self.padding_fraction,
            self.max_fit_window_width_hz,
            self.max_downsampled_points,
            self.skip_duration_s,
            self.reconstruction_duration_s,
        ]
        .contains(&CraftParamSource::ResultProvenance)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CraftDerivedWindow {
    pub region: CraftRegionId,
    pub core_hz: (f64, f64),
    pub padded_hz: (f64, f64),
    pub planned_decimation: usize,
    pub planned_retained_samples: usize,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CraftDerivedPlan {
    pub effective_skip_points: usize,
    pub effective_skip_source: CraftParamSource,
    pub available_points: usize,
    pub actual_filter_taps: usize,
    pub reconstruction_points: usize,
    pub resolved_regions: Vec<CraftRegion>,
    pub fit_windows: Vec<CraftDerivedWindow>,
}

pub fn resolve_craft_invocation(
    data: &NmrData,
    reference: CraftReference,
    overrides: &CraftParamOverrides,
    provenance: Option<&CraftInvocation>,
) -> CraftInvocation {
    let provenance_params = provenance
        .filter(|value| {
            overrides
                .profile
                .is_none_or(|profile| profile == value.params.profile)
        })
        .map(|value| &value.params);
    let profile = overrides
        .profile
        .or_else(|| provenance_params.map(|params| params.profile))
        .unwrap_or_default();
    let defaults = match profile {
        CraftProfile::Conventional => CraftParams::conventional(),
        CraftProfile::Ssfp => CraftParams::ssfp(),
    };
    let profile_source = if overrides.profile.is_some() {
        CraftParamSource::ExplicitInput
    } else if provenance_params.is_some() {
        CraftParamSource::ResultProvenance
    } else {
        CraftParamSource::StableDefault
    };

    macro_rules! resolved {
        ($field:ident) => {{
            if let Some(value) = overrides.$field.clone() {
                (value, CraftParamSource::ExplicitInput)
            } else if let Some(value) = provenance_params.map(|params| params.$field.clone()) {
                (value, CraftParamSource::ResultProvenance)
            } else {
                (defaults.$field.clone(), CraftParamSource::StableDefault)
            }
        }};
    }

    let (mut regions, mut regions_source) = resolved!(regions);
    if overrides.regions.is_none() && provenance_params.is_none() {
        regions = full_bandwidth_region(data, reference).into_iter().collect();
        regions_source = CraftParamSource::InputDerived;
    }
    let (max_components_per_fit_window, max_components_source) =
        resolved!(max_components_per_fit_window);
    let (min_amplitude_to_noise, min_amplitude_source) = resolved!(min_amplitude_to_noise);
    let (linewidth_hz, linewidth_source) = resolved!(linewidth_hz);
    let (filter_taps, filter_taps_source) = resolved!(filter_taps);
    let (padding_fraction, padding_source) = resolved!(padding_fraction);
    let (max_fit_window_width_hz, fit_window_source) = resolved!(max_fit_window_width_hz);
    let (max_downsampled_points, downsample_source) = resolved!(max_downsampled_points);
    let (skip_duration_s, skip_source) = resolved!(skip_duration_s);
    let (reconstruction_duration_s, reconstruction_source) = resolved!(reconstruction_duration_s);
    let params = CraftParams {
        profile,
        regions,
        max_components_per_fit_window,
        min_amplitude_to_noise,
        linewidth_hz,
        filter_taps,
        padding_fraction,
        max_fit_window_width_hz,
        max_downsampled_points,
        skip_duration_s,
        reconstruction_duration_s,
    };
    let sources = CraftParameterSources {
        profile: profile_source,
        regions: regions_source,
        max_components_per_fit_window: max_components_source,
        min_amplitude_to_noise: min_amplitude_source,
        linewidth_hz: linewidth_source,
        filter_taps: filter_taps_source,
        padding_fraction: padding_source,
        max_fit_window_width_hz: fit_window_source,
        max_downsampled_points: downsample_source,
        skip_duration_s: skip_source,
        reconstruction_duration_s: reconstruction_source,
    };
    let derived_plan = derive_plan(data, reference, &params, &sources);
    let assessment = CraftInputAssessment::assess(data, reference, &params, &derived_plan);
    CraftInvocation {
        params,
        sources,
        reference,
        derived_plan,
        assessment,
    }
}

fn full_bandwidth_region(data: &NmrData, reference: CraftReference) -> Option<CraftRegion> {
    let half_width_ppm = data.spectral_width_hz / (2.0 * data.observe_freq_mhz);
    let carrier = reference.effective_carrier_ppm();
    (half_width_ppm.is_finite() && carrier.is_finite()).then(|| {
        CraftRegion::new(
            CraftRegionId(0),
            carrier - half_width_ppm,
            carrier + half_width_ppm,
        )
    })
}

fn derive_plan(
    data: &NmrData,
    reference: CraftReference,
    params: &CraftParams,
    sources: &CraftParameterSources,
) -> CraftDerivedPlan {
    let requested_skip = if data.spectral_width_hz.is_finite() && params.skip_duration_s.is_finite()
    {
        (params.skip_duration_s * data.spectral_width_hz)
            .round()
            .max(0.0) as usize
    } else {
        0
    };
    let group_delay_skip = if data.group_delay.is_finite() {
        data.group_delay.max(0.0).ceil() as usize
    } else {
        0
    };
    let effective_skip_points = requested_skip.max(group_delay_skip).min(data.points.len());
    let effective_skip_source = if group_delay_skip > requested_skip {
        CraftParamSource::InputDerived
    } else {
        sources.skip_duration_s
    };
    let available_points = data.points.len().saturating_sub(effective_skip_points);
    let actual_filter_taps = effective_taps(
        params.filter_taps,
        available_points.min(6_000_usize.saturating_add(params.filter_taps)),
    );
    let acquired_duration = if data.spectral_width_hz.is_finite() && data.spectral_width_hz > 0.0 {
        data.points.len() as f64 / data.spectral_width_hz
    } else {
        0.0
    };
    let reconstruction_duration = params
        .reconstruction_duration_s
        .unwrap_or(acquired_duration);
    let reconstruction_points = if reconstruction_duration.is_finite()
        && reconstruction_duration > 0.0
        && data.spectral_width_hz.is_finite()
        && data.spectral_width_hz > 0.0
    {
        (reconstruction_duration * data.spectral_width_hz)
            .ceil()
            .max(1.0) as usize
    } else {
        0
    };
    let resolved_regions = params.regions.clone();
    let mut fit_windows = Vec::new();
    if data.spectral_width_hz.is_finite()
        && data.spectral_width_hz > 0.0
        && data.observe_freq_mhz.is_finite()
        && data.observe_freq_mhz > 0.0
        && reference.effective_carrier_ppm().is_finite()
        && params.max_fit_window_width_hz.is_finite()
        && params.max_fit_window_width_hz > 0.0
    {
        let half_sw = data.spectral_width_hz * 0.5;
        let carrier = reference.effective_carrier_ppm();
        for selection in &resolved_regions {
            let normalized = selection.normalized();
            let start =
                ((normalized.start_ppm - carrier) * data.observe_freq_mhz).clamp(-half_sw, half_sw);
            let end =
                ((normalized.end_ppm - carrier) * data.observe_freq_mhz).clamp(-half_sw, half_sw);
            if !start.is_finite() || !end.is_finite() || end <= start {
                continue;
            }
            let pieces = ((end - start) / params.max_fit_window_width_hz)
                .ceil()
                .max(1.0) as usize;
            let width = (end - start) / pieces as f64;
            for index in 0..pieces {
                let core = (
                    start + index as f64 * width,
                    start + (index + 1) as f64 * width,
                );
                let padding = width * params.padding_fraction.max(0.0) * 0.5;
                let padded = (
                    (core.0 - padding).max(-half_sw),
                    (core.1 + padding).min(half_sw),
                );
                let padded_width = padded.1 - padded.0;
                let filter_input =
                    available_points.min(6_000_usize.saturating_add(params.filter_taps));
                let mut decimation = (data.spectral_width_hz
                    / (2.0 * padded_width).max(f64::MIN_POSITIVE))
                .floor()
                .max(1.0) as usize;
                if params.max_downsampled_points > 0 {
                    decimation =
                        decimation.max(filter_input.div_ceil(params.max_downsampled_points));
                }
                let retained = filter_input
                    .saturating_sub(actual_filter_taps)
                    .min(6_000)
                    .div_ceil(decimation);
                fit_windows.push(CraftDerivedWindow {
                    region: selection.id,
                    core_hz: core,
                    padded_hz: padded,
                    planned_decimation: decimation,
                    planned_retained_samples: retained,
                });
            }
        }
    }
    CraftDerivedPlan {
        effective_skip_points,
        effective_skip_source,
        available_points,
        actual_filter_taps,
        reconstruction_points,
        resolved_regions,
        fit_windows,
    }
}

fn effective_taps(requested: usize, input_len: usize) -> usize {
    let taps = requested.min(input_len.saturating_sub(1));
    if taps.is_multiple_of(2) {
        taps.saturating_sub(1)
    } else {
        taps
    }
}
