use super::*;
use plotx_core::operation::{DiagnosticCode, OperationKind};
use plotx_core::settings::Settings;
use std::path::{Path, PathBuf};

fn unique_path(extension: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "plotx-recent-report-{}.{}",
        uuid::Uuid::new_v4(),
        extension
    ))
}

fn write_temp(extension: &str, bytes: &[u8]) -> PathBuf {
    let path = unique_path(extension);
    std::fs::write(&path, bytes).unwrap();
    path
}

fn assert_latest_failure(
    app: &PlotxApp,
    expected_kind: OperationKind,
    expected_code: DiagnosticCode,
) {
    let report = app
        .session
        .operation_history
        .operations()
        .next_back()
        .expect("the open failure must be reported");
    assert_eq!(report.kind, expected_kind, "{report:?}");
    assert!(
        report
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == expected_code),
        "{report:?}"
    );
}

#[test]
fn missing_table_like_paths_report_table_import_failures() {
    for extension in ["csv", "tsv", "txt", "xlsx", "opj", "opju"] {
        let path = unique_path(extension);
        assert!(!path.exists());
        let mut app = PlotxApp::new_with_settings(Settings::default());

        open_recent_path(&mut app, &path);

        assert_latest_failure(
            &app,
            OperationKind::TableImport,
            DiagnosticCode::TableImportFailed,
        );
    }
}

#[test]
fn origin_probe_errors_report_table_import_even_without_an_origin_extension() {
    let path = write_temp("bin", b"CPYA invalid\n");
    let mut app = PlotxApp::new_with_settings(Settings::default());

    open_recent_path(&mut app, &path);

    std::fs::remove_file(path).unwrap();
    assert_latest_failure(
        &app,
        OperationKind::TableImport,
        DiagnosticCode::TableImportFailed,
    );
}

#[test]
fn origin_family_mismatches_report_table_import_failures() {
    let path = write_temp("opj", b"CPYUA 4.3668 178\n");
    let mut app = PlotxApp::new_with_settings(Settings::default());

    open_recent_path(&mut app, &path);

    std::fs::remove_file(path).unwrap();
    assert_latest_failure(
        &app,
        OperationKind::TableImport,
        DiagnosticCode::TableImportFailed,
    );
}

#[test]
fn missing_dataset_and_project_paths_keep_dataset_load_failures() {
    for path in [unique_path("abf"), unique_path("plotx")] {
        assert!(!path.exists());
        let mut app = PlotxApp::new_with_settings(Settings::default());

        open_recent_path(&mut app, Path::new(&path));

        assert_latest_failure(
            &app,
            OperationKind::DatasetLoad,
            DiagnosticCode::DatasetLoadFailed,
        );
    }
}

#[test]
fn later_origin_failures_remain_table_import_failures() {
    let path = write_temp("opju", b"CPYUA 4.3668 178\n");
    let mut app = PlotxApp::new_with_settings(Settings::default());

    open_recent_path(&mut app, &path);

    std::fs::remove_file(path).unwrap();
    assert_latest_failure(
        &app,
        OperationKind::TableImport,
        DiagnosticCode::TableImportFailed,
    );
}
