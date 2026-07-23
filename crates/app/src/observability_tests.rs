use super::*;
use std::time::Duration;

fn temp_root(name: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "plotx-{name}-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&root).unwrap();
    root
}

#[test]
fn report_creation_writes_marker_and_expected_content() {
    let root = temp_root("crash-report");
    let report = "PlotX crash report\nPanic: test panic\nBacktrace:\nframes";
    let path = write_crash_report(&root, report).unwrap();

    assert_eq!(std::fs::read_to_string(&path).unwrap(), report);
    assert_eq!(read_marker(&root), Some(path));
    clear_marker(&root).unwrap();
    assert_eq!(read_marker(&root), None);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn every_report_writes_unclean_shutdown_marker() {
    let root = temp_root("background-report");
    let path = write_crash_report(&root, "background panic").unwrap();

    assert!(path.is_file());
    assert_eq!(read_marker(&root), Some(path));
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn crash_rotation_keeps_ten_newest_reports() {
    let root = temp_root("crash-rotation");
    let crashes = root.join("crashes");
    std::fs::create_dir_all(&crashes).unwrap();
    for index in 0..12 {
        std::fs::write(
            crashes.join(format!("crash-{index:02}.txt")),
            index.to_string(),
        )
        .unwrap();
        std::thread::sleep(Duration::from_millis(2));
    }

    rotate_files(&crashes, "crash-", 10).unwrap();
    let names = std::fs::read_dir(&crashes)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    assert_eq!(names.len(), 10);
    assert!(!names.contains(&"crash-00.txt".to_owned()));
    assert!(!names.contains(&"crash-01.txt".to_owned()));
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn log_rotation_keeps_five_newest_sessions() {
    let root = temp_root("log-rotation");
    for index in 0..7 {
        std::fs::write(
            root.join(format!("session-{index:02}.log")),
            index.to_string(),
        )
        .unwrap();
        std::thread::sleep(Duration::from_millis(2));
    }
    rotate_files(&root, "session-", 5).unwrap();
    assert_eq!(std::fs::read_dir(&root).unwrap().count(), 5);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn tail_and_utc_format_are_stable() {
    let root = temp_root("tail");
    let path = root.join("session.log");
    std::fs::write(&path, "one\ntwo\nthree\nfour\n").unwrap();
    assert_eq!(read_tail(&path, 2).unwrap(), "three\nfour");
    assert_eq!(utc_timestamp(UNIX_EPOCH), "1970-01-01T00:00:00Z");
    std::fs::remove_dir_all(root).unwrap();
}
