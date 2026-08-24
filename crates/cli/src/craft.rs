use plotx_core::workflow;
use plotx_processing::craft::{
    CRAFT_ALGORITHM, CRAFT_ALGORITHM_VERSION, CraftParamOverrides, CraftRegion,
    process_craft_cancellable, resolve_craft_invocation,
};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

pub(super) fn run(
    input: &Path,
    output: &Path,
    regions: Vec<CraftRegion>,
    expected_ratios: Vec<f64>,
) -> Result<bool, String> {
    let inputs = acquisition_inputs(input)?;
    if !expected_ratios.is_empty() && (regions.len() != 2 || expected_ratios.len() != inputs.len())
    {
        return Err(
            "--expected-ratio requires exactly two regions and one value per acquisition"
                .to_owned(),
        );
    }
    let overrides = CraftParamOverrides {
        regions: (!regions.is_empty()).then_some(regions.clone()),
        ..CraftParamOverrides::default()
    };
    let mut results = Vec::with_capacity(inputs.len());
    let mut all_succeeded = true;
    let mut all_quality_checks_passed = true;
    let mut all_reference_checks_passed = true;

    for (index, path) in inputs.into_iter().enumerate() {
        let mut item = match analyze(&path, &overrides) {
            Ok(value) => value,
            Err(error) => {
                all_succeeded = false;
                json!({
                    "input": path,
                    "status": "failed",
                    "error": error,
                })
            }
        };
        if let Some(&expected) = expected_ratios.get(index) {
            let observed = item["region_amplitude_ratio"].as_f64();
            let relative_error_percent =
                observed.map(|value| (value - expected).abs() / expected * 100.0);
            let passed = relative_error_percent.is_some_and(|error| error <= 5.0);
            item["reference_validation"] = json!({
                "expected_ratio": expected,
                "observed_ratio": observed,
                "relative_error_percent": relative_error_percent,
                "maximum_relative_error_percent": 5.0,
                "passed": passed,
            });
            all_quality_checks_passed &= passed;
            all_reference_checks_passed &= passed;
        }
        all_quality_checks_passed &= item["quality_checks"]["passed"].as_bool().unwrap_or(false);
        results.push(item);
    }

    let report = json!({
        "schema": "plotx.craft.batch.v1",
        "algorithm": CRAFT_ALGORITHM,
        "algorithm_version": CRAFT_ALGORITHM_VERSION,
        "all_succeeded": all_succeeded,
        "all_quality_checks_passed": all_quality_checks_passed,
        "all_reference_checks_passed": (!expected_ratios.is_empty()).then_some(all_reference_checks_passed),
        "datasets": results,
    });
    let encoded = serde_json::to_vec_pretty(&report)
        .map_err(|error| format!("result serialization failed: {error}"))?;
    std::fs::write(output, encoded)
        .map_err(|error| format!("could not write {}: {error}", output.display()))?;
    eprintln!("plotx-cli: wrote {}", output.display());
    Ok(all_succeeded)
}

fn analyze(path: &Path, overrides: &CraftParamOverrides) -> Result<Value, String> {
    let loaded = workflow::load_dataset(path).map_err(|error| error.to_string())?;
    let dataset = loaded
        .dataset
        .as_nmr()
        .ok_or_else(|| "CRAFT requires a one-dimensional NMR dataset".to_owned())?;
    let invocation = resolve_craft_invocation(
        &dataset.data,
        plotx_processing::craft::CraftReference::acquisition(&dataset.data),
        overrides,
        None,
    );
    if !invocation.assessment.can_run() {
        return Err(invocation
            .assessment
            .first_blocking_message()
            .unwrap_or("CRAFT input cannot be analyzed")
            .to_owned());
    }
    let result = process_craft_cancellable(&dataset.data, &invocation, &|| false)
        .map_err(|error| error.to_string())?;
    let mut quality_issues = Vec::new();
    quality_issues.extend(
        invocation
            .assessment
            .issues
            .iter()
            .map(|issue| issue.message.clone()),
    );
    quality_issues.extend(
        result
            .diagnostics
            .warnings
            .iter()
            .filter(|warning| warning.blocks_quantitation())
            .map(|warning| warning.message.clone()),
    );
    let mut fft_peaks = dataset
        .spectrum()
        .map(|spectrum| {
            spectrum
                .values
                .windows(3)
                .enumerate()
                .filter_map(|(index, values)| {
                    let magnitude = values[1].norm();
                    (magnitude > values[0].norm() && magnitude >= values[2].norm()).then(|| {
                        json!({
                            "chemical_shift_ppm": spectrum.ppm[index + 1],
                            "magnitude": magnitude,
                        })
                    })
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    fft_peaks.sort_by(|left, right| {
        right["magnitude"]
            .as_f64()
            .unwrap_or_default()
            .total_cmp(&left["magnitude"].as_f64().unwrap_or_default())
    });
    fft_peaks.truncate(20);
    let region_fft_peaks = dataset
        .spectrum()
        .map(|spectrum| {
            invocation
                .params
                .regions
                .iter()
                .map(|region| {
                    let region = region.normalized();
                    let mut peaks = spectrum
                        .ppm
                        .iter()
                        .zip(&spectrum.values)
                        .filter(|(ppm, _)| {
                            **ppm >= region.start_ppm && **ppm <= region.end_ppm
                        })
                        .map(|(&ppm, value)| {
                            json!({ "chemical_shift_ppm": ppm, "magnitude": value.norm() })
                        })
                        .collect::<Vec<_>>();
                    peaks.sort_by(|left, right| {
                        right["magnitude"]
                            .as_f64()
                            .unwrap_or_default()
                            .total_cmp(&left["magnitude"].as_f64().unwrap_or_default())
                    });
                    peaks.truncate(5);
                    json!({ "region": region, "strongest_bins": peaks })
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let region_amplitude_ratio = result.region_ratio.map(|ratio| ratio.value);
    Ok(json!({
        "input": path,
        "status": "complete",
        "quality_checks": {
            "passed": quality_issues.is_empty(),
            "issues": quality_issues,
        },
        "inspection": loaded.inspection,
        "acquisition": {
            "spectral_width_hz": dataset.data.spectral_width_hz,
            "observe_frequency_mhz": dataset.data.observe_freq_mhz,
            "carrier_ppm": dataset.data.carrier_ppm,
            "group_delay_points": dataset.data.group_delay,
            "point_count": dataset.data.points.len(),
        },
        "chemical_shift_reference": {
            "offset_ppm": invocation.reference.offset_ppm,
            "effective_carrier_ppm": invocation.reference.effective_carrier_ppm(),
        },
        "invocation": invocation,
        "fft_cross_check_peaks": fft_peaks,
        "region_fft_cross_check": region_fft_peaks,
        "region_summaries": result.region_summaries,
        "region_amplitude_ratio": region_amplitude_ratio,
        "component_count": result.components.len(),
        "components": result.components,
        "diagnostics": result.diagnostics,
    }))
}

fn acquisition_inputs(input: &Path) -> Result<Vec<PathBuf>, String> {
    if !input.is_dir() || input.extension().is_some() || is_raw_acquisition(input) {
        return Ok(vec![input.to_owned()]);
    }
    let mut children = std::fs::read_dir(input)
        .map_err(|error| format!("could not read {}: {error}", input.display()))?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.is_dir() && is_raw_acquisition(path))
        .collect::<Vec<_>>();
    children.sort();
    if children.is_empty() {
        Err(format!(
            "{} is not a raw acquisition and contains no raw acquisition directories",
            input.display()
        ))
    } else {
        Ok(children)
    }
}

fn is_raw_acquisition(path: &Path) -> bool {
    matches!(
        plotx_io::detect_format(path),
        Ok(plotx_io::DataFormat::Nmr(
            plotx_io::NmrFormat::BrukerRaw | plotx_io::NmrFormat::VarianAgilentRaw,
        ))
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bruker_root_with_pdata_is_one_raw_acquisition() {
        let root =
            std::env::temp_dir().join(format!("plotx-craft-bruker-root-{}", std::process::id()));
        std::fs::create_dir_all(root.join("pdata/1")).unwrap();
        std::fs::write(root.join("acqus"), "##TITLE=fixture\n").unwrap();
        std::fs::write(root.join("fid"), []).unwrap();

        assert_eq!(acquisition_inputs(&root).unwrap(), vec![root.clone()]);

        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn batch_root_only_expands_raw_acquisition_children() {
        let root =
            std::env::temp_dir().join(format!("plotx-craft-bruker-batch-{}", std::process::id()));
        let acquisition = root.join("sample");
        std::fs::create_dir_all(&acquisition).unwrap();
        std::fs::create_dir_all(root.join("notes")).unwrap();
        std::fs::write(acquisition.join("acqus"), "##TITLE=fixture\n").unwrap();
        std::fs::write(acquisition.join("fid"), []).unwrap();

        assert_eq!(acquisition_inputs(&root).unwrap(), vec![acquisition]);

        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn directory_without_raw_acquisitions_is_rejected() {
        let root =
            std::env::temp_dir().join(format!("plotx-craft-empty-batch-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();

        let error = acquisition_inputs(&root).unwrap_err();
        assert!(error.contains("contains no raw acquisition directories"));

        std::fs::remove_dir_all(root).unwrap();
    }
}
