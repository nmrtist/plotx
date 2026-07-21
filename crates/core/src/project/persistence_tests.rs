use super::*;

// The name distinguishes tests within a run and the pid distinguishes
// concurrent runs; a clock stamp alone would not, because macOS reports
// `SystemTime` at microsecond resolution.
fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("plotx-{name}-{}", std::process::id()));
    // A recycled pid could leave a directory from an earlier run behind.
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn save_recovery(manager: &RecoveryManager, app: &crate::state::PlotxApp) {
    super::super::save_recovery_snapshot(
        super::super::prepare_recovery_snapshot(app).unwrap(),
        manager.target(),
    )
    .unwrap();
}

#[test]
fn replacement_failure_leaves_original_untouched() {
    let dir = temp_dir("replace-failure");
    let target = dir.join("project.plotx");
    let temp = dir.join("project.plotx.tmp");
    std::fs::write(&target, b"old project").unwrap();
    std::fs::write(&temp, b"new project").unwrap();

    let error = commit_with_replacer(&temp, &target, 1, |_, _, _| {
        Err(io::Error::other("injected replacement failure"))
    })
    .unwrap_err();

    assert_eq!(error.kind(), io::ErrorKind::Other);
    assert_eq!(std::fs::read(&target).unwrap(), b"old project");
    assert_eq!(std::fs::read(&temp).unwrap(), b"new project");
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn replacement_failure_restores_the_existing_backup() {
    let dir = temp_dir("replace-failure-backup");
    let target = dir.join("project.plotx");
    let temp = dir.join("project.plotx.tmp");
    let backup = backup_path(&target, 0);
    std::fs::write(&target, b"current project").unwrap();
    std::fs::write(&temp, b"new project").unwrap();
    std::fs::write(&backup, b"last good backup").unwrap();

    let error = commit_with_replacer(&temp, &target, 1, |_, _, _| {
        Err(io::Error::other("injected replacement failure"))
    })
    .unwrap_err();

    assert_eq!(error.kind(), io::ErrorKind::Other);
    assert_eq!(std::fs::read(&target).unwrap(), b"current project");
    assert_eq!(std::fs::read(&backup).unwrap(), b"last good backup");
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn successful_replacement_keeps_previous_file_as_backup() {
    let dir = temp_dir("backup");
    let target = dir.join("project.plotx");
    let temp = dir.join("project.plotx.tmp");
    std::fs::write(&target, b"old project").unwrap();
    std::fs::write(&temp, b"new project").unwrap();

    assert!(commit_project_file(&temp, &target, 1).unwrap().is_none());

    assert_eq!(std::fs::read(&target).unwrap(), b"new project");
    assert_eq!(
        std::fs::read(backup_path(&target, 0)).unwrap(),
        b"old project"
    );
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        use windows_sys::Win32::Storage::FileSystem::{FILE_ATTRIBUTE_HIDDEN, GetFileAttributesW};
        let wide = backup_path(&target, 0)
            .as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect::<Vec<_>>();
        // SAFETY: `wide` is a live NUL-terminated path buffer.
        let attributes = unsafe { GetFileAttributesW(wide.as_ptr()) };
        assert_ne!(attributes & FILE_ATTRIBUTE_HIDDEN, 0);
    }
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn one_generation_replaces_the_previous_backup() {
    let dir = temp_dir("one-backup");
    let target = dir.join("project.plotx");
    let first_temp = dir.join("first.tmp");
    std::fs::write(&target, b"original").unwrap();
    std::fs::write(&first_temp, b"first save").unwrap();
    commit_project_file(&first_temp, &target, 1).unwrap();

    let second_temp = dir.join("second.tmp");
    std::fs::write(&second_temp, b"second save").unwrap();
    commit_project_file(&second_temp, &target, 1).unwrap();

    assert_eq!(std::fs::read(&target).unwrap(), b"second save");
    assert_eq!(
        std::fs::read(backup_path(&target, 0)).unwrap(),
        b"first save"
    );
    assert!(!backup_path(&target, 1).exists());
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn zero_generations_disables_and_prunes_backups() {
    let dir = temp_dir("no-backup");
    let target = dir.join("project.plotx");
    let first_temp = dir.join("first.tmp");
    std::fs::write(&target, b"original").unwrap();
    std::fs::write(&first_temp, b"first save").unwrap();
    commit_project_file(&first_temp, &target, 1).unwrap();

    let second_temp = dir.join("second.tmp");
    std::fs::write(&second_temp, b"second save").unwrap();
    commit_project_file(&second_temp, &target, 0).unwrap();

    assert_eq!(std::fs::read(&target).unwrap(), b"second save");
    assert!(!backup_path(&target, 0).exists());
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn live_process_recovery_slots_are_not_offered() {
    let dir = temp_dir("live-recovery-slot");
    let root = dir.join("recovery");
    let owner = RecoveryManager::new_in(root.clone()).unwrap();
    let mut app = crate::state::PlotxApp::new();
    app.doc.dirty = true;
    save_recovery(&owner, &app);

    let mut observer = RecoveryManager::new_in(root.clone()).unwrap();
    assert!(observer.pending_recovery().unwrap().is_none());

    drop(owner);
    let mut rescuer = RecoveryManager::new_in(root).unwrap();
    let snapshot = rescuer.pending_recovery().unwrap().unwrap();
    rescuer.discard(&snapshot).unwrap();
    observer.shutdown().unwrap();
    rescuer.shutdown().unwrap();
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn recovery_older_than_a_committed_revision_is_rejected() {
    let dir = temp_dir("stale-recovery");
    let root = dir.join("recovery");
    let original = dir.join("project.plotx");
    let mut app = crate::state::PlotxApp::new();
    app.session.project_backup_generations = 0;
    let first = super::super::save_project(&app, &original, false).unwrap();
    app.doc.project_path = Some(original.clone());
    app.doc.project_revision = Some(first.revision);
    app.doc.dirty = true;

    let owner = RecoveryManager::new_in(root.clone()).unwrap();
    save_recovery(&owner, &app);
    drop(owner);

    super::super::save_project(&app, &original, false).unwrap();
    let mut finder = RecoveryManager::new_in(root).unwrap();
    assert!(finder.pending_recovery().unwrap().is_none());
    finder.shutdown().unwrap();
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn adopt_cleanup_failure_keeps_the_new_slot_usable() {
    let dir = temp_dir("adopt-cleanup-failure");
    let root = dir.join("recovery");
    let owner = RecoveryManager::new_in(root.clone()).unwrap();
    let app = crate::state::PlotxApp::new();
    save_recovery(&owner, &app);
    drop(owner);

    let mut manager = RecoveryManager::new_in(root).unwrap();
    let snapshot = manager.pending_recovery().unwrap().unwrap();
    let warning = manager
        .adopt_with_cleanup(&snapshot, |_slot| {
            Err(ProjectError::Io(io::Error::other(
                "injected slot cleanup failure",
            )))
        })
        .unwrap();

    assert!(warning.unwrap().contains("injected slot cleanup failure"));
    assert_eq!(manager.target().path(), snapshot.path);
    manager.clear_current().unwrap();
    manager.shutdown().unwrap();
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn one_unreadable_slot_does_not_hide_another_valid_snapshot() {
    let dir = temp_dir("bad-slot-isolation");
    let root = dir.join("recovery");
    let bad_owner = RecoveryManager::new_in(root.clone()).unwrap();
    let app = crate::state::PlotxApp::new();
    let mut bad_request = super::super::prepare_recovery_snapshot(&app).unwrap();
    bad_request.metadata.original_path = Some(PathBuf::from("\0"));
    super::super::save_recovery_snapshot(bad_request, bad_owner.target()).unwrap();
    drop(bad_owner);

    let good_owner = RecoveryManager::new_in(root.clone()).unwrap();
    save_recovery(&good_owner, &app);
    drop(good_owner);

    let mut finder = RecoveryManager::new_in(root).unwrap();
    let snapshot = finder.pending_recovery().unwrap().unwrap();
    assert!(snapshot.original_path.is_none());
    finder.discard(&snapshot).unwrap();
    finder.shutdown().unwrap();
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn orphaned_recovery_temp_file_is_reclaimed() {
    let dir = temp_dir("orphaned-recovery-temp");
    let root = dir.join("recovery");
    let owner = RecoveryManager::new_in(root.clone()).unwrap();
    let target = owner.target();
    let slot_dir = target.path().parent().unwrap().to_owned();
    std::fs::write(temporary_path(target.path()), b"partial archive").unwrap();
    drop(target);
    drop(owner);

    let mut finder = RecoveryManager::new_in(root).unwrap();
    assert!(finder.pending_recovery().unwrap().is_none());
    assert!(!slot_dir.exists());
    finder.shutdown().unwrap();
    std::fs::remove_dir_all(dir).unwrap();
}
