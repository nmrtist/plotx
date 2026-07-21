//! Preparing and applying a verified update after the GUI process exits.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use super::UpdateError;

const OLD_SUFFIX: &str = ".old";
const HELPER_ARG: &str = "--plotx-apply-update";
const RELAUNCH_MARKER: &str = "relaunch";

/// Everything the shell needs to hand the swap to a temporary helper after
/// the GUI has fully exited. The helper is a copy of the current executable,
/// so it does not lock the installed target on Windows.
#[derive(Clone, Debug, PartialEq)]
pub struct InstallPlan {
    helper: PathBuf,
    new_binary: PathBuf,
    target: PathBuf,
    staging: PathBuf,
}

/// Extract the verified artifact and copy a standalone helper into its unique
/// staging directory. No installed file is touched while PlotX is running.
pub fn prepare(downloaded: &Path) -> Result<InstallPlan, UpdateError> {
    let target = std::env::current_exe()?;
    let staging = downloaded
        .parent()
        .ok_or_else(|| UpdateError::Protocol("update has no staging directory".into()))?
        .to_owned();
    let new_binary = extract_binary(downloaded, &target)?;
    let helper_name = if cfg!(windows) {
        "plotx-update-helper.exe"
    } else {
        "plotx-update-helper"
    };
    let helper = staging.join(helper_name);
    std::fs::copy(&target, &helper)?;
    #[cfg(unix)]
    set_executable(&helper)?;
    Ok(InstallPlan {
        helper,
        new_binary,
        target,
        staging,
    })
}

impl InstallPlan {
    /// Start the detached helper as the final action after the GUI loop ends.
    pub fn launch(self, relaunch: bool) -> Result<(), UpdateError> {
        if relaunch {
            std::fs::write(self.staging.join(RELAUNCH_MARKER), [])?;
        }
        Command::new(&self.helper)
            .arg(HELPER_ARG)
            .arg(&self.new_binary)
            .arg(&self.target)
            .arg(&self.staging)
            .spawn()
            .map(|_| ())
            .map_err(UpdateError::Io)
    }

    /// Remove a prepared operation that is no longer eligible to install.
    pub(super) fn discard(self) {
        let _ = std::fs::remove_dir_all(self.staging);
    }
}

/// Run helper mode when invoked by [`InstallPlan::launch`]. Returns `None` for
/// a normal GUI invocation and an exit code after a helper invocation.
pub fn run_helper_from_args() -> Option<i32> {
    let mut args = std::env::args_os().skip(1);
    if args.next().as_deref() != Some(std::ffi::OsStr::new(HELPER_ARG)) {
        return None;
    }
    let Some(new_binary) = args.next().map(PathBuf::from) else {
        return Some(2);
    };
    let Some(target) = args.next().map(PathBuf::from) else {
        return Some(2);
    };
    let Some(staging) = args.next().map(PathBuf::from) else {
        return Some(2);
    };

    Some(match apply_staged_update(&new_binary, &target, &staging) {
        Ok(()) => 0,
        Err(_) => 1,
    })
}

pub(super) fn apply_staged_update(
    new_binary: &Path,
    target: &Path,
    staging: &Path,
) -> Result<(), UpdateError> {
    // The parent launched us only after its GUI loop returned, but Windows
    // can retain the image lock briefly while the process finishes unwinding.
    let result = (0..100).find_map(|_| match swap_binary(new_binary, target) {
        Ok(()) => Some(Ok(())),
        Err(error) => {
            std::thread::sleep(Duration::from_millis(100));
            if target.exists() {
                None
            } else {
                Some(Err(error))
            }
        }
    });
    result.unwrap_or_else(|| {
        Err(UpdateError::Protocol(
            "timed out waiting to replace the running executable".into(),
        ))
    })?;

    let relaunch = staging.join(RELAUNCH_MARKER).exists();
    // Remove everything except a still-running Windows helper. That final
    // file and directory are removed by `cleanup_after_restart` next launch.
    let running_helper = std::env::current_exe().ok();
    if let Ok(entries) = std::fs::read_dir(staging) {
        for entry in entries.flatten() {
            let path = entry.path();
            if running_helper.as_ref() == Some(&path) {
                continue;
            }
            if path.is_dir() {
                let _ = std::fs::remove_dir_all(path);
            } else {
                let _ = std::fs::remove_file(path);
            }
        }
    }
    let _ = std::fs::remove_dir(staging);
    if relaunch {
        Command::new(target).spawn().map_err(UpdateError::Io)?;
    }
    Ok(())
}

/// Transactionally replace `target`, rolling back if the new copy fails.
pub(super) fn swap_binary(new_binary: &Path, target: &Path) -> Result<(), UpdateError> {
    let old = old_path(target);
    let _ = std::fs::remove_file(&old);
    std::fs::rename(target, &old)?;
    let install = install_binary(new_binary, target);
    if let Err(error) = install {
        let _ = std::fs::remove_file(target);
        let _ = std::fs::rename(&old, target);
        return Err(error.into());
    }
    Ok(())
}

fn install_binary(source: &Path, target: &Path) -> std::io::Result<()> {
    std::fs::copy(source, target)?;
    #[cfg(unix)]
    set_executable(target)?;
    Ok(())
}

#[cfg(unix)]
fn set_executable(path: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755))?;
    Ok(())
}

fn old_path(target: &Path) -> PathBuf {
    let name = target
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "plotx".to_owned());
    target.with_file_name(format!("{name}{OLD_SUFFIX}"))
}

/// Remove the old executable and helper-only directories left by completed
/// updates. Active downloads and prepared updates contain additional files,
/// so another running PlotX instance can never lose its staging directory.
pub fn cleanup_after_restart() {
    if let Ok(current) = std::env::current_exe() {
        let _ = std::fs::remove_file(old_path(&current));
    }
    cleanup_completed_staging(&std::env::temp_dir());
}

pub(super) fn cleanup_completed_staging(temp_dir: &Path) {
    if let Ok(entries) = std::fs::read_dir(temp_dir) {
        for entry in entries.flatten() {
            let is_staging = entry
                .file_name()
                .to_string_lossy()
                .starts_with("plotx-update-");
            if !is_staging {
                continue;
            }
            let path = entry.path();
            let Ok(mut contents) = std::fs::read_dir(&path) else {
                continue;
            };
            let Some(Ok(helper)) = contents.next() else {
                continue;
            };
            let helper_name = helper.file_name();
            let is_helper =
                helper_name == "plotx-update-helper" || helper_name == "plotx-update-helper.exe";
            if is_helper && contents.next().is_none() && std::fs::remove_file(helper.path()).is_ok()
            {
                let _ = std::fs::remove_dir(path);
            }
        }
    }
}

/// Produce a plain executable from a downloaded archive.
pub(super) fn extract_binary(downloaded: &Path, installed: &Path) -> Result<PathBuf, UpdateError> {
    if downloaded
        .extension()
        .is_none_or(|extension| !extension.eq_ignore_ascii_case("zip"))
    {
        return Ok(downloaded.to_owned());
    }
    let wanted = installed
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    let file = std::fs::File::open(downloaded)?;
    let mut archive =
        zip::ZipArchive::new(file).map_err(|error| UpdateError::Protocol(error.to_string()))?;
    let names: Vec<String> = archive.file_names().map(str::to_owned).collect();
    let entry_name = names
        .iter()
        .find(|name| {
            Path::new(name)
                .file_name()
                .is_some_and(|entry| entry.to_string_lossy() == wanted)
        })
        .or_else(|| (names.len() == 1).then(|| &names[0]))
        .ok_or_else(|| UpdateError::Protocol(format!("update archive doesn't contain {wanted:?}")))?
        .clone();
    let mut entry = archive
        .by_name(&entry_name)
        .map_err(|error| UpdateError::Protocol(error.to_string()))?;
    let out_path = downloaded.with_extension("extracted");
    let mut out = std::fs::File::create(&out_path)?;
    std::io::copy(&mut entry, &mut out)?;
    Ok(out_path)
}
