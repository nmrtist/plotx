use std::cell::RefCell;
use std::path::PathBuf;
#[cfg(windows)]
use std::sync::atomic::{AtomicPtr, Ordering};
use std::time::Duration;

use egui::{Color32, Stroke, StrokeKind};
use plotx_core::state::PlotxApp;

use super::canvas;
use super::file_dialogs;
use super::file_dialogs::image_import::{ImportImageRequest, ImportImageTarget, enqueue};

const DROP_FALLBACK_SETTLE_SECONDS: f64 = 0.5;
const DROP_INCOMPLETE_TIMEOUT_SECONDS: f64 = 2.0;

#[cfg(windows)]
static NATIVE_WINDOW: AtomicPtr<core::ffi::c_void> = AtomicPtr::new(std::ptr::null_mut());

#[cfg(windows)]
pub(crate) fn register_native_window(
    cc: &eframe::CreationContext<'_>,
) -> Option<windows_sys::Win32::Foundation::HWND> {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};

    let Ok(handle) = cc.window_handle() else {
        log::warn!("could not register the native window for external file-drop hit testing");
        return None;
    };
    let RawWindowHandle::Win32(win32) = handle.as_raw() else {
        log::warn!("the native window is not a Win32 window; file-drop hit testing is unavailable");
        return None;
    };
    let window = win32.hwnd.get() as *mut core::ffi::c_void;
    NATIVE_WINDOW.store(window, Ordering::Relaxed);
    Some(window)
}

#[derive(Default)]
struct FileDropBatch {
    paths: Vec<PathBuf>,
    target: Option<ImportImageTarget>,
    expected_path_count: Option<usize>,
    ready_at: Option<f64>,
    hover_paths: Vec<PathBuf>,
    hover_target: Option<ImportImageTarget>,
}

impl FileDropBatch {
    fn observe_hover(
        &mut self,
        hovered_paths: &[PathBuf],
        dropping: bool,
        target: Option<ImportImageTarget>,
    ) {
        if !hovered_paths.is_empty() && self.paths.is_empty() {
            self.hover_paths.clear();
            self.hover_paths.extend_from_slice(hovered_paths);
            self.hover_target = target;
        } else if !dropping && self.paths.is_empty() {
            self.hover_paths.clear();
            self.hover_target = None;
        }
    }

    fn push(&mut self, paths: Vec<PathBuf>, now: f64) {
        if self.paths.is_empty() {
            self.target = Some(self.hover_target.unwrap_or(ImportImageTarget::NewPages));
            // Windows reports one DroppedFile event per path. The hover list
            // gives the physical gesture's batch size before winit clears it.
            self.expected_path_count =
                (!self.hover_paths.is_empty()).then_some(self.hover_paths.len().max(paths.len()));
        }
        self.paths.extend(paths);
        let settle_seconds = if self.expected_path_count.is_some() {
            DROP_INCOMPLETE_TIMEOUT_SECONDS
        } else {
            DROP_FALLBACK_SETTLE_SECONDS
        };
        self.ready_at = Some(now + settle_seconds);
    }

    fn take_ready(&mut self, now: f64) -> Option<(Vec<PathBuf>, ImportImageTarget)> {
        let received_expected_paths = self
            .expected_path_count
            .is_some_and(|expected| self.paths.len() >= expected);
        if !received_expected_paths && self.ready_at.is_none_or(|ready_at| now < ready_at) {
            return None;
        }
        self.ready_at = None;
        self.expected_path_count = None;
        self.hover_paths.clear();
        self.hover_target = None;
        let paths = std::mem::take(&mut self.paths);
        let target = self.target.take().unwrap_or(ImportImageTarget::NewPages);
        Some((paths, target))
    }
}

thread_local! {
    static FILE_DROP_BATCH: RefCell<FileDropBatch> = RefCell::new(FileDropBatch::default());
}

pub(super) fn handle(app: &mut PlotxApp, ctx: &egui::Context) {
    let board_rect =
        ctx.data(|data| data.get_temp::<egui::Rect>(egui::Id::new("plotx.canvas.navigation_rect")));
    let (hovered_paths, dropped, pointer, now) = ctx.input(|input| {
        (
            input
                .raw
                .hovered_files
                .iter()
                .filter_map(|file| file.path.clone())
                .collect::<Vec<_>>(),
            input
                .raw
                .dropped_files
                .iter()
                .filter_map(|file| file.path.clone())
                .collect::<Vec<_>>(),
            input.pointer.hover_pos(),
            input.time,
        )
    });
    let hovered = !hovered_paths.is_empty();
    let pointer = external_file_pointer(ctx, hovered, pointer);
    let hover_target = pointer.and_then(|pointer| {
        board_rect.and_then(|rect| canvas::image_drop_target(app, rect, pointer))
    });
    if hovered {
        paint_overlay(ctx, hover_target);
    }

    let panel_target = hover_target.map(|target| ImportImageTarget::Panel {
        canvas: target.canvas,
        panel: target.panel,
        position: Some(target.position),
    });
    FILE_DROP_BATCH.with(|batch| {
        let mut batch = batch.borrow_mut();
        batch.observe_hover(&hovered_paths, !dropped.is_empty(), panel_target);
    });
    if !dropped.is_empty() {
        FILE_DROP_BATCH.with(|batch| batch.borrow_mut().push(dropped, now));
    }

    let pending = FILE_DROP_BATCH.with(|batch| batch.borrow().ready_at);
    if let Some(ready_at) = pending {
        ctx.request_repaint_after(Duration::from_secs_f64((ready_at - now).max(0.0)));
    }
    let ready = FILE_DROP_BATCH.with(|batch| batch.borrow_mut().take_ready(now));
    if let Some((paths, target)) = ready {
        dispatch(app, paths, target);
    }
}

fn external_file_pointer(
    _ctx: &egui::Context,
    hovered: bool,
    egui_pointer: Option<egui::Pos2>,
) -> Option<egui::Pos2> {
    if !hovered {
        return egui_pointer;
    }
    #[cfg(windows)]
    {
        native_windows_pointer(_ctx)
    }
    #[cfg(not(windows))]
    {
        egui_pointer
    }
}

#[cfg(windows)]
fn native_windows_pointer(ctx: &egui::Context) -> Option<egui::Pos2> {
    use windows_sys::Win32::Foundation::POINT;
    use windows_sys::Win32::Graphics::Gdi::ScreenToClient;
    use windows_sys::Win32::UI::WindowsAndMessaging::GetCursorPos;

    let window = NATIVE_WINDOW.load(Ordering::Relaxed);
    if window.is_null() {
        return None;
    }
    let mut point = POINT { x: 0, y: 0 };
    // SAFETY: `window` is the live eframe HWND registered during app creation;
    // both APIs only read it and write to `point`.
    unsafe {
        if GetCursorPos(&mut point) == 0 || ScreenToClient(window, &mut point) == 0 {
            return None;
        }
    }
    client_pixels_to_egui(point.x, point.y, ctx.pixels_per_point())
}

#[cfg(any(windows, test))]
fn client_pixels_to_egui(x: i32, y: i32, pixels_per_point: f32) -> Option<egui::Pos2> {
    if !pixels_per_point.is_finite() || pixels_per_point <= 0.0 {
        return None;
    }
    Some(egui::pos2(
        x as f32 / pixels_per_point,
        y as f32 / pixels_per_point,
    ))
}

fn dispatch(app: &mut PlotxApp, paths: Vec<PathBuf>, target: ImportImageTarget) {
    if let Some(project) = paths
        .iter()
        .find(|path| file_dialogs::recent_open_kind(path) == file_dialogs::RecentOpenKind::Project)
    {
        let ignored = paths.len().saturating_sub(1);
        file_dialogs::open_recent_path(app, project);
        if ignored > 0 {
            app.session.status = format!(
                "Opening {}. Ignored {ignored} other dropped item(s) because a project open replaces the current document.",
                project.display()
            );
        }
        return;
    }
    let mut images = Vec::new();
    let mut others = Vec::new();
    for path in paths {
        let Some(classified) = file_dialogs::classify_dropped_path(app, &path) else {
            continue;
        };
        if classified.is_image() {
            images.push(path);
        } else {
            others.push((path, classified));
        }
    }
    if !images.is_empty() {
        enqueue(
            app,
            ImportImageRequest {
                paths: images,
                payloads: Vec::new(),
                target,
                allow_first_frame: false,
                strip_metadata: false,
                import_all_tiff_pages: false,
            },
        );
        app.session.status = match target {
            ImportImageTarget::Panel { .. } => {
                "Dropping images into the selected Panel.".to_owned()
            }
            _ => "Each dropped image will create a new page.".to_owned(),
        };
    }
    for (path, classified) in others {
        file_dialogs::dispatch_dropped_path(app, &path, classified);
    }
}

fn paint_overlay(ctx: &egui::Context, target: Option<canvas::ImagePanelDropTarget>) {
    let painter = ctx.layer_painter(egui::LayerId::new(
        egui::Order::Foreground,
        egui::Id::new("file_drop_overlay"),
    ));
    let rect = ctx.content_rect();
    painter.rect_filled(rect, 0.0, Color32::from_black_alpha(140));
    let label = if let Some(target) = target {
        painter.rect_stroke(
            target.screen_rect,
            0.0,
            Stroke::new(3.0_f32, Color32::from_rgb(82, 196, 120)),
            StrokeKind::Inside,
        );
        "Add images to selected Panel"
    } else {
        "Drop files here; each image creates a new page"
    };
    painter.text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        label,
        egui::FontId::new(
            crate::typography::TITLE_1_PT,
            egui::FontFamily::Proportional,
        ),
        Color32::WHITE,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use plotx_core::state::{
        CanvasDocument, CanvasId, DEFAULT_CANVAS_SIZE_MM, ObjectFrame, PanelId,
    };

    fn write_test_image(name: &str) -> PathBuf {
        let source = image::RgbaImage::from_pixel(2, 2, image::Rgba([1, 2, 3, 255]));
        let mut cursor = std::io::Cursor::new(Vec::new());
        image::DynamicImage::ImageRgba8(source)
            .write_to(&mut cursor, image::ImageFormat::Bmp)
            .unwrap();
        let path = std::env::temp_dir().join(format!(
            "plotx-file-drop-{}-{name}.bmp",
            uuid::Uuid::new_v4()
        ));
        std::fs::write(&path, cursor.into_inner()).unwrap();
        path
    }

    fn finish_image_imports(app: &mut PlotxApp) {
        let ctx = egui::Context::default();
        for _ in 0..2_000 {
            file_dialogs::image_import::poll(app, &ctx);
            if !file_dialogs::image_import::has_active_jobs() {
                return;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        panic!(
            "image import did not finish; status: {}",
            app.session.status
        );
    }

    #[test]
    fn split_windows_events_form_one_batch_and_the_next_drop_gets_a_fresh_target() {
        let canvas = CanvasId::new();
        let panel = PanelId::new();
        let panel_target = ImportImageTarget::Panel {
            canvas,
            panel,
            position: Some([4.0, 5.0]),
        };
        let mut batch = FileDropBatch::default();
        batch.observe_hover(
            &[
                PathBuf::from("one.png"),
                PathBuf::from("two.png"),
                PathBuf::from("three.png"),
            ],
            false,
            Some(panel_target),
        );
        batch.push(vec![PathBuf::from("one.png")], 1.0);
        assert!(batch.take_ready(1.2).is_none());
        batch.push(vec![PathBuf::from("two.png")], 1.25);
        assert!(batch.take_ready(1.5).is_none());
        batch.push(vec![PathBuf::from("three.png")], 1.55);
        let (paths, target) = batch.take_ready(1.55).unwrap();
        assert_eq!(
            paths,
            [
                PathBuf::from("one.png"),
                PathBuf::from("two.png"),
                PathBuf::from("three.png")
            ]
        );
        assert_eq!(target, panel_target);

        batch.observe_hover(&[PathBuf::from("four.png")], false, None);
        batch.push(vec![PathBuf::from("four.png")], 2.0);
        let (_, target) = batch.take_ready(2.0).unwrap();
        assert_eq!(target, ImportImageTarget::NewPages);
    }

    #[test]
    fn drop_without_a_hover_snapshot_uses_a_bounded_fallback() {
        let mut batch = FileDropBatch::default();
        batch.push(vec![PathBuf::from("one.png")], 1.0);
        assert!(batch.take_ready(1.49).is_none());
        assert!(batch.take_ready(1.5).is_some());
    }

    #[test]
    fn native_client_pixels_use_the_same_scale_as_egui_pointer_events() {
        assert_eq!(
            client_pixels_to_egui(180, 90, 1.5),
            Some(egui::pos2(120.0, 60.0))
        );
        assert!(client_pixels_to_egui(10, 10, 0.0).is_none());
        assert!(client_pixels_to_egui(10, 10, f32::NAN).is_none());
    }

    #[test]
    fn drop_frame_keeps_the_green_panel_target_when_current_ui_state_disappears() {
        let mut batch = FileDropBatch::default();
        let target = ImportImageTarget::Panel {
            canvas: CanvasId::new(),
            panel: PanelId::new(),
            position: Some([40.0, 50.0]),
        };
        batch.observe_hover(&[PathBuf::from("one.png")], false, Some(target));
        batch.observe_hover(&[], true, None);
        batch.push(vec![PathBuf::from("one.png")], 1.0);
        let (_, committed_target) = batch.take_ready(1.0).unwrap();
        assert_eq!(committed_target, target);
    }

    #[test]
    fn green_panel_target_commits_after_drop_frame_loses_selection() {
        let path = write_test_image("panel");
        let mut app = PlotxApp::default();
        let mut page = CanvasDocument::new("Existing".to_owned(), DEFAULT_CANVAS_SIZE_MM);
        let panel = page.create_panel(
            "Target".to_owned(),
            ObjectFrame::new(10.0, 10.0, 120.0, 80.0),
        );
        let canvas = page.resource_id;
        app.doc.canvases.push(page);
        app.session.active_canvas = Some(0);
        app.select_panel(0, panel);

        let target = ImportImageTarget::Panel {
            canvas,
            panel,
            position: Some([20.0, 20.0]),
        };
        let mut batch = FileDropBatch::default();
        batch.observe_hover(std::slice::from_ref(&path), false, Some(target));
        app.session.ui.hierarchical_selection.clear();
        batch.observe_hover(&[], true, None);
        batch.push(vec![path.clone()], 1.0);
        let (paths, committed_target) = batch.take_ready(1.0).unwrap();
        assert_eq!(committed_target, target);

        dispatch(&mut app, paths, committed_target);
        finish_image_imports(&mut app);
        assert_eq!(app.doc.canvases.len(), 1);
        assert_eq!(app.doc.canvases[0].objects.len(), 1);
        assert_eq!(
            app.doc.canvases[0].parent_panel(app.doc.canvases[0].objects[0].id),
            Some(panel)
        );
        assert_eq!(app.session.undo_stack.len(), 1);

        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn image_drop_batches_are_atomic_and_a_followup_drop_still_creates_a_page() {
        let first = write_test_image("first");
        let second = write_test_image("second");
        let third = write_test_image("third");
        let mut app = PlotxApp::default();
        app.doc.canvases.push(CanvasDocument::new(
            "Existing".to_owned(),
            DEFAULT_CANVAS_SIZE_MM,
        ));
        app.session.active_canvas = Some(0);

        dispatch(
            &mut app,
            vec![first.clone(), second.clone()],
            ImportImageTarget::NewPages,
        );
        finish_image_imports(&mut app);
        assert_eq!(app.doc.canvases.len(), 3);
        assert!(app.doc.canvases[0].objects.is_empty());
        assert_eq!(app.session.undo_stack.len(), 1);
        app.undo();
        assert_eq!(app.doc.canvases.len(), 1);
        app.redo();
        assert_eq!(app.doc.canvases.len(), 3);

        dispatch(&mut app, vec![third.clone()], ImportImageTarget::NewPages);
        finish_image_imports(&mut app);
        assert_eq!(app.doc.canvases.len(), 4);
        assert_eq!(app.doc.canvases[2].objects.len(), 1);
        assert!(app.doc.canvases[3].objects[0].name.ends_with("-third"));

        for path in [first, second, third] {
            std::fs::remove_file(path).unwrap();
        }
    }
}
