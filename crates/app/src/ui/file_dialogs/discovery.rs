use std::path::{Path, PathBuf};

pub(super) fn collect_data_files(folder: &Path, output: &mut Vec<PathBuf>) {
    // Vendor acquisition directories are atomic. Their payload files must
    // never be rediscovered as independent datasets.
    if plotx_io::waters::is_masslynx_raw(folder)
        || plotx_io::bruker::detect_processed(folder).is_some()
        || plotx_io::bruker::is_bruker_dir(folder)
        || plotx_io::varian::is_varian(folder)
    {
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
            let supported_extension = ["abf", "spm", "pfc", "rasx", "vms"]
                .iter()
                .any(|supported| extension.eq_ignore_ascii_case(supported));
            let recognized_raw =
                extension.eq_ignore_ascii_case("raw") && plotx_io::xrd::is_rigaku_raw(&path);
            let recognized_casaxps =
                extension.eq_ignore_ascii_case("txt") && plotx_io::xps::is_casaxps_text(&path);
            if supported_extension || recognized_raw || recognized_casaxps {
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

    #[test]
    fn folder_scan_keeps_only_recognized_raw_files() {
        let root =
            std::env::temp_dir().join(format!("plotx-xrd-discovery-{}", uuid::Uuid::new_v4()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir(&root).unwrap();
        let xrd = root.join("pattern.raw");
        let unrelated = root.join("unrelated.raw");
        std::fs::write(&xrd, b"FI\0\0").unwrap();
        std::fs::write(&unrelated, b"not an XRD file").unwrap();

        let mut found = Vec::new();
        collect_data_files(&root, &mut found);

        assert_eq!(found, vec![xrd]);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn varian_directory_is_atomic() {
        let root =
            std::env::temp_dir().join(format!("plotx-varian-discovery-{}", uuid::Uuid::new_v4()));
        let dataset = root.join("sample.fid");
        std::fs::create_dir_all(&dataset).unwrap();
        std::fs::write(dataset.join("procpar"), b"sw 1 1\n1 1000\n0\n").unwrap();
        std::fs::write(dataset.join("fid"), [0; 32]).unwrap();

        let mut found = Vec::new();
        collect_data_files(&root, &mut found);

        assert_eq!(found, vec![dataset]);
        std::fs::remove_dir_all(root).unwrap();
    }
}
