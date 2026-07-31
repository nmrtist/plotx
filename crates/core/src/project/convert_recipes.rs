//! Recipe application for loaded NMR datasets: replays stored processing
//! pipelines and analysis extensions onto freshly decoded data objects.

use super::*;
use crate::state::{PeakMark, PeakOrigin, PeakSet};

pub fn apply_1d_recipe(dataset: &mut NmrDataset, recipe: &RecipeObject) -> Result<()> {
    let p = &recipe.parameters;
    if let Some(dto) = p.pipelines.first() {
        dataset.pipeline = pipeline_from_dto(dto);
    }
    validate_1d_pipeline(&dataset.data, &dataset.pipeline, p.group_delay_correct)
        .map_err(ProjectError::Invalid)?;
    dataset.next_step_id = recipe
        .extensions
        .get("plotx.step_allocator")
        .and_then(|value| value.get("next_id"))
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0);
    dataset.repair_step_allocator();
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

pub(super) fn read_region_analysis(
    dataset: &mut Nmr2DDataset,
    recipe: &RecipeObject,
) -> Result<()> {
    let extension = recipe
        .extensions
        .get("plotx.region_analysis")
        .ok_or_else(|| {
            ProjectError::Invalid("2D NMR recipe is missing region analysis state".to_owned())
        })?;
    dataset.region_analysis = serde_json::from_value(extension.clone()).map_err(|error| {
        ProjectError::Invalid(format!("invalid region analysis state: {error}"))
    })?;
    dataset.region_analysis.validate().map_err(|error| {
        ProjectError::Invalid(format!("invalid region analysis state: {error}"))
    })?;
    Ok(())
}

pub(super) fn nmr2d_recipe_extensions(
    dataset: &Nmr2DDataset,
    dosy: Option<serde_json::Value>,
) -> serde_json::Value {
    let mut extensions = serde_json::Map::new();
    extensions.insert(
        "plotx.step_allocator".to_owned(),
        serde_json::json!({ "next_id": dataset.next_step_id }),
    );
    extensions.insert(
        "plotx.region_analysis".to_owned(),
        serde_json::json!(&dataset.region_analysis),
    );
    let mut analysis = serde_json::Map::new();
    if !dataset.integrals.is_empty() {
        analysis.insert(
            "integrals_2d".to_owned(),
            serde_json::json!(&dataset.integrals),
        );
    }
    if !dataset.peaks.marks.is_empty() {
        analysis.insert("peaks_2d".to_owned(), serde_json::json!(&dataset.peaks));
    }
    if !analysis.is_empty() {
        extensions.insert(
            "plotx.analysis".to_owned(),
            serde_json::Value::Object(analysis),
        );
    }
    if let Some(dosy) = dosy {
        extensions.insert("plotx.dosy".to_owned(), dosy);
    }
    if extensions.is_empty() {
        serde_json::Value::Null
    } else {
        serde_json::Value::Object(extensions)
    }
}

pub fn apply_2d_recipe(dataset: &mut Nmr2DDataset, recipe: &RecipeObject) -> Result<()> {
    let p = &recipe.parameters;
    let preset = p
        .preset
        .as_deref()
        .map(preset_from_str)
        .unwrap_or(dataset.preset);
    let mut params = dataset.params.clone();
    if let Some(f2) = p.pipelines.first() {
        params.f2 = pipeline_from_dto(f2);
    }
    if let Some(f1) = p.pipelines.get(1) {
        params.f1 = pipeline_from_dto(f1);
    }
    params.layout = p
        .layout
        .as_deref()
        .map(layout_from_str)
        .unwrap_or_else(|| preset.layout());
    params
        .f2
        .output_domain(dataset.data.domain)
        .map_err(|error| ProjectError::Invalid(format!("invalid F2 pipeline: {error}")))?;
    params
        .f1
        .output_domain(dataset.data.domain)
        .map_err(|error| ProjectError::Invalid(format!("invalid F1 pipeline: {error}")))?;

    dataset.preset = preset;
    dataset.params = params;
    dataset.next_step_id = recipe
        .extensions
        .get("plotx.step_allocator")
        .and_then(|value| value.get("next_id"))
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0);
    dataset.repair_step_allocator();
    dataset.group_delay_correct = p.group_delay_correct;
    dataset.has_imaginary = true;
    Ok(())
}
