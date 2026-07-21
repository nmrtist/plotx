use super::*;
use crate::operation::{DiagnosticCode, OperationOutcome};
use crate::state::{Dataset, FloatSeries, materialized_float_series_table};

fn app_with_table() -> PlotxApp {
    let mut app = PlotxApp::new_with_settings(crate::settings::Settings::default());
    let table = materialized_float_series_table(
        ("x".into(), "s".into(), vec![Some(0.0), Some(1.0)]),
        vec![FloatSeries {
            name: "y".into(),
            unit: String::new(),
            values: vec![Some(2.0), Some(3.0)],
            uncertainty: None,
            fit: None,
        }],
        "plotx.test.export-service-table.v1",
    )
    .unwrap();
    app.doc.datasets.push(Dataset::Table(Box::new(table)));
    app.open_data_export(0);
    app
}

fn poll(app: &mut PlotxApp) -> Option<ClipboardExport> {
    for _ in 0..1_000 {
        let text = app.poll_data_export();
        if !app.data_export_busy() {
            return text;
        }
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    panic!("data export worker did not complete");
}

#[test]
fn clipboard_worker_records_completion_and_returns_tsv_to_the_ui() {
    let mut app = app_with_table();
    app.start_data_export_clipboard();
    let payload = poll(&mut app).unwrap();
    assert_eq!(payload.text, "x\ty\n0\t2\n1\t3\n");
    let schema: crate::xlsx::PlotxDelimitedSchemaV1 =
        serde_json::from_str(payload.schema_json.as_deref().unwrap()).unwrap();
    assert_eq!(schema.schema.columns.len(), 2);
    assert_eq!(
        schema.schema.columns[0].unit.as_ref().unwrap().display_unit,
        "s"
    );
    let record = app
        .session
        .operation_history
        .operations()
        .next_back()
        .unwrap();
    assert_eq!(record.outcome, OperationOutcome::Success);
    assert_eq!(
        record.diagnostics[0].code,
        DiagnosticCode::DataExportClipboardReady
    );
}

#[test]
fn file_open_failure_has_a_specific_operation_diagnostic() {
    let mut app = app_with_table();
    let request = app.session.ui.data_export.unwrap().request;
    let snapshot = DataExportSnapshot::capture(&app.doc.datasets[0], request).unwrap();
    app.start_data_export_file(snapshot, std::env::current_dir().unwrap(), Delimiter::Comma);
    assert!(poll(&mut app).is_none());
    let record = app
        .session
        .operation_history
        .operations()
        .next_back()
        .unwrap();
    assert_eq!(record.outcome, OperationOutcome::Failure);
    assert_eq!(
        record.diagnostics[0].code,
        DiagnosticCode::DataExportWriteFailed
    );
    assert_eq!(record.diagnostics[0].context["category"], "file_open");
}
