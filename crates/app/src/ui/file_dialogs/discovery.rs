use std::path::{Path, PathBuf};

pub(super) fn collect_abf_files(folder: &Path, output: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(folder) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        if kind.is_dir() && !kind.is_symlink() {
            collect_abf_files(&path, output);
        } else if kind.is_file()
            && path
                .extension()
                .is_some_and(|extension| extension.eq_ignore_ascii_case("abf"))
        {
            output.push(path);
        }
    }
}
