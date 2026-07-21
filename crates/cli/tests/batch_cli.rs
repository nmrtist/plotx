use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

// A per-process counter, not a clock: macOS reports `SystemTime` at microsecond
// resolution, so two tests starting in the same microsecond would otherwise
// share a root and overwrite each other's `workflow.json`.
static SEQUENCE: AtomicU64 = AtomicU64::new(0);

fn temp_dir() -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "plotx-cli-batch-{}-{}",
        std::process::id(),
        SEQUENCE.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&root).unwrap();
    root
}

fn jcamp(path: &Path) {
    std::fs::write(
        path,
        "##TITLE=fixture\n\
         ##JCAMP-DX=5.01\n\
         ##DATA TYPE=NMR SPECTRUM\n\
         ##XUNITS=PPM\n\
         ##YUNITS=ARBITRARY UNITS\n\
         ##XFACTOR=1\n\
         ##YFACTOR=1\n\
         ##FIRSTX=0\n\
         ##LASTX=3\n\
         ##NPOINTS=4\n\
         ##.OBSERVE FREQUENCY=400\n\
         ##.OBSERVE NUCLEUS=^1H\n\
         ##XYDATA=(X++(Y..Y))\n\
         0 1 2 3 4\n\
         ##END=\n",
    )
    .unwrap();
}

fn scheme(path: &Path) {
    std::fs::write(
        path,
        r#"{
  "schema_version": 1,
  "dimension_count": 1,
  "pipelines": [{"steps": [
    {"kind": "Fft", "enabled": true, "source": "User"},
    {"kind": {"Phase": {"phase0": 0.0, "phase1": 0.0, "pivot_frac": 0.5, "auto": null}}, "enabled": true, "source": "User"}
  ]}],
  "group_delay_correct": false
}"#,
    )
    .unwrap();
}

#[test]
fn batch_cli_exit_stdout_and_saved_manifest_form_one_contract() {
    let root = temp_dir();
    jcamp(&root.join("good.dx"));
    std::fs::write(root.join("broken.dx"), "invalid").unwrap();
    scheme(&root.join("routine.plotxproc"));
    let workflow = root.join("workflow.json");
    std::fs::write(
        &workflow,
        r#"{
  "schema": "plotx.workflow.v1",
  "inputs": {"files": {"kind": "external_files", "paths": ["good.dx", "broken.dx"]}},
  "nodes": [
    {
      "id": "import",
      "tool_id": "data.import",
      "parameters": {},
      "targets": {"kind": "explicit", "ids": []},
      "bindings": [{"parameter": "paths", "source": {"kind": "workflow_input", "name": "files"}}]
    },
    {
      "id": "process",
      "tool_id": "processing.apply_scheme",
      "parameters": {"path": "routine.plotxproc", "compatible_only": true},
      "targets": {"kind": "node_output", "node": "import", "port": "resources"},
      "dependencies": ["import"]
    },
    {
      "id": "export",
      "tool_id": "figure.export",
      "parameters": {"directory": "out", "format": "svg", "overwrite": false},
      "targets": {"kind": "node_output", "node": "import", "port": "resources"},
      "dependencies": ["process"]
    }
  ],
  "failure_policy": "continue_compatible"
}"#,
    )
    .unwrap();
    let manifest_path = root.join("manifest.json");

    let output = Command::new(env!("CARGO_BIN_EXE_plotx-cli"))
        .args([
            "batch",
            "--workflow",
            workflow.to_str().unwrap(),
            "--manifest",
            manifest_path.to_str().unwrap(),
        ])
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(7));
    let stdout: Value = serde_json::from_slice(&output.stdout).unwrap();
    let saved: Value = serde_json::from_slice(&std::fs::read(&manifest_path).unwrap()).unwrap();
    assert_eq!(stdout, saved);
    assert_eq!(stdout["schema"], "plotx.run-manifest.v1");
    assert_eq!(stdout["caller"], "workflow");
    assert_eq!(stdout["nodes"].as_array().unwrap().len(), 3);
    assert_eq!(stdout["errors"].as_array().unwrap().len(), 1);
    assert_eq!(
        stdout["nodes"][0]["result"]["targets"][0]["outcome"],
        "succeeded"
    );
    assert_eq!(
        stdout["nodes"][0]["result"]["targets"][1]["outcome"],
        "failed"
    );
    assert!(root.join("out/good.svg").exists());
    assert!(String::from_utf8_lossy(&output.stderr).contains("running workflow"));
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn missing_workflow_uses_input_exit_code() {
    let root = temp_dir();
    let output = Command::new(env!("CARGO_BIN_EXE_plotx-cli"))
        .args([
            "batch",
            "--workflow",
            root.join("missing.json").to_str().unwrap(),
            "--manifest",
            root.join("manifest.json").to_str().unwrap(),
        ])
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(3));
    assert!(String::from_utf8_lossy(&output.stderr).contains("missing.json"));
    assert!(!root.join("manifest.json").exists());
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn invalid_workflow_uses_usage_exit_code() {
    let root = temp_dir();
    let workflow = root.join("workflow.json");
    std::fs::write(
        &workflow,
        r#"{"schema":"plotx.workflow.v2","inputs":{},"nodes":[],"failure_policy":"strict"}"#,
    )
    .unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_plotx-cli"))
        .args([
            "batch",
            "--workflow",
            workflow.to_str().unwrap(),
            "--manifest",
            root.join("manifest.json").to_str().unwrap(),
        ])
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr).contains("expected schema"));
    assert!(!root.join("manifest.json").exists());
    std::fs::remove_dir_all(root).unwrap();
}
