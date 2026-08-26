use directories::ProjectDirs;
use std::path::PathBuf;

pub fn config_dir() -> Option<PathBuf> {
    ProjectDirs::from("", "", "plotx").map(|dirs| dirs.config_dir().to_path_buf())
}

pub fn data_local_dir() -> Option<PathBuf> {
    ProjectDirs::from("", "", "plotx").map(|dirs| dirs.data_local_dir().to_path_buf())
}

pub fn settings_file() -> Option<PathBuf> {
    config_dir().map(|dir| dir.join("settings.json"))
}
