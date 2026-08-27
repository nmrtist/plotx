use plotx_io::NmrData;

use super::{
    CraftComponent, CraftError, CraftParams, CraftReference, CraftRegion, CraftRegionId,
    CraftRegionRatio, CraftRegionSummary, CraftSignalSuggestion,
};

#[derive(Clone, Copy, Debug)]
pub(super) struct ModelingWindow {
    pub(super) retention_band_hz: (f64, f64),
    pub(super) modeling_band_hz: (f64, f64),
}

pub(super) fn selections_are_valid(regions: &[CraftRegion]) -> bool {
    let ids = regions
        .iter()
        .map(|region| region.id)
        .collect::<std::collections::HashSet<_>>();
    if ids.len() != regions.len()
        || regions.iter().any(|region| {
            !region.start_ppm.is_finite()
                || !region.end_ppm.is_finite()
                || region.start_ppm == region.end_ppm
        })
    {
        return false;
    }
    let mut normalized = regions
        .iter()
        .copied()
        .map(CraftRegion::normalized)
        .collect::<Vec<_>>();
    normalized.sort_by(|left, right| left.start_ppm.total_cmp(&right.start_ppm));
    normalized
        .windows(2)
        .all(|pair| pair[0].end_ppm <= pair[1].start_ppm)
}

pub(super) fn build_modeling_windows(
    data: &NmrData,
    params: &CraftParams,
    reference: CraftReference,
    clear_signals: &[CraftSignalSuggestion],
) -> Result<Vec<ModelingWindow>, CraftError> {
    let half_sw = data.spectral_width_hz * 0.5;
    let effective_carrier_ppm = reference.effective_carrier_ppm();
    let requested: Vec<(CraftRegion, f64, f64)> = if params.regions.is_empty() {
        let selection = CraftRegion::new(
            CraftRegionId(0),
            effective_carrier_ppm - half_sw / data.observe_freq_mhz,
            effective_carrier_ppm + half_sw / data.observe_freq_mhz,
        );
        vec![(selection, -half_sw, half_sw)]
    } else {
        params
            .regions
            .iter()
            .copied()
            .map(CraftRegion::normalized)
            .map(|region| {
                (
                    region,
                    (region.start_ppm - effective_carrier_ppm) * data.observe_freq_mhz,
                    (region.end_ppm - effective_carrier_ppm) * data.observe_freq_mhz,
                )
            })
            .collect()
    };
    let mut requested_cores = Vec::new();
    for (selection, start, end) in requested {
        let start = start.clamp(-half_sw, half_sw);
        let end = end.clamp(-half_sw, half_sw);
        if !start.is_finite() || !end.is_finite() || end <= start {
            return Err(CraftError::InvalidParameters);
        }
        requested_cores.push((
            CraftRegion::new(
                selection.id,
                effective_carrier_ppm + start / data.observe_freq_mhz,
                effective_carrier_ppm + end / data.observe_freq_mhz,
            ),
            start,
            end,
        ));
    }
    requested_cores.sort_by(|left, right| left.1.total_cmp(&right.1));
    if requested_cores.windows(2).any(|pair| pair[0].2 > pair[1].1) {
        return Err(CraftError::InvalidParameters);
    }

    // Modeling windows are a profile-owned protocol. They tile the acquired
    // bandwidth with a fixed physical width and therefore do not change when a
    // user nudges a reporting region boundary.
    let width = params
        .profile
        .modeling_bandwidth_hz()
        .min(data.spectral_width_hz);
    let signal_hz = clear_signals
        .iter()
        .filter_map(|signal| {
            let frequency =
                (signal.chemical_shift_ppm - effective_carrier_ppm) * data.observe_freq_mhz;
            let weight = signal.prominence_sigma.max(f64::MIN_POSITIVE);
            (frequency.is_finite()
                && weight.is_finite()
                && frequency >= -half_sw
                && frequency <= half_sw)
                .then_some((frequency, weight))
        })
        .collect::<Vec<_>>();
    let mut centers = signal_cluster_centers(&signal_hz, width);
    if centers.is_empty() {
        let pieces = (data.spectral_width_hz / width).ceil().max(1.0) as usize;
        centers.extend((0..pieces).map(|index| {
            let start = -half_sw + index as f64 * width;
            (start + (start + width).min(half_sw)) * 0.5
        }));
    }
    let mut regions = Vec::with_capacity(centers.len());
    for (index, center) in centers.iter().copied().enumerate() {
        let nominal_start = (center - width * 0.5).max(-half_sw);
        let nominal_end = (center + width * 0.5).min(half_sw);
        let retention_start = centers
            .get(index.wrapping_sub(1))
            .map_or(nominal_start, |previous| {
                nominal_start.max((previous + center) * 0.5)
            });
        let retention_end = centers
            .get(index + 1)
            .map_or(nominal_end, |next| nominal_end.min((center + next) * 0.5));
        regions.push(ModelingWindow {
            retention_band_hz: (retention_start, retention_end),
            modeling_band_hz: (nominal_start, nominal_end),
        });
    }
    Ok(regions)
}

fn signal_cluster_centers(signals: &[(f64, f64)], window_width_hz: f64) -> Vec<f64> {
    let mut ranked = signals.to_vec();
    ranked.sort_by(|left, right| {
        right
            .1
            .total_cmp(&left.1)
            .then_with(|| left.0.total_cmp(&right.0))
    });
    let minimum_center_spacing = window_width_hz * 0.5;
    let mut seeds = Vec::new();
    for &(frequency, _) in &ranked {
        if seeds
            .iter()
            .all(|seed: &f64| (frequency - *seed).abs() > minimum_center_spacing)
        {
            seeds.push(frequency);
        }
    }

    let mut clusters = vec![Vec::new(); seeds.len()];
    for signal in ranked {
        if let Some((index, _)) = seeds.iter().enumerate().min_by(|(_, left), (_, right)| {
            (signal.0 - **left)
                .abs()
                .total_cmp(&(signal.0 - **right).abs())
        }) {
            clusters[index].push(signal);
        }
    }
    let mut centers = clusters
        .into_iter()
        .filter_map(weighted_median_frequency)
        .collect::<Vec<_>>();
    centers.sort_by(f64::total_cmp);
    centers
}

fn weighted_median_frequency(mut signals: Vec<(f64, f64)>) -> Option<f64> {
    signals.sort_by(|left, right| left.0.total_cmp(&right.0));
    let half_weight = signals.iter().map(|signal| signal.1).sum::<f64>() * 0.5;
    let mut accumulated = 0.0;
    signals.into_iter().find_map(|(frequency, weight)| {
        accumulated += weight;
        (accumulated >= half_weight).then_some(frequency)
    })
}

pub(super) fn summarize_regions(
    components: &[CraftComponent],
    selections: &[CraftRegion],
) -> Vec<CraftRegionSummary> {
    selections
        .iter()
        .map(|selection| {
            let selected = components
                .iter()
                .filter(|component| component.region == selection.id);
            let (component_count, scalar_amplitude_sum_t0, coherent_re, coherent_im) = selected
                .fold((0, 0.0, 0.0, 0.0), |(count, scalar, re, im), component| {
                    (
                        count + 1,
                        scalar + component.amplitude_t0,
                        re + component.amplitude_t0 * component.phase_rad.cos(),
                        im + component.amplitude_t0 * component.phase_rad.sin(),
                    )
                });
            CraftRegionSummary {
                region: selection.id,
                start_ppm: selection.start_ppm,
                end_ppm: selection.end_ppm,
                component_count,
                scalar_amplitude_sum_t0,
                coherent_amplitude_t0: coherent_re.hypot(coherent_im),
            }
        })
        .collect()
}

pub(super) fn region_ratio(summaries: &[CraftRegionSummary]) -> Option<CraftRegionRatio> {
    let [numerator, denominator] = summaries else {
        return None;
    };
    (denominator.coherent_amplitude_t0 > 0.0).then_some(CraftRegionRatio {
        numerator: numerator.region,
        denominator: denominator.region,
        value: numerator.coherent_amplitude_t0 / denominator.coherent_amplitude_t0,
    })
}
