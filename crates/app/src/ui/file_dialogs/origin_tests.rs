use super::*;
use crate::ui::file_dialogs::{RecentOpenKind, recent_open_kind};
use plotx_core::operation::{OperationId, OperationOutcome};
use plotx_core::origin::{ImportedOriginWorksheet, ORIGIN_IMPORT_OPERATION};
use plotx_core::state::PlotxApp;
use plotx_io::origin::OriginLimits;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::sync::Arc;

const OPENOPJ_FIXTURE: &[u8] =
    include_bytes!("../../../../io/tests/fixtures/origin/test-origin-7.0552.opj");

struct PanicOnRead;

impl Read for PanicOnRead {
    fn read(&mut self, _buffer: &mut [u8]) -> io::Result<usize> {
        panic!("bounded reader must reject before reading")
    }
}

struct PersistedSettingsGuard {
    path: PathBuf,
    original: Option<Vec<u8>>,
}

impl PersistedSettingsGuard {
    fn capture() -> Self {
        let path = plotx_core::settings::config_dir()
            .unwrap_or_else(std::env::temp_dir)
            .join("settings.json");
        let original = match std::fs::read(&path) {
            Ok(contents) => Some(contents),
            Err(error) if error.kind() == io::ErrorKind::NotFound => None,
            Err(error) => panic!("could not capture PlotX settings before test: {error}"),
        };
        Self { path, original }
    }
}

impl Drop for PersistedSettingsGuard {
    fn drop(&mut self) {
        let result = match &self.original {
            Some(contents) => std::fs::write(&self.path, contents),
            None => match std::fs::remove_file(&self.path) {
                Ok(()) => Ok(()),
                Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
                Err(error) => Err(error),
            },
        };
        if let Err(error) = result {
            eprintln!("Could not restore PlotX settings after Origin import test: {error}");
        }
        if self.original.is_none()
            && let Some(parent) = self.path.parent()
            && parent
                .read_dir()
                .is_ok_and(|mut entries| entries.next().is_none())
            && let Err(error) = std::fs::remove_dir(parent)
            && error.kind() != io::ErrorKind::NotFound
        {
            eprintln!("Could not remove empty PlotX test settings directory: {error}");
        }
    }
}

fn temp_origin_file(extension: &str, bytes: &[u8]) -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "plotx-origin-app-{}.{}",
        uuid::Uuid::new_v4(),
        extension
    ));
    std::fs::write(&path, bytes).unwrap();
    path
}

fn duplicated_fixture_import() -> (
    Arc<plotx_core::data::MemoryBlockStore>,
    Vec<ImportedOriginWorksheet>,
) {
    let limits = OriginLimits::default();
    let project = plotx_io::origin::read_origin(OPENOPJ_FIXTURE, limits).unwrap();
    let store = Arc::new(plotx_core::data::MemoryBlockStore::default());
    let codecs = plotx_core::data::CodecRegistry::with_arrow_ipc();
    let imported =
        plotx_core::origin::import_origin_project(project, store.as_ref(), &codecs, limits)
            .unwrap();
    let first = imported.into_iter().next().unwrap();
    let second = ImportedOriginWorksheet {
        name: format!("{} copy", first.name),
        snapshot: first.snapshot.clone(),
        source_metadata: first.source_metadata.clone(),
        diagnostics: first.diagnostics.clone(),
        resource_usage: first.resource_usage.clone(),
    };
    (store, vec![first, second])
}

#[test]
fn origin_import_filter_retains_tables_and_adds_experimental_projects() {
    assert_eq!(
        IMPORT_TABLE_FILTER_EXTENSIONS,
        &["csv", "tsv", "txt", "xlsx", "opj", "opju"]
    );
    assert_eq!(
        ORIGIN_PROJECT_FILTER_LABEL,
        "Origin projects (experimental)"
    );
}

#[test]
fn origin_open_file_filter_accepts_both_project_extensions() {
    assert!(OPEN_FILE_FILTER_EXTENSIONS.contains(&"opj"));
    assert!(OPEN_FILE_FILTER_EXTENSIONS.contains(&"opju"));
}

#[test]
fn origin_routing_uses_signature_before_extension() {
    let root = std::env::temp_dir().join(format!("plotx-origin-route-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&root).unwrap();
    let disguised = root.join("project.dat");
    std::fs::write(&disguised, b"CPYA 4.2673 552#\n").unwrap();

    let kind = recent_open_kind(&disguised);

    std::fs::remove_dir_all(root).unwrap();
    assert_eq!(format!("{kind:?}"), "OriginProject");
}

#[test]
fn origin_extension_routes_signature_mismatch_to_origin_adapter() {
    let path = PathBuf::from("not-origin.opj");
    assert_eq!(format!("{:?}", recent_open_kind(&path)), "OriginProject");
}

#[test]
fn origin_default_limit_accepts_exactly_128_mib_and_rejects_one_more_byte() {
    let limits = OriginLimits::default();
    assert_eq!(limits.max_input_bytes, 128 * 1024 * 1024);

    let exact = read_bounded_origin(
        io::repeat(0).take(limits.max_input_bytes as u64),
        Some(limits.max_input_bytes as u64),
        limits,
    )
    .expect("the exact default limit must be accepted");
    assert_eq!(exact.len(), limits.max_input_bytes);
    drop(exact);

    let error = read_bounded_origin(PanicOnRead, Some(limits.max_input_bytes as u64 + 1), limits)
        .expect_err("one byte beyond the default limit must be rejected");
    assert!(error.contains("exceeding"), "{error}");
}

#[test]
fn origin_usize_max_input_limit_is_rejected_before_reading() {
    let limits = OriginLimits {
        max_input_bytes: usize::MAX,
        ..OriginLimits::default()
    };
    let error = read_bounded_origin(PanicOnRead, None, limits).unwrap_err();
    assert!(
        error.contains("invalid Origin limit max_input_bytes"),
        "{error}"
    );
}

#[test]
fn recent_entries_route_to_origin_project_import() {
    assert_eq!(
        format!("{:?}", recent_open_kind(&PathBuf::from("project.OPJU"))),
        "OriginProject"
    );
    assert_ne!(
        recent_open_kind(&PathBuf::from("project.opj")),
        RecentOpenKind::DataFile
    );
}

#[test]
fn origin_signature_mismatch_becomes_a_user_visible_failure_report() {
    let path = temp_origin_file("opj", b"not an Origin project");
    let mut app = PlotxApp::new_with_settings(plotx_core::settings::Settings::default());

    import_origin_project_path(&mut app, &path);

    std::fs::remove_file(path).unwrap();
    assert!(app.session.ui.table_import_preview.is_none());
    assert!(app.session.recent_files.is_empty());
    let report = app
        .session
        .operation_history
        .operations()
        .next_back()
        .expect("a user-visible failure report");
    assert_eq!(report.outcome, OperationOutcome::Failure);
    assert!(
        report.diagnostics.iter().any(|diagnostic| diagnostic
            .message
            .to_ascii_lowercase()
            .contains("signature")),
        "{report:?}"
    );
}

#[test]
fn origin_opju_is_unsupported_without_preview_or_recent_entry() {
    let path = temp_origin_file("opju", b"CPYUA 4.3668 178\n");
    let mut app = PlotxApp::new_with_settings(plotx_core::settings::Settings::default());

    import_origin_project_path(&mut app, &path);

    std::fs::remove_file(path).unwrap();
    assert!(app.session.ui.table_import_preview.is_none());
    assert!(app.session.recent_files.is_empty());
    let report = app
        .session
        .operation_history
        .operations()
        .next_back()
        .expect("OPJU must produce a failure report");
    assert_eq!(report.outcome, OperationOutcome::Failure);
    assert!(report.summary.contains("OPJU"), "{report:?}");
    assert!(
        report.summary.contains("No data was imported"),
        "{report:?}"
    );
}

#[test]
fn origin_core_failure_becomes_a_user_visible_operation_report() {
    let probe = plotx_io::origin::probe_origin(b"CPYA 4.2673 552#\n").unwrap();
    let project = plotx_io::origin::OriginProject {
        probe,
        parameters: Vec::new(),
        notes: Vec::new(),
        workbooks: Vec::new(),
        diagnostics: Vec::new(),
        unsupported_objects: Vec::new(),
        resource_usage: plotx_io::origin::OriginResourceUsage::default(),
    };
    let mut app = PlotxApp::new_with_settings(plotx_core::settings::Settings::default());

    import_origin_project_model(
        &mut app,
        Path::new("empty.opj"),
        Arc::<[u8]>::from(b"CPYA 4.2673 552#\n".as_slice()),
        project,
        OriginLimits::default(),
    );

    let report = app
        .session
        .operation_history
        .operations()
        .next_back()
        .expect("core failures must be recorded");
    assert_eq!(report.outcome, OperationOutcome::Failure);
    assert!(
        report
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.message.contains("no supported")),
        "{report:?}"
    );
}

#[test]
fn origin_recent_file_is_recorded_only_after_confirmed_full_success() {
    let _settings = PersistedSettingsGuard::capture();
    let path = temp_origin_file("opj", OPENOPJ_FIXTURE);
    let mut app = PlotxApp::new_with_settings(plotx_core::settings::Settings::default());

    import_origin_project_path(&mut app, &path);

    assert!(app.doc.datasets.is_empty());
    assert!(app.session.recent_files.is_empty());
    assert_eq!(app.session.operation_history.operation_count(), 0);
    let candidate_count = app
        .session
        .ui
        .table_import_preview
        .as_ref()
        .expect("valid OPJ should produce a preview")
        .candidates
        .len();
    assert!(candidate_count > 0);

    assert!(crate::ui::file_dialogs::commit_table_import_preview(
        &mut app
    ));
    assert_eq!(app.doc.datasets.len(), candidate_count);
    assert_eq!(app.session.recent_files.len(), 1);
    assert_eq!(
        app.session.recent_files[0],
        std::path::absolute(&path).unwrap()
    );
    std::fs::remove_file(path).unwrap();
}

#[test]
fn origin_cancel_leaves_tables_and_recent_files_unchanged() {
    let path = temp_origin_file("opj", OPENOPJ_FIXTURE);
    let mut app = PlotxApp::new_with_settings(plotx_core::settings::Settings::default());
    import_origin_project_path(&mut app, &path);
    assert!(app.session.ui.table_import_preview.take().is_some());

    std::fs::remove_file(path).unwrap();
    assert!(app.doc.datasets.is_empty());
    assert!(app.session.recent_files.is_empty());
    assert_eq!(app.session.operation_history.operation_count(), 0);
}

#[test]
fn origin_candidates_share_source_allocation_and_stable_operation() {
    let (store, imported) = duplicated_fixture_import();
    let source_bytes = Arc::<[u8]>::from(OPENOPJ_FIXTURE);
    let source_pointer = source_bytes.as_ptr();
    let preview = preview_from_imported(
        OperationId(41),
        Path::new("selected-project.opj"),
        source_bytes,
        store,
        imported,
    )
    .unwrap();

    assert_eq!(preview.candidates.len(), 2);
    for candidate in &preview.candidates {
        let source = &candidate.retained_sources[0];
        assert_eq!(source.bytes().as_ptr(), source_pointer);
        assert_eq!(source.media_type, "application/x-origin-project");
        assert_eq!(source.name.as_deref(), Some("selected-project.opj"));
        assert!(
            source
                .metadata
                .contains_key("space.nmrtist.plotx.import.origin.resource_usage")
        );
        assert_eq!(
            candidate.typed_state.envelope.revision.operation.name,
            ORIGIN_IMPORT_OPERATION
        );
    }
}

#[test]
fn origin_zero_candidates_fails_without_indexing_candidate_zero() {
    let result = std::panic::catch_unwind(|| ensure_candidate_count(0));
    let error = result.expect("zero candidates must not panic").unwrap_err();
    assert!(error.contains("no supported"), "{error}");
}

#[test]
fn origin_selector_changes_preview_only_and_confirmation_imports_all_tables() {
    assert_eq!(
        crate::ui::file_dialogs::preview::candidate_selector_label(),
        "Table"
    );
    assert_eq!(
        crate::ui::file_dialogs::preview::all_candidate_import_summary(2),
        "All 2 candidate tables will be imported."
    );
    let (store, imported) = duplicated_fixture_import();
    let mut preview = preview_from_imported(
        OperationId(42),
        Path::new("selected-project.opj"),
        Arc::<[u8]>::from(OPENOPJ_FIXTURE),
        store,
        imported,
    )
    .unwrap();
    preview.selected = 1;
    preview.recent_path = None;
    let mut app = PlotxApp::new_with_settings(plotx_core::settings::Settings::default());
    app.session.ui.table_import_preview = Some(preview);

    assert!(crate::ui::file_dialogs::commit_table_import_preview(
        &mut app
    ));
    assert_eq!(app.doc.datasets.len(), 2);
    assert!(app.session.recent_files.is_empty());
}
