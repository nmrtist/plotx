use std::path::{Path, PathBuf};

pub(super) fn collect_data_files(folder: &Path, output: &mut Vec<PathBuf>) {
    // A MassLynx `.raw` directory is one atomic acquisition. Its numbered
    // payload files must never be rediscovered as independent datasets.
    if plotx_io::waters::is_masslynx_raw(folder) {
        output.push(folder.to_owned());
        return;
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognized_raw_directory_is_atomic() {
        let root = std::env::temp_dir().join(format!("plotx-discovery-{}.raw", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir(&root).unwrap();
        std::fs::write(root.join("_HEADER.TXT"), b"$$ Instrument: test\n").unwrap();
        std::fs::write(root.join("_FUNCTNS.INF"), vec![0; 416]).unwrap();
        std::fs::write(root.join("_FUNC001.IDX"), vec![0; 22]).unwrap();
        std::fs::write(root.join("_FUNC001.DAT"), []).unwrap();
        let mut found = Vec::new();
        collect_data_files(&root, &mut found);
        assert_eq!(found.as_slice(), std::slice::from_ref(&root));
        std::fs::remove_dir_all(root).unwrap();
    }
}
