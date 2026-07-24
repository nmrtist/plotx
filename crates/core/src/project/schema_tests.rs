use super::tests::{sample_app, temp_project};
use super::*;

#[test]
fn pre_release_projects_are_written_with_v1_schema() {
    let path = temp_project("schema_v1");
    let _ = std::fs::remove_file(&path);
    let outcome = save_project(&PlotxApp::new(), &path, false).unwrap();

    let file = File::open(&path).unwrap();
    let mut zip = zip::ZipArchive::new(file).unwrap();
    let manifest: Manifest = read_json(&mut zip, "manifest.json").unwrap();

    assert_eq!(manifest.schema_version, 1);
    assert_eq!(
        manifest.revision.as_deref(),
        Some(outcome.revision.as_str())
    );
    assert!(manifest.recovery.is_none());
    std::fs::remove_file(path).unwrap();
}

#[test]
fn identity_schema_snapshot_uses_typed_text_and_persists_object_allocator() {
    let mut app = sample_app();
    let dataset = app.doc.datasets[0].resource_id();
    let canvas = app.doc.canvases[0].resource_id;
    let object = app.doc.canvases[0].objects[0].id;
    app.session.ui.analysis_selection = Some(AnalysisSelection {
        dataset,
        canvas,
        object,
        x_range: AxisRange::new(1.0, 2.0),
        y_range: None,
    });

    let path = temp_project("identity_schema_snapshot");
    let _ = std::fs::remove_file(&path);
    save_project(&app, &path, false).unwrap();
    let file = File::open(&path).unwrap();
    let mut zip = zip::ZipArchive::new(file).unwrap();
    let manifest: Manifest = read_json(&mut zip, "manifest.json").unwrap();
    let view: serde_json::Value = read_json(&mut zip, &manifest.views[0].path).unwrap();
    let workspace: serde_json::Value = read_json(&mut zip, &manifest.workspace).unwrap();

    assert_eq!(
        view["next_object_id"],
        app.doc.canvases[0].next_object_id.get()
    );
    assert_eq!(view["objects"][0]["id"], object.to_string());
    assert_eq!(
        workspace["analysis_selection"]["dataset"],
        dataset.to_string()
    );
    assert_eq!(
        workspace["analysis_selection"]["canvas"],
        canvas.to_string()
    );
    assert_eq!(
        workspace["analysis_selection"]["object"],
        object.to_string()
    );
    std::fs::remove_file(path).unwrap();
}

#[test]
fn pre_release_loader_rejects_non_v1_schema_without_a_migration_chain() {
    let manifest = Manifest {
        format: FORMAT.to_owned(),
        schema_version: 2,
        app_version: "pre-release".to_owned(),
        revision: None,
        recovery: None,
        save_profile: SaveProfile {
            include_view_snapshots: false,
            snapshot_kind: None,
        },
        objects: Vec::new(),
        views: Vec::new(),
        runs: Vec::new(),
        workspace: "workspace.json".to_owned(),
    };

    assert!(matches!(
        validate_manifest(&manifest),
        Err(ProjectError::Unsupported(_))
    ));
}

#[test]
fn pre_release_scheme_loader_rejects_non_v1_schema_without_a_migration_chain() {
    let path = super::tests::temp_scheme("schema_v2");
    let _ = std::fs::remove_file(&path);
    std::fs::write(
        &path,
        r#"{
            "schema_version": 2,
            "dimension_count": 1,
            "pipelines": [],
            "group_delay_correct": true
        }"#,
    )
    .unwrap();

    assert!(matches!(
        load_scheme(&path),
        Err(ProjectError::Unsupported(_))
    ));
    std::fs::remove_file(path).unwrap();
}

#[test]
fn recovery_restore_keeps_original_save_target_and_marks_document_dirty() {
    let snapshot_path = temp_project("recovery_snapshot");
    let original_path = snapshot_path.with_file_name("original.plotx");
    let _ = std::fs::remove_file(&snapshot_path);
    save_project(&PlotxApp::new(), &snapshot_path, false).unwrap();
    let snapshot = RecoverySnapshot {
        path: snapshot_path.clone(),
        original_path: Some(original_path.clone()),
        base_revision: Some("original-revision".to_owned()),
        modified: std::time::SystemTime::now(),
    };

    let restored = restore_recovery(&snapshot).unwrap();

    assert_eq!(restored.doc.project_path.as_ref(), Some(&original_path));
    assert_eq!(
        restored.doc.project_revision.as_deref(),
        Some("original-revision")
    );
    assert!(restored.doc.dirty);
    std::fs::remove_file(snapshot_path).unwrap();
}

#[test]
fn recovery_capture_shares_document_until_editing_resumes() {
    let mut app = PlotxApp::new();
    let request = prepare_recovery_snapshot(&app).unwrap();

    assert!(app.doc.shares_storage_with(&request.doc));
    app.doc.dirty = true;

    assert!(!app.doc.shares_storage_with(&request.doc));
    assert!(!request.doc.dirty);
}
