use nmr::ExecutionContext;
use plotx_io::{
    nmr_bridge,
    nmr_sampling::{self, IndexBase, SamplingDeclaration},
};
use std::path::{Path, PathBuf};

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/nmr")
        .join(name)
}

fn declaration(grid: usize, rows: &[usize]) -> SamplingDeclaration {
    SamplingDeclaration {
        assertion_id: "plotx-test-schedule".into(),
        source: "synthetic user table".into(),
        grid_shape: vec![grid],
        coordinates: rows.iter().map(|&row| vec![row]).collect(),
        index_base: IndexBase::One,
        component_counts: vec![2],
    }
}

fn read(path: &Path, declaration: SamplingDeclaration) -> std::sync::Arc<nmr::Dataset> {
    nmr_sampling::read(path, declaration, &mut ExecutionContext::default()).unwrap()
}

#[test]
fn bruker_declaration_checks_vendor_evidence_and_preserves_duplicate_observations_offline() {
    let dir = tempfile::tempdir().unwrap();
    for name in ["ser", "acqus", "acqu2s"] {
        std::fs::copy(
            fixture(&format!("bruker-nus/{name}")),
            dir.path().join(name),
        )
        .unwrap();
    }
    assert!(nmr_bridge::load(dir.path()).is_err());
    let declared = declaration(4, &[4, 2]);
    let input = read(dir.path(), declared.clone());
    let raw = input.as_raw().unwrap();
    let schedule = raw.sampling_schedule().unwrap();
    assert_eq!(
        schedule.declaration(),
        Some(&declared.clone().into_native().unwrap())
    );
    assert_eq!(
        schedule
            .coordinates()
            .iter()
            .map(|c| c.as_slice())
            .collect::<Vec<_>>(),
        [&[3], &[1]]
    );
    assert!(!dir.path().join("nuslist").exists());
    for name in ["ser", "acqus", "acqu2s"] {
        assert_eq!(
            std::fs::read(dir.path().join(name)).unwrap(),
            std::fs::read(fixture(&format!("bruker-nus/{name}"))).unwrap()
        );
    }
    let mut bad_lanes = declared.clone();
    bad_lanes.component_counts = vec![1];
    for invalid in [
        declaration(5, &[4, 2]),
        declaration(4, &[5, 2]),
        declaration(4, &[0, 2]),
        declaration(4, &[4]),
        bad_lanes,
    ] {
        assert!(nmr_sampling::load(dir.path(), invalid).is_err());
    }
    let repeated = read(dir.path(), declaration(4, &[2, 2]));
    let traces = repeated.as_raw().unwrap().data().sparse_traces().unwrap();
    assert_eq!(traces[0].coordinate(), traces[1].coordinate());
    assert_ne!(traces[0].samples(), traces[1].samples());
    let mut bytes = Vec::new();
    nmr_bridge::snapshot::write(
        &repeated,
        &mut bytes,
        Default::default(),
        &mut ExecutionContext::default(),
    )
    .unwrap();
    dir.close().unwrap();
    let restored = nmr_bridge::snapshot::read(
        &mut bytes.as_slice(),
        Default::default(),
        &mut ExecutionContext::default(),
    )
    .unwrap();
    assert_eq!(restored.canonical_digests(), repeated.canonical_digests());
    assert_eq!(
        restored.as_raw().unwrap().sampling_schedule(),
        repeated.as_raw().unwrap().sampling_schedule()
    );
    assert!(nmr_sampling::load(&fixture("bruker-nus"), declared).is_ok());
    assert!(nmr_sampling::load(&fixture("bruker-nus"), declaration(4, &[2, 4])).is_err());
    assert!(nmr_sampling::load(&fixture("bruker-1d/pdata/1/1r"), declaration(4, &[4, 2])).is_err());
}

#[test]
fn jeol_declaration_reaches_checked_reader_with_four_components_and_no_source_edits() {
    let path = fixture("jeol-nus-missing.jdf");
    let before = std::fs::read(&path).unwrap();
    assert!(nmr_bridge::load(&path).is_err());
    let input = read(&path, declaration(8, &[1, 2, 2, 8]));
    assert!(
        !nmr_bridge::warnings(&input)
            .iter()
            .any(|warning| warning.message.contains("ExperimentalVendorSemantics"))
    );
    let raw = input.as_raw().unwrap();
    assert_eq!(raw.descriptor().logical_shape(), [8, 3]);
    let traces = raw.data().sparse_traces().unwrap();
    assert_eq!(traces[1].coordinate(), traces[2].coordinate());
    assert_ne!(traces[1].samples(), traces[2].samples());
    // The F1 imaginary lane uses the opposite quadrature orientation.
    assert_eq!(
        traces[0].samples(),
        &[
            nmr::Complex64::new(0.0, -100.0),
            nmr::Complex64::new(1.0, -101.0),
            nmr::Complex64::new(2.0, -102.0),
            nmr::Complex64::new(-200.0, 300.0),
            nmr::Complex64::new(-201.0, 301.0),
            nmr::Complex64::new(-202.0, 302.0),
        ]
    );
    assert_eq!(std::fs::read(path).unwrap(), before);
}

#[test]
fn declaration_json_rejects_unknown_fields_and_cancelled_read() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("schedule.json");
    let declared = declaration(4, &[4, 2]);
    std::fs::write(&path, serde_json::to_vec(&declared).unwrap()).unwrap();
    assert!(nmr_sampling::load_with_declaration_file(&fixture("bruker-nus"), &path).is_ok());
    let mut json = serde_json::to_value(&declared).unwrap();
    json["index_bsae"] = "one".into();
    std::fs::write(&path, serde_json::to_vec(&json).unwrap()).unwrap();
    assert!(nmr_sampling::read_declaration(&path).is_err());
    let token = nmr::CancellationToken::new();
    token.cancel();
    let mut context = ExecutionContext::default().with_cancellation(token);
    assert!(nmr_sampling::read(&fixture("bruker-nus"), declared, &mut context).is_err());
}
