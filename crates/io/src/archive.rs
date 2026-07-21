//! Batch loading from a `.zip` archive: extract to a scratch directory, then
//! walk the tree loading every supported loose spectrum and Bruker acquisition
//! folder.

use crate::{IoError, LoadResult, LoadWarning, LoadWarningCode};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

/// A `.zip` archive, by extension or by the `PK\x03\x04` local-file magic.
pub fn is_zip(path: &Path) -> bool {
    if path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.eq_ignore_ascii_case("zip"))
        .unwrap_or(false)
    {
        return true;
    }
    use std::io::Read;
    let mut magic = [0u8; 4];
    std::fs::File::open(path)
        .and_then(|mut f| f.read_exact(&mut magic))
        .map(|()| &magic == b"PK\x03\x04")
        .unwrap_or(false)
}

/// Extract `path` to a scratch directory and load every recognised dataset
/// inside it, in path order. Entries that don't parse are skipped; the archive
/// itself failing to open is an error. The scratch directory is removed before
/// returning, so the acquisitions are fully in memory.
#[derive(Debug, Clone)]
pub struct ArchiveLoadResult {
    pub items: Vec<LoadResult>,
    pub warnings: Vec<LoadWarning>,
}

pub fn load_zip(path: &Path) -> Result<ArchiveLoadResult, IoError> {
    let file = std::fs::File::open(path)?;
    let mut archive = zip::ZipArchive::new(file).map_err(|e| IoError::Archive(e.to_string()))?;

    let scratch = scratch_dir();
    std::fs::create_dir_all(&scratch)?;
    let loaded = (|| {
        archive
            .extract(&scratch)
            .map_err(|e| IoError::Archive(e.to_string()))?;
        let mut result = ArchiveLoadResult {
            items: Vec::new(),
            warnings: Vec::new(),
        };
        collect_acquisitions(&scratch, &mut result);
        Ok(result)
    })();
    let _ = std::fs::remove_dir_all(&scratch);
    loaded
}

// Unique per-call scratch path under the system temp dir. The counter rather
// than a clock stamp is what makes it unique: macOS reports `SystemTime` at
// microsecond resolution, so two extractions started in the same microsecond
// would share a directory and delete each other's files on cleanup.
fn scratch_dir() -> PathBuf {
    static SEQUENCE: AtomicU64 = AtomicU64::new(0);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    std::env::temp_dir().join(format!(
        "plotx_zip_{}_{nanos}_{}",
        std::process::id(),
        SEQUENCE.fetch_add(1, Ordering::Relaxed)
    ))
}

// Depth-first walk appending each loadable dataset. A Bruker acquisition folder
// is loaded as a unit and not descended into; any other directory is recursed;
// loose JEOL and JCAMP-DX files are read individually.
fn collect_acquisitions(dir: &Path, out: &mut ArchiveLoadResult) {
    if crate::bruker::detect_processed(dir).is_some() || crate::bruker::is_bruker_dir(dir) {
        match crate::load_path(dir) {
            Ok(result) => out.items.push(result),
            Err(error) => out.warnings.push(entry_warning(dir, error)),
        }
        return;
    }
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(error) => {
            out.warnings.push(entry_warning(dir, error.into()));
            return;
        }
    };
    let mut paths: Vec<PathBuf> = entries.filter_map(|e| e.ok().map(|e| e.path())).collect();
    paths.sort();
    for p in paths {
        if p.is_dir() {
            collect_acquisitions(&p, out);
        } else if is_supported_spectrum(&p) {
            match crate::load_path(&p) {
                Ok(result) => out.items.push(result),
                Err(error) => out.warnings.push(entry_warning(&p, error)),
            }
        }
    }
}

fn entry_warning(path: &Path, error: IoError) -> LoadWarning {
    LoadWarning {
        code: LoadWarningCode::ArchiveEntryFailed,
        message: error.to_string(),
        path: Some(path.to_path_buf()),
    }
}

fn is_jdf(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| e.eq_ignore_ascii_case("jdf"))
        .unwrap_or(false)
        || crate::jeol::is_jdf(path)
}

fn is_supported_spectrum(path: &Path) -> bool {
    is_jdf(path) || crate::jcamp_dx::has_jcamp_extension(path)
}
