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
