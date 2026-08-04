//! Sandboxed, read-only scientific scripts used by the Automation window.
//!
//! Scripts receive only snapshots of datasets selected in PlotX. They cannot
//! name arbitrary files or mutate the document; the host exposes neutral data
//! loading and one-dimensional trace-analysis primitives.

#[cfg(test)]
use std::path::Path;

use plotx_analysis::peaks::{DetectParams, detect_peaks, estimate_noise};
#[cfg(test)]
use plotx_io::Acquisition;
use plotx_io::{ChromatogramKind, LiquidChromatographyMethod, MassSpecRun};
use rhai::module_resolvers::DummyModuleResolver;
use rhai::{Array, Dynamic, Engine, EvalAltResult, Map, Position};

const MAX_OPERATIONS: u64 = 20_000_000;
const MAX_ARRAY_SIZE: usize = 1_000_000;
const MAX_MAP_SIZE: usize = 10_000;
const MAX_STRING_SIZE: usize = 1_000_000;
const MAX_RESULT_BYTES: usize = 10_000_000;

#[cfg(test)]
pub(crate) fn run(source: &str, input: &Path) -> Result<serde_json::Value, String> {
    let prepared = prepare_path(input)?;
    run_prepared(source, prepared)
}

pub(crate) fn run_prepared(
    source: &str,
    prepared: serde_json::Value,
) -> Result<serde_json::Value, String> {
    let mut engine = Engine::new();
    engine
        .set_module_resolver(DummyModuleResolver::new())
        .set_max_operations(MAX_OPERATIONS)
        .set_max_expr_depths(64, 32)
        .set_max_call_levels(64)
        .set_max_variables(256)
        .set_max_functions(128)
        .set_max_modules(0)
        .set_max_array_size(MAX_ARRAY_SIZE)
        .set_max_map_size(MAX_MAP_SIZE)
        .set_max_string_size(MAX_STRING_SIZE)
        .on_print(|_| {});

    let prepared = rhai::serde::to_dynamic(prepared)
        .map_err(|error| format!("Could not expose the selected data: {error}"))?;
    engine.register_fn("load_input", move || prepared.clone());
    engine.register_fn("moving_average", moving_average_dynamic);
    engine.register_fn("rolling_percentile", rolling_percentile_dynamic);
    engine.register_fn("estimate_noise", estimate_noise_dynamic);
    engine.register_fn("detect_peaks", detect_peaks_dynamic);
    engine.register_fn("format_number", format_number);

    let value = engine
        .eval::<Dynamic>(source)
        .map_err(|error| format!("Script failed: {error}"))?;
    let result: serde_json::Value = rhai::serde::from_dynamic(&value)
        .map_err(|error| format!("Invalid script result: {error}"))?;
    let result_size = serde_json::to_vec(&result)
        .map_err(|error| format!("Could not measure script result: {error}"))?
        .len();
    if result_size > MAX_RESULT_BYTES {
        return Err(format!(
            "Script result is {result_size} bytes; the limit is {MAX_RESULT_BYTES} bytes."
        ));
    }
    Ok(result)
}

fn format_number(value: f64, decimals: i64) -> String {
    let decimals = usize::try_from(decimals).unwrap_or(0).min(12);
    format!("{value:.decimals$}")
}

fn runtime_error(message: impl Into<String>) -> Box<EvalAltResult> {
    EvalAltResult::ErrorRuntime(message.into().into(), Position::NONE).into()
}

#[cfg(test)]
fn prepare_path(path: &Path) -> Result<serde_json::Value, String> {
    let loaded = plotx_io::load_path(path)
        .map_err(|error| format!("Could not load {}: {error}", path.display()))?;
    let Acquisition::MassSpec(run) = loaded.acquisition else {
        return Err("The selected input is not an LC–MS dataset.".to_owned());
    };
    let method = plotx_io::waters::load_inlet_method(path)
        .map_err(|error| format!("Could not read the LC method: {error}"))?;
    Ok(prepare_run(&run, method))
}

pub(crate) fn prepare_run(
    run: &MassSpecRun,
    method: Option<LiquidChromatographyMethod>,
) -> serde_json::Value {
    let channels = run
        .chromatograms
        .iter()
        .map(|channel| {
            serde_json::json!({
                "id": channel.id.0,
                "kind": match channel.kind {
                    ChromatogramKind::Optical => "optical",
                    ChromatogramKind::Temperature => "temperature",
                    ChromatogramKind::Pressure => "pressure",
                    ChromatogramKind::Housekeeping => "housekeeping",
                    ChromatogramKind::Unknown => "unknown",
                },
                "description": channel.description,
                "coordinate": channel.coordinate,
                "unit": channel.unit,
                "time_min": channel.time_min,
                "values": channel.values,
            })
        })
        .collect::<Vec<_>>();
    let scans = run
        .streams
        .iter()
        .flat_map(|stream| {
            stream.spectra.iter().map(move |scan| {
                serde_json::json!({
                    "stream_id": stream.id.get(),
                    "time_min": scan.retention_time_min,
                    "tic": scan.tic,
                })
            })
        })
        .collect::<Vec<_>>();
    serde_json::json!({
        "source": run.source,
        "instrument": run.instrument,
        "chromatograms": channels,
        "scans": scans,
        "lc_method": method,
    })
}

fn floats(values: Array, name: &str) -> Result<Vec<f64>, Box<EvalAltResult>> {
    values
        .into_iter()
        .map(|value| {
            value
                .as_float()
                .or_else(|_| value.as_int().map(|value| value as f64))
                .map_err(|_| runtime_error(format!("{name} must contain only numbers.")))
        })
        .collect()
}

fn dynamic_array(values: Vec<f64>) -> Array {
    values.into_iter().map(Dynamic::from_float).collect()
}

fn moving_average_dynamic(values: Array, width: i64) -> Result<Array, Box<EvalAltResult>> {
    let values = floats(values, "values")?;
    let width = usize::try_from(width)
        .ok()
        .filter(|width| *width > 0)
        .ok_or_else(|| runtime_error("Moving-average width must be positive."))?;
    Ok(dynamic_array(moving_average(&values, width)))
}

fn rolling_percentile_dynamic(
    values: Array,
    width: i64,
    quantile: f64,
) -> Result<Array, Box<EvalAltResult>> {
    let values = floats(values, "values")?;
    let width = usize::try_from(width)
        .ok()
        .filter(|width| *width > 0)
        .ok_or_else(|| runtime_error("Rolling-percentile width must be positive."))?;
    if !(0.0..=1.0).contains(&quantile) {
        return Err(runtime_error("Percentile must be between 0 and 1."));
    }
    Ok(dynamic_array(rolling_percentile(&values, width, quantile)))
}

fn estimate_noise_dynamic(values: Array) -> Result<f64, Box<EvalAltResult>> {
    Ok(estimate_noise(&floats(values, "values")?))
}

fn detect_peaks_dynamic(xs: Array, ys: Array, options: Map) -> Result<Array, Box<EvalAltResult>> {
    let xs = floats(xs, "x")?;
    let ys = floats(ys, "y")?;
    let number = |key: &str| options.get(key).and_then(|value| value.as_float().ok());
    let integer = |key: &str| options.get(key).and_then(|value| value.as_int().ok());
    let params = DetectParams {
        min_height: number("min_height"),
        min_prominence: number("min_prominence").unwrap_or(0.0),
        min_spacing: number("min_spacing"),
        max_count: integer("max_count").and_then(|value| usize::try_from(value).ok()),
    };
    detect_peaks(&xs, &ys, &params)
        .into_iter()
        .map(|peak| {
            rhai::serde::to_dynamic(serde_json::json!({
                "index": peak.index,
                "x": peak.x,
                "y": peak.y,
                "prominence": peak.prominence,
            }))
            .map_err(|error| runtime_error(error.to_string()))
        })
        .collect()
}

fn moving_average(values: &[f64], width: usize) -> Vec<f64> {
    let radius = width / 2;
    (0..values.len())
        .map(|index| {
            let start = index.saturating_sub(radius);
            let end = (index + radius + 1).min(values.len());
            values[start..end].iter().sum::<f64>() / (end - start) as f64
        })
        .collect()
}

fn rolling_percentile(values: &[f64], width: usize, quantile: f64) -> Vec<f64> {
    let radius = width / 2;
    (0..values.len())
        .map(|index| {
            let start = index.saturating_sub(radius);
            let end = (index + radius + 1).min(values.len());
            let mut window = values[start..end].to_vec();
            window.sort_by(f64::total_cmp);
            let rank = ((window.len() - 1) as f64 * quantile).round() as usize;
            window[rank]
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn script_cannot_read_an_unselected_path() {
        let error = run("load_input()", Path::new("missing.raw")).unwrap_err();
        assert!(error.contains("Could not load"));
    }

    #[test]
    fn filesystem_imports_are_rejected() {
        let error = run_prepared(
            r#"import "C:\\outside\\escape" as escape; escape::value"#,
            serde_json::json!({}),
        )
        .unwrap_err();
        let error_lower = error.to_ascii_lowercase();
        assert!(
            error_lower.contains("module") || error_lower.contains("import"),
            "{error}"
        );
    }

    #[test]
    fn collection_growth_is_bounded() {
        let error = run_prepared(
            "let values = []; values.pad(1_000_000_000, 0); values",
            serde_json::json!({}),
        )
        .unwrap_err();
        let error_lower = error.to_ascii_lowercase();
        assert!(
            error_lower.contains("size") || error_lower.contains("limit"),
            "{error}"
        );
    }

    #[test]
    fn prepared_input_is_available_to_a_script() {
        let result = run_prepared(
            r#"
                let input = load_input();
                #{
                    schema: "plotx.scientific-script-result.v1",
                    summary: #{ "Dataset": input.label },
                    point_count: input.traces[0].x.len(),
                }
            "#,
            serde_json::json!({
                "label": "example input",
                "traces": [{ "x": [0.0, 1.0], "y": [2.0, 3.0] }],
            }),
        )
        .expect("generic script runs");

        assert_eq!(result["schema"], "plotx.scientific-script-result.v1");
        assert_eq!(result["summary"]["Dataset"], "example input");
        assert_eq!(result["point_count"], 2);
    }
}
