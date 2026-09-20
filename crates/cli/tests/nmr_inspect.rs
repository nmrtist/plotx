use std::{path::PathBuf, process::Command};

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../io/tests/fixtures/nmr")
        .join(name)
}

fn inspect(name: &str) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_plotx-cli"))
        .arg("inspect")
        .arg(fixture(name))
        .arg("--json")
        .output()
        .unwrap()
}

#[test]
fn inspection_uses_raw_preference_and_does_not_run_the_default_recipe() {
    let output = inspect("bruker-1d");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["domain"], "time");
    assert_eq!(report["format"], "bruker-raw");
    assert_eq!(report["dimension"]["shape"], serde_json::json!([2]));
}

#[test]
fn selected_processed_input_and_sparse_logical_shape_are_preserved() {
    for (name, format, shape) in [
        ("bruker-1d/pdata/1/1r", "bruker-processed-1d", vec![4]),
        ("bruker-nus", "bruker-raw", vec![4, 2]),
    ] {
        let output = inspect(name);
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(report["format"], format);
        assert_eq!(report["dimension"]["shape"], serde_json::json!(shape));
    }
}

#[test]
fn jeol_without_delay_evidence_is_inspectable_without_blanket_alerts() {
    let output = inspect("jeol-complex.jdf");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["domain"], "time");
    assert!(
        !report["warnings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|warning| warning["code"] == "experimental-nmr-semantics")
    );
}

#[test]
fn ppm_spectrum_is_inspectable_with_its_declared_shape() {
    let output = inspect("jcamp-ppm.dx");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["domain"], "frequency");
    assert_eq!(report["dimension"]["shape"], serde_json::json!([4]));
}

#[test]
fn invalid_vendor_input_does_not_fall_back_to_the_previous_reader() {
    let output = inspect("varian-short-header.fid");
    assert_eq!(output.status.code(), Some(3));
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("header must contain 11 fields"));
}
