use plotx_io::NmrData;

use super::{
    CraftComponent, CraftError, CraftParams, CraftReference, CraftRegion, CraftRegionId,
    CraftRegionRatio, CraftRegionSummary,
};

#[derive(Clone, Copy)]
pub(super) struct HzRegion {
    pub(super) selection: CraftRegion,
    pub(super) core: (f64, f64),
    pub(super) padded: (f64, f64),
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

pub(super) fn build_regions(
    data: &NmrData,
    params: &CraftParams,
    reference: CraftReference,
) -> Result<Vec<HzRegion>, CraftError> {
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

    let mut regions = Vec::new();
    for (selection, start, end) in requested_cores {
        let pieces = ((end - start) / params.max_fit_window_width_hz)
            .ceil()
            .max(1.0) as usize;
        let width = (end - start) / pieces as f64;
        for index in 0..pieces {
            let core = (
                start + index as f64 * width,
                start + (index + 1) as f64 * width,
            );
            let padding = (core.1 - core.0) * params.padding_fraction * 0.5;
            regions.push(HzRegion {
                selection,
                core,
                padded: (
                    (core.0 - padding).max(-half_sw),
                    (core.1 + padding).min(half_sw),
                ),
            });
        }
    }
    Ok(regions)
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
