use egui::{Color32, Pos2, Rect as EguiRect, Sense, Stroke, StrokeKind, Ui, Vec2};
use plotx_core::actions::{Action, PendingViewportEdit};
use plotx_core::layout::{self, MovableEdges, SnapGuide, SnapTargets};
use plotx_core::state::region_color;
use plotx_core::state::{
    AnalysisSelection, AuthorDrag, AxisRange, BOARD_GUTTER_PT, BoardFitTarget, BoardViewport,
    CanvasDocument, CanvasObject, CanvasObjectKind, Dataset, FrameDrag, FrameRef, Integral2DDrag,
    Integral2DDragKind, IntegralDrag, Interaction, MarqueeDrag, ObjectDrag, ObjectDragKind,
    ObjectFrame, ObjectId, PanDrag, PanelLabelDrag, PanelNoteEditState, PhaseDrag, PhaseDragKind,
    PhaseOrient, PlotxApp, Region, RegionDrag, RegionDragKind, ResizeHandle, SHEET_COL_W_PT,
    SHEET_HEADER_H_PT, SHEET_MAX_ROWS, SHEET_ROW_H_PT, Selection, SelectionDrag, TableDataset,
    TextEditState, TileDropPreview, Tool, ZoomAxis, ZoomDrag, board_frames, frame_board_pos,
    frame_board_rect, set_frame_board_pos, toggle_frame_selection_synced,
};
use plotx_core::{Integral2D, IntegralResult};
use plotx_render::Rect as PlotRect;

const PH0_PER_PX: f64 = 0.01;
const PH1_PER_PX: f64 = 0.01;
const PIVOT_GRAB_PX: f32 = 6.0;
const SELECT_MIN_PX: f32 = 6.0;
const DRAG_START_PX: f32 = 5.0;
const WHEEL_ZOOM_SPEED: f32 = 0.0015;
const HANDLE_SIZE_PX: f32 = 8.0;
const MIN_OBJECT_SIZE_PT: f32 = 24.0;
const PANEL_LABEL_HIT_PAD_PX: f32 = 4.0;
const SNAP_PX: f32 = 6.0;

mod authoring;
mod board;
mod board_notes;
mod chrome;
mod geometry;
mod integrals;
mod integrals2d;
mod interactions;
mod navigation;
mod painting;
mod panel_notes;
mod peaks;
mod phase;
mod regions;
mod slices;
mod snap;
mod tiling;

pub(crate) use authoring::*;
pub(crate) use board::*;
pub(crate) use board_notes::*;
pub(crate) use chrome::*;
pub(crate) use geometry::*;
pub(crate) use integrals::*;
pub(crate) use integrals2d::*;
pub(crate) use interactions::*;
pub(crate) use navigation::*;
pub(crate) use painting::*;
pub(crate) use panel_notes::*;
pub(crate) use peaks::*;
pub(crate) use phase::*;
pub(crate) use regions::*;
pub(crate) use slices::*;
pub(crate) use snap::*;
pub(crate) use tiling::*;

#[derive(Clone, Copy)]
pub(crate) enum CanvasInteractionClearScope {
    Transient,
    Selection,
    All,
}

pub fn render_central(app: &mut PlotxApp, ui: &mut Ui) {
    let Some(ci) = app.session.active_canvas else {
        welcome_page(app, ui);
        return;
    };
    canvas_breadcrumb(app, ci, ui);
    let avail = ui.available_rect_before_wrap();
    let (resp, painter) = ui.allocate_painter(avail.size(), Sense::click_and_drag());
    let rect = resp.rect;
    let chrome = ChromeStyle::from_visuals(ui.visuals(), app.session.canvas_accent);
    ensure_board_view(app, rect);
    drive_board_fit(app, ui, rect);

    // Gesture handlers below read the raw pointer, so nothing else stops them from
    // acting on board content that lies (clipped) under a side bar, a popup or a
    // window. A live drag keeps the pointer wherever it wanders.
    let pointer_hits_canvas_layer = ui
        .input(|input| input.pointer.hover_pos())
        .is_none_or(|pos| {
            ui.ctx()
                .layer_id_at(pos)
                .is_none_or(|layer| layer == ui.layer_id())
        });
    let pointer_owned = app.session.ui.interaction.is_active()
        || (ui.rect_contains_pointer(rect) && pointer_hits_canvas_layer);

    let view_consumed = pointer_owned && handle_navigation(app, ci, rect, ui);

    // Suppressed only while a non-frame gesture is mid-drag, so a live data/object
    // drag isn't interrupted by a frame switch — a fresh click still activates
    // another figure.
    let frame_consumed = if pointer_owned
        && !view_consumed
        && matches!(
            app.session.ui.interaction,
            Interaction::Idle | Interaction::Frame(_)
        ) {
        dispatch_frame_gesture(app, rect, ui)
    } else {
        false
    };
    // `dispatch_frame_gesture` may have switched the active frame.
    let ci = app.session.active_canvas.unwrap_or(ci);
    let page = page_screen_rect(app.session.board, &app.doc.canvases[ci], rect);

    // Processing direct manipulation must update the document before its plots
    // are painted, so the changed spectrum is visible in this same frame.
    if pointer_owned && app.session.tool == Tool::ManualPhase {
        handle_phase_before_paint(app, ci, rect, ui);
    }

    let author_active = app.session.tool.creates_object();
    if pointer_owned && author_active && !view_consumed && !frame_consumed {
        handle_author_create(app, ci, rect, ui);
    }
    let label_consumed = if !pointer_owned || view_consumed || frame_consumed || author_active {
        false
    } else {
        handle_panel_label_interactions(app, ci, rect, ui)
    };
    let caption_consumed =
        if !pointer_owned || view_consumed || frame_consumed || author_active || label_consumed {
            false
        } else {
            handle_frame_caption_interactions(app, rect, ui)
        };
    if pointer_owned
        && !view_consumed
        && !frame_consumed
        && !author_active
        && !label_consumed
        && !caption_consumed
    {
        if app.session.tool.is_layout_tool() {
            handle_object_interactions(app, ci, rect, ui, &resp);
        } else if app.session.tool.is_data_tool() {
            handle_data_tool_target(app, ci, rect, ui, &resp);
        }
    }

    let frame_stroke = Stroke::new(1.0_f32, ui.visuals().widgets.noninteractive.bg_stroke.color);
    // Pages float on the board the way chrome cards float on the workspace: a
    // soft shadow keeps a white page legible on the light workspace fill.
    let page_shadow = egui::epaint::Shadow {
        offset: [0, 2],
        blur: 10,
        spread: 0,
        color: Color32::from_black_alpha(if ui.visuals().dark_mode { 110 } else { 36 }),
    };
    for other in 0..app.doc.canvases.len() {
        if other == ci {
            continue;
        }
        let other_page = page_screen_rect(app.session.board, &app.doc.canvases[other], rect);
        painter.add(page_shadow.as_shape(frame_card_rect(other_page), header_corner_radius()));
        paint_document(app, other, rect, &painter);
        painter.rect_stroke(other_page, 0.0, frame_stroke, StrokeKind::Inside);
    }
    painter.add(page_shadow.as_shape(frame_card_rect(page), header_corner_radius()));
    paint_document(app, ci, rect, &painter);
    paint_frame_headers(app, rect, ui, &painter);
    paint_frame_captions(app, rect, ui, &painter);
    render_inline_panel_note_editor(app, rect, ui);
    paint_sheet_frames(app, rect, ui, &painter);
    paint_layout_overlay(app, ci, rect, &painter, chrome);
    paint_axis_zoom(app, ci, rect, &painter, chrome);
    paint_author_drag(app, ci, rect, &painter, chrome);
    paint_marquee(app, ci, rect, &painter, chrome);
    paint_panel_label_selection(app, ci, rect, &painter, chrome);
    paint_object_selection(app, ci, rect, page, &painter, chrome);
    paint_tile_preview(app, rect, &painter, chrome);
    super::canvas_size::page_size_chrome(app, ci, page, rect, ui);
    if pointer_owned {
        canvas_cursor(app, ci, rect, ui);
    }

    if data_edit_target(app, ci).is_none() {
        resp.context_menu(|ui| arrange_context_menu(app, ci, ui));
    }

    let Some(object_id) =
        data_edit_target(app, ci).or_else(|| app.doc.canvases[ci].active_plot_object_id())
    else {
        return;
    };
    let Some(di) = app.doc.canvases[ci]
        .object(object_id)
        .and_then(|object| object.dataset())
    else {
        return;
    };
    let Some(outer) = object_screen_rect(app.session.board, &app.doc.canvases[ci], object_id, rect)
    else {
        return;
    };
    let outer_rect = plot_rect(outer);
    let plot = {
        let fig = &app.doc.canvases[ci]
            .object(object_id)
            .and_then(|object| object.plot())
            .unwrap()
            .figure;
        let zoom = app.session.board.zoom;
        let layout = plotx_render::axis_layout(fig, outer.width / zoom, outer.height / zoom);
        let proj = plotx_render::Projector::new(fig, outer, &layout.margins.scaled(zoom));
        proj.plot
    };

    if data_edit_target(app, ci) == Some(object_id)
        && !matches!(app.session.ui.interaction, Interaction::Pan(_))
    {
        match app.session.tool {
            // Ahead of the `pointer_owned` guard: the pivot line is painted here,
            // and it stays on screen whatever the pointer is over.
            Tool::ManualPhase => {
                let axis = app.doc.datasets[di].active_phase_axis(app.session.ui.phase_axis);
                if let Some(pivot_ppm) = displayed_phase_pivot_ppm(app, di, axis) {
                    let fig = &app.doc.canvases[ci]
                        .object(object_id)
                        .and_then(|object| object.plot())
                        .unwrap()
                        .figure;
                    match axis.orient() {
                        PhaseOrient::Vertical => {
                            let (mn, sp, rv) = (fig.x.min, fig.x.span(), fig.x.reversed);
                            // Pin the line to the nearest edge when the pivot sits
                            // outside the current view, so it never silently vanishes.
                            let px = x_to_screen(pivot_ppm, plot, mn, sp, rv)
                                .clamp(plot.left, plot.right());
                            painter.line_segment(
                                [Pos2::new(px, plot.top), Pos2::new(px, plot.bottom())],
                                Stroke::new(1.5_f32, chrome.pivot),
                            );
                        }
                        PhaseOrient::Horizontal => {
                            let (mn, sp, rv) = (fig.y.min, fig.y.span(), fig.y.reversed);
                            let py = y_to_screen(pivot_ppm, plot, mn, sp, rv)
                                .clamp(plot.top, plot.bottom());
                            painter.line_segment(
                                [Pos2::new(plot.left, py), Pos2::new(plot.right(), py)],
                                Stroke::new(1.5_f32, chrome.pivot),
                            );
                        }
                    }
                }
            }
            _ if !pointer_owned => {}
            Tool::Select => {}
            Tool::BrowseZoom => {
                handle_view_interactions(app, ci, object_id, outer_rect, plot, ui, &resp)
            }
            Tool::SelectRegion | Tool::LineFit | Tool::Annotate | Tool::PeakAnalysis => {
                handle_selection_drag(app, ci, object_id, di, plot, ui);
            }
            Tool::Regions => handle_region_drag(app, ci, object_id, di, plot, ui),
            Tool::Integrate => {
                if app.doc.datasets[di]
                    .as_nmr2d()
                    .is_some_and(|dataset| dataset.is_true_2d())
                {
                    handle_integral_2d_drag(app, ci, object_id, di, plot, ui, &resp)
                } else {
                    handle_integral_drag(app, ci, object_id, di, plot, ui, &resp)
                }
            }
            Tool::Peaks => handle_peaks(app, ci, object_id, di, plot, ui, &resp),
            Tool::Slice => handle_slice(app, ci, object_id, di, plot, ui),
            Tool::Text
            | Tool::PanelLabel
            | Tool::Rect
            | Tool::Ellipse
            | Tool::Line
            | Tool::Arrow => {}
        }
    }

    paint_zoom_drag(app, ci, object_id, plot, &painter, chrome);
    paint_regions(app, ci, object_id, di, plot, &painter, chrome);
    paint_integrals(app, ci, object_id, di, plot, &painter, chrome);
    paint_integrals_2d(app, ci, object_id, di, plot, &painter, chrome);
    paint_peaks(app, ci, object_id, di, plot, &painter, chrome);
    paint_slice(app, ci, object_id, di, plot, &painter);
    paint_analysis_selection(app, ci, object_id, plot, &painter, chrome);
    paint_selection_drag(app, ci, object_id, plot, &painter, chrome);
}

fn welcome_page(app: &mut PlotxApp, ui: &mut Ui) {
    ui.add_space(28.0);
    let width = ui.available_width().min(500.0);
    ui.vertical_centered(|ui| {
        ui.allocate_ui_with_layout(
            Vec2::new(width, ui.available_height()),
            egui::Layout::top_down(egui::Align::Min),
            |ui| {
                ui.label(
                    egui::RichText::new("PlotX")
                        .size(34.0)
                        .color(ui.visuals().strong_text_color()),
                );
                ui.label(
                    egui::RichText::new("Scientific data analysis and figure preparation")
                        .size(17.0)
                        .color(ui.visuals().text_color()),
                );
                if !cfg!(feature = "datafusion") {
                    ui.add_space(8.0);
                    ui.label(
                        egui::RichText::new("Development build: large tables may run more slowly.")
                            .color(Color32::from_rgb(0xE0, 0x6C, 0x22)),
                    );
                }
                ui.add_space(22.0);
                welcome_start(app, ui);
            },
        );
    });
}

fn welcome_start(app: &mut PlotxApp, ui: &mut Ui) {
    use egui_phosphor::regular as icon;

    welcome_heading(ui, "Start");
    if welcome_action(ui, icon::FILE, "Open data…") {
        crate::ui::file_dialogs::open_file(app);
    }
    if welcome_action(ui, icon::FOLDER, "Open data folder…") {
        crate::ui::file_dialogs::open_folder(app);
    }
    if welcome_action(ui, icon::FOLDER_OPEN, "Open project…") {
        crate::ui::file_dialogs::open_project(app);
    }
    if welcome_action(ui, icon::TABLE, "Import table…") {
        crate::ui::file_dialogs::import_delimited_table(app);
    }
    if welcome_action(ui, icon::FILE_PLUS, "New empty data table") {
        app.new_table_dataset();
    }

    welcome_recent(app, ui);

    ui.add_space(20.0);
    welcome_heading(ui, "Tip");
    ui.label(
        egui::RichText::new("Drop data anywhere in the workspace to open it.")
            .color(ui.visuals().weak_text_color()),
    );
}

/// The most recent openable paths, straight back into work. Icons come from
/// the path shape alone (no filesystem probing in the paint loop); a missing
/// file surfaces through the loader's normal failure report on click.
fn welcome_recent(app: &mut PlotxApp, ui: &mut Ui) {
    use egui_phosphor::regular as icon;
    const WELCOME_RECENT_LIMIT: usize = 5;

    if app.session.recent_files.is_empty() {
        return;
    }
    ui.add_space(20.0);
    welcome_heading(ui, "Recent");
    let mut open: Option<std::path::PathBuf> = None;
    for path in app.session.recent_files.iter().take(WELCOME_RECENT_LIMIT) {
        let name = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.display().to_string());
        let glyph = match path.extension() {
            Some(extension) if extension.eq_ignore_ascii_case("plotx") => icon::FOLDER_OPEN,
            Some(_) => icon::FILE,
            // Bruker acquisitions (`fid`/`ser`) are extensionless *files*;
            // other extensionless paths are folder gestures.
            None if path.file_name().is_some_and(|name| {
                name.eq_ignore_ascii_case("fid") || name.eq_ignore_ascii_case("ser")
            }) =>
            {
                icon::FILE
            }
            None => icon::FOLDER,
        };
        let clicked = ui
            .add(
                egui::Button::new(
                    egui::RichText::new(format!("{glyph}  {name}"))
                        .size(14.0)
                        .color(ui.visuals().hyperlink_color),
                )
                .frame(false),
            )
            .on_hover_text(path.display().to_string())
            .clicked();
        if clicked {
            open = Some(path.clone());
        }
    }
    if let Some(path) = open {
        crate::ui::file_dialogs::open_recent_path(app, &path);
    }
}

fn welcome_heading(ui: &mut Ui, text: &str) {
    ui.label(
        egui::RichText::new(text)
            .size(19.0)
            .color(ui.visuals().strong_text_color()),
    );
    ui.add_space(4.0);
}

fn welcome_action(ui: &mut Ui, glyph: &str, label: &str) -> bool {
    ui.add(
        egui::Button::new(
            egui::RichText::new(format!("{glyph}  {label}"))
                .size(14.0)
                .color(ui.visuals().hyperlink_color),
        )
        .frame(false),
    )
    .clicked()
}

fn canvas_breadcrumb(app: &PlotxApp, ci: usize, ui: &mut Ui) {
    ui.horizontal(|ui| {
        ui.add_space(2.0);
        ui.small(app.doc.canvases[ci].name.clone());
        ui.weak("›");
        ui.small(app.session.tool.label());
        if let Some(id) = data_edit_target(app, ci).or_else(|| app.session.ui.selection.object()) {
            let title = app.doc.canvases[ci]
                .object(id)
                .map(|object| {
                    object
                        .plot()
                        .and_then(|plot| plot.panel.user_note.lines().next())
                        .filter(|line| !line.trim().is_empty())
                        .map(str::to_owned)
                        .unwrap_or_else(|| object.name.clone())
                })
                .unwrap_or_default();
            ui.weak("›");
            ui.small(format!("\"{title}\""));
        }
    });
    ui.add_space(2.0);
}

/// A specific handler that sets its own cursor later in the frame wins.
fn canvas_cursor(app: &PlotxApp, ci: usize, rect: egui::Rect, ui: &Ui) {
    let Some(p) = ui
        .input(|i| i.pointer.hover_pos())
        .filter(|p| rect.contains(*p))
    else {
        return;
    };
    // Ambient pan reads on top of the tool cursor: an active data-pan grabs, and
    // holding Space arms the hand anywhere on the board.
    if matches!(app.session.ui.interaction, Interaction::Pan(_)) {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
        return;
    }
    if ui.input(|i| i.key_down(egui::Key::Space)) {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
        return;
    }
    let icon = if app.session.tool.is_layout_tool() {
        match screen_to_page_unbounded(app.session.board, &app.doc.canvases[ci], rect, p)
            .and_then(|page| hit_object(&app.doc.canvases[ci], page, app.session.board.zoom))
        {
            Some(hit) => match hit.kind {
                ObjectDragKind::Move => egui::CursorIcon::Grab,
                ObjectDragKind::Resize(handle) => resize_cursor(handle),
            },
            None => egui::CursorIcon::Default,
        }
    } else if app.session.tool.is_data_tool() {
        match data_edit_target(app, ci)
            .and_then(|id| object_screen_rect(app.session.board, &app.doc.canvases[ci], id, rect))
        {
            Some(frame) if plot_contains(frame, p) => egui::CursorIcon::Crosshair,
            _ => egui::CursorIcon::Default,
        }
    } else {
        return;
    };
    ui.ctx().set_cursor_icon(icon);
}

fn resize_cursor(handle: ResizeHandle) -> egui::CursorIcon {
    match handle {
        ResizeHandle::TopLeft | ResizeHandle::BottomRight => egui::CursorIcon::ResizeNwSe,
        ResizeHandle::TopRight | ResizeHandle::BottomLeft => egui::CursorIcon::ResizeNeSw,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use plotx_core::state::{
        CanvasObject, CanvasObjectKind, CanvasViewport, PanelMeta, PlotObject, TextBox,
    };
    use plotx_figure::{Axis, Figure};

    #[test]
    fn hit_object_selects_text_box() {
        let mut canvas = CanvasDocument::new("page".to_owned(), [200.0, 200.0]);
        canvas.objects.push(CanvasObject {
            id: 7,
            name: "Text".to_owned(),
            frame: ObjectFrame::new(20.0, 20.0, 100.0, 30.0),
            locked: false,
            visible: true,
            group: None,
            kind: CanvasObjectKind::Text(TextBox::label("hi".to_owned())),
        });

        let hit = hit_object(&canvas, Pos2::new(50.0, 30.0), 1.0);

        assert_eq!(hit.map(|hit| hit.object), Some(7));
    }

    #[test]
    fn hit_object_finds_object_outside_page_bounds() {
        let mut canvas = CanvasDocument::new("page".to_owned(), [100.0, 100.0]);
        canvas.objects.push(CanvasObject {
            id: 1,
            name: "plot".to_owned(),
            frame: ObjectFrame::new(-30.0, 20.0, 50.0, 40.0),
            locked: false,
            visible: true,
            group: None,
            kind: CanvasObjectKind::Plot(Box::new(PlotObject {
                binding: plotx_core::state::DataBinding::single(0),
                chart: plotx_core::state::ChartSpec::default(),
                stack: plotx_core::state::StackSpec::default(),
                projections: plotx_core::state::AxisProjections::default(),
                axis_overrides: plotx_core::state::AxisOverrides::default(),
                figure: Figure::new("plot", Axis::new("x", 0.0, 1.0), Axis::new("y", 0.0, 1.0)),
                viewport: CanvasViewport::from_figure(&Figure::new(
                    "plot",
                    Axis::new("x", 0.0, 1.0),
                    Axis::new("y", 0.0, 1.0),
                )),
                panel: PanelMeta::new("title".to_owned(), 50.0),
            })),
        });

        let hit = hit_object(&canvas, Pos2::new(-10.0, 30.0), 1.0);

        assert_eq!(hit.map(|hit| hit.object), Some(1));
    }

    #[test]
    fn data_edit_target_requires_data_tool_and_selected_plot() {
        let mut app = PlotxApp::new();
        let mut canvas = CanvasDocument::new("page".to_owned(), [200.0, 200.0]);
        canvas.objects.push(CanvasObject {
            id: 3,
            name: "plot".to_owned(),
            frame: ObjectFrame::new(10.0, 10.0, 80.0, 60.0),
            locked: false,
            visible: true,
            group: None,
            kind: CanvasObjectKind::Plot(Box::new(PlotObject {
                binding: plotx_core::state::DataBinding::single(0),
                chart: plotx_core::state::ChartSpec::default(),
                stack: plotx_core::state::StackSpec::default(),
                projections: plotx_core::state::AxisProjections::default(),
                axis_overrides: plotx_core::state::AxisOverrides::default(),
                figure: Figure::new("plot", Axis::new("x", 0.0, 1.0), Axis::new("y", 0.0, 1.0)),
                viewport: CanvasViewport::from_figure(&Figure::new(
                    "plot",
                    Axis::new("x", 0.0, 1.0),
                    Axis::new("y", 0.0, 1.0),
                )),
                panel: PanelMeta::new("title".to_owned(), 50.0),
            })),
        });
        app.doc.canvases.push(canvas);
        app.session.active_canvas = Some(0);
        app.doc.canvases[0].selected_object = Some(3);

        app.session.tool = Tool::Select;
        assert_eq!(data_edit_target(&app, 0), None);

        app.session.tool = Tool::BrowseZoom;
        assert_eq!(data_edit_target(&app, 0), Some(3));
    }

    #[test]
    fn phase_editor_open_drives_on_plot_pivot() {
        use num_complex::Complex64;
        use plotx_core::state::{Dataset, NmrDataset, PhaseAxis};
        use plotx_io::{Domain, NmrData};
        use std::f64::consts::TAU;

        let npoints = 256;
        let (sw, obs, carrier) = (4000.0, 400.0, 5.0);
        let dt = 1.0 / sw;
        let points = (0..npoints)
            .map(|k| {
                let t = k as f64 * dt;
                let decay = (-t / 0.25f64).exp();
                let freq_hz = (2.0 - carrier) * obs;
                Complex64::from_polar(decay, TAU * freq_hz * t)
            })
            .collect();
        let data = NmrData {
            points,
            domain: Domain::Time,
            spectral_width_hz: sw,
            observe_freq_mhz: obs,
            carrier_ppm: carrier,
            nucleus: "1H".to_owned(),
            source: "synthetic".to_owned(),
            group_delay: 0.0,
        };

        let mut app = PlotxApp::new();
        app.doc
            .datasets
            .push(Dataset::Nmr(Box::new(NmrDataset::load(data))));
        let mut canvas = CanvasDocument::new("page".to_owned(), [200.0, 200.0]);
        let id = canvas.allocate_object_id();
        let obj = app.build_plot_object(
            0,
            ObjectFrame::new(10.0, 10.0, 80.0, 60.0),
            id,
            "plot".into(),
        );
        canvas.objects.push(obj);
        app.doc.canvases.push(canvas);
        app.session.active_canvas = Some(0);
        app.focus_single(0);

        let pivot = Color32::from_rgb(0xE0, 0x6C, 0x22);
        let count = |app: &mut PlotxApp| {
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1000.0, 800.0),
                )),
                ..Default::default()
            };
            let ctx = egui::Context::default();
            // Two passes: the first lays out the board, the second paints with a
            // stable geometry.
            let _ = ctx.run_ui(input.clone(), |ui| {
                egui::CentralPanel::default().show_inside(ui, |ui| render_central(app, ui));
            });
            let out = ctx.run_ui(input, |ui| {
                egui::CentralPanel::default().show_inside(ui, |ui| render_central(app, ui));
            });
            out.shapes
                .iter()
                .filter(|cs| match &cs.shape {
                    egui::epaint::Shape::LineSegment { stroke, .. } => stroke.color == pivot,
                    egui::epaint::Shape::Circle(c) => c.fill == pivot,
                    _ => false,
                })
                .count()
        };

        let phase_id = app.doc.datasets[0]
            .axis_pipeline(PhaseAxis::Direct)
            .unwrap()
            .steps
            .iter()
            .find(|s| matches!(s.kind, plotx_processing::StepKind::Phase(_)))
            .unwrap()
            .id;

        app.sync_phase_interaction();
        assert_eq!(count(&mut app), 0, "no pivot before the Phase editor opens");

        app.session.ui.proc_expanded_step = Some(phase_id);
        app.sync_phase_interaction();
        assert_eq!(app.session.tool, Tool::ManualPhase);
        assert!(
            count(&mut app) > 0,
            "pivot appears while the Phase editor is open"
        );

        app.session.ui.proc_expanded_step = None;
        app.sync_phase_interaction();
        assert_ne!(app.session.tool, Tool::ManualPhase);
        assert_eq!(count(&mut app), 0, "pivot gone after the editor collapses");
    }
}
