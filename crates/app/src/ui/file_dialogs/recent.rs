use super::{
    PlotxApp, import_delimited_table_path, import_xlsx_table_path, load_and_note, open_folder_path,
    origin,
};
use std::io::Read;

const OPEN_HEADER_BYTES: usize = 129;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RecentOpenKind {
    Project,
    DelimitedTable,
    XlsxTable,
    OriginProject,
    Folder,
    DataFile,
}

pub(crate) fn classify_open_path(
    path: &std::path::Path,
) -> Result<RecentOpenKind, plotx_io::origin::OriginError> {
    let has_extension = |target: &str| {
        path.extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case(target))
    };
    if path.is_dir() {
        return Ok(RecentOpenKind::Folder);
    }
    if path.is_file()
        && let Ok((header, length)) = read_open_header(path)
    {
        let header = &header[..length];
        if header.starts_with(b"CPYA") || header.starts_with(b"CPYUA") {
            plotx_io::origin::probe_origin(header)?;
            return Ok(RecentOpenKind::OriginProject);
        }
    }
    Ok(if has_extension("plotx") {
        RecentOpenKind::Project
    } else if has_extension("csv") || has_extension("tsv") || has_extension("txt") {
        RecentOpenKind::DelimitedTable
    } else if has_extension("xlsx") {
        RecentOpenKind::XlsxTable
    } else if has_extension("opj") || has_extension("opju") {
        RecentOpenKind::OriginProject
    } else {
        RecentOpenKind::DataFile
    })
}

fn read_open_header(path: &std::path::Path) -> std::io::Result<([u8; OPEN_HEADER_BYTES], usize)> {
    let mut file = std::fs::File::open(path)?;
    let mut header = [0_u8; OPEN_HEADER_BYTES];
    let mut length = 0;
    while length < header.len() {
        match file.read(&mut header[length..]) {
            Ok(0) => break,
            Ok(read) => length += read,
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(error),
        }
    }
    Ok((header, length))
}

#[cfg(test)]
pub(crate) fn recent_open_kind(path: &std::path::Path) -> RecentOpenKind {
    classify_open_path(path).unwrap_or(RecentOpenKind::OriginProject)
}

pub(crate) fn open_recent_path(app: &mut PlotxApp, path: &std::path::Path) {
    let kind = match classify_open_path(path) {
        Ok(kind) => kind,
        Err(error) => {
            origin::record_origin_probe_failure(app, path, error);
            return;
        }
    };
    match kind {
        RecentOpenKind::Project => app.load_project_from(path),
        RecentOpenKind::DelimitedTable => import_delimited_table_path(app, path),
        RecentOpenKind::XlsxTable => import_xlsx_table_path(app, path),
        RecentOpenKind::OriginProject => origin::import_origin_project_path(app, path),
        RecentOpenKind::Folder => open_folder_path(app, path),
        RecentOpenKind::DataFile => load_and_note(app, path),
    }
}
