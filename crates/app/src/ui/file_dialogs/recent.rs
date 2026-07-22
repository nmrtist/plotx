use super::{
    PlotxApp, import_delimited_table_path, import_xlsx_table_path, load_and_note, open_folder_path,
    origin,
};
use plotx_core::operation::{Diagnostic, DiagnosticCode, OperationKind, OperationReport, Severity};
use plotx_io::origin::{OriginError, OriginFormat};
use std::fmt;
use std::io::{self, Read};
use std::path::Path;

const OPEN_HEADER_BYTES: usize = 129;
type OpenHeader = ([u8; OPEN_HEADER_BYTES], usize);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RecentOpenKind {
    Project,
    DelimitedTable,
    XlsxTable,
    OriginProject,
    Folder,
    DataFile,
}

impl RecentOpenKind {
    fn is_table_import(self) -> bool {
        matches!(
            self,
            Self::DelimitedTable | Self::XlsxTable | Self::OriginProject
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum OpenPathEntryType {
    Directory,
    RegularFile,
    Other,
}

#[derive(Debug)]
pub(crate) enum OpenPathError {
    Io {
        stage: &'static str,
        error: io::Error,
    },
    OriginProbe(OriginError),
    OriginFamilyMismatch {
        extension: &'static str,
        detected: &'static str,
    },
    NonRegularFile,
}

impl OpenPathError {
    fn io(stage: &'static str, error: io::Error) -> Self {
        Self::Io { stage, error }
    }

    fn stage(&self) -> &'static str {
        match self {
            Self::Io { stage, .. } => stage,
            Self::OriginProbe(_) => "origin_probe",
            Self::OriginFamilyMismatch { .. } => "origin_family",
            Self::NonRegularFile => "file_type",
        }
    }
}

impl fmt::Display for OpenPathError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io {
                stage: "metadata",
                error,
            } => write!(
                formatter,
                "the selected path could not be inspected: {error}"
            ),
            Self::Io { error, .. } => {
                write!(
                    formatter,
                    "the selected file header could not be read: {error}"
                )
            }
            Self::OriginProbe(error) => {
                write!(formatter, "Origin project detection failed: {error}")
            }
            Self::OriginFamilyMismatch {
                extension,
                detected,
            } => write!(
                formatter,
                "the .{extension} extension does not match the detected {detected} project signature; no data was imported"
            ),
            Self::NonRegularFile => write!(
                formatter,
                "the selected path is neither a regular file nor a directory"
            ),
        }
    }
}

impl std::error::Error for OpenPathError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { error, .. } => Some(error),
            Self::OriginProbe(error) => Some(error),
            Self::OriginFamilyMismatch { .. } | Self::NonRegularFile => None,
        }
    }
}

pub(crate) fn classify_open_path(path: &Path) -> Result<RecentOpenKind, OpenPathError> {
    let metadata = std::fs::metadata(path).map_err(|error| OpenPathError::io("metadata", error))?;
    let entry_type = if metadata.is_dir() {
        OpenPathEntryType::Directory
    } else if metadata.is_file() {
        OpenPathEntryType::RegularFile
    } else {
        OpenPathEntryType::Other
    };
    classify_open_path_with_header(path, entry_type, || read_open_header(path))
}

pub(crate) fn classify_open_path_with_header<F>(
    path: &Path,
    entry_type: OpenPathEntryType,
    read_header: F,
) -> Result<RecentOpenKind, OpenPathError>
where
    F: FnOnce() -> io::Result<OpenHeader>,
{
    match entry_type {
        OpenPathEntryType::Directory => return Ok(RecentOpenKind::Folder),
        OpenPathEntryType::Other => return Err(OpenPathError::NonRegularFile),
        OpenPathEntryType::RegularFile => {}
    }

    let (header, length) = read_header().map_err(|error| OpenPathError::io("header", error))?;
    let header = &header[..length];
    if header.starts_with(b"CPYA") || header.starts_with(b"CPYUA") {
        let probe = plotx_io::origin::probe_origin(header).map_err(OpenPathError::OriginProbe)?;
        reject_origin_family_mismatch(path, probe.format)?;
        return Ok(RecentOpenKind::OriginProject);
    }

    Ok(extension_open_kind(path))
}

fn reject_origin_family_mismatch(path: &Path, detected: OriginFormat) -> Result<(), OpenPathError> {
    let extension = path.extension().and_then(|extension| extension.to_str());
    let expected = match extension {
        Some(extension) if extension.eq_ignore_ascii_case("opj") => Some(OriginFormat::Opj),
        Some(extension) if extension.eq_ignore_ascii_case("opju") => Some(OriginFormat::Opju),
        _ => None,
    };
    if let Some(expected) = expected
        && expected != detected
    {
        return Err(OpenPathError::OriginFamilyMismatch {
            extension: match expected {
                OriginFormat::Opj => "opj",
                OriginFormat::Opju => "opju",
            },
            detected: match detected {
                OriginFormat::Opj => "OPJ",
                OriginFormat::Opju => "OPJU",
            },
        });
    }
    Ok(())
}

fn extension_open_kind(path: &Path) -> RecentOpenKind {
    let has_extension = |target: &str| {
        path.extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case(target))
    };
    if has_extension("plotx") {
        RecentOpenKind::Project
    } else if has_extension("csv") || has_extension("tsv") || has_extension("txt") {
        RecentOpenKind::DelimitedTable
    } else if has_extension("xlsx") {
        RecentOpenKind::XlsxTable
    } else if has_extension("opj") || has_extension("opju") {
        RecentOpenKind::OriginProject
    } else {
        RecentOpenKind::DataFile
    }
}

fn read_open_header(path: &Path) -> io::Result<OpenHeader> {
    let mut file = std::fs::File::open(path)?;
    let mut header = [0_u8; OPEN_HEADER_BYTES];
    let mut length = 0;
    while length < header.len() {
        match file.read(&mut header[length..]) {
            Ok(0) => break,
            Ok(read) => length += read,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(error),
        }
    }
    Ok((header, length))
}

pub(crate) fn open_recent_path(app: &mut PlotxApp, path: &Path) {
    let kind = match classify_open_path(path) {
        Ok(kind) => kind,
        Err(error) => {
            record_open_path_failure(app, path, error);
            return;
        }
    };
    if kind.is_table_import() && app.session.ui.table_import_preview.is_some() {
        record_pending_table_import(app, path);
        return;
    }
    match kind {
        RecentOpenKind::Project => app.load_project_from(path),
        RecentOpenKind::DelimitedTable => import_delimited_table_path(app, path),
        RecentOpenKind::XlsxTable => import_xlsx_table_path(app, path),
        RecentOpenKind::OriginProject => origin::import_origin_project_path(app, path),
        RecentOpenKind::Folder => open_folder_path(app, path),
        RecentOpenKind::DataFile => load_and_note(app, path),
    }
}

fn record_open_path_failure(app: &mut PlotxApp, path: &Path, error: OpenPathError) {
    let operation_id = app.session.begin_operation();
    let message = error.to_string();
    app.session.record_operation(OperationReport::<()>::failure(
        operation_id,
        OperationKind::DatasetLoad,
        format!("The selected path could not be opened: {message}."),
        Diagnostic::new(Severity::Error, DiagnosticCode::DatasetLoadFailed, message)
            .with_source("app.open_path")
            .with_context("path", path.display().to_string())
            .with_context("stage", error.stage())
            .with_context("error", error.to_string()),
    ));
}

fn record_pending_table_import(app: &mut PlotxApp, path: &Path) {
    let operation_id = app.session.begin_operation();
    let message =
        "Finish or cancel the current table import preview before importing another table.";
    app.session.record_operation(OperationReport::<()>::failure(
        operation_id,
        OperationKind::TableImport,
        message,
        Diagnostic::new(Severity::Error, DiagnosticCode::TableImportFailed, message)
            .with_source("app.table_import")
            .with_context("path", path.display().to_string())
            .with_context("stage", "preview_pending"),
    ));
}
