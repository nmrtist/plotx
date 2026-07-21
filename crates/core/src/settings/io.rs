use super::{SETTINGS_SCHEMA_VERSION, Settings, migrate, paths};
use std::io;
use std::path::{Path, PathBuf};

pub fn load() -> Settings {
    let Some(path) = paths::settings_file() else {
        return Settings::default();
    };
    load_from_paths(&path, paths::legacy_preferences_file().as_deref())
}

pub fn save(settings: &Settings) -> io::Result<()> {
    let Some(path) = paths::settings_file() else {
        return Ok(());
    };
    save_to_path(&path, settings)
}

pub(crate) fn load_from_paths(path: &Path, legacy: Option<&Path>) -> Settings {
    if let Ok(data) = std::fs::read(path) {
        return load_from_bytes(&data, Some(path));
    }
    if let Some(legacy) = legacy
        && let Ok(data) = std::fs::read(legacy)
    {
        return load_from_bytes(&data, None);
    }
    Settings::default()
}

pub(crate) fn save_to_path(path: &Path, settings: &Settings) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut settings = settings.clone();
    settings.schema_version = SETTINGS_SCHEMA_VERSION;
    settings.app_version = env!("CARGO_PKG_VERSION").to_owned();
    settings.general.project_backup_generations = settings
        .general
        .project_backup_generations
        .min(super::MAX_PROJECT_BACKUP_GENERATIONS);
    let data = serde_json::to_vec_pretty(&settings).map_err(io::Error::other)?;
    let tmp = temporary_path(path);
    std::fs::write(&tmp, data)?;
    match std::fs::rename(&tmp, path) {
        Ok(()) => Ok(()),
        Err(_) if path.exists() => {
            std::fs::remove_file(path)?;
            std::fs::rename(tmp, path)
        }
        Err(err) => Err(err),
    }
}

fn load_from_bytes(data: &[u8], quarantine_path: Option<&Path>) -> Settings {
    let Ok(raw) = serde_json::from_slice::<serde_json::Value>(data) else {
        if let Some(path) = quarantine_path {
            quarantine_corrupt(path);
        }
        return Settings::default();
    };
    let from = raw
        .get("schema_version")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0)
        .min(u32::MAX as u64) as u32;
    let migrated = migrate::migrate(raw, from);
    let mut settings: Settings = serde_json::from_value(migrated).unwrap_or_default();
    settings.schema_version = SETTINGS_SCHEMA_VERSION;
    settings.general.project_backup_generations = settings
        .general
        .project_backup_generations
        .min(super::MAX_PROJECT_BACKUP_GENERATIONS);
    // `app_version` deliberately keeps the value the file was written with:
    // it is how the shell detects "first launch after an update". Saving
    // stamps the current version (see `save_to_path`).
    settings
}

fn temporary_path(path: &Path) -> PathBuf {
    let mut tmp = path.to_owned();
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .map(|name| format!("{name}.tmp"))
        .unwrap_or_else(|| "settings.json.tmp".to_owned());
    tmp.set_file_name(name);
    tmp
}

fn quarantine_corrupt(path: &Path) {
    let Some(parent) = path.parent() else {
        return;
    };
    for i in 1..1000 {
        let candidate = parent.join(format!("settings.corrupt-{i}.json"));
        if !candidate.exists() {
            let _ = std::fs::rename(path, candidate);
            return;
        }
    }
}
