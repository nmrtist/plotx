use super::*;

/// Coarse board grid (pt) that whole page-frames fall back to when dragged clear
/// of any neighbour — about half a default page width, so pages land in tidy
/// columns/rows. Edge/gutter magnetism (see `snap_frame_pos`) takes priority.
pub(crate) const BOARD_GRID_PT: f32 = 360.0;

/// Height (screen px) of the grab-bar/label strip drawn just above each page on
/// the board. Fixed in screen space so it stays readable and grabbable at any
/// board zoom. Board chrome only — never part of the exported/presented page.
pub(crate) const FRAME_HEADER_PX: f32 = 18.0;

/// Screen headroom (px) that zoom-to-fit reserves above the fitted content so
/// the frame header and the size-chip row stacked above it still have a home
/// at fit zoom instead of being pushed off-screen.
pub(crate) const FIT_CHROME_PX: f32 = FRAME_HEADER_PX + 30.0;

const FIT_ZOOM_ID: &str = "board_fit_zoom";
const FIT_PAN_X_ID: &str = "board_fit_pan_x";
const FIT_PAN_Y_ID: &str = "board_fit_pan_y";
const FIT_SETTLE_EPS: f32 = 1e-3;

/// Begin a spring-animated zoom-to-fit onto `frame` (a page or a sheet). Clears
/// `auto_fit` so the all-frames fit stops fighting the glide, and seeds the
/// springs from the live board so the animation starts where the view is now
/// instead of snapping.
pub(crate) fn request_board_fit(app: &mut PlotxApp, ctx: &egui::Context, frame: FrameRef) {
    app.session.board_fit = Some(BoardFitTarget::Frame(frame));
    seed_board_fit_springs(app, ctx);
}

pub(crate) fn request_board_fit_region(app: &mut PlotxApp, ctx: &egui::Context, bbox: [f32; 4]) {
    app.session.board_fit = Some(BoardFitTarget::Region(bbox));
    seed_board_fit_springs(app, ctx);
}

pub(crate) fn request_board_fit_viewport(
    app: &mut PlotxApp,
    ctx: &egui::Context,
    zoom: f32,
    pan: [f32; 2],
) {
    app.session.board_fit = Some(BoardFitTarget::Viewport { zoom, pan });
    seed_board_fit_springs(app, ctx);
}

/// Hand the board viewport to the user for the lifetime of a direct-manipulation
/// gesture. Both the all-frames auto-fit and any in-flight zoom-to-fit glide
/// compete with the user for the page↔screen transform; a gesture that re-samples
/// that transform each frame would read the animator's motion as phantom pointer
/// travel (the "click a plot and it jumps" bug). Grabbing content is an explicit
/// signal that the user, not the animator, owns the view — so we cancel the glide
/// outright (not merely pause it) and stop auto-fit. Call once at every gesture
/// start; `UiState::gesture_active` then keeps the animators out until it ends.
pub(crate) fn freeze_board_for_gesture(app: &mut PlotxApp) {
    app.session.board_fit = None;
    app.session.board.auto_fit = false;
}

/// Clear `auto_fit` and seed the fit springs from the live board so the glide
/// starts where the view is now instead of snapping.
fn seed_board_fit_springs(app: &mut PlotxApp, ctx: &egui::Context) {
    app.session.board.auto_fit = false;
    crate::ui::switcher::seed_spring(ctx, egui::Id::new(FIT_ZOOM_ID), app.session.board.zoom);
    crate::ui::switcher::seed_spring(ctx, egui::Id::new(FIT_PAN_X_ID), app.session.board.pan[0]);
    crate::ui::switcher::seed_spring(ctx, egui::Id::new(FIT_PAN_Y_ID), app.session.board.pan[1]);
}

pub(crate) fn frame_zoom_menu(app: &mut PlotxApp, ui: &mut Ui) {
    let n = app.session.ui.frame_selection.len();
    let label = if n > 1 {
        format!("Zoom to {n} selected frames")
    } else {
        "Zoom to fit all frames".to_owned()
    };
    if ui.button(label).clicked() {
        zoom_to_selection(app, ui.ctx());
        ui.close();
    }
    ui.separator();
}

pub(crate) fn zoom_to_selection(app: &mut PlotxApp, ctx: &egui::Context) {
    let bbox = if app.session.ui.frame_selection.is_empty() {
        all_frames_bbox(app)
    } else {
        let rects = app
            .session
            .ui
            .frame_selection
            .clone()
            .into_iter()
            .filter_map(|f| frame_board_rect(app, f));
        bbox_of_rects(rects)
    };
    if let Some((min_x, min_y, max_x, max_y)) = bbox {
        request_board_fit_region(app, ctx, [min_x, min_y, max_x, max_y]);
    }
}

pub(crate) fn drive_board_fit(app: &mut PlotxApp, ui: &Ui, screen: egui::Rect) {
    if app.session.ui.gesture_active() {
        debug_assert!(
            app.session.board_fit.is_none(),
            "a gesture must freeze board_fit; something re-armed it mid-gesture"
        );
        return;
    }
    let Some(fit) = app.session.board_fit else {
        return;
    };
    let (target_zoom, target_pan) = match fit {
        BoardFitTarget::Frame(frame) => match frame_board_rect(app, frame) {
            Some(r) => {
                let vp = board_fit_bbox_with_chrome((r.left, r.top, r.right(), r.bottom()), screen);
                (vp.zoom, vp.pan)
            }
            None => {
                app.session.board_fit = None;
                return;
            }
        },
        BoardFitTarget::Region(b) => {
            let vp = board_fit_bbox_with_chrome((b[0], b[1], b[2], b[3]), screen);
            (vp.zoom, vp.pan)
        }
        BoardFitTarget::Viewport { zoom, pan } => (zoom, pan),
    };
    let target = BoardViewport {
        zoom: target_zoom,
        pan: target_pan,
        auto_fit: false,
    };
    let ctx = ui.ctx();
    let dt = ui.input(|i| i.stable_dt);
    app.session.board.zoom =
        crate::ui::switcher::animate_spring(ctx, egui::Id::new(FIT_ZOOM_ID), target.zoom, dt);
    app.session.board.pan[0] =
        crate::ui::switcher::animate_spring(ctx, egui::Id::new(FIT_PAN_X_ID), target.pan[0], dt);
    app.session.board.pan[1] =
        crate::ui::switcher::animate_spring(ctx, egui::Id::new(FIT_PAN_Y_ID), target.pan[1], dt);
    app.session.board.auto_fit = false;
    if (app.session.board.zoom - target.zoom).abs() < FIT_SETTLE_EPS
        && (app.session.board.pan[0] - target.pan[0]).abs() < FIT_SETTLE_EPS
        && (app.session.board.pan[1] - target.pan[1]).abs() < FIT_SETTLE_EPS
    {
        app.session.board_fit = None;
    }
}

/// Rounding of a header strip's top corners; the frame shadow shares it so the
/// shadow hugs the tab-plus-page silhouette.
pub(crate) fn header_corner_radius() -> egui::CornerRadius {
    egui::CornerRadius {
        nw: 5,
        ne: 5,
        sw: 0,
        se: 0,
    }
}

/// The frame rect together with its header tab — the card silhouette the drop
/// shadow is cast from, so the tab doesn't sit shadowless on a shadowed page.
pub(crate) fn frame_card_rect(frame: egui::Rect) -> egui::Rect {
    egui::Rect::from_min_max(
        Pos2::new(frame.left(), frame.top() - FRAME_HEADER_PX),
        frame.max,
    )
}

pub(crate) fn header_strip_rect(frame: egui::Rect) -> egui::Rect {
    egui::Rect::from_min_max(
        Pos2::new(frame.left(), frame.top() - FRAME_HEADER_PX),
        Pos2::new(frame.right(), frame.top()),
    )
}

pub(crate) fn frame_header_rect(bt: &BoardTransform, canvas: &CanvasDocument) -> egui::Rect {
    header_strip_rect(bt.page_screen_rect(canvas))
}

pub(crate) fn frame_screen_rect(
    bt: &BoardTransform,
    app: &PlotxApp,
    frame: FrameRef,
) -> Option<egui::Rect> {
    frame_board_rect(app, frame).map(|r| bt.board_rect_screen(r))
}

/// Every frame in topmost-first hit order: sheets (painted last, so on top) then
/// pages by descending index, with the active page bumped to the very front.
fn frame_hit_order(app: &PlotxApp) -> Vec<FrameRef> {
    let mut order = board_frames(app);
    order.reverse();
    if let Some(active) = app.session.active_canvas.map(FrameRef::Page)
        && order.contains(&active)
    {
        order.retain(|&f| f != active);
        order.insert(0, active);
    }
    order
}

pub(crate) fn frame_at(app: &PlotxApp, screen: egui::Rect, p: Pos2) -> Option<FrameRef> {
    let bt = BoardTransform::from_board(app.session.board, screen);
    frame_hit_order(app)
        .into_iter()
        .find(|&f| frame_screen_rect(&bt, app, f).is_some_and(|r| r.contains(p)))
}

pub(crate) fn frame_header_at(app: &PlotxApp, screen: egui::Rect, p: Pos2) -> Option<FrameRef> {
    let bt = BoardTransform::from_board(app.session.board, screen);
    frame_hit_order(app)
        .into_iter()
        .find(|&f| frame_screen_rect(&bt, app, f).is_some_and(|r| header_strip_rect(r).contains(p)))
}

pub(crate) fn paint_frame_headers(
    app: &PlotxApp,
    screen: egui::Rect,
    ui: &Ui,
    painter: &egui::Painter,
) {
    let bt = BoardTransform::from_board(app.session.board, screen);
    for (ci, canvas) in app.doc.canvases.iter().enumerate() {
        let header = frame_header_rect(&bt, canvas);
        if !screen.intersects(header) {
            continue;
        }
        let selected = frame_is_selected(app, FrameRef::Page(ci));
        paint_header_strip(header, &canvas.name, selected, ui, painter);
    }
}

/// The header strip drawn above a page or sheet frame: a tab with rounded top
/// corners sitting flush on the frame's top edge, its title truncated with an
/// ellipsis instead of spilling past the strip.
fn paint_header_strip(
    strip: egui::Rect,
    title: &str,
    selected: bool,
    ui: &Ui,
    painter: &egui::Painter,
) {
    let visuals = ui.visuals();
    let (fill, text_color) = if selected {
        (visuals.selection.bg_fill, visuals.strong_text_color())
    } else {
        (visuals.widgets.inactive.bg_fill, visuals.weak_text_color())
    };
    let border = Stroke::new(1.0_f32, visuals.widgets.noninteractive.bg_stroke.color);
    let radius = header_corner_radius();
    painter.rect_filled(strip, radius, fill);
    painter.rect_stroke(strip, radius, border, StrokeKind::Inside);
    let mut job = egui::text::LayoutJob::simple_singleline(
        title.to_owned(),
        egui::FontId::proportional(11.0),
        text_color,
    );
    job.wrap = egui::text::TextWrapping::truncate_at_width((strip.width() - 12.0).max(0.0));
    let galley = painter.ctx().fonts_mut(|f| f.layout_job(job));
    let pos = egui::pos2(strip.left() + 6.0, strip.center().y - galley.size().y * 0.5);
    painter.galley(pos, galley, text_color);
}

/// Screen gap between a page's bottom edge and its caption text.
pub(crate) const CAPTION_GAP_PX: f32 = 6.0;

/// Board chrome only — never exported/presented.
pub(crate) fn paint_frame_captions(
    app: &PlotxApp,
    screen: egui::Rect,
    ui: &Ui,
    painter: &egui::Painter,
) {
    let bt = BoardTransform::from_board(app.session.board, screen);
    let color = ui.visuals().text_color();
    for canvas in &app.doc.canvases {
        if !canvas.caption_visible {
            continue;
        }
        let mut lines: Vec<String> = Vec::new();
        if !canvas.caption.trim().is_empty() {
            lines.push(canvas.caption.clone());
        }
        lines.extend(
            canvas
                .panel_notes()
                .into_iter()
                .map(|(letter, note)| format!("{letter} — {note}")),
        );
        if lines.is_empty() {
            continue;
        }
        let page = bt.page_screen_rect(canvas);
        let font = egui::FontId::proportional((11.0 * bt.zoom).clamp(7.0, 28.0));
        let galley = painter.layout(lines.join("\n"), font, color, page.width().max(1.0));
        let top_left = Pos2::new(page.left(), page.bottom() + CAPTION_GAP_PX);
        if !screen.intersects(egui::Rect::from_min_size(top_left, galley.size())) {
            continue;
        }
        painter.galley(top_left, galley, color);
    }
}

/// Board chrome only — the sheet's editable form is the modal data-sheet window
/// (double-click in the Data list).
pub(crate) fn paint_sheet_frames(
    app: &PlotxApp,
    screen: egui::Rect,
    ui: &Ui,
    painter: &egui::Painter,
) {
    let bt = BoardTransform::from_board(app.session.board, screen);
    for (di, dataset) in app.doc.datasets.iter().enumerate() {
        let Some(t) = dataset.as_table() else {
            continue;
        };
        let rect = bt.board_rect_screen(t.board_rect_pt());
        let strip = header_strip_rect(rect);
        if !screen.intersects(rect) && !screen.intersects(strip) {
            continue;
        }
        if screen.intersects(strip) {
            let selected = frame_is_selected(app, FrameRef::Sheet(di));
            paint_header_strip(strip, &dataset.display_name(), selected, ui, painter);
        }
        if screen.intersects(rect) {
            paint_sheet_body(t, rect, bt.zoom, ui, painter);
        }
    }
}

fn sheet_headers(t: &TableDataset) -> Vec<String> {
    t.typed_state
        .envelope
        .revision
        .snapshot
        .schema
        .columns
        .iter()
        .map(|column| {
            column.unit.as_ref().map_or_else(
                || column.name.clone(),
                |unit| format!("{} ({})", column.name, unit.display_unit),
            )
        })
        .collect()
}

/// Compact cell text for a value, switching to scientific notation for extreme
/// magnitudes so a fixed-width column stays readable.
fn fmt_number(v: f64) -> String {
    if v.is_nan() {
        "NaN".to_owned()
    } else if v == f64::INFINITY {
        "+Inf".to_owned()
    } else if v == f64::NEG_INFINITY {
        "-Inf".to_owned()
    } else if v == 0.0 {
        "0".to_owned()
    } else if v.abs() >= 1.0e4 || v.abs() < 1.0e-3 {
        format!("{v:.2e}")
    } else {
        format!("{v:.3}")
    }
}

fn fmt_cell(value: &plotx_core::data::ScalarValue) -> String {
    use plotx_core::data::ScalarValue;
    match value {
        ScalarValue::Null => "NULL".into(),
        ScalarValue::Boolean(value) => value.to_string(),
        ScalarValue::Int64(value) => value.to_string(),
        ScalarValue::Float64(value) => fmt_number(*value),
        ScalarValue::Utf8(value) => value.clone(),
        ScalarValue::Categorical(value) => format!("#{value}"),
        ScalarValue::Date(value) => value.to_string(),
        ScalarValue::Time(value) | ScalarValue::Timestamp(value) | ScalarValue::Duration(value) => {
            value.to_string()
        }
        ScalarValue::Extension { storage, .. } => fmt_cell(storage),
    }
}

/// Cell text is only drawn once the board zoom leaves it legible (font size floor).
fn paint_sheet_body(
    t: &TableDataset,
    rect: egui::Rect,
    zoom: f32,
    ui: &Ui,
    painter: &egui::Painter,
) {
    let visuals = ui.visuals();
    let grid = Stroke::new(1.0_f32, visuals.widgets.noninteractive.bg_stroke.color);
    let col_w = SHEET_COL_W_PT * zoom;
    let row_h = SHEET_ROW_H_PT * zoom;
    let header_h = SHEET_HEADER_H_PT * zoom;
    let cols = t.sheet_cols();
    let rows = t.visible_rows();
    let preview = t.typed_rows(SHEET_MAX_ROWS, &[]).ok();

    painter.rect_filled(rect, 0.0, visuals.extreme_bg_color);
    let header_rect = egui::Rect::from_min_size(rect.min, egui::vec2(rect.width(), header_h));
    painter.rect_filled(header_rect, 0.0, visuals.widgets.inactive.bg_fill);

    for k in 0..=cols {
        let x = rect.left() + k as f32 * col_w;
        painter.line_segment(
            [Pos2::new(x, rect.top()), Pos2::new(x, rect.bottom())],
            grid,
        );
    }
    let footer = if t.rows_overflow() { 1 } else { 0 };
    for r in 0..=rows + footer {
        let y = rect.top() + header_h + r as f32 * row_h;
        painter.line_segment(
            [Pos2::new(rect.left(), y), Pos2::new(rect.right(), y)],
            grid,
        );
    }
    painter.rect_stroke(rect, 0.0, grid, StrokeKind::Inside);

    let font_size = (SHEET_ROW_H_PT * 0.62 * zoom).min(13.0);
    if font_size < 6.0 {
        return;
    }
    let font = egui::FontId::proportional(font_size);
    let clip = painter.with_clip_rect(rect);
    let left = |k: usize| rect.left() + k as f32 * col_w + 4.0;

    for (k, header) in sheet_headers(t).iter().enumerate() {
        clip.text(
            Pos2::new(left(k), rect.top() + header_h * 0.5),
            egui::Align2::LEFT_CENTER,
            header,
            font.clone(),
            visuals.strong_text_color(),
        );
    }
    for r in 0..rows {
        let cy = rect.top() + header_h + (r as f32 + 0.5) * row_h;
        for (k, column) in preview
            .as_ref()
            .into_iter()
            .flat_map(|preview| preview.columns.iter())
            .enumerate()
        {
            let value = column
                .values
                .get(r)
                .unwrap_or(&plotx_core::data::ScalarValue::Null);
            clip.text(
                Pos2::new(left(k), cy),
                egui::Align2::LEFT_CENTER,
                fmt_cell(value),
                font.clone(),
                visuals.text_color(),
            );
        }
    }
    if t.rows_overflow() {
        let cy = rect.top() + header_h + (rows as f32 + 0.5) * row_h;
        clip.text(
            Pos2::new(left(0), cy),
            egui::Align2::LEFT_CENTER,
            format!(
                "+{} more rows",
                t.typed_state
                    .envelope
                    .revision
                    .snapshot
                    .row_count
                    .saturating_sub(rows as u64)
            ),
            font,
            visuals.weak_text_color(),
        );
    }
}

fn activate_frame(app: &mut PlotxApp, frame: FrameRef) {
    app.session.ui.frame_selection = vec![frame];
    match frame {
        FrameRef::Page(ci) => {
            activate_page(app, ci);
            if let Some(canvas) = app.doc.canvases.get(ci) {
                let lead = canvas
                    .active_dataset()
                    .and_then(|id| app.doc.dataset_index(id));
                let datasets = app.doc.page_dataset_indices(ci);
                app.focus_datasets(&datasets, lead);
            }
        }
        FrameRef::Sheet(di) => app.focus_single(di),
    }
}

pub(crate) fn frame_is_selected(app: &PlotxApp, frame: FrameRef) -> bool {
    let is_active = match frame {
        FrameRef::Page(ci) => app.session.active_canvas == Some(ci),
        FrameRef::Sheet(di) => app.active_dataset() == Some(di),
    };
    is_active || app.session.ui.frame_selection.contains(&frame)
}

fn activate_page(app: &mut PlotxApp, ci: usize) {
    if app.session.active_canvas == Some(ci) {
        return;
    }
    if let Some(old) = app.session.active_canvas {
        clear_canvas_interaction_state(app, old, CanvasInteractionClearScope::Transient);
    }
    app.session.active_canvas = Some(ci);
    app.sync_selection_to_active_canvas();
}

pub(crate) fn dispatch_frame_gesture(app: &mut PlotxApp, rect: egui::Rect, ui: &Ui) -> bool {
    let (pressed, double, hover, extend) = ui.input(|i| {
        (
            i.pointer.primary_pressed(),
            i.pointer
                .button_double_clicked(egui::PointerButton::Primary),
            i.pointer.hover_pos(),
            i.modifiers.shift || i.modifiers.command || i.modifiers.ctrl,
        )
    });
    if extend
        && pressed
        && let Some(p) = hover
        && let Some(frame) = frame_at(app, rect, p).or_else(|| frame_header_at(app, rect, p))
    {
        toggle_frame_selection_synced(app, frame);
        return true;
    }
    // Handled ahead of the header-drag path so the zero-move drag begun by the
    // two clicks is dropped, not driven.
    if double
        && let Some(p) = hover
        && let Some(frame) = frame_header_at(app, rect, p)
    {
        if matches!(app.session.ui.interaction, Interaction::Frame(_)) {
            app.reset_interaction();
        }
        activate_frame(app, frame);
        request_board_fit(app, ui.ctx(), frame);
        return true;
    }
    if handle_frame_drag(app, rect, ui) {
        return true;
    }
    if let (true, Some(p)) = (pressed, hover)
        && let Some(frame) = frame_at(app, rect, p)
    {
        activate_frame(app, frame);
    }
    if let (true, Some(p)) = (double, hover)
        && let Some(FrameRef::Sheet(di)) = frame_at(app, rect, p)
    {
        app.session.ui.sheet_open = Some(di);
    }
    false
}

fn handle_frame_drag(app: &mut PlotxApp, rect: egui::Rect, ui: &Ui) -> bool {
    let (hover, primary_down, primary_pressed, primary_released, esc) = ui.input(|i| {
        (
            i.pointer.hover_pos(),
            i.pointer.primary_down(),
            i.pointer.primary_pressed(),
            i.pointer.primary_released(),
            i.key_pressed(egui::Key::Escape),
        )
    });

    if esc {
        if matches!(app.session.ui.interaction, Interaction::Frame(_)) {
            app.reset_interaction();
        }
        return false;
    }

    if !matches!(app.session.ui.interaction, Interaction::Frame(_))
        && primary_pressed
        && let Some(p) = hover
        && let Some(frame) = frame_header_at(app, rect, p)
    {
        activate_frame(app, frame);
        if let Some(before) = frame_board_pos(app, frame) {
            let start = BoardTransform::from_board(app.session.board, rect).screen_to_world(p);
            app.begin_interaction(Interaction::Frame(FrameDrag {
                frame,
                before,
                start_world: [start.x, start.y],
            }));
        }
    }

    let drag = match &app.session.ui.interaction {
        Interaction::Frame(d) => *d,
        _ => return false,
    };

    if primary_down && let Some(p) = hover {
        let world = BoardTransform::from_board(app.session.board, rect).screen_to_world(p);
        let candidate = [
            drag.before[0] + (world.x - drag.start_world[0]),
            drag.before[1] + (world.y - drag.start_world[1]),
        ];
        let snapped = snap_dragged_frame(app, drag.frame, candidate);
        set_frame_board_pos(app, drag.frame, snapped);
        app.session.board_fit = None;
        app.session.board.auto_fit = false;
        ui.ctx().request_repaint();
    }

    if primary_released || !primary_down {
        app.reset_interaction();
        if let Some(after) = frame_board_pos(app, drag.frame) {
            let action = match drag.frame {
                FrameRef::Page(ci) => Action::move_canvas_on_board(ci, drag.before, after),
                FrameRef::Sheet(di) => Action::move_sheet_on_board(di, drag.before, after),
            };
            app.execute_action(action);
        }
    }

    true
}

#[cfg(test)]
#[path = "board_tests.rs"]
mod tests;
