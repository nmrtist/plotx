use plotx_core::actions::{Action, PanelState};
use plotx_core::operation::{
    Diagnostic, DiagnosticCode, OperationId, OperationKind, OperationReport, Severity,
};
use plotx_core::state::{
    AssetId, AssetRecord, CanvasDocument, CanvasId, CanvasObject, CanvasObjectKind, ContentId,
    ObjectFrame, PanelId, PlotxApp, RasterImageContent,
};
use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc};

mod consent;
pub(crate) use consent::large_image_consent_window;
mod clipboard;
pub(crate) use clipboard::paste_clipboard_image;
mod geometry;
use geometry::{initial_image_size, physical_image_size_mm};
mod jobs;
pub(crate) use jobs::poll;
mod new_pages;
mod picker;
pub(crate) use picker::{
    import_image_paths, import_images, import_images_first_frame, import_images_without_metadata,
    import_tiff_pages, replace_selected_image,
};
mod proxy;
use proxy::{insert_proxy, rebuild_preview};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ImportImageState {
    Queued,
    Probing,
    AwaitingLargeImageConsent,
    DecodingProxy,
    ReadyToCommit,
    Committed,
    Failed,
    Cancelled,
}

#[derive(Clone, Debug)]
pub(crate) struct ImportImageRequest {
    pub paths: Vec<PathBuf>,
    pub payloads: Vec<(String, Vec<u8>)>,
    pub target: ImportImageTarget,
    pub allow_first_frame: bool,
    pub strip_metadata: bool,
    pub import_all_tiff_pages: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum ImportImageTarget {
    NewPages,
    Canvas {
        canvas: CanvasId,
        position: Option<[f32; 2]>,
    },
    Panel {
        canvas: CanvasId,
        panel: PanelId,
        position: Option<[f32; 2]>,
    },
    Replace {
        canvas: CanvasId,
        content: ContentId,
    },
}

struct Candidate {
    basename: String,
    bytes: Arc<Vec<u8>>,
    sha256: [u8; 32],
    format: String,
    pixel_size: [u32; 2],
    auto_page_size_mm: [f32; 2],
    page_index: u32,
    warning: Option<String>,
    preview: plotx_io::image::ProxyImage,
}

#[derive(Clone, Copy)]
struct ImportOptions {
    allow_first_frame: bool,
    strip_metadata: bool,
    import_all_tiff_pages: bool,
}

#[derive(Debug)]
struct Failure {
    basename: String,
    detected_format: String,
    stage: &'static str,
    reason: String,
    remedy: &'static str,
}

enum WorkerEvent {
    State(ImportImageState),
    Finished(Vec<Result<Candidate, Failure>>),
}

struct ImportJob {
    request: ImportImageRequest,
    operation: OperationId,
    state: ImportImageState,
    receiver: mpsc::Receiver<WorkerEvent>,
    cancelled: Arc<AtomicBool>,
    consent: mpsc::Sender<bool>,
}

#[derive(Default)]
struct ImageImportManager {
    jobs: Vec<ImportJob>,
}

thread_local! {
    static MANAGER: RefCell<ImageImportManager> = RefCell::new(ImageImportManager::default());
}

pub(crate) fn enqueue(app: &mut PlotxApp, request: ImportImageRequest) {
    let operation = app.session.begin_operation();
    let (sender, receiver) = mpsc::channel();
    let (consent, worker_consent) = mpsc::channel();
    let cancelled = Arc::new(AtomicBool::new(false));
    let worker_cancelled = Arc::clone(&cancelled);
    let paths = request.paths.clone();
    let payloads = request.payloads.clone();
    let options = ImportOptions {
        allow_first_frame: request.allow_first_frame,
        strip_metadata: request.strip_metadata,
        import_all_tiff_pages: request.import_all_tiff_pages,
    };
    std::thread::spawn(move || {
        worker(
            paths,
            payloads,
            worker_cancelled,
            sender,
            worker_consent,
            options,
        )
    });
    MANAGER.with(|manager| {
        manager.borrow_mut().jobs.push(ImportJob {
            request,
            operation,
            state: ImportImageState::Queued,
            receiver,
            cancelled,
            consent,
        });
    });
    app.session.status = "Image import queued.".to_owned();
}

pub(crate) fn has_active_jobs() -> bool {
    MANAGER.with(|manager| !manager.borrow().jobs.is_empty())
}

pub(crate) fn cancel_all(app: &mut PlotxApp) {
    MANAGER.with(|manager| {
        for job in &mut manager.borrow_mut().jobs {
            job.cancelled.store(true, Ordering::Relaxed);
            let _ = job.consent.send(false);
            job.state = ImportImageState::Cancelled;
        }
    });
    app.session.status = "Cancelling image import…".to_owned();
}

fn worker(
    paths: Vec<PathBuf>,
    payloads: Vec<(String, Vec<u8>)>,
    cancelled: Arc<AtomicBool>,
    sender: mpsc::Sender<WorkerEvent>,
    consent: mpsc::Receiver<bool>,
    options: ImportOptions,
) {
    let _ = sender.send(WorkerEvent::State(ImportImageState::Probing));
    let mut results = Vec::with_capacity(paths.len());
    for path in paths {
        if cancelled.load(Ordering::Relaxed) {
            let _ = sender.send(WorkerEvent::State(ImportImageState::Cancelled));
            return;
        }
        match prepare_path(&path, &sender, &consent, &cancelled, options) {
            Ok(candidates) => results.extend(candidates.into_iter().map(Ok)),
            Err(error) => results.push(Err(error)),
        }
        if cancelled.load(Ordering::Relaxed) {
            let _ = sender.send(WorkerEvent::State(ImportImageState::Cancelled));
            return;
        }
    }
    for (name, bytes) in payloads {
        if cancelled.load(Ordering::Relaxed) {
            let _ = sender.send(WorkerEvent::State(ImportImageState::Cancelled));
            return;
        }
        match prepare_bytes(name, bytes, &sender, &consent, &cancelled, options) {
            Ok(candidates) => results.extend(candidates.into_iter().map(Ok)),
            Err(error) => results.push(Err(error)),
        }
        if cancelled.load(Ordering::Relaxed) {
            let _ = sender.send(WorkerEvent::State(ImportImageState::Cancelled));
            return;
        }
    }
    let _ = sender.send(WorkerEvent::State(ImportImageState::ReadyToCommit));
    let _ = sender.send(WorkerEvent::Finished(results));
}

fn prepare_path(
    path: &Path,
    sender: &mpsc::Sender<WorkerEvent>,
    consent: &mpsc::Receiver<bool>,
    cancelled: &AtomicBool,
    options: ImportOptions,
) -> Result<Vec<Candidate>, Failure> {
    let basename = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("<unnamed>")
        .to_owned();
    let bytes = std::fs::read(path).map_err(|error| {
        failure(
            &basename,
            "unknown",
            "read",
            error.to_string(),
            "Check file permissions and retry.",
        )
    })?;
    prepare_bytes(basename, bytes, sender, consent, cancelled, options)
}

fn prepare_bytes(
    basename: String,
    bytes: Vec<u8>,
    sender: &mpsc::Sender<WorkerEvent>,
    consent: &mpsc::Receiver<bool>,
    cancelled: &AtomicBool,
    options: ImportOptions,
) -> Result<Vec<Candidate>, Failure> {
    let detected = plotx_io::image::sniff(&bytes).name().to_owned();
    let source_dpi = plotx_io::image::metadata_dpi(&bytes);
    let probe = plotx_io::image::probe(&bytes).map_err(|error| {
        failure(
            &basename,
            &detected,
            "probe",
            error.to_string(),
            "Choose a supported PNG, JPEG, TIFF, WebP, or BMP file.",
        )
    })?;
    if options.import_all_tiff_pages && probe.format != plotx_io::image::RasterFormat::Tiff {
        return Err(failure(
            &basename,
            &detected,
            "probe_pages",
            "the all-pages command accepts TIFF sources only".to_owned(),
            "Choose Add Images… for PNG, JPEG, WebP, or BMP sources.",
        ));
    }
    if probe.animated && !options.allow_first_frame {
        return Err(failure(
            &basename,
            &detected,
            "probe",
            "animated images are not supported by this import path".to_owned(),
            "Export the intended frame as a static PNG, JPEG, TIFF, WebP, or BMP and retry.",
        ));
    }
    if probe.class == plotx_io::image::ResourceClass::Rejected {
        return Err(failure(
            &basename,
            &detected,
            "limits",
            format!(
                "{} × {} pixels and {} decoded bytes exceed the hard limit",
                probe.width, probe.height, probe.decoded_bytes
            ),
            "Reduce the image dimensions before importing.",
        ));
    }
    let bytes = if options.strip_metadata {
        plotx_io::image::strip_metadata_to_png(&bytes, options.allow_first_frame).map_err(
            |error| {
                failure(
                    &basename,
                    &detected,
                    "strip_metadata",
                    error.to_string(),
                    "Use the normal import path for a large image, or reduce its dimensions first.",
                )
            },
        )?
    } else {
        bytes
    };
    let mut notices = Vec::new();
    if options.strip_metadata {
        notices.push(
            "Metadata was removed explicitly and the embedded pixels were re-encoded as PNG.",
        );
    }
    if probe.animated {
        notices.push(
            "Only the first animation frame is displayed; original animated bytes remain embedded.",
        );
    }
    if probe.high_precision {
        notices.push(
            "High-precision source is displayed as RGBA8 sRGB; original bytes remain embedded.",
        );
    }
    if probe.has_icc || probe.has_exif {
        notices.push("ICC/EXIF metadata is preserved in the embedded original bytes.");
    }
    if probe.pages > 1 && !options.import_all_tiff_pages {
        notices.push("Multi-page TIFF detected; only page one is imported by this command. Use Add All TIFF Pages… to create one image per page.");
    }
    let preview = if probe.class == plotx_io::image::ResourceClass::ProxyRequired {
        let _ = sender.send(WorkerEvent::State(
            ImportImageState::AwaitingLargeImageConsent,
        ));
        match consent.recv() {
            Ok(true) => {}
            Ok(false) | Err(_) => {
                cancelled.store(true, Ordering::Relaxed);
                return Err(failure(
                    &basename,
                    &detected,
                    "consent",
                    "large-image import was cancelled".to_owned(),
                    "Choose the image again if you want to import it.",
                ));
            }
        }
        let _ = sender.send(WorkerEvent::State(ImportImageState::DecodingProxy));
        let proxy = plotx_io::image::decode_proxy_rgba8(&bytes, 2048).map_err(|error| {
            failure(
                &basename,
                &detected,
                "proxy_decode",
                error.to_string(),
                "Reduce the image dimensions below the 500 megapixel hard limit and retry.",
            )
        })?;
        notices.push("Large image embedded; a bounded proxy is used for interactive preview.");
        proxy
    } else {
        rebuild_preview(&bytes, 0).map_err(|error| {
            failure(
                &basename,
                &detected,
                "decode",
                error.to_string(),
                "Retry or re-encode the image in a supported format.",
            )
        })?
    };
    let warning = (!notices.is_empty()).then(|| notices.join(" "));
    let sha256 = plotx_io::image::sha256(&bytes);
    let bytes = Arc::new(bytes);
    let format = if options.strip_metadata {
        "png".to_owned()
    } else {
        probe.format.name().to_ascii_lowercase()
    };
    let mut candidates = vec![Candidate {
        basename,
        bytes,
        sha256,
        format,
        pixel_size: [probe.width, probe.height],
        auto_page_size_mm: physical_image_size_mm([probe.width, probe.height], source_dpi),
        page_index: 0,
        warning,
        preview,
    }];
    if options.import_all_tiff_pages && probe.format == plotx_io::image::RasterFormat::Tiff {
        for page_index in 1..probe.pages {
            let decoded = plotx_io::image::decode_rgba8_page(
                candidates[0].bytes.as_slice(),
                page_index,
                false,
            )
            .map_err(|error| {
                failure(
                    &candidates[0].basename,
                    "TIFF",
                    "decode_page",
                    error.to_string(),
                    "Re-encode the failing TIFF page as a standalone PNG and retry.",
                )
            })?;
            candidates.push(Candidate {
                basename: format!(
                    "{} — page {}",
                    Path::new(&candidates[0].basename)
                        .file_stem()
                        .and_then(|name| name.to_str())
                        .unwrap_or("Image"),
                    page_index + 1
                ),
                bytes: Arc::clone(&candidates[0].bytes),
                sha256,
                format: "tiff".to_owned(),
                pixel_size: [decoded.probe.width, decoded.probe.height],
                auto_page_size_mm: physical_image_size_mm(
                    [decoded.probe.width, decoded.probe.height],
                    source_dpi,
                ),
                page_index,
                warning: decoded.probe.high_precision.then(|| {
                    "High-precision TIFF page is displayed as RGBA8 sRGB; original bytes remain embedded."
                        .to_owned()
                }),
                preview: rebuild_preview(candidates[0].bytes.as_slice(), page_index).map_err(
                    |error| {
                        failure(
                            &candidates[0].basename,
                            "TIFF",
                            "preview_page",
                            error.to_string(),
                            "Re-encode the failing TIFF page as a standalone PNG and retry.",
                        )
                    },
                )?,
            });
        }
    }
    Ok(candidates)
}

fn commit(app: &mut PlotxApp, job: &mut ImportJob, results: Vec<Result<Candidate, Failure>>) {
    if job.cancelled.load(Ordering::Relaxed) {
        job.state = ImportImageState::Cancelled;
        return;
    }
    if job.request.target == ImportImageTarget::NewPages {
        new_pages::commit(app, job, results);
        return;
    }

    let target_canvas = match job.request.target {
        ImportImageTarget::Canvas { canvas, .. }
        | ImportImageTarget::Panel { canvas, .. }
        | ImportImageTarget::Replace { canvas, .. } => canvas,
        ImportImageTarget::NewPages => unreachable!("new-page imports returned above"),
    };
    let Some(ci) = app.doc.canvas_index(target_canvas) else {
        job.state = ImportImageState::Cancelled;
        record_failures(
            app,
            job.operation,
            0,
            vec![failure(
                "<batch>",
                "unknown",
                "commit",
                "target page was deleted".to_owned(),
                "Choose a page and import the images again.",
            )],
        );
        return;
    };
    let (target_panel, drop_position, replace_content) = match job.request.target {
        ImportImageTarget::Canvas { position, .. } => (None, position, None),
        ImportImageTarget::Panel {
            panel, position, ..
        } => {
            let Some(target) = app.doc.canvases[ci].panel(panel) else {
                fail_stale_panel(app, job, "target panel was deleted");
                return;
            };
            if target.locked {
                fail_stale_panel(app, job, "target panel became locked");
                return;
            }
            (Some(panel), position, None)
        }
        ImportImageTarget::Replace { content, .. } => (None, None, Some(content)),
        ImportImageTarget::NewPages => unreachable!("new-page imports returned above"),
    };
    let page_before = app.doc.canvases[ci].clone();
    let before = PanelState::of(&page_before);
    let mut page = page_before.clone();
    let mut failures = Vec::new();
    let mut imported = 0usize;
    let mut warnings = Vec::new();
    let mut staged_assets: Vec<AssetRecord> = Vec::new();
    for result in results {
        let candidate = match result {
            Ok(candidate) => candidate,
            Err(error) => {
                failures.push(error);
                continue;
            }
        };
        let asset = app
            .doc
            .assets
            .iter()
            .find_map(|(id, asset)| (asset.sha256 == candidate.sha256).then_some(*id))
            .or_else(|| {
                staged_assets
                    .iter()
                    .find_map(|asset| (asset.sha256 == candidate.sha256).then_some(asset.id))
            })
            .unwrap_or_else(AssetId::new);
        if let Some(content) = replace_content {
            if let Some(item) = page.object_mut(content)
                && let CanvasObjectKind::RasterImage(image) = &mut item.kind
            {
                image.asset = asset;
                image.page_index = candidate.page_index;
                if !app.doc.assets.contains_key(&asset)
                    && !staged_assets.iter().any(|record| record.id == asset)
                {
                    staged_assets.push(AssetRecord {
                        id: asset,
                        sha256: candidate.sha256,
                        format: candidate.format.clone(),
                        pixel_size: candidate.pixel_size,
                        bytes: candidate.bytes.as_ref().clone(),
                    });
                }
                insert_proxy(
                    app,
                    candidate.sha256,
                    candidate.page_index,
                    candidate.preview,
                );
                imported += 1;
                if let Some(warning) = candidate.warning {
                    warnings.push((candidate.basename, candidate.format, warning));
                }
                continue;
            }
            failures.push(failure(
                &candidate.basename,
                &candidate.format,
                "commit",
                "the image being replaced no longer exists".to_owned(),
                "Select an image and choose Replace Image again.",
            ));
            continue;
        }
        let id = page.allocate_object_id();
        let aspect = candidate.pixel_size[0] as f32 / candidate.pixel_size[1].max(1) as f32;
        let [width, height] = initial_image_size(page.size_pt(), aspect);
        let base = drop_position.unwrap_or([12.0, 12.0]);
        let offset = imported as f32 * 6.0;
        let mut frame = ObjectFrame::new(base[0] + offset, base[1] + offset, width, height);
        if let Some(panel) = target_panel.and_then(|panel| page.panel(panel).cloned()) {
            frame.x -= panel.frame.x;
            frame.y -= panel.frame.y;
        }
        page.objects.push(CanvasObject {
            id,
            name: Path::new(&candidate.basename)
                .file_stem()
                .and_then(|name| name.to_str())
                .unwrap_or("Image")
                .to_owned(),
            frame,
            locked: false,
            visible: true,
            kind: CanvasObjectKind::RasterImage({
                let mut image = RasterImageContent::new(asset);
                image.page_index = candidate.page_index;
                image
            }),
        });
        if let Some(panel) = target_panel.and_then(|panel| page.panel_mut(panel)) {
            panel.item_order.push(id);
        }
        if !app.doc.assets.contains_key(&asset)
            && !staged_assets.iter().any(|record| record.id == asset)
        {
            staged_assets.push(AssetRecord {
                id: asset,
                sha256: candidate.sha256,
                format: candidate.format.clone(),
                pixel_size: candidate.pixel_size,
                bytes: candidate.bytes.as_ref().clone(),
            });
        }
        insert_proxy(
            app,
            candidate.sha256,
            candidate.page_index,
            candidate.preview,
        );
        if let Some(warning) = candidate.warning {
            warnings.push((candidate.basename, candidate.format, warning));
        }
        imported += 1;
    }
    if imported > 0 {
        let mut actions = Vec::new();
        actions.extend(staged_assets.into_iter().map(|asset| Action::SetAsset {
            id: asset.id,
            before: None,
            after: Some(asset),
        }));
        actions.push(Action::ReplacePanelState {
            canvas: ci,
            before,
            after: PanelState::of(&page),
        });
        match app.try_execute_action(Action::Composite(actions)) {
            Ok(()) => job.state = ImportImageState::Committed,
            Err(error) => {
                imported = 0;
                job.state = ImportImageState::Failed;
                failures.push(failure(
                    "<batch>",
                    "unknown",
                    "commit",
                    error.to_string(),
                    "Retry the import; if it still fails, review the diagnostic history.",
                ));
            }
        }
    } else {
        job.state = ImportImageState::Failed;
    }
    record_result(app, job.operation, imported, failures, warnings);
}

fn fail_stale_panel(app: &mut PlotxApp, job: &mut ImportJob, reason: &str) {
    job.state = ImportImageState::Cancelled;
    record_failures(
        app,
        job.operation,
        0,
        vec![failure(
            "<batch>",
            "unknown",
            "commit",
            reason.to_owned(),
            "Select an unlocked Panel and import the images again.",
        )],
    );
}

fn failure(
    basename: &str,
    detected: &str,
    stage: &'static str,
    reason: String,
    remedy: &'static str,
) -> Failure {
    Failure {
        basename: basename.to_owned(),
        detected_format: detected.to_owned(),
        stage,
        reason,
        remedy,
    }
}

fn record_result(
    app: &mut PlotxApp,
    operation: OperationId,
    imported: usize,
    failures: Vec<Failure>,
    warnings: Vec<(String, String, String)>,
) {
    let failed = failures.len();
    let summary = format!("Imported {imported} image(s); {failed} failed.");
    if imported == 0 {
        record_failures(app, operation, imported, failures);
        return;
    }
    let mut report = if failed == 0 && warnings.is_empty() {
        OperationReport::success(operation, OperationKind::ImageImport, summary, ())
    } else {
        OperationReport::warning(operation, OperationKind::ImageImport, summary, ())
    };
    for failure in failures {
        report = report.with_diagnostic(diagnostic(failure));
    }
    for (file, detected_format, warning) in warnings {
        report = report.with_diagnostic(
            Diagnostic::new(
                Severity::Warning,
                DiagnosticCode::ImageImportWarning,
                warning,
            )
            .with_source("app.image_import")
            .with_context("file", file)
            .with_context("detected_format", detected_format)
            .with_context("stage", "proxy_or_commit")
            .with_context("reason", "safe fallback used")
            .with_context(
                "next_step",
                "Review the placed image and retry with a smaller source if needed.",
            ),
        );
    }
    app.session.record_operation(report);
}

fn record_failures(
    app: &mut PlotxApp,
    operation: OperationId,
    imported: usize,
    failures: Vec<Failure>,
) {
    let mut failures = failures.into_iter();
    let first = failures.next().unwrap_or_else(|| {
        failure(
            "<batch>",
            "unknown",
            "commit",
            "no image was ready to commit".to_owned(),
            "Choose another image and retry.",
        )
    });
    let mut report = OperationReport::<()>::failure(
        operation,
        OperationKind::ImageImport,
        format!("Imported {imported} image(s); import failed."),
        diagnostic(first),
    );
    for failure in failures {
        report = report.with_diagnostic(diagnostic(failure));
    }
    app.session.record_operation(report);
}

fn diagnostic(error: Failure) -> Diagnostic {
    Diagnostic::new(
        Severity::Error,
        DiagnosticCode::ImageImportFailed,
        "Image import failed; the document and undo history were not changed for this file.",
    )
    .with_source("app.image_import")
    .with_context("file", error.basename)
    .with_context("detected_format", error.detected_format)
    .with_context("stage", error.stage)
    .with_context("reason", error.reason)
    .with_context("next_step", error.remedy)
}

#[cfg(test)]
#[path = "image_import_tests.rs"]
mod tests;
