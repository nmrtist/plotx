use super::synthetic_1d;
use crate::actions::DatasetProcessingState;
use crate::project::{SchemeApplicationPolicy, load_scheme, plan_scheme_application, save_scheme};
use crate::state::{Dataset, NmrDataset, PlotxApp};
use std::path::PathBuf;

fn temp_scheme(name: &str) -> PathBuf {
    let base = std::env::var_os("CARGO_TARGET_TMPDIR")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    base.join(format!(
        "plotx-scheme-apply-{name}-{}.plotxproc",
        std::process::id()
    ))
}

fn group_delay(app: &PlotxApp, di: usize) -> bool {
    match &app.doc.datasets[di] {
        Dataset::Nmr(n) => n.group_delay_correct,
        _ => panic!("expected 1D dataset"),
    }
}

#[test]
fn batch_template_apply_filters_incompatible_targets_and_undoes_as_one_step() {
    let mut app = PlotxApp::new();
    for _ in 0..2 {
        app.doc
            .datasets
            .push(Dataset::Nmr(Box::new(NmrDataset::load(synthetic_1d()))));
    }
    if let Dataset::Nmr(n) = &mut app.doc.datasets[0] {
        n.group_delay_correct = false;
    }

    let path = temp_scheme("batch");
    save_scheme(&path, &app.doc.datasets[0]).unwrap();
    let scheme = load_scheme(&path).unwrap();
    let _ = std::fs::remove_file(&path);

    let plan = plan_scheme_application(&scheme, &app.doc.datasets, &[0, 1, 7]);
    assert_eq!(plan.compatible_count(), 2);
    assert_eq!(plan.incompatible_count(), 1);
    assert!(plan.prepare(SchemeApplicationPolicy::StrictAll).is_none());

    let prepared = plan
        .prepare(SchemeApplicationPolicy::CompatibleOnly)
        .unwrap();
    assert_eq!(prepared.applied_targets, vec![0, 1]);
    assert_eq!(prepared.skipped_targets, vec![7]);

    let before: Vec<DatasetProcessingState> = app
        .doc
        .datasets
        .iter()
        .map(DatasetProcessingState::from_dataset)
        .collect();
    assert!(group_delay(&app, 1));
    app.execute_action(prepared.action);
    assert!(!group_delay(&app, 1));
    assert_eq!(app.session.undo_stack.len(), 1);

    app.undo();
    let restored: Vec<DatasetProcessingState> = app
        .doc
        .datasets
        .iter()
        .map(DatasetProcessingState::from_dataset)
        .collect();
    assert_eq!(restored, before);
    assert!(group_delay(&app, 1));
    assert!(!group_delay(&app, 0));
}

/// P1-2 regression. A `.plotxproc` is a documented, hand-writable recipe and
/// carries no identities: `apply_scheme` remints every step from the target
/// dataset's allocator, so a required `id` field would make authors spell out a
/// value that is thrown away. Dropping `#[serde(default)]` from
/// `ProcessingStepDto::id` makes the parse below fail with `missing field id`.
#[test]
fn a_hand_written_scheme_without_step_ids_loads_and_applies() {
    let json = r#"{
        "schema_version": 1,
        "dimension_count": 1,
        "pipelines": [{"steps": [
            {"kind": "Fft", "enabled": true, "source": "User"},
            {"kind": {"Phase": {"phase0": 0.25, "phase1": 0.0, "pivot_frac": 0.5, "auto": null}},
             "enabled": true, "source": "User"}
        ]}],
        "group_delay_correct": false
    }"#;
    let scheme: crate::project::ProcessingScheme =
        serde_json::from_str(json).expect("a recipe may omit step identities");

    let mut app = PlotxApp::new();
    app.doc
        .datasets
        .push(Dataset::Nmr(Box::new(NmrDataset::load(synthetic_1d()))));

    let plan = plan_scheme_application(&scheme, &app.doc.datasets, &[0]);
    assert_eq!(plan.compatible_count(), 1);
    let prepared = plan.prepare(SchemeApplicationPolicy::StrictAll).unwrap();
    app.execute_action(prepared.action);

    // Reminting is what makes the missing ids harmless: the adopted steps must
    // be unique and sit below the owner's allocator.
    let n = app.doc.datasets[0].as_nmr().unwrap();
    let mut ids: Vec<_> = n.pipeline.steps.iter().map(|step| step.id).collect();
    assert_eq!(ids.len(), 2);
    assert!(ids.iter().all(|id| id.get() < n.next_step_id));
    ids.sort();
    ids.dedup();
    assert_eq!(ids.len(), 2, "adopted steps must not share an identity");
    assert!(!group_delay(&app, 0));
}

/// The other half of P1-2: a saved recipe must not contain the discarded field
/// at all, so what the app writes matches what the docs ask users to write.
#[test]
fn a_saved_scheme_omits_step_identities() {
    let mut app = PlotxApp::new();
    app.doc
        .datasets
        .push(Dataset::Nmr(Box::new(NmrDataset::load(synthetic_1d()))));
    let path = temp_scheme("no-step-ids");
    save_scheme(&path, &app.doc.datasets[0]).unwrap();
    let written = std::fs::read_to_string(&path).unwrap();
    let _ = std::fs::remove_file(&path);

    assert!(
        !written.contains("\"id\""),
        "a detached recipe carries no identities:\n{written}"
    );
}
