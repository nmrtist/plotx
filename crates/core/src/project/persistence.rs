use super::{FileStamp, Manifest, ProjectError, RecoveryMetadata, Result, temporary_path};
use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::SystemTime;

const RECOVERY_FILE: &str = "recovery.plotx";
const RECOVERY_LOCK_FILE: &str = "slot.lock";
static REVISION_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Debug)]
pub struct RecoverySnapshot {
    pub path: PathBuf,
    pub original_path: Option<PathBuf>,
    pub base_revision: Option<String>,
    pub modified: SystemTime,
}

struct RecoverySlot {
    dir: PathBuf,
    lock: Arc<File>,
}

pub struct RecoveryTarget {
    path: PathBuf,
    _lock: Arc<File>,
}

impl RecoveryTarget {
    pub(super) fn path(&self) -> &Path {
        &self.path
    }
}

pub struct RecoveryManager {
    root: PathBuf,
    current: RecoverySlot,
    claimed: HashMap<PathBuf, Arc<File>>,
}

pub(super) fn commit_project_file(
    temp: &Path,
    target: &Path,
    backup_count: usize,
) -> Result<Option<String>> {
    let backup_count =
        backup_count.min(usize::from(crate::settings::MAX_PROJECT_BACKUP_GENERATIONS));
    let backup = commit_with_replacer(temp, target, backup_count, atomic_replace)?;
    sync_file(target)?;
    sync_parent(target)?;
    let warning = backup.and_then(|path| {
        conceal_backup(&path)
            .err()
            .map(|error| format!("The backup was created but could not be hidden: {error}"))
    });
    Ok(warning)
}

pub(super) fn commit_recovery_file(temp: &Path, target: &Path) -> Result<()> {
    commit_with_replacer(temp, target, 0, atomic_replace)?;
    sync_file(target)?;
    sync_parent(target)?;
    Ok(())
}

/// Install a fully written sibling temporary file without project backups.
/// Shared with run manifests so every durable core artifact uses the same
/// platform-specific atomic replacement and sync guarantees.
pub(crate) fn commit_atomic_file(temp: &Path, target: &Path) -> io::Result<()> {
    atomic_replace(temp, target, None)?;
    sync_file(target)?;
    sync_parent(target)
}

fn commit_with_replacer(
    temp: &Path,
    target: &Path,
    backup_count: usize,
    replacer: impl FnOnce(&Path, &Path, Option<&Path>) -> io::Result<()>,
) -> io::Result<Option<PathBuf>> {
    if !target.exists() {
        replacer(temp, target, None)?;
        return Ok(None);
    }
    let backup = if backup_count == 0 {
        None
    } else {
        Some(backup_path(target, 0))
    };
    let staged_backup = if backup_count == 0 {
        None
    } else {
        stage_primary_backup(target)?
    };
    if let Err(replace_error) = replacer(temp, target, backup.as_deref()) {
        if let Err(restore_error) = restore_primary_backup(target, staged_backup.as_deref()) {
            return Err(io::Error::new(
                replace_error.kind(),
                format!(
                    "{replace_error}; additionally failed to restore the previous backup: {restore_error}"
                ),
            ));
        }
        return Err(replace_error);
    }
    finish_backup_rotation(target, backup_count, staged_backup.as_deref())?;
    prune_excess_backups(target, backup_count)?;
    Ok(backup)
}

fn stage_primary_backup(target: &Path) -> io::Result<Option<PathBuf>> {
    let primary = backup_path(target, 0);
    let staging = temporary_path(&primary);
    if staging.exists() {
        if primary.exists() {
            remove_if_exists(&staging)?;
        } else {
            std::fs::rename(&staging, &primary)?;
        }
    }
    if !primary.exists() {
        return Ok(None);
    }
    std::fs::rename(&primary, &staging)?;
    Ok(Some(staging))
}

fn restore_primary_backup(target: &Path, staged: Option<&Path>) -> io::Result<()> {
    let Some(staged) = staged else {
        return Ok(());
    };
    let primary = backup_path(target, 0);
    remove_if_exists(&primary)?;
    std::fs::rename(staged, primary)
}

fn finish_backup_rotation(target: &Path, count: usize, staged: Option<&Path>) -> io::Result<()> {
    let Some(staged) = staged else {
        return Ok(());
    };
    if count == 1 {
        return remove_if_exists(staged);
    }
    remove_if_exists(&backup_path(target, count - 1))?;
    for index in (1..count - 1).rev() {
        let source = backup_path(target, index);
        if source.exists() {
            std::fs::rename(&source, backup_path(target, index + 1))?;
        }
    }
    std::fs::rename(staged, backup_path(target, 1))?;
    Ok(())
}

fn prune_excess_backups(target: &Path, keep: usize) -> io::Result<()> {
    for index in keep..usize::from(crate::settings::MAX_PROJECT_BACKUP_GENERATIONS) {
        remove_if_exists(&backup_path(target, index))?;
    }
    Ok(())
}

fn backup_path(target: &Path, index: usize) -> PathBuf {
    let name = target
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "project.plotx".to_owned());
    let hidden_name = if name.starts_with('.') {
        name
    } else {
        format!(".{name}")
    };
    let suffix = if index == 0 {
        ".bak".to_owned()
    } else {
        format!(".bak.{index}")
    };
    target.with_file_name(format!("{hidden_name}{suffix}"))
}

#[cfg(unix)]
fn backup_install_staging_path(backup: &Path) -> PathBuf {
    let name = backup
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "project.plotx.bak".to_owned());
    backup.with_file_name(format!("{name}.new.tmp"))
}

#[cfg(unix)]
fn atomic_replace(temp: &Path, target: &Path, backup: Option<&Path>) -> io::Result<()> {
    if let Some(backup) = backup {
        let staging = backup_install_staging_path(backup);
        remove_if_exists(&staging)?;
        std::fs::copy(target, &staging)?;
        File::open(&staging)?.sync_all()?;
        std::fs::rename(&staging, backup)?;
    }
    // POSIX rename replaces an existing regular file atomically. Both paths are
    // siblings, so a cross-device rename cannot occur.
    std::fs::rename(temp, target)
}

#[cfg(windows)]
fn atomic_replace(temp: &Path, target: &Path, backup: Option<&Path>) -> io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{
        MOVEFILE_WRITE_THROUGH, MoveFileExW, ReplaceFileW,
    };

    let target_exists = target.exists();
    let wide = |path: &Path| {
        path.as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect::<Vec<_>>()
    };
    let temp = wide(temp);
    let target = wide(target);
    let backup = backup.map(wide);
    // SAFETY: all pointers refer to NUL-terminated buffers that remain alive for
    // the call. ReplaceFileW is the Windows primitive that preserves the old
    // destination when replacement fails and can create the backup atomically.
    let ok = unsafe {
        if target_exists {
            ReplaceFileW(
                target.as_ptr(),
                temp.as_ptr(),
                backup.as_ref().map_or(std::ptr::null(), |p| p.as_ptr()),
                0,
                std::ptr::null(),
                std::ptr::null(),
            )
        } else {
            MoveFileExW(temp.as_ptr(), target.as_ptr(), MOVEFILE_WRITE_THROUGH)
        }
    };
    if ok == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

#[cfg(not(any(unix, windows)))]
fn atomic_replace(temp: &Path, target: &Path, backup: Option<&Path>) -> io::Result<()> {
    if let Some(backup) = backup {
        std::fs::copy(target, backup)?;
        File::open(backup)?.sync_all()?;
    }
    std::fs::rename(temp, target)
}

#[cfg(unix)]
fn sync_parent(path: &Path) -> io::Result<()> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    File::open(parent)?.sync_all()
}

#[cfg(not(unix))]
fn sync_parent(_path: &Path) -> io::Result<()> {
    // The installed file is synced explicitly. Opening a Windows directory for
    // sync requires special share flags; ReplaceFileW already provides the
    // atomic namespace update needed here.
    Ok(())
}

fn remove_if_exists(path: &Path) -> io::Result<()> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

fn sync_file(path: &Path) -> io::Result<()> {
    OpenOptions::new()
        .read(true)
        .write(true)
        .open(path)?
        .sync_all()
}

#[cfg(windows)]
fn conceal_backup(path: &Path) -> io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_ATTRIBUTE_HIDDEN, GetFileAttributesW, INVALID_FILE_ATTRIBUTES, SetFileAttributesW,
    };

    let wide = path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    // SAFETY: `wide` is NUL-terminated and remains alive for both calls.
    let attributes = unsafe { GetFileAttributesW(wide.as_ptr()) };
    if attributes == INVALID_FILE_ATTRIBUTES {
        return Err(io::Error::last_os_error());
    }
    if attributes & FILE_ATTRIBUTE_HIDDEN != 0 {
        return Ok(());
    }
    // SAFETY: the same live NUL-terminated path buffer is used here.
    if unsafe { SetFileAttributesW(wide.as_ptr(), attributes | FILE_ATTRIBUTE_HIDDEN) } == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

#[cfg(not(windows))]
fn conceal_backup(_path: &Path) -> io::Result<()> {
    // A leading dot is the portable hidden-file convention on Unix platforms.
    Ok(())
}

fn recovery_root() -> Result<PathBuf> {
    let root = crate::settings::data_local_dir()
        .map(|root| root.join("recovery"))
        .ok_or_else(|| ProjectError::Invalid("recovery directory is unavailable".to_owned()))?;
    std::fs::create_dir_all(&root)?;
    Ok(root)
}

pub(super) fn new_revision() -> String {
    let nanos = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let sequence = REVISION_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    format!("{}-{nanos}-{sequence}", std::process::id())
}

pub(super) fn file_stamp(path: &Path) -> io::Result<Option<FileStamp>> {
    let metadata = match std::fs::metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    let modified_nanos = metadata
        .modified()
        .unwrap_or(SystemTime::UNIX_EPOCH)
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    Ok(Some(FileStamp {
        modified_nanos,
        len: metadata.len(),
    }))
}

impl RecoveryManager {
    pub fn new() -> Result<Self> {
        Self::new_in(recovery_root()?)
    }

    fn new_in(root: PathBuf) -> Result<Self> {
        std::fs::create_dir_all(&root)?;
        let current = create_recovery_slot(&root)?;
        Ok(Self {
            root,
            current,
            claimed: HashMap::new(),
        })
    }

    pub fn target(&self) -> RecoveryTarget {
        RecoveryTarget {
            path: self.current.dir.join(RECOVERY_FILE),
            _lock: Arc::clone(&self.current.lock),
        }
    }

    pub fn pending_recovery(&mut self) -> Result<Option<RecoverySnapshot>> {
        let current_dir = self.current.dir.clone();
        let mut best: Option<(RecoverySnapshot, PathBuf, Arc<File>)> = None;
        let mut first_error: Option<ProjectError> = None;
        for entry in std::fs::read_dir(&self.root)? {
            let entry = match entry {
                Ok(entry) => entry,
                Err(error) => {
                    remember_error(&mut first_error, error.into());
                    continue;
                }
            };
            let dir = entry.path();
            let file_type = match entry.file_type() {
                Ok(file_type) => file_type,
                Err(error) => {
                    remember_error(&mut first_error, error.into());
                    continue;
                }
            };
            if dir == current_dir || !file_type.is_dir() {
                continue;
            }
            let lock = match try_claim_slot(&dir) {
                Ok(Some(lock)) => lock,
                Ok(None) => continue,
                Err(error) => {
                    remember_error(&mut first_error, error);
                    continue;
                }
            };
            let path = dir.join(RECOVERY_FILE);
            let inspected = inspect_recovery(&path);
            let (metadata, modified) = match inspected {
                Ok(Some(value)) => value,
                Ok(None) => {
                    if let Err(error) = clear_snapshot_files(&path) {
                        remember_error(&mut first_error, error);
                        continue;
                    }
                    drop(lock);
                    if let Err(error) = cleanup_slot_dir(&dir) {
                        remember_error(&mut first_error, error);
                    }
                    continue;
                }
                Err(error) => {
                    remember_error(&mut first_error, error);
                    continue;
                }
            };
            let stale = match recovery_is_stale(&metadata) {
                Ok(stale) => stale,
                Err(error) => {
                    remember_error(&mut first_error, error);
                    continue;
                }
            };
            if stale {
                if let Err(error) = clear_snapshot_files(&path) {
                    remember_error(&mut first_error, error);
                    continue;
                }
                drop(lock);
                if let Err(error) = cleanup_slot_dir(&dir) {
                    remember_error(&mut first_error, error);
                }
                continue;
            }
            let snapshot = RecoverySnapshot {
                path,
                original_path: metadata.original_path,
                base_revision: metadata.base_revision,
                modified,
            };
            if best
                .as_ref()
                .is_none_or(|(current, _, _)| snapshot.modified > current.modified)
            {
                best = Some((snapshot, dir, lock));
            }
        }
        if let Some((snapshot, dir, lock)) = best {
            self.claimed.insert(dir, lock);
            Ok(Some(snapshot))
        } else if let Some(error) = first_error {
            Err(error)
        } else {
            Ok(None)
        }
    }

    pub fn adopt(&mut self, snapshot: &RecoverySnapshot) -> Result<Option<String>> {
        self.adopt_with_cleanup(snapshot, cleanup_slot)
    }

    fn adopt_with_cleanup(
        &mut self,
        snapshot: &RecoverySnapshot,
        cleanup: impl FnOnce(RecoverySlot) -> Result<()>,
    ) -> Result<Option<String>> {
        let dir = snapshot
            .path
            .parent()
            .ok_or_else(|| ProjectError::Invalid("recovery slot has no parent".to_owned()))?
            .to_owned();
        let lock = self.claimed.remove(&dir).ok_or_else(|| {
            ProjectError::Invalid("recovery slot is not claimed by this process".to_owned())
        })?;
        let previous = std::mem::replace(&mut self.current, RecoverySlot { dir, lock });
        Ok(cleanup(previous)
            .err()
            .map(|error| format!("could not remove the previous empty recovery slot: {error}")))
    }

    pub fn discard(&mut self, snapshot: &RecoverySnapshot) -> Result<()> {
        clear_snapshot_files(&snapshot.path)?;
        let Some(dir) = snapshot.path.parent().map(Path::to_path_buf) else {
            return Ok(());
        };
        if let Some(lock) = self.claimed.remove(&dir) {
            drop(lock);
            cleanup_slot_dir(&dir)?;
        }
        Ok(())
    }

    pub fn clear_current(&self) -> Result<()> {
        clear_snapshot_files(&self.current.dir.join(RECOVERY_FILE))
    }

    pub fn shutdown(self) -> Result<()> {
        self.clear_current()?;
        cleanup_slot(self.current)
    }
}

fn remember_error(first: &mut Option<ProjectError>, error: ProjectError) {
    if first.is_none() {
        *first = Some(error);
    }
}

fn create_recovery_slot(root: &Path) -> Result<RecoverySlot> {
    for _ in 0..100 {
        let dir = root.join(format!("slot-{}", new_revision()));
        match std::fs::create_dir(&dir) {
            Ok(()) => {
                let file = OpenOptions::new()
                    .read(true)
                    .write(true)
                    .create_new(true)
                    .open(dir.join(RECOVERY_LOCK_FILE))?;
                file.try_lock().map_err(|error| match error {
                    std::fs::TryLockError::Error(error) => ProjectError::Io(error),
                    std::fs::TryLockError::WouldBlock => ProjectError::Invalid(
                        "new recovery slot was unexpectedly locked".to_owned(),
                    ),
                })?;
                return Ok(RecoverySlot {
                    dir,
                    lock: Arc::new(file),
                });
            }
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error.into()),
        }
    }
    Err(ProjectError::Invalid(
        "could not allocate a unique recovery slot".to_owned(),
    ))
}

fn try_claim_slot(dir: &Path) -> Result<Option<Arc<File>>> {
    let file = match OpenOptions::new()
        .read(true)
        .write(true)
        .open(dir.join(RECOVERY_LOCK_FILE))
    {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    match file.try_lock() {
        Ok(()) => Ok(Some(Arc::new(file))),
        Err(std::fs::TryLockError::WouldBlock) => Ok(None),
        Err(std::fs::TryLockError::Error(error)) => Err(error.into()),
    }
}

fn inspect_recovery(path: &Path) -> Result<Option<(RecoveryMetadata, SystemTime)>> {
    let modified = match std::fs::metadata(path) {
        Ok(metadata) => metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH),
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    let file = File::open(path)?;
    let mut zip = zip::ZipArchive::new(file)?;
    let manifest: Manifest = super::read_json(&mut zip, "manifest.json")?;
    super::validate_manifest(&manifest)?;
    let metadata = manifest.recovery.ok_or_else(|| {
        ProjectError::Invalid("recovery archive has no embedded recovery metadata".to_owned())
    })?;
    Ok(Some((metadata, modified)))
}

fn project_revision(path: &Path) -> Result<Option<String>> {
    let file = File::open(path)?;
    let mut zip = zip::ZipArchive::new(file)?;
    let manifest: Manifest = super::read_json(&mut zip, "manifest.json")?;
    super::validate_manifest(&manifest)?;
    Ok(manifest.revision)
}

fn recovery_is_stale(metadata: &RecoveryMetadata) -> Result<bool> {
    let Some(original_path) = metadata.original_path.as_deref() else {
        return Ok(false);
    };
    let current_file = file_stamp(original_path)?;
    if current_file.is_none() {
        return Ok(false);
    }
    match project_revision(original_path) {
        Ok(current_revision) if metadata.base_revision.is_some() || current_revision.is_some() => {
            Ok(metadata.base_revision != current_revision)
        }
        Ok(_) | Err(_) => Ok(metadata.base_file != current_file),
    }
}

fn clear_snapshot_files(path: &Path) -> Result<()> {
    remove_if_exists(path)?;
    remove_if_exists(&temporary_path(path))?;
    Ok(())
}

fn cleanup_slot(slot: RecoverySlot) -> Result<()> {
    let dir = slot.dir;
    drop(slot.lock);
    cleanup_slot_dir(&dir)
}

fn cleanup_slot_dir(dir: &Path) -> Result<()> {
    remove_if_exists(&dir.join(RECOVERY_LOCK_FILE))?;
    match std::fs::remove_dir(dir) {
        Ok(()) => Ok(()),
        Err(error)
            if matches!(
                error.kind(),
                io::ErrorKind::NotFound | io::ErrorKind::DirectoryNotEmpty
            ) =>
        {
            Ok(())
        }
        Err(error) => Err(error.into()),
    }
}

#[cfg(test)]
#[path = "persistence_tests.rs"]
mod tests;
