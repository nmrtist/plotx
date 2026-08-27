use plotx_io::NmrData;

use super::diagnostics::{
    CraftModelingWindowDiagnostic, CraftStabilityDiagnostics, CraftStabilityMetric,
    CraftStabilityRegion,
};
use super::regions::{region_ratio, selections_are_valid, summarize_regions};
use super::{CraftComponent, CraftComponentId, CraftModelingPolicy, CraftReference, CraftRegion};

pub(super) fn stability_diagnostics(
    all_components: &[CraftComponent],
    selections: &[CraftRegion],
    windows: &[CraftModelingWindowDiagnostic],
    policy: CraftModelingPolicy,
    reference: CraftReference,
    data: &NmrData,
) -> CraftStabilityDiagnostics {
    let delta_ppm = (0.01_f64).max(8.0 / data.observe_freq_mhz.max(f64::MIN_POSITIVE));
    let mut perturbations = vec![("original".to_owned(), selections.to_vec())];
    for (name, start_delta, end_delta) in [
        ("shift left", -delta_ppm, -delta_ppm),
        ("shift right", delta_ppm, delta_ppm),
        ("expand", -delta_ppm, delta_ppm),
        ("contract", delta_ppm, -delta_ppm),
    ] {
        perturbations.push((
            name.to_owned(),
            selections
                .iter()
                .map(|region| {
                    CraftRegion::new(
                        region.id,
                        region.start_ppm + start_delta,
                        region.end_ppm + end_delta,
                    )
                })
                .collect(),
        ));
    }
    for (index, region) in selections.iter().enumerate() {
        for (side, start_delta, end_delta) in [
            ("left edge left", -delta_ppm, 0.0),
            ("left edge right", delta_ppm, 0.0),
            ("right edge left", 0.0, -delta_ppm),
            ("right edge right", 0.0, delta_ppm),
        ] {
            let mut moved = selections.to_vec();
            moved[index] = CraftRegion::new(
                region.id,
                region.start_ppm + start_delta,
                region.end_ppm + end_delta,
            );
            perturbations.push((format!("region {} {side}", region.id.0), moved));
        }
    }

    let carrier = reference.effective_carrier_ppm();
    let half_ppm = data.spectral_width_hz / (2.0 * data.observe_freq_mhz);
    let lower = carrier - half_ppm;
    let upper = carrier + half_ppm;
    let mut skipped = Vec::new();
    let mut observations = Vec::new();
    for (name, regions) in perturbations {
        if !selections_are_valid(&regions)
            || regions.iter().any(|region| {
                let region = region.normalized();
                region.start_ppm < lower || region.end_ppm > upper
            })
        {
            skipped.push(format!("{name}: invalid or overlapping regions"));
            continue;
        }
        let assigned = components_for_regions(all_components, &regions);
        let summaries = summarize_regions(&assigned, &regions);
        let ratio = region_ratio(&summaries).map(|ratio| ratio.value);
        observations.push((summaries, ratio));
    }

    let total_model_order = windows
        .iter()
        .map(|window| window.selected_model_order)
        .sum();
    let regions = selections
        .iter()
        .map(|selection| {
            let summaries = observations
                .iter()
                .filter_map(|(summaries, _)| {
                    summaries
                        .iter()
                        .find(|summary| summary.region == selection.id)
                })
                .collect::<Vec<_>>();
            let values = summaries
                .iter()
                .map(|summary| summary.coherent_amplitude_t0)
                .collect::<Vec<_>>();
            CraftStabilityRegion {
                region: selection.id,
                metric: stability_metric(&values),
                component_count_min: summaries
                    .iter()
                    .map(|summary| summary.component_count)
                    .min()
                    .unwrap_or(0),
                component_count_max: summaries
                    .iter()
                    .map(|summary| summary.component_count)
                    .max()
                    .unwrap_or(0),
                model_order_min: total_model_order,
                model_order_max: total_model_order,
            }
        })
        .collect::<Vec<_>>();
    let ratio_values = observations
        .iter()
        .filter_map(|(_, ratio)| *ratio)
        .collect::<Vec<_>>();
    let ratio = (selections.len() == 2).then(|| stability_metric(&ratio_values));
    let passed = !all_components.is_empty()
        && !observations.is_empty()
        && regions.iter().all(|region| {
            region.metric.relative_dispersion <= policy.boundary_stability_relative_tolerance
                && region.component_count_min == region.component_count_max
        })
        && ratio.as_ref().is_none_or(|metric| {
            ratio_values.len() == observations.len()
                && metric.relative_dispersion <= policy.boundary_stability_relative_tolerance
        });
    CraftStabilityDiagnostics {
        delta_ppm,
        regions,
        ratio,
        passed,
        skipped,
    }
}

pub(super) fn components_for_regions(
    components: &[CraftComponent],
    regions: &[CraftRegion],
) -> Vec<CraftComponent> {
    components
        .iter()
        .filter_map(|component| {
            regions
                .iter()
                .find(|region| {
                    let region = region.normalized();
                    component.chemical_shift_ppm >= region.start_ppm
                        && component.chemical_shift_ppm <= region.end_ppm
                })
                .map(|region| {
                    let mut selected = component.clone();
                    selected.region = region.id;
                    selected
                })
        })
        .enumerate()
        .map(|(id, mut component)| {
            component.id = CraftComponentId(id as u64);
            component
        })
        .collect()
}

fn stability_metric(values: &[f64]) -> CraftStabilityMetric {
    if values.is_empty() {
        return CraftStabilityMetric {
            median: 0.0,
            minimum: 0.0,
            maximum: 0.0,
            relative_dispersion: f64::MAX,
        };
    }
    let mut sorted = values.to_vec();
    sorted.sort_by(f64::total_cmp);
    let middle = sorted.len() / 2;
    let median = if sorted.len().is_multiple_of(2) {
        (sorted[middle - 1] + sorted[middle]) * 0.5
    } else {
        sorted[middle]
    };
    let minimum = sorted[0];
    let maximum = sorted[sorted.len() - 1];
    CraftStabilityMetric {
        median,
        minimum,
        maximum,
        relative_dispersion: (maximum - minimum) / median.abs().max(f64::MIN_POSITIVE),
    }
}
