use super::*;
use crate::ui::file_dialogs::recent::{
    OpenPathEntryType, classify_open_handle, classify_open_path, classify_open_path_with_header,
    dispatch_classified_path, open_file_for_classification,
};
use crate::ui::file_dialogs::{RecentOpenKind, open_recent_path};
use plotx_core::operation::{OperationId, OperationOutcome};
use plotx_core::origin::{ImportedOriginWorksheet, ORIGIN_IMPORT_OPERATION};
use plotx_core::state::PlotxApp;
use plotx_io::origin::OriginLimits;
use std::io::{self, Read, Seek, SeekFrom};
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

struct SeekFailure;

impl Read for SeekFailure {
    fn read(&mut self, _buffer: &mut [u8]) -> io::Result<usize> {
        panic!("a failed rewind must stop before reading")
    }
}

impl Seek for SeekFailure {
    fn seek(&mut self, _position: SeekFrom) -> io::Result<u64> {
        Err(io::Error::other("injected rewind failure"))
    }
}

struct ReadFailure;

impl Read for ReadFailure {
    fn read(&mut self, _buffer: &mut [u8]) -> io::Result<usize> {
        Err(io::Error::other("injected full-read failure"))
    }
}

impl Seek for ReadFailure {
    fn seek(&mut self, _position: SeekFrom) -> io::Result<u64> {
        Ok(0)
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

    let kind = classify_open_path(&disguised).unwrap();

    std::fs::remove_dir_all(root).unwrap();
    assert_eq!(kind.kind(), RecentOpenKind::OriginProject);
}

#[test]
fn origin_extension_routes_signature_mismatch_to_origin_adapter() {
    let path = PathBuf::from("not-origin.opj");
    let kind = classify_open_path_with_header(&path, OpenPathEntryType::RegularFile, || {
        Ok(([0_u8; 129], 0))
    })
    .unwrap();
    assert_eq!(format!("{kind:?}"), "OriginProject");
}

#[test]
fn origin_pending_preview_rejects_a_second_table_path_without_replacement() {
    let first = temp_origin_file("opj", OPENOPJ_FIXTURE);
    let second = temp_origin_file("csv", b"time,value\n0,1\n");
    let mut app = PlotxApp::new_with_settings(plotx_core::settings::Settings::default());

    open_recent_path(&mut app, &first);
    let first_preview_path = app
        .session
        .ui
        .table_import_preview
        .as_ref()
        .expect("the first table path should create a preview")
        .recent_path
        .clone();
    open_recent_path(&mut app, &second);

    std::fs::remove_file(first).unwrap();
    std::fs::remove_file(second).unwrap();
    let preview = app
        .session
        .ui
        .table_import_preview
        .as_ref()
        .expect("the first preview must remain pending");
    assert_eq!(preview.recent_path, first_preview_path);
    assert!(app.doc.datasets.is_empty());
    assert!(app.session.recent_files.is_empty());
    let report = app
        .session
        .operation_history
        .operations()
        .next_back()
        .expect("the rejected second import should be reported");
    assert_eq!(report.outcome, OperationOutcome::Failure);
    assert!(
        report
            .summary
            .to_ascii_lowercase()
            .contains("finish or cancel"),
        "{report:?}"
    );
}

#[cfg(unix)]
#[test]
fn origin_dispatch_reuses_the_classified_handle_after_path_replacement() {
    use std::os::unix::net::UnixListener;

    let id = uuid::Uuid::new_v4();
    let path = PathBuf::from("/tmp").join(format!("px-{id}.opj"));
    let original_path = PathBuf::from("/tmp").join(format!("px-{id}.saved"));
    std::fs::write(&path, OPENOPJ_FIXTURE).unwrap();
    let classified = classify_open_path(&path).unwrap();
    assert_eq!(classified.kind(), RecentOpenKind::OriginProject);
    std::fs::rename(&path, &original_path).unwrap();
    let replacement = UnixListener::bind(&path).unwrap();
    let mut app = PlotxApp::new_with_settings(plotx_core::settings::Settings::default());

    dispatch_classified_path(&mut app, &path, classified);

    drop(replacement);
    std::fs::remove_file(&path).unwrap();
    std::fs::remove_file(original_path).unwrap();
    let preview = app
        .session
        .ui
        .table_import_preview
        .as_ref()
        .expect("dispatch must consume the original classified file handle");
    assert_eq!(preview.recent_path.as_deref(), Some(path.as_path()));
    assert!(!preview.candidates.is_empty());
    assert!(app.doc.datasets.is_empty());
    assert!(app.session.recent_files.is_empty());
}

#[cfg(unix)]
#[test]
fn origin_classification_rejects_non_regular_handle_metadata() {
    let device = std::fs::File::open("/dev/null").unwrap();

    let error = classify_open_handle(Path::new("device.opj"), device)
        .expect_err("a character-device handle must be rejected before header reads");

    assert!(error.to_string().contains("regular file"), "{error}");
}

#[cfg(unix)]
#[test]
fn origin_classification_opens_paths_in_nonblocking_mode() {
    use std::os::fd::AsRawFd;

    let path = temp_origin_file("opj", OPENOPJ_FIXTURE);
    let file = open_file_for_classification(&path).unwrap();
    let flags = unsafe { libc::fcntl(file.as_raw_fd(), libc::F_GETFL) };
    std::fs::remove_file(path).unwrap();

    assert_ne!(flags, -1, "F_GETFL must succeed");
    assert_ne!(flags & libc::O_NONBLOCK, 0);
}

#[test]
fn origin_rewind_and_full_read_errors_are_propagated() {
    let limits = OriginLimits::default();
    let rewind_error = read_origin_handle(&mut SeekFailure, Some(0), limits)
        .expect_err("rewind errors must stop the import");
    assert_eq!(rewind_error.stage, "rewind");
    assert!(
        rewind_error.detail.contains("injected rewind failure"),
        "{}",
        rewind_error.detail
    );

    let read_error = read_origin_handle(&mut ReadFailure, Some(0), limits)
        .expect_err("full-read errors must stop the import");
    assert_eq!(read_error.stage, "read");
    assert!(
        read_error.detail.contains("injected full-read failure"),
        "{}",
        read_error.detail
    );
}

#[test]
fn origin_oversized_metadata_is_rejected_before_rewind() {
    let limits = OriginLimits::default();
    let oversized = u64::try_from(limits.max_input_bytes).unwrap() + 1;

    let error = read_origin_handle(&mut SeekFailure, Some(oversized), limits)
        .expect_err("known oversized input must be rejected before rewinding");

    assert_eq!(error.stage, "metadata");
    assert!(error.detail.contains("input bytes"), "{}", error.detail);
}

#[cfg(unix)]
#[test]
fn origin_routing_metadata_errors_are_not_silently_discarded() {
    use std::os::unix::fs::symlink;

    let path = std::env::temp_dir().join(format!(
        "plotx-origin-metadata-loop-{}.opj",
        uuid::Uuid::new_v4()
    ));
    symlink(&path, &path).unwrap();

    let result = classify_open_path(&path);
    let mut app = PlotxApp::new_with_settings(plotx_core::settings::Settings::default());
    open_recent_path(&mut app, &path);

    std::fs::remove_file(path).unwrap();
    assert!(
        result.is_err(),
        "metadata errors must be propagated: {result:?}"
    );
    let report = app
        .session
        .operation_history
        .operations()
        .next_back()
        .expect("routing metadata errors must be user-visible");
    assert_eq!(report.outcome, OperationOutcome::Failure);
    assert!(
        report
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.source.as_deref() == Some("app.open_path")),
        "{report:?}"
    );
}

#[test]
fn origin_header_read_errors_are_propagated_by_the_shared_classifier() {
    let result = classify_open_path_with_header(
        Path::new("unreadable.bin"),
        OpenPathEntryType::RegularFile,
        || {
            Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "injected header read failure",
            ))
        },
    );

    let error = result.expect_err("header read errors must not fall back to an extension route");
    assert!(error.to_string().contains("header"), "{error}");
    assert!(
        error.to_string().contains("injected header read failure"),
        "{error}"
    );
}

#[test]
fn origin_non_regular_file_classification_never_reads_a_header() {
    let result =
        classify_open_path_with_header(Path::new("stream.opj"), OpenPathEntryType::Other, || {
            panic!("non-regular paths must be rejected before header reads")
        });

    let error = result.expect_err("non-regular paths must be rejected");
    assert!(error.to_string().contains("regular file"), "{error}");
}

#[cfg(unix)]
#[test]
fn origin_routing_rejects_non_regular_files_without_opening_them() {
    use std::os::unix::net::UnixListener;

    let path = PathBuf::from("/tmp").join(format!("px-{}.opj", uuid::Uuid::new_v4()));
    let listener = UnixListener::bind(&path).unwrap();

    let result = classify_open_path(&path);

    drop(listener);
    std::fs::remove_file(path).unwrap();
    assert!(
        result.is_err(),
        "non-regular files must be rejected before opening: {result:?}"
    );
}

#[test]
fn origin_opj_extension_rejects_an_opju_signature() {
    let path = temp_origin_file("opj", b"CPYUA 4.3668 178\n");
    let mut app = PlotxApp::new_with_settings(plotx_core::settings::Settings::default());

    open_recent_path(&mut app, &path);

    std::fs::remove_file(path).unwrap();
    assert!(app.session.ui.table_import_preview.is_none());
    assert!(app.session.recent_files.is_empty());
    let report = app
        .session
        .operation_history
        .operations()
        .next_back()
        .expect("the extension/signature mismatch must be reported");
    assert_eq!(report.outcome, OperationOutcome::Failure);
    assert!(report.summary.contains("does not match"), "{report:?}");
}

#[test]
fn origin_opju_extension_rejects_an_opj_signature() {
    let path = temp_origin_file("opju", OPENOPJ_FIXTURE);
    let mut app = PlotxApp::new_with_settings(plotx_core::settings::Settings::default());

    open_recent_path(&mut app, &path);

    std::fs::remove_file(path).unwrap();
    assert!(app.session.ui.table_import_preview.is_none());
    assert!(app.session.recent_files.is_empty());
    let report = app
        .session
        .operation_history
        .operations()
        .next_back()
        .expect("the extension/signature mismatch must be reported");
    assert_eq!(report.outcome, OperationOutcome::Failure);
    assert!(report.summary.contains("does not match"), "{report:?}");
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
    let classify = |path: &Path| {
        classify_open_path_with_header(path, OpenPathEntryType::RegularFile, || {
            Ok(([0_u8; 129], 0))
        })
        .unwrap()
    };
    assert_eq!(
        format!("{:?}", classify(Path::new("project.OPJU"))),
        "OriginProject"
    );
    assert_ne!(classify(Path::new("project.opj")), RecentOpenKind::DataFile);
}

#[test]
fn origin_signature_mismatch_becomes_a_user_visible_failure_report() {
    let path = temp_origin_file("opj", b"not an Origin project");
    let mut app = PlotxApp::new_with_settings(plotx_core::settings::Settings::default());

    open_recent_path(&mut app, &path);

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

    open_recent_path(&mut app, &path);

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
    let path = temp_origin_file("opj", OPENOPJ_FIXTURE);
    let mut app = PlotxApp::new_with_settings(plotx_core::settings::Settings::default());
    let mut noted_paths = Vec::new();

    open_recent_path(&mut app, &path);

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

    assert!(
        crate::ui::file_dialogs::commit_table_import_preview_with_recent(&mut app, |app, path| {
            let path = std::path::absolute(path).unwrap();
            noted_paths.push(path.clone());
            app.session.recent_files.push(path);
        },)
    );
    assert_eq!(app.doc.datasets.len(), candidate_count);
    assert_eq!(app.session.recent_files.len(), 1);
    assert_eq!(
        app.session.recent_files[0],
        std::path::absolute(&path).unwrap()
    );
    assert_eq!(noted_paths, app.session.recent_files);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn origin_cancel_leaves_tables_and_recent_files_unchanged() {
    let path = temp_origin_file("opj", OPENOPJ_FIXTURE);
    let mut app = PlotxApp::new_with_settings(plotx_core::settings::Settings::default());
    open_recent_path(&mut app, &path);
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

    assert!(
        crate::ui::file_dialogs::commit_table_import_preview_with_recent(&mut app, |_, _| panic!(
            "a preview without a recent path must not persist settings"
        ),)
    );
    assert_eq!(app.doc.datasets.len(), 2);
    assert!(app.session.recent_files.is_empty());
}
