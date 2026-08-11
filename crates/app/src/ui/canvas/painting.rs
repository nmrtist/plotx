use super::*;
use plotx_core::state::{PeakOrigin, PeakSet};

pub(crate) fn paint_zoom_drag(
    app: &PlotxApp,
    ci: usize,
    object_id: ObjectId,
    plot: PlotRect,
    painter: &egui::Painter,
    chrome: ChromeStyle,
) {
    let Some(r) = active_box_zoom_rect(app, ci, object_id, plot) else {
        return;
    };
    painter.rect_filled(r, 0.0, chrome.selection_fill);
    painter.rect_stroke(
        r,
        0.0,
        Stroke::new(1.0_f32, chrome.selection_stroke),
        StrokeKind::Inside,
    );
}

fn active_box_zoom_rect(
    app: &PlotxApp,
    ci: usize,
    object_id: ObjectId,
    plot: PlotRect,
) -> Option<EguiRect> {
    let drag = match &app.session.ui.interaction {
        Interaction::Zoom(d) if d.axis == ZoomAxis::Box => *d,
        _ => return None,
    };
    if drag.canvas != ci || drag.object != object_id {
        return None;
    }
    let r = EguiRect::from_two_pos(pos(drag.start), pos(drag.current)).intersect(plot_rect(plot));
    if r.width() < 1.0 || r.height() < 1.0 {
        return None;
    }
    Some(r)
}

/// Recomputes the plot rect from the drag's own object so it paints under any
/// tool, regardless of which figure is selected.
pub(crate) fn paint_axis_zoom(
    app: &PlotxApp,
    ci: usize,
    rect: egui::Rect,
    painter: &egui::Painter,
    chrome: ChromeStyle,
) {
    let drag = match &app.session.ui.interaction {
        Interaction::Zoom(d) if d.axis != ZoomAxis::Box => *d,
        _ => return,
    };
    if drag.canvas != ci {
        return;
    }
    let Some(plot) = plot_inner_rect(app, ci, drag.object, rect) else {
        return;
    };
    let (start, current) = (pos(drag.start), pos(drag.current));
    let band = match drag.axis {
        ZoomAxis::X => EguiRect::from_min_max(
            Pos2::new(start.x.min(current.x), plot.top),
            Pos2::new(start.x.max(current.x), plot.bottom()),
        ),
        ZoomAxis::Y => EguiRect::from_min_max(
            Pos2::new(plot.left, start.y.min(current.y)),
            Pos2::new(plot.right(), start.y.max(current.y)),
        ),
        ZoomAxis::Box => return,
    };
    let r = band.intersect(plot_rect(plot));
    if r.width() < 1.0 || r.height() < 1.0 {
        return;
    }
    painter.rect_filled(r, 0.0, chrome.selection_fill);
    painter.rect_stroke(
        r,
        0.0,
        Stroke::new(1.0_f32, chrome.selection_stroke),
        StrokeKind::Inside,
    );
}

pub(crate) fn paint_analysis_selection(
    app: &PlotxApp,
    ci: usize,
    object_id: ObjectId,
    plot: PlotRect,
    painter: &egui::Painter,
    chrome: ChromeStyle,
) {
    let Some(selection) = &app.session.ui.analysis_selection else {
        return;
    };
    if selection.canvas != app.doc.canvases[ci].resource_id || selection.object != object_id {
        return;
    }
    let Some(object) = app.doc.canvases[ci].object(object_id) else {
        return;
    };
    let Some(plot_object) = object.plot() else {
        return;
    };
    let fig = plot_object.figure();
    let x0 = x_to_screen(
        selection.x_range.min,
        plot,
        fig.x.min,
        fig.x.span(),
        fig.x.reversed,
    );
    let x1 = x_to_screen(
        selection.x_range.max,
        plot,
        fig.x.min,
        fig.x.span(),
        fig.x.reversed,
    );
    let r = EguiRect::from_min_max(
        Pos2::new(x0.min(x1), plot.top),
        Pos2::new(x0.max(x1), plot.bottom()),
    )
    .intersect(plot_rect(plot));
    if r.width() < 1.0 {
        return;
    }
    painter.rect_filled(r, 0.0, chrome.selection_fill);
    painter.rect_stroke(
        r,
        0.0,
        Stroke::new(1.0_f32, chrome.selection_stroke),
        StrokeKind::Inside,
    );
}

/// The figure owns persistent region bands; this overlay only adds editing
/// handles and the new-band preview while the Regions tool is active.
pub(crate) fn paint_regions(
    app: &PlotxApp,
    ci: usize,
    object_id: ObjectId,
    dataset: usize,
    plot: PlotRect,
    painter: &egui::Painter,
    chrome: ChromeStyle,
) {
    if app.session.tool != Tool::Regions {
        return;
    }
    let Some(fig) = app.doc.canvases[ci]
        .object(object_id)
        .and_then(|object| object.plot())
        .map(|plot| plot.figure())
    else {
        return;
    };
    let Some(state) = app
        .doc
        .datasets
        .get(dataset)
        .and_then(Dataset::region_analysis)
    else {
        return;
    };
    let selected = app
        .session
        .ui
        .selected_region
        .and_then(|selection| selection.in_dataset(app.doc.datasets[dataset].resource_id()));
    for region in &state.regions {
        if selected != Some(region.id) {
            continue;
        }
        let x0 = x_to_screen(region.lo, plot, fig.x.min, fig.x.span(), fig.x.reversed);
        let x1 = x_to_screen(region.hi, plot, fig.x.min, fig.x.span(), fig.x.reversed);
        let r = EguiRect::from_min_max(
            Pos2::new(x0.min(x1), plot.top),
            Pos2::new(x0.max(x1), plot.bottom()),
        )
        .intersect(plot_rect(plot));
        if r.width() < 1.0 {
            continue;
        }
        let [cr, cg, cb] = region.color;
        let stroke_col = Color32::from_rgb(cr, cg, cb);
        let is_sel = true;
        painter.rect_stroke(
            r,
            0.0,
            Stroke::new(if is_sel { 2.0_f32 } else { 1.0_f32 }, stroke_col),
            StrokeKind::Inside,
        );
        if is_sel {
            for ex in [r.left(), r.right()] {
                painter.line_segment(
                    [Pos2::new(ex, r.top()), Pos2::new(ex, r.bottom())],
                    Stroke::new(2.5_f32, stroke_col),
                );
            }
        }
    }

    if let Interaction::Region(drag) = &app.session.ui.interaction
        && drag.dataset == app.doc.datasets[dataset].resource_id()
        && drag.canvas == ci
        && drag.kind == RegionDragKind::NewBand
    {
        let x0 = x_to_screen(
            drag.anchor_ppm,
            plot,
            fig.x.min,
            fig.x.span(),
            fig.x.reversed,
        );
        let x1 = x_to_screen(
            drag.current_ppm,
            plot,
            fig.x.min,
            fig.x.span(),
            fig.x.reversed,
        );
        let r = EguiRect::from_min_max(
            Pos2::new(x0.min(x1), plot.top),
            Pos2::new(x0.max(x1), plot.bottom()),
        )
        .intersect(plot_rect(plot));
        if r.width() >= 1.0 {
            painter.rect_filled(r, 0.0, chrome.selection_fill);
            painter.rect_stroke(
                r,
                0.0,
                Stroke::new(1.0_f32, chrome.selection_stroke),
                StrokeKind::Inside,
            );
        }
    }
}

pub(crate) fn paint_integrals(
    app: &PlotxApp,
    ci: usize,
    object_id: ObjectId,
    dataset: usize,
    plot: PlotRect,
    painter: &egui::Painter,
    chrome: ChromeStyle,
) {
    if app.session.tool != Tool::Integrate {
        return;
    }
    let Some(fig) = app.doc.canvases[ci]
        .object(object_id)
        .and_then(|object| object.plot())
        .map(|plot| plot.figure())
    else {
        return;
    };
    let Some(n) = app.doc.datasets.get(dataset).and_then(Dataset::as_nmr) else {
        return;
    };
    let selected = app.session.ui.selected_integral;
    let hover_x = painter.ctx().input(|input| {
        input
            .pointer
            .hover_pos()
            .filter(|position| plot_rect(plot).contains(*position))
            .map(|position| position.x)
    });
    for integ in &n.integrals {
        let x0 = x_to_screen(
            integ.start_ppm,
            plot,
            fig.x.min,
            fig.x.span(),
            fig.x.reversed,
        );
        let x1 = x_to_screen(integ.end_ppm, plot, fig.x.min, fig.x.span(), fig.x.reversed);
        let r = EguiRect::from_min_max(
            Pos2::new(x0.min(x1), plot.top),
            Pos2::new(x0.max(x1), plot.bottom()),
        )
        .intersect(plot_rect(plot));
        if r.width() < 1.0 {
            continue;
        }
        let color = chrome.integral;
        let [cr, cg, cb, _] = color.to_array();
        let is_sel = selected == Some(integ.id);
        let is_hovered = hover_x.is_some_and(|x| x >= r.left() && x <= r.right());
        if is_sel || is_hovered {
            painter.rect_filled(r, 0.0, Color32::from_rgba_unmultiplied(cr, cg, cb, 30));
        }
        for edge in [r.left(), r.right()] {
            painter.line_segment(
                [Pos2::new(edge, r.top()), Pos2::new(edge, r.bottom())],
                Stroke::new(
                    if is_sel { 2.0_f32 } else { 1.0_f32 },
                    color.gamma_multiply(0.65),
                ),
            );
        }
        if is_sel {
            for ex in [r.left(), r.right()] {
                painter.rect_filled(
                    EguiRect::from_center_size(
                        Pos2::new(ex, (r.top() + r.bottom()) * 0.5),
                        Vec2::new(6.0, 16.0),
                    ),
                    1.0,
                    color,
                );
            }
        }
    }

    if let Interaction::Integral(drag) = &app.session.ui.interaction
        && drag.dataset == dataset
        && drag.canvas == ci
        && drag.kind == RegionDragKind::NewBand
    {
        let x0 = x_to_screen(
            drag.anchor_ppm,
            plot,
            fig.x.min,
            fig.x.span(),
            fig.x.reversed,
        );
        let x1 = x_to_screen(
            drag.current_ppm,
            plot,
            fig.x.min,
            fig.x.span(),
            fig.x.reversed,
        );
        let r = EguiRect::from_min_max(
            Pos2::new(x0.min(x1), plot.top),
            Pos2::new(x0.max(x1), plot.bottom()),
        )
        .intersect(plot_rect(plot));
        if r.width() >= 1.0 {
            painter.rect_filled(r, 0.0, chrome.selection_fill);
            painter.rect_stroke(
                r,
                0.0,
                Stroke::new(1.0_f32, chrome.selection_stroke),
                StrokeKind::Inside,
            );
        }
    }
}

/// Markers: hollow for a live detection, filled for a hand-placed one, ringed
/// when selected. Labels themselves come from the figure.
pub(crate) fn paint_peaks(
    app: &PlotxApp,
    ci: usize,
    object_id: ObjectId,
    dataset: usize,
    plot: PlotRect,
    painter: &egui::Painter,
    chrome: ChromeStyle,
) {
    if app.session.tool != Tool::Peaks {
        return;
    }
    let column = app.session.ui.peak_column;
    let Some(trace) = app
        .doc
        .datasets
        .get(dataset)
        .and_then(|d| d.displayed_trace(column))
    else {
        return;
    };
    let Some(peaks) = app.doc.datasets.get(dataset).and_then(Dataset::peaks) else {
        return;
    };
    let Some(fig) = app.doc.canvases[ci]
        .object(object_id)
        .and_then(|object| object.plot())
        .map(|plot| plot.figure())
    else {
        return;
    };
    let drag_threshold = match app.interaction() {
        Interaction::PeakThreshold(drag) if drag.canvas == ci && drag.object == object_id => {
            Some(drag.y)
        }
        _ => None,
    };
    let sy = |v: f64| y_to_screen(v, plot, fig.y.min, fig.y.span(), fig.y.reversed);
    let sx = |v: f64| x_to_screen(v, plot, fig.x.min, fig.x.span(), fig.x.reversed);
    // Confine every marker, ring, line and preview to the plot box so nothing spills
    // into the axes or margins when the view is zoomed in.
    let painter = painter.with_clip_rect(plot_rect(plot));

    let line_y = drag_threshold
        .or(peaks.detector.threshold)
        .unwrap_or_else(|| PeakSet::auto_threshold(&trace));
    let ly = sy(line_y);
    if ly >= plot.top && ly <= plot.bottom() {
        for seg in egui::Shape::dashed_line(
            &[Pos2::new(plot.left, ly), Pos2::new(plot.right(), ly)],
            Stroke::new(1.0_f32, chrome.peak),
            6.0,
            4.0,
        ) {
            painter.add(seg);
        }
    }

    if let Some(y) = drag_threshold {
        for (px, py) in PeakSet::detect_at(&trace, Some(y), peaks.detector.max_count) {
            let at = Pos2::new(sx(px), sy(py));
            if plot_contains(plot, at) {
                painter.circle_stroke(at, 3.0, Stroke::new(1.5_f32, chrome.peak));
            }
        }
    }

    let resolved = peaks.resolve();
    let selected = app.session.ui.selected_peak;
    for peak in &resolved {
        let p = Pos2::new(sx(peak.x), sy(peak.y));
        if !plot_contains(plot, p) {
            continue;
        }
        match peak.origin {
            PeakOrigin::Manual => painter.circle_filled(p, 3.0, chrome.peak),
            PeakOrigin::Detected => {
                painter.circle_stroke(p, 3.0, Stroke::new(1.5_f32, chrome.peak))
            }
        };
        if peak.mark_id.is_some() && peak.mark_id == selected {
            painter.circle_stroke(p, 5.5, Stroke::new(2.0_f32, chrome.selection_active));
        }
    }

    if let Interaction::PeakBand(drag) = app.interaction()
        && drag.canvas == ci
        && drag.object == object_id
    {
        let r = EguiRect::from_min_max(
            Pos2::new(sx(drag.anchor_x).min(sx(drag.current_x)), plot.top),
            Pos2::new(sx(drag.anchor_x).max(sx(drag.current_x)), plot.bottom()),
        )
        .intersect(plot_rect(plot));
        if r.width() >= 1.0 {
            painter.rect_filled(r, 0.0, chrome.selection_fill);
            painter.rect_stroke(
                r,
                0.0,
                Stroke::new(1.0_f32, chrome.selection_stroke),
                StrokeKind::Inside,
            );
        }
        return;
    }

    // Hidden over a marker or the threshold line, where a press does something else.
    if app.interaction().is_active() {
        return;
    }
    let Some(hp) = painter.ctx().input(|i| i.pointer.hover_pos()) else {
        return;
    };
    if !plot_contains(plot, hp) {
        return;
    }
    let near_marker = resolved
        .iter()
        .any(|peak| Pos2::new(sx(peak.x), sy(peak.y)).distance(hp) <= 10.0);
    let on_line = (hp.y - ly).abs() <= 6.0;
    if near_marker || on_line {
        return;
    }
    let hover_x = screen_to_x(hp.x, plot, fig.x.min, fig.x.span(), fig.x.reversed);
    let (px, py) = trace.snap(hover_x);
    let at = Pos2::new(sx(px), sy(py));
    if plot_contains(plot, at) {
        painter.circle_stroke(at, 4.0, Stroke::new(1.5_f32, chrome.selection_active));
        painter.circle_filled(at, 1.5, chrome.selection_active);
    }
}

pub(crate) fn paint_selection_drag(
    app: &PlotxApp,
    ci: usize,
    object_id: ObjectId,
    plot: PlotRect,
    painter: &egui::Painter,
    chrome: ChromeStyle,
) {
    let drag = match &app.session.ui.interaction {
        Interaction::Selection(d) => *d,
        _ => return,
    };
    if drag.canvas != ci || drag.object != object_id {
        return;
    }
    let a = clamp_to_plot(plot, pos(drag.start));
    let b = clamp_to_plot(plot, pos(drag.current));
    let r = EguiRect::from_min_max(
        Pos2::new(a.x.min(b.x), plot.top),
        Pos2::new(a.x.max(b.x), plot.bottom()),
    )
    .intersect(plot_rect(plot));
    if r.width() < 1.0 {
        return;
    }
    painter.rect_filled(r, 0.0, chrome.selection_fill);
    painter.rect_stroke(
        r,
        0.0,
        Stroke::new(1.0_f32, chrome.selection_stroke),
        StrokeKind::Inside,
    );
}

pub(crate) fn paint_document(app: &PlotxApp, ci: usize, rect: egui::Rect, painter: &egui::Painter) {
    super::image_painting::paint_document(app, ci, rect, painter);
}

pub(crate) fn paint_layout_overlay(
    app: &PlotxApp,
    ci: usize,
    rect: egui::Rect,
    painter: &egui::Painter,
    chrome: ChromeStyle,
) {
    let canvas = &app.doc.canvases[ci];
    let bt = BoardTransform::from_board(app.session.board, rect);
    let page = bt.page_screen_rect(canvas);
    let zoom = bt.zoom;

    let layout_tool = app.session.tool.is_layout_tool();
    if layout_tool {
        let [top, right, bottom, left] = canvas.layout.margin_mm;
        let mm = plotx_core::state::MM_TO_PT * zoom;
        let stroke = Stroke::new(1.0_f32, chrome.margin_guide);
        let dashed = |points: [Pos2; 2]| {
            for segment in egui::Shape::dashed_line(&points, stroke, 5.0, 4.0) {
                painter.add(segment);
            }
        };
        if top > 0.0 {
            let y = page.top() + top * mm;
            dashed([Pos2::new(page.left(), y), Pos2::new(page.right(), y)]);
        }
        if right > 0.0 {
            let x = page.right() - right * mm;
            dashed([Pos2::new(x, page.top()), Pos2::new(x, page.bottom())]);
        }
        if bottom > 0.0 {
            let y = page.bottom() - bottom * mm;
            dashed([Pos2::new(page.left(), y), Pos2::new(page.right(), y)]);
        }
        if left > 0.0 {
            let x = page.left() + left * mm;
            dashed([Pos2::new(x, page.top()), Pos2::new(x, page.bottom())]);
        }
    }

    if canvas.layout.show_grid && layout_tool {
        let stroke = Stroke::new(1.0_f32, chrome.layout_grid);
        for cell in layout::grid_frames(canvas.size_pt(), &canvas.layout) {
            let r = EguiRect::from_min_size(
                Pos2::new(page.left() + cell.x * zoom, page.top() + cell.y * zoom),
                Vec2::new(cell.width * zoom, cell.height * zoom),
            );
            painter.rect_stroke(r, 0.0, stroke, StrokeKind::Inside);
        }
    }

    let stroke = Stroke::new(1.0_f32, chrome.snap_guide);
    for guide in &app.session.ui.snap_guides {
        if guide.vertical {
            let x = page.left() + guide.pos * zoom;
            painter.line_segment(
                [Pos2::new(x, page.top()), Pos2::new(x, page.bottom())],
                stroke,
            );
        } else {
            let y = page.top() + guide.pos * zoom;
            painter.line_segment(
                [Pos2::new(page.left(), y), Pos2::new(page.right(), y)],
                stroke,
            );
        }
    }
}

pub(crate) fn paint_author_drag(
    app: &PlotxApp,
    ci: usize,
    rect: egui::Rect,
    painter: &egui::Painter,
    chrome: ChromeStyle,
) {
    let drag = match &app.session.ui.interaction {
        Interaction::Author(d) => *d,
        _ => return,
    };
    if drag.canvas != ci {
        return;
    }
    let bt = BoardTransform::from_board(app.session.board, rect);
    let page = bt.page_screen_rect(&app.doc.canvases[ci]);
    let zoom = bt.zoom;
    let to_screen = |p: [f32; 2]| Pos2::new(page.left() + p[0] * zoom, page.top() + p[1] * zoom);
    let r = EguiRect::from_two_pos(to_screen(drag.start), to_screen(drag.current));
    painter.rect_filled(r, 0.0, chrome.selection_fill);
    painter.rect_stroke(
        r,
        0.0,
        Stroke::new(1.0_f32, chrome.selection_stroke),
        StrokeKind::Inside,
    );
}

pub(crate) fn paint_panel_label_selection(
    app: &PlotxApp,
    ci: usize,
    rect: egui::Rect,
    painter: &egui::Painter,
    chrome: ChromeStyle,
) {
    let Some((canvas, object_id)) = app.panel_label_selection() else {
        return;
    };
    if canvas != ci {
        return;
    }
    let Some(r) =
        panel_label_screen_rect(app.session.board, &app.doc.canvases[ci], object_id, rect)
    else {
        return;
    };
    painter.rect_stroke(
        r,
        0.0,
        Stroke::new(1.0_f32, chrome.selection_active),
        StrokeKind::Inside,
    );
}

pub(crate) fn paint_object_selection(
    app: &PlotxApp,
    ci: usize,
    rect: egui::Rect,
    _page: egui::Rect,
    painter: &egui::Painter,
    chrome: ChromeStyle,
) {
    super::panel_selection::paint(app, ci, rect, painter, chrome);
    let selection = &app.session.ui.selection;
    let mut ids = selection.objects().to_vec();
    if let Some(primary) = selection.object().filter(|id| !ids.contains(id)) {
        ids.push(primary);
    }
    let handles = ids.len() == 1 && app.session.tool.is_layout_tool();
    let editing_panel = app
        .session
        .ui
        .hierarchical_selection
        .editing_panel()
        .filter(|(canvas, _)| *canvas == app.doc.canvases[ci].resource_id);
    for id in ids {
        let frame = editing_panel
            .and_then(|(_, panel)| {
                (app.doc.canvases[ci].parent_panel(id) == Some(panel)).then(|| {
                    content_screen_rect(app.session.board, &app.doc.canvases[ci], id, rect)
                })
            })
            .flatten()
            .or_else(|| object_screen_rect(app.session.board, &app.doc.canvases[ci], id, rect));
        let Some(frame) = frame else { continue };
        let r = plot_rect(frame);
        let stroke = if data_edit_target(app, ci) == Some(id) {
            Stroke::new(2.0_f32, chrome.selection_active)
        } else {
            Stroke::new(1.5_f32, chrome.selection_stroke)
        };
        painter.rect_stroke(r, 0.0, stroke, StrokeKind::Inside);
        if handles {
            for p in [
                r.left_top(),
                r.right_top(),
                r.left_bottom(),
                r.right_bottom(),
            ] {
                painter.rect_filled(
                    egui::Rect::from_center_size(p, egui::vec2(HANDLE_SIZE_PX, HANDLE_SIZE_PX)),
                    0.0,
                    chrome.selection_stroke,
                );
            }
        }
    }
}

pub(crate) fn paint_marquee(
    app: &PlotxApp,
    ci: usize,
    rect: egui::Rect,
    painter: &egui::Painter,
    chrome: ChromeStyle,
) {
    let marq = match &app.session.ui.interaction {
        Interaction::Marquee(d) => *d,
        _ => return,
    };
    if marq.canvas != ci {
        return;
    }
    let bt = BoardTransform::from_board(app.session.board, rect);
    let page = bt.page_screen_rect(&app.doc.canvases[ci]);
    let zoom = bt.zoom;
    let to_screen = |p: [f32; 2]| Pos2::new(page.left() + p[0] * zoom, page.top() + p[1] * zoom);
    let r = EguiRect::from_two_pos(to_screen(marq.start), to_screen(marq.current));
    painter.rect_filled(r, 0.0, chrome.selection_fill);
    painter.rect_stroke(
        r,
        0.0,
        Stroke::new(1.0_f32, chrome.selection_stroke),
        StrokeKind::Inside,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn browse_zoom_box_interaction_has_a_preview_rect() {
        let mut app = PlotxApp::new();
        app.session.tool = Tool::BrowseZoom;
        app.session.ui.interaction = Interaction::Zoom(ZoomDrag {
            canvas: 2,
            object: ObjectId::new(7),
            start: [10.0, 20.0],
            current: [40.0, 60.0],
            axis: ZoomAxis::Box,
        });

        let rect = active_box_zoom_rect(
            &app,
            2,
            ObjectId::new(7),
            PlotRect::new(0.0, 0.0, 100.0, 100.0),
        );

        assert_eq!(
            rect,
            Some(EguiRect::from_min_max(
                Pos2::new(10.0, 20.0),
                Pos2::new(40.0, 60.0)
            ))
        );
    }
}
