use super::*;
use crate::export::ExportFormat;
use crate::settings::{MAX_EXPORT_DPI, MIN_EXPORT_DPI, Settings};
use crate::state::{CanvasDocument, PlotxApp, SettingsDialog};
use std::path::PathBuf;

fn app_with_dpi(dpi: u16) -> PlotxApp {
    let mut settings = Settings::default();
    settings.export.dpi = dpi;
    PlotxApp::new_with_settings(settings)
}

fn temp_settings(name: &str) -> PathBuf {
    let base = std::env::var_os("CARGO_TARGET_TMPDIR")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    base.join(format!("plotx-property-dpi-{name}-{}", std::process::id()))
}

fn plan_dpi(app: &PlotxApp, value: i64) -> PropertyCommit {
    app.plan_property_write(
        export_dpi::DPI,
        std::slice::from_ref(&app.app_target()),
        &PropertyValue::Int(value),
    )
    .expect("DPI write plans")
}

#[test]
fn dpi_edit_is_settings_only_and_not_undoable() {
    let mut app = app_with_dpi(crate::export::DEFAULT_BITMAP_DPI);
    let undo_len = app.session.undo_stack.len();
    let dirty = app.doc.dirty;
    let revision = app.doc.automation_revision;

    let commit = plan_dpi(&app, 450);
    assert_eq!(commit.applied.len(), 1);
    assert!(commit.document_action.is_none());
    assert_eq!(
        commit
            .app_preferences
            .as_ref()
            .map(|settings| settings.export.dpi),
        Some(450)
    );
    assert_eq!(
        app.commit_property_with_settings_writer(commit, |_| Ok(())),
        1
    );

    assert_eq!(app.settings.export.dpi, 450);
    assert_eq!(app.session.undo_stack.len(), undo_len);
    assert_eq!(app.doc.dirty, dirty);
    assert_eq!(app.doc.automation_revision, revision);
}

#[test]
fn dpi_same_value_write_is_an_explicit_skip() {
    let app = app_with_dpi(450);
    let commit = plan_dpi(&app, 450);

    assert!(commit.applied.is_empty());
    assert_eq!(commit.skipped.len(), 1);
    assert_eq!(commit.skipped[0].reason, SkipReason::AlreadyAtValue);
    assert!(commit.document_action.is_none());
    assert!(commit.app_preferences.is_none());
}

#[test]
fn dpi_out_of_range_error_names_the_value_and_actual_bounds() {
    let app = app_with_dpi(crate::export::DEFAULT_BITMAP_DPI);
    let value = i64::from(MAX_EXPORT_DPI) + 1;
    let error = app
        .plan_property_write(
            export_dpi::DPI,
            std::slice::from_ref(&app.app_target()),
            &PropertyValue::Int(value),
        )
        .expect_err("an out-of-range DPI must be refused");
    let message = error.to_string();

    assert!(message.contains(&value.to_string()), "{message}");
    assert!(
        message.contains(&format!("{MIN_EXPORT_DPI}–{MAX_EXPORT_DPI} dpi")),
        "{message}"
    );
    assert_eq!(app.settings.export.dpi, crate::export::DEFAULT_BITMAP_DPI);
}

#[test]
fn dpi_edit_roundtrips_and_supplies_the_next_export_default() {
    let path = temp_settings("roundtrip").with_extension("json");
    if path.exists() {
        std::fs::remove_file(&path).expect("remove stale test settings");
    }
    let mut app = app_with_dpi(crate::export::DEFAULT_BITMAP_DPI);
    let commit = plan_dpi(&app, 600);
    app.commit_property_with_settings_writer(commit, |settings| {
        crate::settings::save_to_path(&path, settings)
    });

    let loaded = crate::settings::load_from_paths(&path, None);
    if path.exists() {
        std::fs::remove_file(&path).expect("remove test settings");
    }
    assert_eq!(loaded.export.dpi, 600);

    let mut restarted = PlotxApp::new_with_settings(loaded);
    // Avoid a publication preset: selecting one deliberately replaces the
    // invocation DPI and would test preset precedence rather than the app
    // default this slice owns.
    restarted
        .doc
        .canvases
        .push(CanvasDocument::new("page".to_owned(), [123.0, 77.0]));
    restarted.session.active_canvas = Some(0);
    restarted.request_export(ExportFormat::Png);
    assert_eq!(
        restarted
            .session
            .ui
            .export_options
            .as_ref()
            .map(|dialog| dialog.dpi),
        Some(600)
    );
}

#[test]
fn failed_dpi_flush_keeps_the_live_value_and_reports_the_failure() {
    let blocking_parent = temp_settings("blocked-parent");
    if blocking_parent.exists() {
        std::fs::remove_file(&blocking_parent).expect("remove stale blocker");
    }
    std::fs::write(&blocking_parent, b"this is a file").expect("create blocker");
    let path = blocking_parent.join("settings.json");
    let mut app = app_with_dpi(crate::export::DEFAULT_BITMAP_DPI);
    let undo_len = app.session.undo_stack.len();
    let dirty = app.doc.dirty;

    let commit = plan_dpi(&app, 720);
    app.commit_property_with_settings_writer(commit, |settings| {
        crate::settings::save_to_path(&path, settings)
    });
    std::fs::remove_file(&blocking_parent).expect("remove blocker");

    assert_eq!(app.settings.export.dpi, 720);
    assert_eq!(app.session.undo_stack.len(), undo_len);
    assert_eq!(app.doc.dirty, dirty);
    assert!(
        app.session.status.contains("Couldn't save preferences"),
        "{}",
        app.session.status
    );
    assert!(
        app.session
            .status
            .contains("changes apply this session only"),
        "{}",
        app.session.status
    );
}

#[test]
fn open_preferences_draft_tracks_a_catalog_dpi_edit() {
    let mut app = app_with_dpi(crate::export::DEFAULT_BITMAP_DPI);
    app.session.ui.settings_dialog = Some(SettingsDialog::new(app.settings.clone()));
    let mut draft = app
        .session
        .ui
        .settings_dialog
        .as_ref()
        .expect("dialog open")
        .draft
        .clone();
    draft.export.trim_to_visible_content = true;
    app.apply_settings(draft);

    let commit = plan_dpi(&app, 450);
    app.commit_property_with_settings_writer(commit, |_| Ok(()));
    let next_flush = app
        .session
        .ui
        .settings_dialog
        .as_ref()
        .expect("dialog remains open")
        .draft
        .clone();
    assert_eq!(next_flush.export.dpi, 450);
    assert!(next_flush.export.trim_to_visible_content);

    app.apply_settings(next_flush);
    assert_eq!(app.settings.export.dpi, 450);
    assert!(app.settings.export.trim_to_visible_content);
}
