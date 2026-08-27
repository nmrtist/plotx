use plotx_io::NmrData;
use serde::{Deserialize, Serialize};

use super::preflight::detect_clear_signals;
use super::regions::build_modeling_windows;
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
    pub maximum_model_order: Option<usize>,
    pub minimum_amplitude_to_noise: Option<f64>,
    pub component_linewidth_bounds_hz: Option<(f64, f64)>,
    pub fir_filter_taps: Option<usize>,
    pub maximum_modeled_sample_count: Option<usize>,
    pub skip_duration_s: Option<f64>,
    pub reconstruction_duration_s: Option<Option<f64>>,
}

impl CraftParamOverrides {
    /// Treat a complete externally supplied parameter object as explicit input.
    pub fn from_params(params: CraftParams) -> Self {
        Self {
            profile: Some(params.profile),
            regions: Some(params.regions),
            maximum_model_order: Some(params.maximum_model_order),
            minimum_amplitude_to_noise: Some(params.minimum_amplitude_to_noise),
            component_linewidth_bounds_hz: Some(params.component_linewidth_bounds_hz),
            fir_filter_taps: Some(params.fir_filter_taps),
            maximum_modeled_sample_count: Some(params.maximum_modeled_sample_count),
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
    pub maximum_model_order: CraftParamSource,
    pub minimum_amplitude_to_noise: CraftParamSource,
    pub component_linewidth_bounds_hz: CraftParamSource,
    pub fir_filter_taps: CraftParamSource,
    pub maximum_modeled_sample_count: CraftParamSource,
    pub skip_duration_s: CraftParamSource,
    pub reconstruction_duration_s: CraftParamSource,
}

impl CraftParameterSources {
    pub fn uses_result_provenance(&self) -> bool {
        [
            self.profile,
            self.regions,
            self.maximum_model_order,
            self.minimum_amplitude_to_noise,
            self.component_linewidth_bounds_hz,
            self.fir_filter_taps,
            self.maximum_modeled_sample_count,
            self.skip_duration_s,
            self.reconstruction_duration_s,
        ]
        .contains(&CraftParamSource::ResultProvenance)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CraftDerivedModelingWindow {
    pub retention_band_hz: (f64, f64),
    pub modeling_band_hz: (f64, f64),
    pub planned_decimation_factor: usize,
    pub planned_modeled_sample_count: usize,
    pub planned_modeled_duration_s: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct CraftModelingPolicy {
    pub modeling_bandwidth_hz: f64,
    pub modeling_duration_s: f64,
    pub validation_tail_fraction: f64,
    pub boundary_stability_relative_tolerance: f64,
    pub component_linewidth_bounds_hz: (f64, f64),
}

impl Default for CraftModelingPolicy {
    fn default() -> Self {
        Self {
            modeling_bandwidth_hz: 250.0,
            modeling_duration_s: 1.0,
            validation_tail_fraction: 0.2,
            boundary_stability_relative_tolerance: 0.01,
            component_linewidth_bounds_hz: (0.05, 20.0),
        }
    }
}

impl CraftModelingPolicy {
    pub(super) fn for_params(params: &CraftParams) -> Self {
        Self {
            modeling_bandwidth_hz: params.profile.modeling_bandwidth_hz(),
            modeling_duration_s: params.profile.modeling_duration_s(),
            component_linewidth_bounds_hz: params.component_linewidth_bounds_hz,
            ..Self::default()
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CraftDerivedPlan {
    pub effective_skip_points: usize,
    pub effective_skip_source: CraftParamSource,
    pub available_points: usize,
    pub effective_fir_filter_taps: usize,
    pub reconstruction_points: usize,
    pub resolved_regions: Vec<CraftRegion>,
    pub modeling_windows: Vec<CraftDerivedModelingWindow>,
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
    let (maximum_model_order, maximum_model_order_source) = resolved!(maximum_model_order);
    let (minimum_amplitude_to_noise, minimum_amplitude_source) =
        resolved!(minimum_amplitude_to_noise);
    let (component_linewidth_bounds_hz, component_linewidth_bounds_source) =
        resolved!(component_linewidth_bounds_hz);
    let (fir_filter_taps, fir_filter_taps_source) = resolved!(fir_filter_taps);
    let (maximum_modeled_sample_count, maximum_modeled_sample_count_source) =
        resolved!(maximum_modeled_sample_count);
    let (skip_duration_s, skip_source) = resolved!(skip_duration_s);
    let (reconstruction_duration_s, reconstruction_source) = resolved!(reconstruction_duration_s);
    let params = CraftParams {
        profile,
        regions,
        maximum_model_order,
        minimum_amplitude_to_noise,
        component_linewidth_bounds_hz,
        fir_filter_taps,
        maximum_modeled_sample_count,
        skip_duration_s,
        reconstruction_duration_s,
    };
    let sources = CraftParameterSources {
        profile: profile_source,
        regions: regions_source,
        maximum_model_order: maximum_model_order_source,
        minimum_amplitude_to_noise: minimum_amplitude_source,
        component_linewidth_bounds_hz: component_linewidth_bounds_source,
        fir_filter_taps: fir_filter_taps_source,
        maximum_modeled_sample_count: maximum_modeled_sample_count_source,
        skip_duration_s: skip_source,
        reconstruction_duration_s: reconstruction_source,
    };
    let modeling_policy = CraftModelingPolicy::for_params(&params);
    let derived_plan = derive_plan(data, reference, &params, &sources, modeling_policy);
    let assessment = CraftInputAssessment::assess(data, reference, &params, &derived_plan);
    CraftInvocation {
        params,
        sources,
        reference,
        derived_plan,
        assessment,
        modeling_policy,
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
    modeling_policy: CraftModelingPolicy,
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
    let fit_points = if data.spectral_width_hz.is_finite() && data.spectral_width_hz > 0.0 {
        (modeling_policy.modeling_duration_s * data.spectral_width_hz)
            .ceil()
            .max(1.0) as usize
    } else {
        0
    };
    let effective_fir_filter_taps = effective_taps(
        params.fir_filter_taps,
        available_points.min(fit_points.saturating_add(params.fir_filter_taps)),
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
    let mut modeling_windows = Vec::new();
    if data.spectral_width_hz.is_finite()
        && data.spectral_width_hz > 0.0
        && data.observe_freq_mhz.is_finite()
        && data.observe_freq_mhz > 0.0
        && reference.effective_carrier_ppm().is_finite()
        && params.profile.modeling_bandwidth_hz().is_finite()
    {
        let filter_input = available_points.min(fit_points.saturating_add(params.fir_filter_taps));
        let clear_signals = detect_clear_signals(data, reference, effective_skip_points);
        for window in
            build_modeling_windows(data, params, reference, &clear_signals).unwrap_or_default()
        {
            let modeled_bandwidth_hz = window.modeling_band_hz.1 - window.modeling_band_hz.0;
            let mut decimation = (data.spectral_width_hz
                / (2.0 * modeled_bandwidth_hz).max(f64::MIN_POSITIVE))
            .floor()
            .max(1.0) as usize;
            if params.maximum_modeled_sample_count > 0 {
                decimation =
                    decimation.max(filter_input.div_ceil(params.maximum_modeled_sample_count));
            }
            let retained = filter_input
                .saturating_sub(effective_fir_filter_taps)
                .min(fit_points)
                .div_ceil(decimation);
            modeling_windows.push(CraftDerivedModelingWindow {
                retention_band_hz: window.retention_band_hz,
                modeling_band_hz: window.modeling_band_hz,
                planned_decimation_factor: decimation,
                planned_modeled_sample_count: retained,
                planned_modeled_duration_s: retained as f64 * decimation as f64
                    / data.spectral_width_hz,
            });
        }
    }
    CraftDerivedPlan {
        effective_skip_points,
        effective_skip_source,
        available_points,
        effective_fir_filter_taps,
        reconstruction_points,
        resolved_regions,
        modeling_windows,
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
