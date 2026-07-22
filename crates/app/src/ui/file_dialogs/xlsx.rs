use super::*;
use plotx_core::state::{
    TableImportCandidate, TableImportPreviewState, TableImportSource, TypedTableState,
};
use std::sync::Arc;

pub(super) fn import_xlsx_table_path(app: &mut PlotxApp, path: &std::path::Path) {
    let operation_id = app.session.begin_operation();
    let bytes = match std::fs::read(path) {
        Ok(bytes) => Arc::<[u8]>::from(bytes),
        Err(error) => {
            record_failure(
                app,
                operation_id,
                path,
                "read",
                "The selected XLSX workbook could not be read.",
                error.to_string(),
            );
            return;
        }
    };
    let workbook = match plotx_io::xlsx::read_xlsx(path) {
        Ok(workbook) => workbook,
        Err(error) => {
            record_failure(
                app,
                operation_id,
                path,
                "parse",
                "XLSX workbook parsing failed.",
                error.to_string(),
            );
            return;
        }
    };
    let store = Arc::new(plotx_core::data::MemoryBlockStore::default());
    let codecs = plotx_core::data::CodecRegistry::with_arrow_ipc();
    let imported = match plotx_core::xlsx::import_xlsx_workbook(&workbook, store.as_ref(), &codecs)
    {
        Ok(imported) if !imported.is_empty() => imported,
        Ok(_) => {
            record_failure(
                app,
                operation_id,
                path,
                "worksheet_selection",
                "The workbook has no visible data tables.",
                "no visible non-empty table".into(),
            );
            return;
        }
        Err(error) => {
            record_failure(
                app,
                operation_id,
                path,
                "typed_materialization",
                "Typed XLSX table materialization failed.",
                error.to_string(),
            );
            return;
        }
    };

    let sheet_count = imported.len();
    let mut warning_count = 0;
    let mut report = OperationReport::success(
        operation_id,
        OperationKind::TableImport,
        format!("Imported {sheet_count} table(s) from an XLSX workbook."),
        (),
    );
    let mut candidates = Vec::with_capacity(sheet_count);
    for sheet in imported {
        let row_count = sheet.snapshot.row_count;
        warning_count += sheet.diagnostics.len();
        let typed_state = match TypedTableState::imported_with_operation(
            sheet.snapshot,
            Arc::clone(&store),
            "plotx.import.xlsx.v1",
        ) {
            Ok(state) => state,
            Err(error) => {
                record_failure(
                    app,
                    operation_id,
                    path,
                    "revision",
                    "The initial XLSX table revision could not be created.",
                    error.to_string(),
                );
                return;
            }
        };
        let mut source = TableImportSource::new(Arc::clone(&bytes), xlsx_media_type());
        source.name = path
            .file_name()
            .and_then(|name| name.to_str())
            .map(str::to_owned);
        source.metadata.insert(
            "space.nmrtist.plotx.import.worksheet".into(),
            serde_json::Value::String(sheet.name.clone()),
        );
        source.metadata.insert(
            "space.nmrtist.plotx.import.formula_policy".into(),
            serde_json::Value::String("cached-values-only".into()),
        );
        let dataset_name = if sheet_count == 1 {
            path.file_stem()
                .and_then(|name| name.to_str())
                .unwrap_or(&sheet.name)
                .to_owned()
        } else {
            sheet.name.clone()
        };
        candidates.push(TableImportCandidate {
            name: dataset_name,
            retained_sources: vec![source],
            typed_state,
            x_binding: None,
            series_bindings: Vec::new(),
        });
        report = report.with_diagnostic(
            Diagnostic::new(
                Severity::Info,
                DiagnosticCode::TableImportSucceeded,
                format!("Imported table '{}' with {row_count} row(s).", sheet.name),
            )
            .with_source("core.xlsx")
            .with_context("path", path.display().to_string())
            .with_context("worksheet", sheet.name.clone()),
        );
        for diagnostic in sheet.diagnostics {
            report = report.with_diagnostic(
                Diagnostic::new(
                    Severity::Warning,
                    DiagnosticCode::TableImportWarning,
                    diagnostic,
                )
                .with_source("core.xlsx")
                .with_context("path", path.display().to_string())
                .with_context("worksheet", sheet.name.clone()),
            );
        }
    }
    if warning_count > 0 {
        report.outcome = plotx_core::operation::OperationOutcome::Warning;
        report.summary =
            format!("Imported {sheet_count} XLSX table(s) with {warning_count} warning(s).");
    }
    app.session.ui.table_import_preview = Some(TableImportPreviewState {
        candidates,
        selected: 0,
        report,
        recent_path: Some(path.to_owned()),
    });
}

fn record_failure(
    app: &mut PlotxApp,
    operation_id: plotx_core::operation::OperationId,
    path: &std::path::Path,
    stage: &str,
    message: &str,
    error: String,
) {
    app.session.record_operation(OperationReport::<()>::failure(
        operation_id,
        OperationKind::TableImport,
        "XLSX table import failed.",
        Diagnostic::new(Severity::Error, DiagnosticCode::TableImportFailed, message)
            .with_source("app.table_import.xlsx")
            .with_context("path", path.display().to_string())
            .with_context("stage", stage)
            .with_context("error", error),
    ));
}

fn xlsx_media_type() -> &'static str {
    "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet"
}

pub(super) fn read_delimited_sidecar(
    data_path: &std::path::Path,
) -> Result<
    Option<(
        plotx_core::xlsx::PlotxDelimitedSchemaV1,
        plotx_core::state::TableImportSource,
    )>,
    String,
> {
    let path = plotx_core::data_export::delimited_sidecar_path(data_path);
    let bytes = match std::fs::read(&path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(format!("PlotX schema sidecar could not be read: {error}"));
        }
    };
    let sidecar: plotx_core::xlsx::PlotxDelimitedSchemaV1 = serde_json::from_slice(&bytes)
        .map_err(|error| format!("PlotX schema sidecar is invalid: {error}"))?;
    if sidecar.schema_version != 1 {
        return Err(format!(
            "PlotX schema sidecar version {} is unsupported",
            sidecar.schema_version
        ));
    }
    let mut source = plotx_core::state::TableImportSource::new(
        std::sync::Arc::<[u8]>::from(bytes),
        "application/vnd.plotx.table-schema+json",
    );
    source.name = path
        .file_name()
        .and_then(|name| name.to_str())
        .map(str::to_owned);
    source.metadata.insert(
        "space.nmrtist.plotx.import.source".into(),
        serde_json::Value::String("schema_sidecar".into()),
    );
    Ok(Some((sidecar, source)))
}
