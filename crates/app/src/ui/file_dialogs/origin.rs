use std::io::{Read, Seek, SeekFrom};
use std::path::Path;
use std::sync::Arc;

use plotx_core::operation::{
    Diagnostic, DiagnosticCode, OperationId, OperationKind, OperationReport, Severity,
};
use plotx_core::origin::{
    ImportedOriginWorksheet, ORIGIN_IMPORT_OPERATION, OriginImportError, import_origin_project,
};
use plotx_core::state::{
    PlotxApp, TableImportCandidate, TableImportPreviewState, TableImportSource, TypedTableState,
};
use plotx_io::origin::{
    OriginDiagnostic, OriginDiagnosticSeverity, OriginError, OriginLimits, OriginProject,
    probe_origin, read_origin,
};

pub(super) const IMPORT_TABLE_FILTER_EXTENSIONS: &[&str] = &["csv", "tsv", "txt", "xlsx", "opj"];
pub(super) const ORIGIN_PROJECT_FILTER_LABEL: &str =
    "Origin projects (experimental: OPJ import; OPJU recognition only)";
pub(super) const ORIGIN_PROJECT_FILTER_EXTENSIONS: &[&str] = &["opj", "opju"];
pub(super) const OPEN_FILE_FILTER_EXTENSIONS: &[&str] = &["abf", "jdf", "fid", "ser", "zip", "opj"];

const ORIGIN_MEDIA_TYPE: &str = "application/x-origin-project";
const ORIGIN_READ_CHUNK_BYTES: usize = 16 * 1024;

#[derive(Debug)]
pub(super) struct OpenOriginSource {
    file: std::fs::File,
    metadata_len: u64,
}

impl OpenOriginSource {
    pub(super) fn new(file: std::fs::File, metadata_len: u64) -> Self {
        Self { file, metadata_len }
    }

    fn into_parts(self) -> (std::fs::File, u64) {
        (self.file, self.metadata_len)
    }
}

struct OriginFailure {
    stage: &'static str,
    message: String,
    detail: String,
}

#[derive(Clone, Copy)]
struct SourceByteLimit {
    resource: &'static str,
    limit: usize,
    maximum: u64,
    sentinel: usize,
}

impl OriginFailure {
    fn io(stage: &'static str, message: impl Into<String>, error: impl ToString) -> Self {
        Self {
            stage,
            message: message.into(),
            detail: error.to_string(),
        }
    }

    fn parser(stage: &'static str, error: OriginError) -> Self {
        let message = match &error {
            OriginError::UnrecognizedFormat => {
                "The selected file does not have a recognized Origin project signature. No data was imported."
                    .to_owned()
            }
            OriginError::UnsupportedOpjuVariant { message } => message.clone(),
            OriginError::NoSupportedWorksheet => {
                "The Origin project contains no supported table data. No data was imported."
                    .to_owned()
            }
            OriginError::LimitExceeded { .. }
            | OriginError::InvalidLimit { .. }
            | OriginError::ArithmeticOverflow { .. }
            | OriginError::AllocationFailed { .. } => {
                format!("The Origin project could not be imported safely: {error}. No data was imported.")
            }
            _ => format!("The Origin project could not be read: {error}. No data was imported."),
        };
        Self {
            stage,
            message,
            detail: error.to_string(),
        }
    }

    fn core(error: OriginImportError) -> Self {
        let message = match &error {
            OriginImportError::NoSupportedWorksheet => {
                "The Origin project contains no supported table data. No data was imported."
                    .to_owned()
            }
            _ => format!(
                "The Origin project could not be converted into PlotX tables: {error}. No data was imported."
            ),
        };
        Self {
            stage: "convert",
            message,
            detail: error.to_string(),
        }
    }
}

pub(super) fn import_origin_project_source(
    app: &mut PlotxApp,
    path: &Path,
    source: OpenOriginSource,
) {
    let limits = OriginLimits::default();
    let result = read_origin_source(source, limits).and_then(|source_bytes| {
        probe_origin(&source_bytes).map_err(|error| OriginFailure::parser("probe", error))?;
        let project = read_origin(&source_bytes, limits)
            .map_err(|error| OriginFailure::parser("parse", error))?;
        Ok((source_bytes, project))
    });
    match result {
        Ok((source_bytes, project)) => {
            import_origin_project_model(app, path, source_bytes, project, limits);
        }
        Err(error) => {
            let operation_id = app.session.begin_operation();
            install_origin_result(app, operation_id, path, Err(error));
        }
    }
}

pub(super) fn import_origin_project_model(
    app: &mut PlotxApp,
    path: &Path,
    source_bytes: Arc<[u8]>,
    project: OriginProject,
    limits: OriginLimits,
) {
    let operation_id = app.session.begin_operation();
    let result = preview_from_project(operation_id, path, source_bytes, project, limits);
    install_origin_result(app, operation_id, path, result);
}

fn read_origin_source(
    source: OpenOriginSource,
    limits: OriginLimits,
) -> Result<Arc<[u8]>, OriginFailure> {
    let (mut file, metadata_len) = source.into_parts();
    read_origin_handle(&mut file, Some(metadata_len), limits)
}

fn read_origin_handle<R: Read + Seek>(
    mut reader: R,
    metadata_len: Option<u64>,
    limits: OriginLimits,
) -> Result<Arc<[u8]>, OriginFailure> {
    let source_limit = checked_source_byte_limit(limits)
        .map_err(|error| OriginFailure::io("limits", limit_message(&error), error))?;
    if let Some(length) = metadata_len
        && length > source_limit.maximum
    {
        let error = source_too_large(length, source_limit);
        return Err(OriginFailure::io("metadata", limit_message(&error), error));
    }
    reader.seek(SeekFrom::Start(0)).map_err(|error| {
        OriginFailure::io(
            "rewind",
            "The selected Origin project could not be rewound for import. No data was imported.",
            error,
        )
    })?;
    read_bounded_origin(reader, metadata_len, limits).map_err(|error| OriginFailure {
        stage: "read",
        message: limit_message(&error),
        detail: error,
    })
}

pub(super) fn read_bounded_origin<R: Read>(
    mut reader: R,
    metadata_len: Option<u64>,
    limits: OriginLimits,
) -> Result<Arc<[u8]>, String> {
    let source_limit = checked_source_byte_limit(limits)?;
    if let Some(length) = metadata_len
        && length > source_limit.maximum
    {
        return Err(source_too_large(length, source_limit));
    }

    let mut bytes = Vec::new();
    let mut chunk = [0_u8; ORIGIN_READ_CHUNK_BYTES];
    loop {
        let remaining = source_limit
            .sentinel
            .checked_sub(bytes.len())
            .ok_or_else(|| {
                OriginError::ArithmeticOverflow {
                    resource: "Origin source sentinel bytes",
                }
                .to_string()
            })?;
        let request = remaining.min(chunk.len());
        let read = match reader.read(&mut chunk[..request]) {
            Ok(0) => break,
            Ok(read) if read <= request => read,
            Ok(read) => {
                return Err(format!(
                    "the bounded Origin project read failed: the reader returned {read} bytes for a {request}-byte buffer"
                ));
            }
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(error) => {
                return Err(format!("the bounded Origin project read failed: {error}"));
            }
        };
        let next_len = bytes.len().checked_add(read).ok_or_else(|| {
            OriginError::ArithmeticOverflow {
                resource: "Origin source bytes",
            }
            .to_string()
        })?;
        if next_len > source_limit.limit {
            return Err(OriginError::LimitExceeded {
                resource: source_limit.resource,
                limit: source_limit.limit,
                actual: next_len,
            }
            .to_string());
        }
        reserve_source_capacity(&mut bytes, next_len, source_limit, limits)?;
        let read_bytes = chunk.get(..read).ok_or_else(|| {
            "the bounded Origin project read failed: the reader exceeded its buffer".to_owned()
        })?;
        bytes.extend_from_slice(read_bytes);
    }

    let conversion_peak = bytes.capacity().checked_add(bytes.len()).ok_or_else(|| {
        OriginError::ArithmeticOverflow {
            resource: "Origin source Arc conversion",
        }
        .to_string()
    })?;
    if conversion_peak > limits.max_total_owned_bytes {
        return Err(OriginError::LimitExceeded {
            resource: "total owned bytes",
            limit: limits.max_total_owned_bytes,
            actual: conversion_peak,
        }
        .to_string());
    }
    Ok(Arc::<[u8]>::from(bytes))
}

fn reserve_source_capacity(
    bytes: &mut Vec<u8>,
    required_len: usize,
    source_limit: SourceByteLimit,
    limits: OriginLimits,
) -> Result<(), String> {
    let old_capacity = bytes.capacity();
    if required_len <= old_capacity {
        return Ok(());
    }
    let doubled = old_capacity.checked_mul(2).unwrap_or(source_limit.sentinel);
    let target_capacity = required_len.max(doubled).min(source_limit.limit);
    let additional = target_capacity.checked_sub(bytes.len()).ok_or_else(|| {
        OriginError::ArithmeticOverflow {
            resource: "Origin source allocation",
        }
        .to_string()
    })?;
    bytes.try_reserve_exact(additional).map_err(|_| {
        OriginError::AllocationFailed {
            resource: "Origin source bytes",
            requested: target_capacity,
        }
        .to_string()
    })?;
    let actual_capacity = bytes.capacity();
    if actual_capacity > limits.max_total_owned_bytes {
        return Err(OriginError::LimitExceeded {
            resource: "total owned bytes",
            limit: limits.max_total_owned_bytes,
            actual: actual_capacity,
        }
        .to_string());
    }
    Ok(())
}

fn checked_source_byte_limit(limits: OriginLimits) -> Result<SourceByteLimit, String> {
    limits.validate().map_err(|error| error.to_string())?;
    let (resource, limit_name, limit) = if limits.max_input_bytes <= limits.max_total_owned_bytes {
        ("input bytes", "max_input_bytes", limits.max_input_bytes)
    } else {
        (
            "total owned bytes",
            "max_total_owned_bytes",
            limits.max_total_owned_bytes,
        )
    };
    let sentinel = limit.checked_add(1).ok_or_else(|| {
        invalid_limit(
            limit_name,
            limit,
            "the limit must leave room for an oversize sentinel byte",
        )
        .to_string()
    })?;
    let maximum = u64::try_from(limit).map_err(|_| {
        invalid_limit(
            limit_name,
            limit,
            "the source-byte limit cannot be represented by the bounded reader",
        )
        .to_string()
    })?;
    Ok(SourceByteLimit {
        resource,
        limit,
        maximum,
        sentinel,
    })
}

fn invalid_limit(name: &'static str, value: usize, reason: &'static str) -> OriginError {
    OriginError::InvalidLimit {
        name,
        value,
        reason,
    }
}

fn source_too_large(actual: u64, source_limit: SourceByteLimit) -> String {
    let actual = usize::try_from(actual).unwrap_or(usize::MAX);
    OriginError::LimitExceeded {
        resource: source_limit.resource,
        limit: source_limit.limit,
        actual,
    }
    .to_string()
}

fn limit_message(detail: impl std::fmt::Display) -> String {
    format!("The Origin project could not be imported safely: {detail}. No data was imported.")
}

fn preview_from_project(
    operation_id: OperationId,
    path: &Path,
    source_bytes: Arc<[u8]>,
    project: OriginProject,
    limits: OriginLimits,
) -> Result<TableImportPreviewState, OriginFailure> {
    let store = Arc::new(plotx_core::data::MemoryBlockStore::default());
    let codecs = plotx_core::data::CodecRegistry::with_arrow_ipc();
    let imported = import_origin_project(project, store.as_ref(), &codecs, limits)
        .map_err(OriginFailure::core)?;
    preview_from_imported(operation_id, path, source_bytes, store, imported).map_err(|error| {
        OriginFailure::io(
            "revision",
            "The Origin project could not be prepared for preview. No data was imported.",
            error,
        )
    })
}

pub(super) fn preview_from_imported(
    operation_id: OperationId,
    path: &Path,
    source_bytes: Arc<[u8]>,
    store: Arc<plotx_core::data::MemoryBlockStore>,
    imported: Vec<ImportedOriginWorksheet>,
) -> Result<TableImportPreviewState, String> {
    ensure_candidate_count(imported.len())?;
    let candidate_count = imported.len();
    let project_diagnostics = imported
        .first()
        .map(|worksheet| worksheet.diagnostics.clone())
        .unwrap_or_default();
    let file_name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string());
    let mut candidates = Vec::with_capacity(candidate_count);
    let mut candidate_diagnostics = Vec::with_capacity(candidate_count);

    for worksheet in imported {
        let row_count = worksheet.snapshot.row_count;
        let worksheet_name = worksheet.name;
        let typed_state = TypedTableState::imported_with_operation(
            worksheet.snapshot,
            Arc::clone(&store),
            ORIGIN_IMPORT_OPERATION,
        )
        .map_err(|error| error.to_string())?;
        let mut source = TableImportSource::new(Arc::clone(&source_bytes), ORIGIN_MEDIA_TYPE);
        source.name = Some(file_name.clone());
        source.metadata = worksheet.source_metadata;
        candidates.push(TableImportCandidate {
            name: worksheet_name.clone(),
            retained_sources: vec![source],
            typed_state,
            x_binding: None,
            series_bindings: Vec::new(),
        });
        candidate_diagnostics.push(
            Diagnostic::new(
                Severity::Info,
                DiagnosticCode::TableImportSucceeded,
                format!("Prepared Origin table '{worksheet_name}' with {row_count} row(s)."),
            )
            .with_source("core.origin")
            .with_context("path", path.display().to_string())
            .with_context("table", worksheet_name),
        );
    }

    let mut diagnostics = project_diagnostics
        .iter()
        .map(origin_diagnostic)
        .collect::<Vec<_>>();
    let warning_count = diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.severity == Severity::Warning)
        .count();
    diagnostics.extend(candidate_diagnostics);
    let summary = if warning_count == 0 {
        format!("Imported {candidate_count} Origin table(s).")
    } else {
        format!("Imported {candidate_count} Origin table(s) with {warning_count} warning(s).")
    };
    let mut report = if warning_count == 0 {
        OperationReport::success(operation_id, OperationKind::TableImport, summary, ())
    } else {
        OperationReport::warning(operation_id, OperationKind::TableImport, summary, ())
    };
    for diagnostic in diagnostics {
        report = report.with_diagnostic(diagnostic);
    }
    Ok(TableImportPreviewState {
        candidates,
        selected: 0,
        report,
        recent_path: Some(path.to_owned()),
    })
}

pub(super) fn ensure_candidate_count(count: usize) -> Result<(), String> {
    if count == 0 {
        Err("the Origin project contains no supported table candidates".to_owned())
    } else {
        Ok(())
    }
}

fn origin_diagnostic(diagnostic: &OriginDiagnostic) -> Diagnostic {
    let severity = match diagnostic.severity {
        OriginDiagnosticSeverity::Info => Severity::Info,
        OriginDiagnosticSeverity::Warning => Severity::Warning,
    };
    let mut result = Diagnostic::new(
        severity,
        if severity == Severity::Warning {
            DiagnosticCode::TableImportWarning
        } else {
            DiagnosticCode::TableImportSucceeded
        },
        diagnostic.message.clone(),
    )
    .with_source("io.origin")
    .with_context("origin_code", format!("{:?}", diagnostic.code));
    if let Some(location) = &diagnostic.location {
        if let Some(workbook) = &location.workbook {
            result = result.with_context("workbook", workbook.clone());
        }
        if let Some(worksheet) = &location.worksheet {
            result = result.with_context("table", worksheet.clone());
        }
        if let Some(column) = &location.column {
            result = result.with_context("column", column.clone());
        }
        if let Some(offset) = location.byte_offset {
            result = result.with_context("byte_offset", offset.to_string());
        }
    }
    result
}

fn install_origin_result(
    app: &mut PlotxApp,
    operation_id: OperationId,
    path: &Path,
    result: Result<TableImportPreviewState, OriginFailure>,
) {
    match result {
        Ok(preview) => app.session.ui.table_import_preview = Some(preview),
        Err(error) => {
            app.session.record_operation(OperationReport::<()>::failure(
                operation_id,
                OperationKind::TableImport,
                error.message.clone(),
                Diagnostic::new(
                    Severity::Error,
                    DiagnosticCode::TableImportFailed,
                    error.message,
                )
                .with_source("app.table_import.origin")
                .with_context("path", path.display().to_string())
                .with_context("stage", error.stage)
                .with_context("error", error.detail),
            ));
        }
    }
}

#[cfg(test)]
#[path = "origin_allocation_tests.rs"]
mod origin_allocation_tests;

#[cfg(test)]
#[path = "origin_tests.rs"]
mod origin_tests;
