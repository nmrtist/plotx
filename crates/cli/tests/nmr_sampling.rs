use serde_json::{Value, json};
use std::{
    path::{Path, PathBuf},
    process::Command,
};

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../io/tests/fixtures/nmr")
        .join(name)
}
fn declaration() -> Value {
    json!({"assertion_id": "cli-table", "source": "synthetic CLI input", "grid_shape": [4], "coordinates": [[4], [2]], "index_base": "one", "component_counts": [2]})
}
fn raw(dir: &Path) {
    for name in ["ser", "acqus", "acqu2s"] {
        std::fs::copy(fixture(&format!("bruker-nus/{name}")), dir.join(name)).unwrap();
    }
}

#[test]
fn inspect_and_process_use_explicit_declaration_and_reject_conflicts() {
    let dir = tempfile::tempdir().unwrap();
    raw(dir.path());
    let table = dir.path().join("sampling.json");
    std::fs::write(&table, serde_json::to_vec(&declaration()).unwrap()).unwrap();
    let inspect = Command::new(env!("CARGO_BIN_EXE_plotx-cli"))
        .arg("inspect")
        .arg(dir.path())
        .arg("--sampling-declaration")
        .arg(&table)
        .arg("--json")
        .output()
        .unwrap();
    assert!(
        inspect.status.success(),
        "{}",
        String::from_utf8_lossy(&inspect.stderr)
    );
    let value: Value = serde_json::from_slice(&inspect.stdout).unwrap();
    assert_eq!(value["dimension"]["shape"], json!([4, 2]));
    let scheme = dir.path().join("empty.plotxproc");
    std::fs::write(&scheme, r#"{"schema_version":1,"dimension_count":2,"pipelines":[{"steps":[]},{"steps":[]}],"group_delay_correct":false}"#).unwrap();
    let svg = dir.path().join("spectrum.svg");
    let process = Command::new(env!("CARGO_BIN_EXE_plotx-cli"))
        .arg("process")
        .arg(dir.path())
        .arg("--sampling-declaration")
        .arg(&table)
        .arg("--scheme")
        .arg(&scheme)
        .arg("--output")
        .arg(&svg)
        .output()
        .unwrap();
    assert!(
        process.status.success(),
        "{}",
        String::from_utf8_lossy(&process.stderr)
    );
    assert!(svg.exists());
    let mut bad = declaration();
    bad["component_counts"] = json!([1]);
    std::fs::write(&table, serde_json::to_vec(&bad).unwrap()).unwrap();
    let rejected = Command::new(env!("CARGO_BIN_EXE_plotx-cli"))
        .arg("inspect")
        .arg(dir.path())
        .arg("--sampling-declaration")
        .arg(&table)
        .arg("--json")
        .output()
        .unwrap();
    assert_eq!(rejected.status.code(), Some(3));
    assert!(rejected.stdout.is_empty());
    assert!(!dir.path().join("nuslist").exists());
}

#[test]
fn batch_import_checks_the_same_declaration() {
    let dir = tempfile::tempdir().unwrap();
    raw(dir.path());
    let workflow = dir.path().join("workflow.json");
    let manifest = dir.path().join("manifest.json");
    let definition = json!({
        "schema": "plotx.workflow.v1", "inputs": {},
        "nodes": [{"id":"import", "tool_id":"data.import",
            "parameters":{"paths":[dir.path()], "sampling_declaration":declaration()},
            "targets":{"kind":"explicit", "ids":[]}}],
        "failure_policy":"continue_compatible"
    });
    std::fs::write(&workflow, serde_json::to_vec(&definition).unwrap()).unwrap();
    let result = Command::new(env!("CARGO_BIN_EXE_plotx-cli"))
        .arg("batch")
        .arg("--workflow")
        .arg(&workflow)
        .arg("--manifest")
        .arg(&manifest)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&result.stderr),
        String::from_utf8_lossy(&result.stdout)
    );
    let value: Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(
        value["nodes"][0]["result"]["targets"][0]["outcome"],
        "succeeded"
    );
    assert_eq!(
        value,
        serde_json::from_slice::<Value>(&std::fs::read(manifest).unwrap()).unwrap()
    );
}
