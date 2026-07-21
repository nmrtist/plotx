use super::*;
use crate::operation::{
    Diagnostic, DiagnosticCode, OperationId, OperationKind, OperationReport, Severity,
};
use crate::state::PlotxApp;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, TryRecvError};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DataExportDialogState {
    pub dataset: usize,
    pub request: DataExportRequest,
}

pub struct DataExportJob {
    receiver: Receiver<JobResult>,
    /// Whether the worker writes a file (as opposed to preparing clipboard
    /// text); classifies a worker that died without sending a result.
    file: bool,
}

enum JobResult {
    File(Result<PathBuf, JobFailure>),
    Clipboard(Result<ClipboardExport, JobFailure>),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClipboardExport {
    pub text: String,
    /// PlotX's typed table contract for the custom clipboard MIME format.
    /// Standard clipboard consumers continue to receive `text` as TSV.
    pub schema_json: Option<String>,
}

enum FileFormat {
    Delimited(Delimiter),
    Xlsx,
}

struct JobFailure {
    category: &'static str,
    message: String,
    path: Option<PathBuf>,
}

impl PlotxApp {
    pub fn open_data_export(&mut self, dataset: usize) {
        let Some(current) = self.doc.datasets.get(dataset) else {
            self.report_data_export_unavailable(DataExportError::Unavailable);
            return;
        };
        let availability = DataExportAvailability::for_dataset(current);
        let Some(content) = availability.contents.first().copied() else {
            self.report_data_export_unavailable(DataExportError::Unavailable);
            return;
        };
        self.session.ui.data_export = Some(DataExportDialogState {
            dataset,
            request: DataExportRequest {
                content,
                channel: availability.default_channel,
                shape: TableShape::Matrix,
            },
        });
    }

    /// Callers capture the snapshot themselves (the dialog already needs it for
    /// the suggested file name), so the data is not cloned a second time here.
    pub fn start_data_export_file(
        &mut self,
        snapshot: DataExportSnapshot,
        path: PathBuf,
        delimiter: Delimiter,
    ) {
        self.start_data_export_file_with_format(snapshot, path, FileFormat::Delimited(delimiter));
    }

    pub fn start_data_export_xlsx_file(&mut self, snapshot: DataExportSnapshot, path: PathBuf) {
        self.start_data_export_file_with_format(snapshot, path, FileFormat::Xlsx);
    }

    fn start_data_export_file_with_format(
        &mut self,
        snapshot: DataExportSnapshot,
        path: PathBuf,
        format: FileFormat,
    ) {
        if self.data_export_busy() {
            return;
        }
        let operation_id = self.session.begin_operation();
        let (sender, receiver) = mpsc::channel();
        std::thread::spawn(move || {
            let result = (|| {
                match format {
                    FileFormat::Delimited(delimiter) => {
                        let file = std::fs::File::create(&path).map_err(|error| JobFailure {
                            category: "file_open",
                            message: error.to_string(),
                            path: Some(path.clone()),
                        })?;
                        let mut output = std::io::BufWriter::new(file);
                        snapshot
                            .write_to(&mut output, delimiter)
                            .map_err(|error| JobFailure {
                                category: "file_write",
                                message: error.to_string(),
                                path: Some(path.clone()),
                            })?;
                        output.flush().map_err(|error| JobFailure {
                            category: "file_flush",
                            message: error.to_string(),
                            path: Some(path.clone()),
                        })?;
                        snapshot
                            .write_delimited_sidecar(&path)
                            .map_err(|error| JobFailure {
                                category: "sidecar_write",
                                message: error.to_string(),
                                path: Some(path.clone()),
                            })?;
                    }
                    FileFormat::Xlsx => snapshot.write_xlsx(&path).map_err(|error| JobFailure {
                        category: error.category(),
                        message: error.to_string(),
                        path: Some(path.clone()),
                    })?,
                }
                Ok(path)
            })();
            let _ = sender.send(JobResult::File(result));
        });
        self.session.data_export_operation = Some(operation_id);
        self.session.data_export_job = Some(DataExportJob {
            receiver,
            file: true,
        });
        self.session.status = "Exporting numerical data…".into();
    }

    pub fn start_data_export_clipboard(&mut self) {
        let Some((snapshot, operation_id)) = self.prepare_data_export() else {
            return;
        };
        let (sender, receiver) = mpsc::channel();
        std::thread::spawn(move || {
            let result = (|| {
                let text = snapshot.to_text(Delimiter::Tab)?;
                let schema_json = snapshot.clipboard_schema_json()?;
                Ok(ClipboardExport { text, schema_json })
            })()
            .map_err(|error: DataExportError| JobFailure {
                category: "serialization",
                message: error.to_string(),
                path: None,
            });
            let _ = sender.send(JobResult::Clipboard(result));
        });
        self.session.data_export_operation = Some(operation_id);
        self.session.data_export_job = Some(DataExportJob {
            receiver,
            file: false,
        });
        self.session.status = "Preparing TSV for the clipboard…".into();
    }

    pub fn data_export_busy(&self) -> bool {
        self.session.data_export_job.is_some()
    }

    /// Poll the worker and return clipboard text that must be committed on the UI thread.
    pub fn poll_data_export(&mut self) -> Option<ClipboardExport> {
        let result = match self
            .session
            .data_export_job
            .as_ref()
            .map(|job| job.receiver.try_recv())
        {
            Some(Ok(result)) => result,
            Some(Err(TryRecvError::Empty)) | None => return None,
            Some(Err(TryRecvError::Disconnected)) => {
                let failure = JobFailure {
                    category: "worker_disconnected",
                    message: "the data export worker stopped without returning a result".into(),
                    path: None,
                };
                if self
                    .session
                    .data_export_job
                    .as_ref()
                    .is_some_and(|job| job.file)
                {
                    JobResult::File(Err(failure))
                } else {
                    JobResult::Clipboard(Err(failure))
                }
            }
        };
        self.session.data_export_job = None;
        let operation_id = self
            .session
            .data_export_operation
            .take()
            .unwrap_or_else(|| self.session.begin_operation());
        match result {
            JobResult::File(Ok(path)) => {
                self.session.record_operation(
                    OperationReport::success(
                        operation_id,
                        OperationKind::DataExport,
                        "Exported numerical data.",
                        (),
                    )
                    .with_diagnostic(
                        Diagnostic::new(
                            Severity::Info,
                            DiagnosticCode::DataExportSucceeded,
                            "The numerical data file was written successfully.",
                        )
                        .with_source("core.data_export")
                        .with_context("path", path.display().to_string()),
                    ),
                );
                None
            }
            JobResult::Clipboard(Ok(payload)) => {
                self.session.record_operation(
                    OperationReport::success(
                        operation_id,
                        OperationKind::DataExport,
                        "Copied numerical data as TSV.",
                        (),
                    )
                    .with_diagnostic(
                        Diagnostic::new(
                            Severity::Info,
                            DiagnosticCode::DataExportClipboardReady,
                            "The TSV clipboard request completed successfully.",
                        )
                        .with_source("core.data_export")
                        .with_context("stage", "clipboard_request"),
                    ),
                );
                Some(payload)
            }
            JobResult::File(Err(failure)) => {
                self.record_data_export_failure(operation_id, failure, true);
                None
            }
            JobResult::Clipboard(Err(failure)) => {
                self.record_data_export_failure(operation_id, failure, false);
                None
            }
        }
    }

    fn prepare_data_export(&mut self) -> Option<(DataExportSnapshot, OperationId)> {
        if self.data_export_busy() {
            return None;
        }
        let Some(dialog) = self.session.ui.data_export else {
            self.report_data_export_unavailable(DataExportError::Unavailable);
            return None;
        };
        let snapshot = match self
            .doc
            .datasets
            .get(dialog.dataset)
            .ok_or(DataExportError::Unavailable)
            .and_then(|dataset| DataExportSnapshot::capture(dataset, dialog.request))
        {
            Ok(snapshot) => snapshot,
            Err(error) => {
                self.report_data_export_unavailable(error);
                return None;
            }
        };
        Some((snapshot, self.session.begin_operation()))
    }

    pub fn report_data_export_unavailable(&mut self, error: DataExportError) {
        let operation_id = self.session.begin_operation();
        self.session
            .record_operation(OperationReport::<()>::failure(
                operation_id,
                OperationKind::DataExport,
                error.to_string(),
                Diagnostic::new(
                    Severity::Error,
                    DiagnosticCode::DataExportUnavailable,
                    "Data export is not available for the current selection.",
                )
                .with_source("core.data_export")
                .with_context("category", error.category())
                .with_context("error", error.to_string()),
            ));
    }

    fn record_data_export_failure(
        &mut self,
        operation_id: OperationId,
        failure: JobFailure,
        file: bool,
    ) {
        let code = if file {
            DiagnosticCode::DataExportWriteFailed
        } else {
            DiagnosticCode::DataExportSerializationFailed
        };
        let mut diagnostic = Diagnostic::new(
            Severity::Error,
            code,
            if file {
                "The numerical data file could not be written."
            } else {
                "The TSV clipboard content could not be generated."
            },
        )
        .with_source("core.data_export")
        .with_context("category", failure.category)
        .with_context("error", failure.message);
        if let Some(path) = failure.path {
            diagnostic = diagnostic.with_context("path", path.display().to_string());
        }
        self.session
            .record_operation(OperationReport::<()>::failure(
                operation_id,
                OperationKind::DataExport,
                "Data export failed.",
                diagnostic,
            ));
    }
}

#[cfg(test)]
#[path = "service_tests.rs"]
mod tests;
