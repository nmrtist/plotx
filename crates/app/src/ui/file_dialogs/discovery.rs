use std::path::{Path, PathBuf};

pub(super) fn collect_data_files(folder: &Path, output: &mut Vec<PathBuf>) -> std::io::Result<()> {
    // Vendor acquisition directories are atomic. Their payload files must
    // never be rediscovered as independent datasets.
    if plotx_io::waters::is_masslynx_raw(folder) || plotx_io::nmr_bridge::is_candidate(folder) {
        output.push(folder.to_owned());
        return Ok(());
    }
    for entry in std::fs::read_dir(folder)? {
        let entry = entry?;
        let path = entry.path();
        let kind = entry.file_type()?;
        if kind.is_dir() && !kind.is_symlink() {
            collect_data_files(&path, output)?;
        } else if kind.is_file() {
            let extension = path
                .extension()
                .and_then(|value| value.to_str())
                .unwrap_or("");
            let supported_extension = ["abf", "spm", "pfc", "rasx", "vms", "wiff"]
                .iter()
                .any(|supported| extension.eq_ignore_ascii_case(supported));
            let recognized_raw =
                extension.eq_ignore_ascii_case("raw") && plotx_io::xrd::is_rigaku_raw(&path);
            let recognized_casaxps =
                extension.eq_ignore_ascii_case("txt") && plotx_io::xps::is_casaxps_text(&path);
            if supported_extension
                || recognized_raw
                || recognized_casaxps
                || plotx_io::nmr_bridge::is_candidate(&path)
            {
                output.push(path);
            }
        }
    }
    Ok(())
}

/// Keep companion files out of the batch while doing all discovery I/O off-thread.
pub(super) fn discover_folder(folder: &Path) -> Result<Vec<PathBuf>, String> {
    let mut files = Vec::new();
    collect_data_files(folder, &mut files).map_err(|error| error.to_string())?;
    if files.is_empty() {
        return Ok(vec![folder.to_owned()]);
    }
    files.sort();
    let mut companions = std::collections::HashSet::new();
    for file in &files {
        if file
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("pfc"))
        {
            // The actual import will surface a failed probe as a per-file error.
            match plotx_io::load_path(file) {
                Ok(loaded) => companions.extend(loaded.provenance.companion_paths),
                Err(error) => {
                    log::warn!("Companion discovery failed for {}: {error}", file.display())
                }
            }
        }
    }
    files.retain(|file| !companions.contains(file));
    Ok(files)
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
        collect_data_files(&root, &mut found).unwrap();
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
        collect_data_files(&root, &mut found).unwrap();

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
        collect_data_files(&root, &mut found).unwrap();

        assert_eq!(found, vec![dataset]);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn folder_scan_discovers_only_the_primary_wiff_file() {
        let root =
            std::env::temp_dir().join(format!("plotx-wiff-discovery-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let wiff = root.join("sample.WIFF");
        std::fs::write(&wiff, b"container").unwrap();
        std::fs::write(root.join("sample.WIFF.scan"), b"scans").unwrap();
        std::fs::write(root.join("sample.wiff2"), b"wiff2").unwrap();
        std::fs::write(root.join("sample.timeseries.data"), b"data").unwrap();

        let mut found = Vec::new();
        collect_data_files(&root, &mut found).unwrap();

        assert_eq!(found, vec![wiff]);
        std::fs::remove_dir_all(root).unwrap();
    }
}
