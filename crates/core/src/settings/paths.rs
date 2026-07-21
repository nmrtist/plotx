use directories::ProjectDirs;
use std::path::PathBuf;

pub fn config_dir() -> Option<PathBuf> {
    ProjectDirs::from("", "", "plotx").map(|dirs| dirs.config_dir().to_path_buf())
}

pub fn settings_file() -> Option<PathBuf> {
    config_dir().map(|dir| dir.join("settings.json"))
}

pub fn legacy_preferences_file() -> Option<PathBuf> {
    if let Ok(appdata) = std::env::var("APPDATA") {
        return Some(
            PathBuf::from(appdata)
                .join("plotx")
                .join("preferences.json"),
        );
    }
    if let Ok(xdg) = std::env::var("XDG_CONFIG_HOME") {
        return Some(PathBuf::from(xdg).join("plotx").join("preferences.json"));
    }
    std::env::var("HOME").ok().map(|home| {
        PathBuf::from(home)
            .join(".config")
            .join("plotx")
            .join("preferences.json")
    })
}
