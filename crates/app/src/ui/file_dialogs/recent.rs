use super::{
    PlotxApp, import_delimited_table_path, import_xlsx_table_path, load_and_note, open_folder_path,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RecentOpenKind {
    Project,
    DelimitedTable,
    XlsxTable,
    Folder,
    DataFile,
}

pub(crate) fn recent_open_kind(path: &std::path::Path) -> RecentOpenKind {
    let has_extension = |target: &str| {
        path.extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case(target))
    };
    if path.is_dir() {
        RecentOpenKind::Folder
    } else if has_extension("plotx") {
        RecentOpenKind::Project
    } else if has_extension("csv") || has_extension("tsv") || has_extension("txt") {
        RecentOpenKind::DelimitedTable
    } else if has_extension("xlsx") {
        RecentOpenKind::XlsxTable
    } else {
        RecentOpenKind::DataFile
    }
}

pub(crate) fn open_recent_path(app: &mut PlotxApp, path: &std::path::Path) {
    match recent_open_kind(path) {
        RecentOpenKind::Project => app.load_project_from(path),
        RecentOpenKind::DelimitedTable => import_delimited_table_path(app, path),
        RecentOpenKind::XlsxTable => import_xlsx_table_path(app, path),
        RecentOpenKind::Folder => open_folder_path(app, path),
        RecentOpenKind::DataFile => load_and_note(app, path),
    }
}
