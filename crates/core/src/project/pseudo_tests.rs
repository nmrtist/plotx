use super::*;
use crate::state::{Dataset, Nmr2DDataset, PlotxApp};
use crate::{IltParams, PseudoDisplay};
use num_complex::Complex64;
use plotx_io::{
    AxisSource, DiffusionMeta, Dim, Domain, NmrData2D, PseudoAxis, PseudoKind, QuadMode,
};
use std::path::{Path, PathBuf};

fn temp_project(name: &str) -> PathBuf {
    let base = std::env::var_os("CARGO_TARGET_TMPDIR")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    base.join(format!("plotx-{name}-{}.plotx", std::process::id()))
}

fn synthetic_dosy_2d() -> NmrData2D {
    let (cols, rows) = (64usize, 8usize);
    let meta = DiffusionMeta {
        gamma: 2.675_222e8,
        delta: 2e-3,
        big_delta: 0.1,
        tau: 0.0,
        shape_factor: 1.0 / 3.0,
    };
    let direct = Dim {
        spectral_width_hz: 5000.0,
        observe_freq_mhz: 400.0,
        carrier_ppm: 5.0,
        nucleus: "1H".to_owned(),
        group_delay: 0.0,
    };
    let g: Vec<f64> = (0..rows)
        .map(|i| 0.02 + i as f64 * (0.28 - 0.02) / (rows as f64 - 1.0))
        .collect();
    let dt = 1.0 / direct.spectral_width_hz;
    let f_hz = direct.observe_freq_mhz;
    let mut data = Vec::with_capacity(rows * cols);
    for &gr in &g {
        let att = (-1.2e-9 * meta.b_factor(gr)).exp();
        for j in 0..cols {
            let t = j as f64 * dt;
            let decay = (-t / 0.2).exp();
            data.push(Complex64::from_polar(
                att * decay,
                std::f64::consts::TAU * f_hz * t,
            ));
        }
    }
    NmrData2D {
        data,
        rows,
        cols,
        domain: Domain::Time,
        direct: direct.clone(),
        indirect: direct,
        quad: QuadMode::Complex,
        indirect_conjugate: false,
        experiment: Some("bpp_ste_diffusion".to_owned()),
        pseudo_axis: Some(PseudoAxis {
            name: "g".to_owned(),
            kind: PseudoKind::Gradient,
            values: g,
            unit: "mT/m".to_owned(),
            source: AxisSource::EmbeddedRamp,
        }),
        diffusion: Some(meta),
        nus: None,
        source: "synthetic DOSY".to_owned(),
    }
}

fn inject_fit_curve_pseudo_extension(path: &Path) {
    let file = std::fs::File::open(path).unwrap();
    let mut zip = zip::ZipArchive::new(file).unwrap();
    let mut entries = Vec::new();
    for i in 0..zip.len() {
        let mut file = zip.by_index(i).unwrap();
        if file.is_dir() {
            continue;
        }
        let name = file.name().to_owned();
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes).unwrap();
        if name == "objects/recipe_000000/object.json" {
            let mut value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
            value["extensions"]["plotx.pseudo"] = serde_json::json!({
                "display": "FitCurve",
                "fit": {
                    "region_ppm": [0.8, 1.2],
                    "kind": "Diffusion",
                    "value": 1.2e-9,
                    "sigma": 1.0e-11,
                    "r2": 0.999,
                    "points": [[20.0, 1.0], [280.0, 0.4]],
                    "ruler_unit": "mT/m"
                }
            });
            bytes = serde_json::to_vec_pretty(&value).unwrap();
        }
        entries.push((name, bytes));
    }
    drop(zip);

    let tmp = temporary_path(path);
    let file = std::fs::File::create(&tmp).unwrap();
    let mut out = zip::ZipWriter::new(file);
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    for (name, bytes) in entries {
        out.start_file(name, options).unwrap();
        out.write_all(&bytes).unwrap();
    }
    out.finish().unwrap();
    std::fs::remove_file(path).unwrap();
    std::fs::rename(tmp, path).unwrap();
}

fn rewrite_project(path: &Path, mut edit: impl FnMut(&str, &mut Vec<u8>) -> bool) {
    let file = std::fs::File::open(path).unwrap();
    let mut zip = zip::ZipArchive::new(file).unwrap();
    let mut entries = Vec::new();
    for i in 0..zip.len() {
        let mut file = zip.by_index(i).unwrap();
        if file.is_dir() {
            continue;
        }
        let name = file.name().to_owned();
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes).unwrap();
        if edit(&name, &mut bytes) {
            entries.push((name, bytes));
        }
    }
    drop(zip);

    let tmp = temporary_path(path);
    let file = std::fs::File::create(&tmp).unwrap();
    let mut out = zip::ZipWriter::new(file);
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    for (name, bytes) in entries {
        out.start_file(name, options).unwrap();
        out.write_all(&bytes).unwrap();
    }
    out.finish().unwrap();
    std::fs::remove_file(path).unwrap();
    std::fs::rename(tmp, path).unwrap();
}

fn pseudo_project_with_view(name: &str) -> PathBuf {
    let mut app = PlotxApp::new();
    let dataset = Dataset::Nmr2D(Box::new(Nmr2DDataset::load(synthetic_dosy_2d())));
    let canvas = crate::workflow::build_default_canvas(&dataset, "strict-pseudo");
    app.doc.datasets.push(dataset);
    app.doc.canvases.push(canvas);
    let path = temp_project(name);
    let _ = std::fs::remove_file(&path);
    save_project(&app, &path, false).unwrap();
    path
}

#[test]
fn load_rejects_duplicate_series_ids_scalar_collections_and_unknown_source_fields() {
    let cases = ["duplicate", "scalar", "unknown-field"];
    for case in cases {
        let path = pseudo_project_with_view(case);
        rewrite_project(&path, |name, bytes| {
            if !name.starts_with("views/") || !name.ends_with(".json") {
                return true;
            }
            let mut view: serde_json::Value = serde_json::from_slice(bytes).unwrap();
            let series = view["objects"][0]["series"].as_array_mut().unwrap();
            match case {
                "duplicate" => series.push(series[0].clone()),
                "scalar" => {
                    let source = series[0]["source"].as_object_mut().unwrap();
                    source.insert("kind".into(), "field".into());
                    source.remove("item");
                }
                "unknown-field" => {
                    series[0]["source"]["unexpected"] = serde_json::json!(true);
                }
                _ => unreachable!(),
            }
            *bytes = serde_json::to_vec_pretty(&view).unwrap();
            true
        });
        let error = match load_project(&path) {
            Ok(_) => panic!("{case} project unexpectedly loaded"),
            Err(error) => error.to_string(),
        };
        let _ = std::fs::remove_file(path);
        let expected = match case {
            "duplicate" => "duplicate series id",
            "scalar" => "trace collection as a scalar field",
            "unknown-field" => "unknown field",
            _ => unreachable!(),
        };
        assert!(error.contains(expected), "{case}: {error}");
    }
}

fn assert_f64_bits_equal(actual: &[f64], expected: &[f64]) {
    assert_eq!(
        actual
            .iter()
            .map(|value| value.to_bits())
            .collect::<Vec<_>>(),
        expected
            .iter()
            .map(|value| value.to_bits())
            .collect::<Vec<_>>()
    );
}

#[test]
fn project_load_ignores_stored_pseudo_fit_curve() {
    let mut app = PlotxApp::new();
    let ds = Nmr2DDataset::load(synthetic_dosy_2d());
    app.doc.datasets.push(Dataset::Nmr2D(Box::new(ds)));

    let path = temp_project("pseudo-fit-curve");
    let _ = std::fs::remove_file(&path);
    save_project(&app, &path, false).unwrap();
    inject_fit_curve_pseudo_extension(&path);
    let loaded = load_project(&path).unwrap();
    let _ = std::fs::remove_file(&path);

    let Dataset::Nmr2D(n) = &loaded.doc.datasets[0] else {
        panic!("expected a 2D NMR dataset");
    };
    assert_eq!(n.display, PseudoDisplay::Stack);
    assert!(n.is_pseudo());
    assert_eq!(n.data.rows, 8);
    assert_eq!(n.data.cols, 64);
    assert!(n.data.diffusion.is_some());
}

#[test]
fn project_round_trip_restores_both_real_dosy_maps_after_retransform() {
    let mut app = PlotxApp::new();
    let mut ds = Nmr2DDataset::load(synthetic_dosy_2d());
    assert!(ds.build_dosy_map(), "the real per-column fit must populate");
    let original_dosy = ds.dosy_map.clone().unwrap();
    let params = crate::IltParams {
        lambda: 0.02,
        d_min: 1e-11,
        d_max: 1e-8,
        n_grid: 32,
    };
    assert!(ds.build_ilt_map(params), "the real ILT fit must populate");
    let original_ilt = ds.ilt_map.clone().unwrap();
    let plotx_processing::Processed2D::Stack(original_stack) = &ds.processed else {
        panic!("synthetic DOSY must process as a stack");
    };
    let original_stack = original_stack.clone();
    app.doc.datasets.push(Dataset::Nmr2D(Box::new(ds)));

    let path = temp_project("dosy-results");
    let _ = std::fs::remove_file(&path);
    save_project(&app, &path, false).unwrap();
    let mut loaded = load_project(&path).unwrap();
    let _ = std::fs::remove_file(&path);

    let Dataset::Nmr2D(restored) = &loaded.doc.datasets[0] else {
        panic!("expected a 2D NMR dataset");
    };
    assert_eq!(restored.display, PseudoDisplay::DosyMap);
    assert_eq!(restored.dosy_method, crate::DosyMethod::Ilt(params));
    let plotx_processing::Processed2D::Stack(restored_stack) = &restored.processed else {
        panic!("restored DOSY must process as a stack");
    };
    assert_eq!(restored_stack.traces.len(), original_stack.traces.len());
    assert_f64_bits_equal(&restored_stack.ppm, &original_stack.ppm);
    assert_f64_bits_equal(
        &restored.data.pseudo_axis.as_ref().unwrap().values,
        &app.doc.datasets[0]
            .as_nmr2d()
            .unwrap()
            .data
            .pseudo_axis
            .as_ref()
            .unwrap()
            .values,
    );
    for (actual, expected) in restored_stack.traces.iter().zip(&original_stack.traces) {
        assert_eq!(
            actual
                .iter()
                .map(|value| (value.re.to_bits(), value.im.to_bits()))
                .collect::<Vec<_>>(),
            expected
                .iter()
                .map(|value| (value.re.to_bits(), value.im.to_bits()))
                .collect::<Vec<_>>()
        );
    }
    let dosy = restored.dosy_map.as_ref().expect("per-column map restored");
    assert_f64_bits_equal(&dosy.ppm, &original_dosy.ppm);
    assert_f64_bits_equal(&dosy.d, &original_dosy.d);
    assert_f64_bits_equal(&dosy.amp, &original_dosy.amp);
    let ilt = restored.ilt_map.as_ref().expect("ILT map restored");
    assert_f64_bits_equal(&ilt.ppm, &original_ilt.ppm);
    assert_f64_bits_equal(&ilt.d_grid, &original_ilt.d_grid);
    assert_eq!(ilt.amp.len(), original_ilt.amp.len());
    for (actual, expected) in ilt.amp.iter().zip(&original_ilt.amp) {
        assert_f64_bits_equal(actual, expected);
    }
    assert!(restored.dosy_provenance.is_some());
    assert!(restored.ilt_provenance.is_some());
    assert!(
        restored.dosy_provenance_warning.is_none(),
        "{:?}",
        restored.dosy_provenance_warning
    );
    let figure = restored.figure();
    assert!(figure.title.starts_with("DOSY (ILT)"), "{}", figure.title);
    assert!(
        !figure.contours.is_empty(),
        "the restored ILT map, not the stack fallback, must be rendered"
    );
    // The middle lifecycle stage, resolved exactly the way a real build resolves
    // it: no explicit input has been entered for this dataset, so the reopened
    // result's provenance must outrank the application default.
    loaded.settings.processing.ilt_lambda = 0.9;
    assert_eq!(loaded.explicit_ilt_input_for(0), None);
    let resolved = loaded.resolve_ilt_params_for(0, loaded.explicit_ilt_input_for(0));
    assert_eq!(
        resolved.lambda, params.lambda,
        "reopening a result must offer its provenance before the app default"
    );

    // Third stage: with the provenance gone, the same call falls through to the
    // application default rather than to whatever the panel last held.
    loaded.doc.datasets[0]
        .as_nmr2d_mut()
        .unwrap()
        .ilt_provenance = None;
    assert_eq!(
        loaded
            .resolve_ilt_params_for(0, loaded.explicit_ilt_input_for(0))
            .lambda,
        0.9,
        "with no provenance the application default must be used"
    );

    // First stage: an explicit input for this dataset outranks both.
    loaded.set_explicit_ilt_input(
        0,
        IltParams {
            lambda: 0.37,
            ..params
        },
    );
    assert_eq!(
        loaded
            .resolve_ilt_params_for(0, loaded.explicit_ilt_input_for(0))
            .lambda,
        0.37,
        "an explicit input must outrank provenance and the default"
    );
}

#[test]
fn mismatched_fingerprint_keeps_the_stored_map_and_reports_both_fingerprints() {
    let mut app = PlotxApp::new();
    let mut ds = Nmr2DDataset::load(synthetic_dosy_2d());
    assert!(ds.build_dosy_map());
    let original = ds.dosy_map.clone().unwrap();
    app.doc.datasets.push(Dataset::Nmr2D(Box::new(ds)));

    let path = temp_project("dosy-fingerprint");
    let _ = std::fs::remove_file(&path);
    save_project(&app, &path, false).unwrap();
    rewrite_project(&path, |name, bytes| {
        if name.ends_with("/data.bin") {
            bytes[0] ^= 1;
        }
        true
    });
    let loaded = load_project(&path).unwrap();
    let _ = std::fs::remove_file(&path);

    let Dataset::Nmr2D(restored) = &loaded.doc.datasets[0] else {
        panic!("expected a 2D NMR dataset");
    };
    let map = restored
        .dosy_map
        .as_ref()
        .expect("stored map remains present");
    assert_f64_bits_equal(&map.ppm, &original.ppm);
    assert_f64_bits_equal(&map.d, &original.d);
    assert_f64_bits_equal(&map.amp, &original.amp);
    assert!(restored.figure().title.starts_with("DOSY —"));
    let warning = restored
        .dosy_provenance_warning
        .as_deref()
        .expect("mismatch is user-visible");
    // Assert on the evidence the message must carry, not on its phrasing: both
    // the saved and the rebuilt identifier, and they must actually differ — a
    // message naming the same value twice would read like a diagnosis while
    // proving nothing.
    let stored = &restored
        .dosy_provenance
        .as_ref()
        .expect("stored provenance remains present")
        .data_fingerprint;
    let plotx_processing::Processed2D::Stack(stack) = &restored.processed else {
        panic!("the reopened dataset must still process as a stack");
    };
    let reconstructed = crate::state::dosy_data_fingerprint(
        stack,
        &restored.data.pseudo_axis.as_ref().unwrap().values,
        restored.data.diffusion.as_ref().unwrap(),
    );
    assert_ne!(stored, &reconstructed);
    assert!(warning.contains(&stored[..12]), "{warning}");
    assert!(warning.contains(&reconstructed[..12]), "{warning}");
    assert!(warning.contains("stored map is being shown"), "{warning}");
    assert!(warning.contains("Rebuild"), "{warning}");
}

#[test]
fn missing_selected_blob_explains_the_stack_fallback() {
    let mut app = PlotxApp::new();
    let mut ds = Nmr2DDataset::load(synthetic_dosy_2d());
    assert!(ds.build_dosy_map());
    app.doc.datasets.push(Dataset::Nmr2D(Box::new(ds)));

    let path = temp_project("dosy-missing-blob");
    let _ = std::fs::remove_file(&path);
    save_project(&app, &path, false).unwrap();
    rewrite_project(&path, |name, _| !name.ends_with("/dosy.bin"));
    let loaded = load_project(&path).unwrap();
    let _ = std::fs::remove_file(&path);

    let Dataset::Nmr2D(restored) = &loaded.doc.datasets[0] else {
        panic!("expected a 2D NMR dataset");
    };
    assert_eq!(restored.display, PseudoDisplay::DosyMap);
    assert!(restored.dosy_map.is_none());
    assert!(restored.figure().title.starts_with("Pseudo-2D stack —"));
    let warning = restored
        .dosy_provenance_warning
        .as_deref()
        .expect("the fallback is explained");
    assert!(warning.contains("could not be loaded"), "{warning}");
    assert!(warning.contains("showing the stack"), "{warning}");
    assert!(warning.contains("Build"), "{warning}");
}

/// Guards the workspace's `serde_json/float_roundtrip` feature.
///
/// Without it, serde_json's fast float parser returns a value one ULP away from
/// the one that was written for roughly a tenth of all `f64`s. Every number in a
/// project — gradient rulers, spectral widths, phases, region bounds, fit results
/// — travels through this path, and a drifted input silently invalidates the
/// analysis fingerprints derived from it. The failure is invisible without a bit
/// comparison, so it is pinned here rather than left to be rediscovered.
#[test]
fn project_json_numbers_survive_a_round_trip_bit_for_bit() {
    let mut state: u64 = 0x243F_6A88_85A3_08D3;
    let mut checked = 0usize;
    for _ in 0..50_000 {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        // Spread across the magnitudes PlotX actually persists: gradient
        // amplitudes, diffusion coefficients, ppm values, Hz frequencies.
        let mantissa = (state >> 11) as f64 / (1u64 << 53) as f64;
        for scale in [1e-11, 1e-3, 1.0, 1e3, 1e8] {
            let value = mantissa * scale;
            let text = serde_json::to_string(&value).expect("serialize");
            let back: f64 = serde_json::from_str(&text).expect("deserialize");
            assert_eq!(
                back.to_bits(),
                value.to_bits(),
                "{value} round-tripped through JSON as {back}"
            );
            checked += 1;
        }
    }
    assert_eq!(checked, 250_000);
}

/// A view snapshot is a picture of a figure the document can still produce. If
/// the stored map did not survive the load it cannot, so replaying the snapshot
/// would leave the saved DOSY contours on the canvas while the load report says
/// the stack is shown. The canvas and the report have to agree.
#[test]
fn a_snapshot_is_not_replayed_when_the_stored_map_could_not_be_restored() {
    let mut app = PlotxApp::new();
    let mut ds = Nmr2DDataset::load(synthetic_dosy_2d());
    assert!(ds.build_dosy_map());
    let action = crate::actions::Action::insert_dataset_with_default_canvas(
        &app,
        Dataset::Nmr2D(Box::new(ds)),
        "DOSY".to_owned(),
        crate::state::DEFAULT_CANVAS_SIZE_MM,
    );
    app.execute_action(action);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    while app.compute_busy() && std::time::Instant::now() < deadline {
        app.poll_compute();
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    app.poll_compute();
    assert!(!app.compute_busy(), "DOSY contour build did not settle");

    let path = temp_project("dosy-snapshot-bypass");
    let _ = std::fs::remove_file(&path);
    // `true` = write view snapshots, which is the configuration this guards.
    save_project(&app, &path, true).unwrap();

    let saved = load_project(&path).unwrap();
    let saved_contours = saved.doc.canvases[0]
        .objects
        .iter()
        .find_map(|object| object.plot())
        .expect("the saved canvas has a plot")
        .figure()
        .contours
        .len();
    assert!(saved_contours > 0, "the snapshot must hold DOSY contours");

    rewrite_project(&path, |name, _| !name.ends_with("dosy.bin"));
    let loaded = load_project(&path).unwrap();
    let _ = std::fs::remove_file(&path);

    let restored = loaded.doc.datasets[0].as_nmr2d().unwrap();
    assert!(restored.dosy_map.is_none(), "the map must be gone");
    assert!(restored.missing_selected_map_note().is_some());
    let contours = loaded.doc.canvases[0]
        .objects
        .iter()
        .find_map(|object| object.plot())
        .expect("the canvas still has a plot")
        .figure()
        .contours
        .len();
    assert_eq!(
        contours, 0,
        "the canvas must not keep drawing contours the document can no longer produce"
    );
}

/// Reopening a project whose selected method has no map must not bake that fact
/// into stored state: switching to the method that *does* have a map has to stop
/// the complaint, or the panel keeps claiming the stack is shown while the map is
/// on screen.
#[test]
fn the_missing_map_complaint_does_not_survive_selecting_a_method_that_has_one() {
    let mut app = PlotxApp::new();
    let mut ds = Nmr2DDataset::load(synthetic_dosy_2d());
    let params = IltParams {
        lambda: 0.02,
        d_min: 1e-11,
        d_max: 1e-8,
        n_grid: 32,
    };
    assert!(ds.build_ilt_map(params));
    // Only the ILT map exists, but the per-column method is what gets saved.
    ds.dosy_method = crate::DosyMethod::MonoExp;
    ds.display = PseudoDisplay::DosyMap;
    app.doc.datasets.push(Dataset::Nmr2D(Box::new(ds)));

    let path = temp_project("dosy-stale-complaint");
    let _ = std::fs::remove_file(&path);
    save_project(&app, &path, false).unwrap();
    let mut loaded = load_project(&path).unwrap();
    let _ = std::fs::remove_file(&path);

    let restored = loaded.doc.datasets[0].as_nmr2d().unwrap();
    assert!(restored.ilt_map.is_some(), "the ILT map round-trips");
    assert!(restored.dosy_map.is_none());
    let note = restored
        .missing_selected_map_note()
        .expect("the selected per-column map is genuinely absent");
    assert!(note.contains("per-column"), "{note}");
    assert!(
        restored.dosy_provenance_warning.is_none(),
        "a derived condition must not be stored: {:?}",
        restored.dosy_provenance_warning
    );

    loaded.set_pseudo_dosy_method(0, crate::DosyMethod::Ilt(params));
    let restored = loaded.doc.datasets[0].as_nmr2d().unwrap();
    assert_eq!(
        restored.missing_selected_map_note(),
        None,
        "the ILT map is present, so nothing may still claim a fallback"
    );
    assert!(restored.dosy_provenance_warning.is_none());
    assert!(restored.figure().title.starts_with("DOSY (ILT)"));
}
