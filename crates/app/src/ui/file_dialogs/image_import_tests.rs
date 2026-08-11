use super::*;
use plotx_core::state::DEFAULT_CANVAS_SIZE_MM;

fn png_payload() -> Vec<u8> {
    let source = image::RgbaImage::from_pixel(2, 2, image::Rgba([1, 2, 3, 4]));
    let mut cursor = std::io::Cursor::new(Vec::new());
    image::DynamicImage::ImageRgba8(source)
        .write_to(&mut cursor, image::ImageFormat::Png)
        .unwrap();
    cursor.into_inner()
}

fn multipage_tiff_payload() -> Vec<u8> {
    use tiff::encoder::{TiffEncoder, colortype};
    let mut cursor = std::io::Cursor::new(Vec::new());
    {
        let mut encoder = TiffEncoder::new(&mut cursor).unwrap();
        encoder
            .write_image::<colortype::Gray8>(2, 1, &[10, 20])
            .unwrap();
        encoder
            .write_image::<colortype::RGB8>(1, 2, &[1, 2, 3, 4, 5, 6])
            .unwrap();
    }
    cursor.into_inner()
}

fn candidate(name: &str, pixel_size: [u32; 2]) -> Candidate {
    let bytes = png_payload();
    Candidate {
        basename: name.to_owned(),
        sha256: plotx_io::image::sha256(&bytes),
        preview: rebuild_preview(&bytes, 0).unwrap(),
        bytes: Arc::new(bytes),
        format: "png".to_owned(),
        pixel_size,
        auto_page_size_mm: physical_image_size_mm(pixel_size, None),
        page_index: 0,
        warning: None,
    }
}

fn ready_job(app: &mut PlotxApp, target: ImportImageTarget) -> ImportJob {
    let (_events, receiver) = mpsc::channel();
    let (consent, _worker_consent) = mpsc::channel();
    ImportJob {
        request: ImportImageRequest {
            paths: Vec::new(),
            payloads: Vec::new(),
            target,
            allow_first_frame: false,
            strip_metadata: false,
            import_all_tiff_pages: false,
        },
        operation: app.session.begin_operation(),
        state: ImportImageState::ReadyToCommit,
        receiver,
        cancelled: Arc::new(AtomicBool::new(false)),
        consent,
    }
}

#[test]
fn worker_reports_ordered_states_and_a_ready_candidate() {
    let (sender, receiver) = mpsc::channel();
    let (_consent_sender, consent) = mpsc::channel();
    worker(
        Vec::new(),
        vec![("sample.dat".to_owned(), png_payload())],
        Arc::new(AtomicBool::new(false)),
        sender,
        consent,
        ImportOptions {
            allow_first_frame: false,
            strip_metadata: false,
            import_all_tiff_pages: false,
        },
    );
    assert!(matches!(
        receiver.recv().unwrap(),
        WorkerEvent::State(ImportImageState::Probing)
    ));
    assert!(matches!(
        receiver.recv().unwrap(),
        WorkerEvent::State(ImportImageState::ReadyToCommit)
    ));
    let WorkerEvent::Finished(results) = receiver.recv().unwrap() else {
        panic!("expected finished event")
    };
    let candidate = results.into_iter().next().unwrap().ok().unwrap();
    assert_eq!(candidate.pixel_size, [2, 2]);
    assert_eq!(candidate.format, "png");
}

#[test]
fn cancelled_worker_releases_payload_without_a_commit_event() {
    let (sender, receiver) = mpsc::channel();
    let (_consent_sender, consent) = mpsc::channel();
    worker(
        Vec::new(),
        vec![("sample.png".to_owned(), png_payload())],
        Arc::new(AtomicBool::new(true)),
        sender,
        consent,
        ImportOptions {
            allow_first_frame: false,
            strip_metadata: false,
            import_all_tiff_pages: false,
        },
    );
    assert!(matches!(
        receiver.recv().unwrap(),
        WorkerEvent::State(ImportImageState::Probing)
    ));
    assert!(matches!(
        receiver.recv().unwrap(),
        WorkerEvent::State(ImportImageState::Cancelled)
    ));
    assert!(receiver.try_recv().is_err());
}

#[test]
fn disconnected_worker_is_removed_and_reported() {
    let mut app = PlotxApp::default();
    let job = ready_job(&mut app, ImportImageTarget::NewPages);
    MANAGER.with(|manager| manager.borrow_mut().jobs.push(job));

    poll(&mut app, &egui::Context::default());

    assert!(!has_active_jobs());
    let report = app
        .session
        .operation_history
        .operations()
        .next_back()
        .expect("the disconnected worker must produce an operation report");
    assert_eq!(report.kind, OperationKind::ImageImport);
    assert!(
        report
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == DiagnosticCode::ImageImportFailed)
    );
}

#[test]
fn all_tiff_pages_share_original_bytes_and_keep_distinct_page_indices() {
    let (_sender_guard, sender) = mpsc::channel::<bool>();
    let (events, _receiver) = mpsc::channel();
    let candidates = prepare_bytes(
        "stack.tiff".to_owned(),
        multipage_tiff_payload(),
        &events,
        &sender,
        &AtomicBool::new(false),
        ImportOptions {
            allow_first_frame: false,
            strip_metadata: false,
            import_all_tiff_pages: true,
        },
    )
    .unwrap();
    assert_eq!(candidates.len(), 2);
    assert_eq!(candidates[0].page_index, 0);
    assert_eq!(candidates[1].page_index, 1);
    assert_eq!(candidates[1].pixel_size, [1, 2]);
    assert!(Arc::ptr_eq(&candidates[0].bytes, &candidates[1].bytes));
}

#[test]
fn successful_import_into_empty_project_creates_one_undoable_page() {
    let mut app = PlotxApp::default();
    assert!(app.doc.canvases.is_empty());
    let bytes = png_payload();
    let preview = rebuild_preview(&bytes, 0).unwrap();
    let candidate = Candidate {
        basename: "dropped.png".to_owned(),
        sha256: plotx_io::image::sha256(&bytes),
        bytes: Arc::new(bytes),
        format: "png".to_owned(),
        pixel_size: [2, 2],
        auto_page_size_mm: physical_image_size_mm([2, 2], None),
        page_index: 0,
        warning: None,
        preview,
    };
    let (_events, receiver) = mpsc::channel();
    let (consent, _worker_consent) = mpsc::channel();
    let mut job = ImportJob {
        request: ImportImageRequest {
            paths: Vec::new(),
            payloads: Vec::new(),
            target: ImportImageTarget::NewPages,
            allow_first_frame: false,
            strip_metadata: false,
            import_all_tiff_pages: false,
        },
        operation: app.session.begin_operation(),
        state: ImportImageState::ReadyToCommit,
        receiver,
        cancelled: Arc::new(AtomicBool::new(false)),
        consent,
    };

    commit(&mut app, &mut job, vec![Ok(candidate)]);
    assert_eq!(app.doc.canvases.len(), 1);
    assert_eq!(app.doc.canvases[0].objects.len(), 1);
    assert_eq!(app.doc.assets.len(), 1);
    assert_eq!(app.session.active_canvas, Some(0));
    assert_eq!(
        app.doc.canvases[0].objects[0].frame,
        ObjectFrame::new(
            0.0,
            0.0,
            app.doc.canvases[0].size_pt()[0],
            app.doc.canvases[0].size_pt()[1],
        )
    );

    let path = std::env::temp_dir().join(format!(
        "plotx-empty-project-image-import-{}.plotx",
        uuid::Uuid::new_v4()
    ));
    plotx_core::project::save_project(&app, &path, false).unwrap();
    let loaded = plotx_core::project::load_project(&path).unwrap();
    std::fs::remove_file(path).unwrap();
    assert_eq!(loaded.doc.canvases.len(), 1);
    assert_eq!(loaded.doc.canvases[0].objects.len(), 1);
    assert_eq!(loaded.doc.assets.len(), 1);

    app.undo();
    assert!(app.doc.canvases.is_empty());
    assert!(app.doc.assets.is_empty());
    assert_eq!(app.session.active_canvas, None);

    app.redo();
    assert_eq!(app.doc.canvases.len(), 1);
    assert_eq!(app.doc.canvases[0].objects.len(), 1);
    assert_eq!(app.doc.assets.len(), 1);
}

#[test]
fn multiple_images_into_empty_project_create_distinct_pages_in_source_order() {
    let mut app = PlotxApp::default();
    let make_candidate = |name: &str, pixel_size: [u32; 2]| {
        let bytes = png_payload();
        Candidate {
            basename: name.to_owned(),
            sha256: plotx_io::image::sha256(&bytes),
            preview: rebuild_preview(&bytes, 0).unwrap(),
            bytes: Arc::new(bytes),
            format: "png".to_owned(),
            pixel_size,
            auto_page_size_mm: physical_image_size_mm(pixel_size, None),
            page_index: 0,
            warning: None,
        }
    };
    let (_events, receiver) = mpsc::channel();
    let (consent, _worker_consent) = mpsc::channel();
    let mut job = ImportJob {
        request: ImportImageRequest {
            paths: Vec::new(),
            payloads: Vec::new(),
            target: ImportImageTarget::NewPages,
            allow_first_frame: false,
            strip_metadata: false,
            import_all_tiff_pages: false,
        },
        operation: app.session.begin_operation(),
        state: ImportImageState::ReadyToCommit,
        receiver,
        cancelled: Arc::new(AtomicBool::new(false)),
        consent,
    };
    commit(
        &mut app,
        &mut job,
        vec![
            Ok(make_candidate("first.png", [400, 200])),
            Ok(make_candidate("second.png", [200, 400])),
        ],
    );
    assert_eq!(app.doc.canvases.len(), 2);
    assert_eq!(app.doc.canvases[0].objects[0].name, "first");
    assert_eq!(app.doc.canvases[1].objects[0].name, "second");
    assert_ne!(app.doc.canvases[0].size_mm, app.doc.canvases[1].size_mm);
    assert_eq!(app.session.undo_stack.len(), 1);
    assert_eq!(
        app.doc.canvases[0]
            .panel_letter(app.doc.canvases[0].objects[0].id)
            .as_deref(),
        Some("a")
    );
    app.undo();
    assert!(app.doc.canvases.is_empty());
    app.redo();
    assert_eq!(app.doc.canvases.len(), 2);
}

#[test]
fn sequential_new_page_batches_never_retarget_the_active_imported_page() {
    let mut app = PlotxApp::default();
    app.doc.canvases.push(CanvasDocument::new(
        "Existing".to_owned(),
        DEFAULT_CANVAS_SIZE_MM,
    ));
    app.session.active_canvas = Some(0);

    let mut first = ready_job(&mut app, ImportImageTarget::NewPages);
    commit(
        &mut app,
        &mut first,
        vec![
            Ok(candidate("first.png", [400, 200])),
            Ok(candidate("second.png", [200, 400])),
        ],
    );
    assert_eq!(app.doc.canvases.len(), 3);
    assert_eq!(app.session.active_canvas, Some(2));
    assert!(app.doc.canvases[0].objects.is_empty());
    assert_eq!(app.session.undo_stack.len(), 1);

    let mut second = ready_job(&mut app, ImportImageTarget::NewPages);
    commit(
        &mut app,
        &mut second,
        vec![Ok(candidate("third.png", [300, 300]))],
    );
    assert_eq!(app.doc.canvases.len(), 4);
    assert_eq!(app.session.active_canvas, Some(3));
    assert_eq!(app.doc.canvases[2].objects.len(), 1);
    assert_eq!(app.doc.canvases[3].objects[0].name, "third");
    assert_eq!(app.session.undo_stack.len(), 2);

    app.undo();
    assert_eq!(app.doc.canvases.len(), 3);
    assert_eq!(app.session.active_canvas, Some(2));
    app.undo();
    assert_eq!(app.doc.canvases.len(), 1);
    assert_eq!(app.session.active_canvas, Some(0));
    app.redo();
    app.redo();
    assert_eq!(app.doc.canvases.len(), 4);
    assert_eq!(app.session.active_canvas, Some(3));
}

#[test]
fn panel_target_that_locks_before_commit_fails_without_loose_content() {
    let mut app = PlotxApp::default();
    let mut page = CanvasDocument::new("Existing".to_owned(), DEFAULT_CANVAS_SIZE_MM);
    let panel = page.create_panel(
        "Panel".to_owned(),
        ObjectFrame::new(10.0, 10.0, 100.0, 80.0),
    );
    let canvas = page.resource_id;
    app.doc.canvases.push(page);
    app.session.active_canvas = Some(0);
    let mut job = ready_job(
        &mut app,
        ImportImageTarget::Panel {
            canvas,
            panel,
            position: Some([20.0, 20.0]),
        },
    );
    app.doc.canvases[0].panel_mut(panel).unwrap().locked = true;

    commit(
        &mut app,
        &mut job,
        vec![Ok(candidate("late.png", [200, 100]))],
    );

    assert_eq!(job.state, ImportImageState::Cancelled);
    assert!(app.doc.canvases[0].objects.is_empty());
    assert!(app.doc.assets.is_empty());
    assert!(app.session.undo_stack.is_empty());
    assert!(app.session.status.contains("failed"));
}

#[test]
fn failed_import_into_empty_project_does_not_create_a_page_or_undo_step() {
    let mut app = PlotxApp::default();
    let (_events, receiver) = mpsc::channel();
    let (consent, _worker_consent) = mpsc::channel();
    let mut job = ImportJob {
        request: ImportImageRequest {
            paths: Vec::new(),
            payloads: Vec::new(),
            target: ImportImageTarget::NewPages,
            allow_first_frame: false,
            strip_metadata: false,
            import_all_tiff_pages: false,
        },
        operation: app.session.begin_operation(),
        state: ImportImageState::ReadyToCommit,
        receiver,
        cancelled: Arc::new(AtomicBool::new(false)),
        consent,
    };
    commit(
        &mut app,
        &mut job,
        vec![Err(failure(
            "broken.png",
            "PNG",
            "decode",
            "truncated payload".to_owned(),
            "Choose another image.",
        ))],
    );
    assert!(app.doc.canvases.is_empty());
    assert!(app.doc.assets.is_empty());
    assert_eq!(job.state, ImportImageState::Failed);
    app.undo();
    assert!(app.doc.canvases.is_empty());
}

#[test]
fn initial_image_size_uses_page_points_and_preserves_aspect() {
    let size = initial_image_size([510.0, 340.0], 2.0);
    assert_eq!(size, [331.5, 165.75]);
    let portrait = initial_image_size([510.0, 340.0], 0.5);
    assert!((portrait[0] - 110.5).abs() < 1e-4);
    assert!((portrait[1] - 221.0).abs() < 1e-4);
}

#[test]
fn automatic_page_uses_physical_size_and_caps_width_at_nature_single_column() {
    let sized = physical_image_size_mm([600, 300], Some([300.0, 300.0]));
    assert!((sized[0] - 50.8).abs() < 1e-4);
    assert!((sized[1] - 25.4).abs() < 1e-4);

    let capped = physical_image_size_mm([2400, 1200], None);
    assert!((capped[0] - 89.0).abs() < 1e-4);
    assert!((capped[1] - 44.5).abs() < 1e-4);

    let portrait = physical_image_size_mm([1200, 2400], None);
    assert!((portrait[0] - 89.0).abs() < 1e-4);
    assert!((portrait[1] - 178.0).abs() < 1e-4);
}
