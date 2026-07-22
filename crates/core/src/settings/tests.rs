use super::*;
use std::path::{Path, PathBuf};

fn temp_settings(name: &str) -> PathBuf {
    let base = std::env::var_os("CARGO_TARGET_TMPDIR")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    base.join(format!("plotx-settings-{name}-{}.json", std::process::id()))
}

#[test]
fn missing_fields_take_defaults() {
    let settings: Settings = serde_json::from_str(r#"{"general":{}}"#).unwrap();
    assert!(settings.general.snap_enabled);
    assert_eq!(settings.general.project_backup_generations, 1);
    assert_eq!(settings.export.dpi, crate::export::DEFAULT_BITMAP_DPI);
    assert!(!settings.export.trim_to_visible_content);
    assert_eq!(
        settings.appearance.graphics_power,
        GraphicsPowerPreference::LowPower
    );
}

#[test]
fn v0_preferences_load_as_settings() {
    let path = temp_settings("v0");
    let legacy = temp_settings("legacy");
    let _ = std::fs::remove_file(&path);
    let _ = std::fs::remove_file(&legacy);
    std::fs::write(
        &legacy,
        r#"{"include_view_snapshots":true,"snap_enabled":false}"#,
    )
    .unwrap();

    let settings = io::load_from_paths(&path, Some(&legacy));
    let _ = std::fs::remove_file(&legacy);

    assert!(settings.export.include_view_snapshots);
    assert!(!settings.general.snap_enabled);
    assert_eq!(settings.schema_version, SETTINGS_SCHEMA_VERSION);
}

#[test]
fn unversioned_nested_settings_keep_their_fields() {
    let path = temp_settings("unversioned-nested");
    let _ = std::fs::remove_file(&path);
    std::fs::write(
        &path,
        r#"{"general":{"snap_enabled":false},"appearance":{"theme":"dark"},"export":{"include_view_snapshots":true,"dpi":450}}"#,
    )
    .unwrap();

    let settings = io::load_from_paths(&path, None);
    let _ = std::fs::remove_file(&path);

    assert!(!settings.general.snap_enabled);
    assert_eq!(settings.appearance.theme, ThemeMode::Dark);
    assert!(settings.export.include_view_snapshots);
    assert_eq!(settings.export.dpi, 450);
    assert_eq!(settings.schema_version, SETTINGS_SCHEMA_VERSION);
}

#[test]
fn unknown_or_missing_theme_falls_back_to_system() {
    let unknown: Settings =
        serde_json::from_str(r#"{"appearance":{"theme":"solarized"}}"#).unwrap();
    assert_eq!(unknown.appearance.theme, ThemeMode::System);

    let missing: Settings = serde_json::from_str(r#"{"appearance":{}}"#).unwrap();
    assert_eq!(missing.appearance.theme, ThemeMode::System);

    let dark: Settings = serde_json::from_str(r#"{"appearance":{"theme":"dark"}}"#).unwrap();
    assert_eq!(dark.appearance.theme, ThemeMode::Dark);
}

#[test]
fn backup_generation_count_is_bounded_on_load() {
    let path = temp_settings("backup-bound");
    let _ = std::fs::remove_file(&path);
    std::fs::write(&path, r#"{"general":{"project_backup_generations":255}}"#).unwrap();

    let settings = io::load_from_paths(&path, None);
    let _ = std::fs::remove_file(&path);

    assert_eq!(
        settings.general.project_backup_generations,
        MAX_PROJECT_BACKUP_GENERATIONS
    );
}

#[test]
fn save_and_load_roundtrip() {
    let path = temp_settings("roundtrip");
    let _ = std::fs::remove_file(&path);
    let mut settings = Settings::default();
    settings.general.snap_enabled = false;
    settings.general.project_backup_generations = 3;
    settings.export.include_view_snapshots = true;
    settings.export.trim_to_visible_content = true;

    io::save_to_path(&path, &settings).unwrap();
    let loaded = io::load_from_paths(&path, None);
    let _ = std::fs::remove_file(&path);

    assert!(!loaded.general.snap_enabled);
    assert_eq!(loaded.general.project_backup_generations, 3);
    assert!(loaded.export.include_view_snapshots);
    assert!(loaded.export.trim_to_visible_content);
}

#[test]
fn corrupt_file_quarantines_and_defaults() {
    let path = temp_settings("corrupt");
    let _ = std::fs::remove_file(&path);
    std::fs::write(&path, b"{").unwrap();

    let settings = io::load_from_paths(&path, None);
    let quarantined = (1..1000)
        .map(|i| {
            path.parent()
                .unwrap()
                .join(format!("settings.corrupt-{i}.json"))
        })
        .find(|path| path.exists());
    if let Some(path) = &quarantined {
        let _ = std::fs::remove_file(path);
    }

    assert_eq!(settings, Settings::default());
    assert!(quarantined.is_some());
}

#[test]
fn recent_files_note_dedupes_fronts_and_truncates() {
    let mut recent = RecentFiles::default();
    for index in 0..(MAX_RECENT_FILES + 3) {
        recent.note(PathBuf::from(format!("C:/data/run-{index}.abf")));
    }
    assert_eq!(recent.files.len(), MAX_RECENT_FILES);
    assert_eq!(recent.files[0], PathBuf::from("C:/data/run-12.abf"));

    // Re-noting an existing entry moves it to the front without growing.
    recent.note(PathBuf::from("C:/data/run-5.abf"));
    assert_eq!(recent.files.len(), MAX_RECENT_FILES);
    assert_eq!(recent.files[0], PathBuf::from("C:/data/run-5.abf"));
    assert_eq!(
        recent
            .files
            .iter()
            .filter(|path| path.as_path() == Path::new("C:/data/run-5.abf"))
            .count(),
        1
    );
}
