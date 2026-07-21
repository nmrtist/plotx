//! Structured, UI-independent reports for user-triggered workflows.
//!
//! Low-level crates keep returning domain errors. Application boundaries map
//! those errors into these reports for GUI and future headless frontends.

use std::collections::{BTreeMap, VecDeque};
use std::fmt;

pub const OPERATION_HISTORY_CAPACITY: usize = 128;
pub const DIAGNOSTIC_HISTORY_CAPACITY: usize = 512;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    Info,
    Warning,
    Error,
}

impl Severity {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Info => "info",
            Self::Warning => "warning",
            Self::Error => "error",
        }
    }
}

/// Stable identifiers whose user-facing wording may evolve independently.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DiagnosticCode {
    DatasetLoadSucceeded,
    DatasetLoadFailed,
    DatasetLoadWarning,
    TableImportSucceeded,
    TableImportWarning,
    TableImportFailed,
    ProjectLoadSucceeded,
    ProjectLoadFailed,
    ProjectSaveSucceeded,
    ProjectSaveFailed,
    ExportUnavailable,
    ExportSucceeded,
    ExportProducedNoFiles,
    ExportFailed,
    DataExportUnavailable,
    DataExportSucceeded,
    DataExportSerializationFailed,
    DataExportWriteFailed,
    DataExportClipboardReady,
    ClipboardImageUnavailable,
    ClipboardImageSucceeded,
    ClipboardImageFailed,
    ClipboardFigurePartial,
    ProcessingSchemeLoadFailed,
    ProcessingSchemeApplySucceeded,
    ProcessingSchemeApplyFailed,
    ProcessingSchemeSaveSucceeded,
    ProcessingSchemeSaveFailed,
}

impl DiagnosticCode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::DatasetLoadSucceeded => "dataset.load.succeeded",
            Self::DatasetLoadFailed => "dataset.load.failed",
            Self::DatasetLoadWarning => "dataset.load.warning",
            Self::TableImportSucceeded => "table.import.succeeded",
            Self::TableImportWarning => "table.import.warning",
            Self::TableImportFailed => "table.import.failed",
            Self::ProjectLoadSucceeded => "project.load.succeeded",
            Self::ProjectLoadFailed => "project.load.failed",
            Self::ProjectSaveSucceeded => "project.save.succeeded",
            Self::ProjectSaveFailed => "project.save.failed",
            Self::ExportUnavailable => "export.unavailable",
            Self::ExportSucceeded => "export.succeeded",
            Self::ExportProducedNoFiles => "export.no_output",
            Self::ExportFailed => "export.failed",
            Self::DataExportUnavailable => "data.export.unavailable",
            Self::DataExportSucceeded => "data.export.succeeded",
            Self::DataExportSerializationFailed => "data.export.serialization_failed",
            Self::DataExportWriteFailed => "data.export.write_failed",
            Self::DataExportClipboardReady => "data.export.clipboard_ready",
            Self::ClipboardImageUnavailable => "clipboard.image.unavailable",
            Self::ClipboardImageSucceeded => "clipboard.image.succeeded",
            Self::ClipboardImageFailed => "clipboard.image.failed",
            Self::ClipboardFigurePartial => "clipboard.figure.partial",
            Self::ProcessingSchemeLoadFailed => "processing.scheme.load.failed",
            Self::ProcessingSchemeApplySucceeded => "processing.scheme.apply.succeeded",
            Self::ProcessingSchemeApplyFailed => "processing.scheme.apply.failed",
            Self::ProcessingSchemeSaveSucceeded => "processing.scheme.save.succeeded",
            Self::ProcessingSchemeSaveFailed => "processing.scheme.save.failed",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Diagnostic {
    pub severity: Severity,
    pub code: DiagnosticCode,
    pub message: String,
    /// Paths belong in structured context rather than the message, allowing
    /// deterministic redaction when users copy support information.
    pub context: BTreeMap<String, String>,
    /// Logical producer such as `core.project`, never a filesystem location.
    pub source: Option<String>,
}

impl Diagnostic {
    pub fn new(severity: Severity, code: DiagnosticCode, message: impl Into<String>) -> Self {
        Self {
            severity,
            code,
            message: message.into(),
            context: BTreeMap::new(),
            source: None,
        }
    }

    pub fn with_context(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.context.insert(key.into(), value.into());
        self
    }

    pub fn with_source(mut self, source: impl Into<String>) -> Self {
        self.source = Some(source.into());
        self
    }

    pub fn sanitized_text(&self) -> String {
        let mut text = format!(
            "[{}] {}: {}",
            self.severity.as_str(),
            self.code.as_str(),
            self.message
        );
        if let Some(source) = &self.source {
            text.push_str(&format!("; source={source}"));
        }
        for (key, value) in &self.context {
            let value = if context_is_sensitive(key) {
                "[redacted]"
            } else {
                value.as_str()
            };
            text.push_str(&format!("; {key}={value}"));
        }
        text
    }
}

fn context_is_sensitive(key: &str) -> bool {
    let key = key.to_ascii_lowercase();
    key.contains("path")
        || key.contains("file")
        || key.contains("directory")
        || key.contains("location")
}

/// Ordered so "acknowledged up to this id" comparisons work: ids come from a
/// single increasing per-history counter (whose u64 wrap is unreachable in
/// practice), and the counter travels with the history across project loads.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct OperationId(pub u64);

impl fmt::Display for OperationId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OperationKind {
    DatasetLoad,
    TableImport,
    ProjectLoad,
    ProjectSave,
    Export,
    DataExport,
    ClipboardImageCopy,
    ClipboardFigureCopy,
    ProcessingSchemeLoadAndApply,
    ProcessingSchemeSave,
}

impl OperationKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::DatasetLoad => "dataset.load",
            Self::TableImport => "table.import",
            Self::ProjectLoad => "project.load",
            Self::ProjectSave => "project.save",
            Self::Export => "export",
            Self::DataExport => "data.export",
            Self::ClipboardImageCopy => "clipboard.image.copy",
            Self::ClipboardFigureCopy => "clipboard.figure.copy",
            Self::ProcessingSchemeLoadAndApply => "processing.scheme.load_and_apply",
            Self::ProcessingSchemeSave => "processing.scheme.save",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OperationOutcome {
    Success,
    Warning,
    Failure,
}

impl OperationOutcome {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Success => "success",
            Self::Warning => "warning",
            Self::Failure => "failure",
        }
    }
}

#[derive(Clone, Debug)]
pub struct OperationReport<T> {
    pub id: OperationId,
    pub kind: OperationKind,
    pub outcome: OperationOutcome,
    pub summary: String,
    pub diagnostics: Vec<Diagnostic>,
    value: Option<T>,
}

impl<T> OperationReport<T> {
    pub fn success(
        id: OperationId,
        kind: OperationKind,
        summary: impl Into<String>,
        value: T,
    ) -> Self {
        Self {
            id,
            kind,
            outcome: OperationOutcome::Success,
            summary: summary.into(),
            diagnostics: Vec::new(),
            value: Some(value),
        }
    }

    pub fn warning(
        id: OperationId,
        kind: OperationKind,
        summary: impl Into<String>,
        value: T,
    ) -> Self {
        Self {
            id,
            kind,
            outcome: OperationOutcome::Warning,
            summary: summary.into(),
            diagnostics: Vec::new(),
            value: Some(value),
        }
    }

    pub fn failure(
        id: OperationId,
        kind: OperationKind,
        summary: impl Into<String>,
        diagnostic: Diagnostic,
    ) -> Self {
        Self {
            id,
            kind,
            outcome: OperationOutcome::Failure,
            summary: summary.into(),
            diagnostics: vec![diagnostic],
            value: None,
        }
    }

    pub fn with_diagnostic(mut self, diagnostic: Diagnostic) -> Self {
        if diagnostic.severity == Severity::Error {
            self.outcome = OperationOutcome::Failure;
        } else if diagnostic.severity == Severity::Warning
            && self.outcome == OperationOutcome::Success
        {
            self.outcome = OperationOutcome::Warning;
        }
        self.diagnostics.push(diagnostic);
        self
    }

    pub fn into_parts(self) -> (OperationRecord, Option<T>) {
        (
            OperationRecord {
                id: self.id,
                kind: self.kind,
                outcome: self.outcome,
                summary: self.summary,
                diagnostics: self.diagnostics,
                completion_order: 0,
            },
            self.value,
        )
    }
}

#[derive(Clone, Debug)]
pub struct OperationRecord {
    pub id: OperationId,
    pub kind: OperationKind,
    pub outcome: OperationOutcome,
    pub summary: String,
    pub diagnostics: Vec<Diagnostic>,
    /// Monotonic order in which reports were recorded, independent of when
    /// their operations received an ID.
    pub completion_order: u64,
}

#[derive(Clone, Debug)]
pub struct RecordedDiagnostic {
    pub operation_id: OperationId,
    pub diagnostic: Diagnostic,
}

#[derive(Debug)]
pub struct OperationHistory {
    next_id: u64,
    next_completion_order: u64,
    operations: VecDeque<OperationRecord>,
    diagnostics: VecDeque<RecordedDiagnostic>,
}

impl Default for OperationHistory {
    fn default() -> Self {
        Self {
            next_id: 1,
            next_completion_order: 1,
            operations: VecDeque::with_capacity(OPERATION_HISTORY_CAPACITY),
            diagnostics: VecDeque::with_capacity(DIAGNOSTIC_HISTORY_CAPACITY),
        }
    }
}

impl OperationHistory {
    pub fn next_id(&mut self) -> OperationId {
        let id = OperationId(self.next_id);
        self.next_id = self.next_id.wrapping_add(1).max(1);
        id
    }

    pub fn push(&mut self, mut record: OperationRecord) {
        record.completion_order = self.next_completion_order;
        self.next_completion_order = self.next_completion_order.wrapping_add(1).max(1);
        for diagnostic in &record.diagnostics {
            push_bounded(
                &mut self.diagnostics,
                RecordedDiagnostic {
                    operation_id: record.id,
                    diagnostic: diagnostic.clone(),
                },
                DIAGNOSTIC_HISTORY_CAPACITY,
            );
        }
        push_bounded(&mut self.operations, record, OPERATION_HISTORY_CAPACITY);
    }

    pub fn operations(&self) -> impl DoubleEndedIterator<Item = &OperationRecord> {
        self.operations.iter()
    }

    pub fn diagnostics(&self) -> impl DoubleEndedIterator<Item = &RecordedDiagnostic> {
        self.diagnostics.iter()
    }

    pub fn operation_count(&self) -> usize {
        self.operations.len()
    }

    pub fn diagnostic_count(&self) -> usize {
        self.diagnostics.len()
    }

    pub fn clear(&mut self) {
        self.operations.clear();
        self.diagnostics.clear();
    }

    pub fn sanitized_text(&self) -> String {
        let mut lines = Vec::new();
        for operation in &self.operations {
            lines.push(format!(
                "operation #{} {}: {}",
                operation.id,
                operation.kind.as_str(),
                operation.outcome.as_str()
            ));
            lines.extend(operation.diagnostics.iter().map(Diagnostic::sanitized_text));
        }
        lines.join("\n")
    }
}

fn push_bounded<T>(queue: &mut VecDeque<T>, value: T, capacity: usize) {
    if queue.len() == capacity {
        queue.pop_front();
    }
    queue.push_back(value);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn history_evicts_oldest_records_at_its_fixed_limit() {
        let mut history = OperationHistory::default();
        for index in 0..=OPERATION_HISTORY_CAPACITY {
            let id = history.next_id();
            let report =
                OperationReport::success(id, OperationKind::Export, format!("export {index}"), ());
            history.push(report.into_parts().0);
        }
        assert_eq!(history.operation_count(), OPERATION_HISTORY_CAPACITY);
        assert_eq!(history.operations().next().unwrap().id, OperationId(2));
    }

    #[test]
    fn copied_diagnostics_redact_paths_but_keep_other_context() {
        let diagnostic = Diagnostic::new(
            Severity::Error,
            DiagnosticCode::ProjectLoadFailed,
            "The project could not be opened.",
        )
        .with_context("path", "C:\\Users\\person\\study.plotx")
        .with_context("error", "invalid header");
        let text = diagnostic.sanitized_text();
        assert!(!text.contains("person"));
        assert!(text.contains("path=[redacted]"));
        assert!(text.contains("error=invalid header"));
    }

    #[test]
    fn analysis_workflow_identifiers_are_stable() {
        assert_eq!(OperationKind::TableImport.as_str(), "table.import");
        assert_eq!(
            OperationKind::ClipboardImageCopy.as_str(),
            "clipboard.image.copy"
        );
        assert_eq!(
            OperationKind::ClipboardFigureCopy.as_str(),
            "clipboard.figure.copy"
        );
        assert_eq!(
            DiagnosticCode::ClipboardFigurePartial.as_str(),
            "clipboard.figure.partial"
        );
        assert_eq!(OperationKind::DataExport.as_str(), "data.export");
        assert_eq!(
            DiagnosticCode::DataExportWriteFailed.as_str(),
            "data.export.write_failed"
        );
    }
}
