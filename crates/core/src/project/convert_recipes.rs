//! Recipe application for loaded NMR datasets: replays stored processing
//! pipelines and analysis extensions onto freshly decoded data objects.

use super::*;
use crate::state::{PeakMark, PeakOrigin, PeakSet};

pub fn apply_1d_recipe(dataset: &mut NmrDataset, recipe: &RecipeObject) -> Result<()> {
    let p = &recipe.parameters;
    if let Some(dto) = p.pipelines.first() {
        dataset.pipeline = pipeline_from_dto(dto);
    }
    dataset.group_delay_correct = p.group_delay_correct;
    dataset.has_imaginary = true;
    if let Some(analysis) = recipe.extensions.get("plotx.analysis") {
        dataset.peaks = analysis
            .get("peaks")
            .cloned()
            .and_then(|v| serde_json::from_value(v).ok())
            .unwrap_or_else(|| legacy_peaks(analysis));
        dataset.integrals = match analysis.get("integrals") {
            Some(value) => serde_json::from_value(value.clone()).map_err(|error| {
                ProjectError::Invalid(format!("plotx.analysis.integrals is malformed: {error}"))
            })?,
            None => Vec::new(),
        };
        dataset.reseed_integral_ids();
        dataset.line_fits = analysis
            .get("line_fits")
            .cloned()
            .and_then(|v| serde_json::from_value(v).ok())
            .unwrap_or_default();
        dataset.next_line_fit_id = dataset
            .line_fits
            .iter()
            .map(|f| f.id.saturating_add(1))
            .max()
            .unwrap_or(0);
        dataset.multiplets = analysis
            .get("multiplets")
            .cloned()
            .and_then(|v| serde_json::from_value(v).ok())
            .unwrap_or_default();
        dataset.next_multiplet_id = dataset
            .multiplets
            .iter()
            .map(|m| m.id.saturating_add(1))
            .max()
            .unwrap_or(0);
    }
    Ok(())
}

fn legacy_peaks(analysis: &serde_json::Value) -> PeakSet {
    let mut peaks = PeakSet::default();
    if let Some(arr) = analysis.get("annotations").and_then(|v| v.as_array()) {
        for a in arr {
            let (Some(x), Some(y)) = (
                a.get("ppm").and_then(serde_json::Value::as_f64),
                a.get("intensity").and_then(serde_json::Value::as_f64),
            ) else {
                continue;
            };
            let id = peaks.next_id();
            peaks.marks.push(PeakMark {
                id,
                x,
                y,
                origin: PeakOrigin::Manual,
                label: a
                    .get("text")
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_owned),
            });
        }
    }
    peaks
}

pub(super) fn read_regions(dataset: &mut Nmr2DDataset, recipe: &RecipeObject) {
    let Some(ext) = recipe.extensions.get("plotx.regions") else {
        return;
    };
    if let Some(regions) = ext
        .get("regions")
        .cloned()
        .and_then(|v| serde_json::from_value::<Vec<Region>>(v).ok())
    {
        dataset.regions = regions;
    }
    if let Some(metric) = ext
        .get("metric")
        .cloned()
        .and_then(|v| serde_json::from_value::<RegionMetric>(v).ok())
    {
        dataset.region_metric = metric;
    }
    dataset.next_region_id = ext
        .get("next_id")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or_else(|| dataset.regions.iter().map(|r| r.id + 1).max().unwrap_or(0));
}

pub(super) fn nmr2d_recipe_extensions(dataset: &Nmr2DDataset) -> serde_json::Value {
    let mut extensions = serde_json::Map::new();
    if !dataset.regions.is_empty() {
        extensions.insert(
            "plotx.regions".to_owned(),
            serde_json::json!({
                "regions": &dataset.regions,
                "metric": &dataset.region_metric,
                "next_id": dataset.next_region_id,
            }),
        );
    }
    if !dataset.integrals.is_empty() {
        extensions.insert(
            "plotx.analysis".to_owned(),
            serde_json::json!({ "integrals_2d": &dataset.integrals }),
        );
    }
    if extensions.is_empty() {
        serde_json::Value::Null
    } else {
        serde_json::Value::Object(extensions)
    }
}

pub fn apply_2d_recipe(dataset: &mut Nmr2DDataset, recipe: &RecipeObject) {
    let p = &recipe.parameters;
    dataset.preset = p
        .preset
        .as_deref()
        .map(preset_from_str)
        .unwrap_or(dataset.preset);
    if let Some(f2) = p.pipelines.first() {
        dataset.params.f2 = pipeline_from_dto(f2);
    }
    if let Some(f1) = p.pipelines.get(1) {
        dataset.params.f1 = pipeline_from_dto(f1);
    }
    dataset.params.layout = p
        .layout
        .as_deref()
        .map(layout_from_str)
        .unwrap_or_else(|| dataset.preset.layout());
    dataset.group_delay_correct = p.group_delay_correct;
    dataset.has_imaginary = true;
}
