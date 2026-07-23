use std::path::{Path, PathBuf};

pub(super) fn collect_data_files(folder: &Path, output: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(folder) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        if kind.is_dir() && !kind.is_symlink() {
            collect_data_files(&path, output);
        } else if kind.is_file() {
            let extension = path
                .extension()
                .and_then(|value| value.to_str())
                .unwrap_or("");
            if ["abf", "spm", "pfc"]
                .iter()
                .any(|supported| extension.eq_ignore_ascii_case(supported))
            {
                output.push(path);
            }
        }
    }
}
