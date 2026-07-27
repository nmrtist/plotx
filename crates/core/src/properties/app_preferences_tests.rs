use super::*;
use crate::properties::{AggregateValue, PropertyCommit};
use crate::settings::Settings;
use std::path::PathBuf;

fn plan(app: &PlotxApp, property: PropertyId, value: PropertyValue) -> PropertyCommit {
    app.plan_property_write(property, std::slice::from_ref(&app.app_target()), &value)
        .unwrap_or_else(|error| panic!("{property}: {error}"))
}

fn commit(app: &mut PlotxApp, property: PropertyId, value: PropertyValue) {
    let planned = plan(app, property, value);
    assert_eq!(
        app.commit_property_with_settings_writer(planned, |_| Ok(())),
        1
    );
}

fn temp_project(name: &str) -> PathBuf {
    let base = std::env::var_os("CARGO_TARGET_TMPDIR")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    base.join(format!(
        "plotx-app-preference-{name}-{}.plotx",
        std::process::id()
    ))
}

#[test]
fn catalog_snap_edit_survives_project_save() {
    let path = temp_project("snap-save");
    if path.exists() {
        std::fs::remove_file(&path).expect("remove stale project");
    }
    let mut app = PlotxApp::new_with_settings(Settings::default());
    commit(&mut app, SNAP_ENABLED, PropertyValue::Bool(false));
    assert!(!app.settings.general.snap_enabled);

    assert!(app.save_project_to(&path, false), "project save succeeds");
    if path.exists() {
        std::fs::remove_file(&path).expect("remove project");
    }
    assert!(
        !app.settings.general.snap_enabled,
        "saving a project must not restore a stale session mirror"
    );
}

#[test]
fn app_preference_write_is_not_document_undo() {
    let mut app = PlotxApp::new_with_settings(Settings::default());
    let undo = app.session.undo_stack.len();
    let dirty = app.doc.dirty;
    commit(
        &mut app,
        KEEP_EMPTY_SOURCE_CANVAS,
        PropertyValue::Bool(true),
    );
    assert!(app.settings.general.keep_empty_source_canvas);
    assert_eq!(app.session.undo_stack.len(), undo);
    assert_eq!(app.doc.dirty, dirty);
}

#[test]
fn accent_color_reports_the_headless_derived_default() {
    let app = PlotxApp::new_with_settings(Settings::default());
    let address = PropertyAddress::new(app.app_target(), ACCENT_COLOR);
    let resolved = app.resolve_property(&address).expect("accent resolves");
    assert_eq!(
        resolved.value,
        AggregateValue::Uniform(PropertyValue::Color(ACCENT_PLACEHOLDER))
    );
    assert_eq!(
        resolved.default_value,
        Some(PropertyValue::Color(ACCENT_PLACEHOLDER))
    );
    assert!(!resolved.is_modified());
    assert_eq!(resolved.availability, Availability::Editable);
}

#[test]
fn accent_color_set_and_reset_use_one_catalog_operation_each() {
    let mut app = PlotxApp::new_with_settings(Settings::default());
    let write = app
        .plan_property_write(
            ACCENT_COLOR,
            std::slice::from_ref(&app.app_target()),
            &PropertyValue::Color(Color::rgb(12, 34, 56)),
        )
        .expect("accent write plans");
    assert_eq!(write.applied.len(), 1);
    assert_eq!(
        app.commit_property_with_settings_writer(write, |_| Ok(())),
        1
    );
    assert_eq!(app.settings.appearance.canvas_accent, Some([12, 34, 56]));

    let reset = app
        .plan_property_reset(ACCENT_COLOR, std::slice::from_ref(&app.app_target()))
        .expect("accent reset plans");
    assert_eq!(reset.applied.len(), 1);
    assert_eq!(
        app.commit_property_with_settings_writer(reset, |_| Ok(())),
        1
    );
    assert_eq!(app.settings.appearance.canvas_accent, None);
}

#[test]
fn backup_bound_rejects_the_value_and_names_the_actual_limit() {
    let app = PlotxApp::new_with_settings(Settings::default());
    let rejected = i64::from(MAX_PROJECT_BACKUP_GENERATIONS) + 1;
    let error = app
        .plan_property_write(
            PROJECT_BACKUP_GENERATIONS,
            std::slice::from_ref(&app.app_target()),
            &PropertyValue::Int(rejected),
        )
        .expect_err("the declared bound is enforced by the provider");
    let message = error.to_string();
    assert!(message.contains(&rejected.to_string()), "{message}");
    assert!(
        message.contains(&MAX_PROJECT_BACKUP_GENERATIONS.to_string()),
        "{message}"
    );
}

#[test]
fn all_eleven_app_preferences_reset_through_their_catalog_definitions() {
    let mut settings = Settings::default();
    settings.general.snap_enabled = false;
    settings.general.keep_empty_source_canvas = true;
    settings.general.project_backup_generations = MAX_PROJECT_BACKUP_GENERATIONS;
    settings.appearance.theme = ThemeMode::Dark;
    settings.appearance.graphics_power = GraphicsPowerPreference::HighPerformance;
    settings.appearance.canvas_accent = Some([12, 34, 56]);
    settings.export.include_view_snapshots = true;
    settings.export.trim_to_visible_content = true;
    settings.canvas_size.scale_content = true;
    settings.updates.auto_check = false;
    settings.updates.channel = UpdateChannelSetting::Beta;
    let mut app = PlotxApp::new_with_settings(settings);

    for property in [
        SNAP_ENABLED,
        KEEP_EMPTY_SOURCE_CANVAS,
        PROJECT_BACKUP_GENERATIONS,
        THEME,
        GRAPHICS_POWER,
        ACCENT_COLOR,
        INCLUDE_VIEW_SNAPSHOTS,
        TRIM_TO_VISIBLE_CONTENT,
        SCALE_CONTENT,
        AUTO_CHECK_UPDATES,
        UPDATE_CHANNEL,
    ] {
        let planned = app
            .plan_property_reset(property, std::slice::from_ref(&app.app_target()))
            .unwrap_or_else(|error| panic!("{property}: {error}"));
        assert_eq!(planned.applied.len(), 1, "{property}");
        assert_eq!(
            app.commit_property_with_settings_writer(planned, |_| Ok(())),
            1,
            "{property}"
        );
    }

    let defaults = Settings::default();
    assert_eq!(app.settings.general, defaults.general);
    assert_eq!(app.settings.appearance.theme, defaults.appearance.theme);
    assert_eq!(
        app.settings.appearance.graphics_power,
        defaults.appearance.graphics_power
    );
    assert_eq!(app.settings.appearance.canvas_accent, None);
    assert_eq!(app.settings.export, defaults.export);
    assert_eq!(
        app.settings.canvas_size.scale_content,
        defaults.canvas_size.scale_content
    );
    assert_eq!(app.settings.updates, defaults.updates);
}

#[test]
fn snap_catalog_and_legacy_setter_share_one_authoritative_value_and_clear_guides() {
    let mut app = PlotxApp::new_with_settings(Settings::default());
    app.session.ui.snap_guides.push(crate::layout::SnapGuide {
        vertical: true,
        pos: 12.0,
    });

    commit(&mut app, SNAP_ENABLED, PropertyValue::Bool(false));
    assert!(!app.settings.general.snap_enabled);
    assert!(app.session.ui.snap_guides.is_empty());
    assert_eq!(
        app.resolve_property(&PropertyAddress::new(app.app_target(), SNAP_ENABLED))
            .expect("snap resolves")
            .value,
        AggregateValue::Uniform(PropertyValue::Bool(false))
    );

    app.set_snap_enabled(true);
    assert!(app.settings.general.snap_enabled);
    assert_eq!(
        app.resolve_property(&PropertyAddress::new(app.app_target(), SNAP_ENABLED))
            .expect("snap resolves after the toolbar setter")
            .value,
        AggregateValue::Uniform(PropertyValue::Bool(true))
    );
}
