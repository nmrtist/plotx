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

pub(crate) fn active_box_zoom_rect(
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

pub(crate) fn paint_craft_regions(
    app: &PlotxApp,
    ci: usize,
    object_id: ObjectId,
    dataset: usize,
    plot: PlotRect,
    painter: &egui::Painter,
    chrome: ChromeStyle,
) {
    if app.session.tool != Tool::CraftRegions
        || app.session.ui.craft_task_dataset != Some(app.doc.datasets[dataset].resource_id())
    {
        return;
    }
    let Some(fig) = app.doc.canvases[ci]
        .object(object_id)
        .and_then(|object| object.plot())
        .map(|plot| plot.figure())
    else {
        return;
    };
    let dataset_id = app.doc.datasets[dataset].resource_id();
    let regions = craft_preview_regions(app, dataset_id);
    for (index, region) in regions.iter().enumerate() {
        let x0 = x_to_screen(
            region.start_ppm,
            plot,
            fig.x.min,
            fig.x.span(),
            fig.x.reversed,
        );
        let x1 = x_to_screen(
            region.end_ppm,
            plot,
            fig.x.min,
            fig.x.span(),
            fig.x.reversed,
        );
        let rect = EguiRect::from_min_max(
            Pos2::new(x0.min(x1), plot.top),
            Pos2::new(x0.max(x1), plot.bottom()),
        )
        .intersect(plot_rect(plot));
        if rect.width() < 1.0 {
            continue;
        }
        let [red, green, blue] = region_color(index);
        let color = Color32::from_rgb(red, green, blue);
        painter.rect_filled(rect, 0.0, color.gamma_multiply(0.12));
        painter.rect_stroke(
            rect,
            0.0,
            Stroke::new(
                if app.session.ui.craft_selected_region == Some(region.id) {
                    2.0_f32
                } else {
                    1.0_f32
                },
                color,
            ),
            StrokeKind::Inside,
        );
        if app.session.ui.craft_selected_region == Some(region.id) {
            for x in [rect.left(), rect.right()] {
                painter.line_segment(
                    [Pos2::new(x, rect.top()), Pos2::new(x, rect.bottom())],
                    Stroke::new(2.5_f32, color),
                );
            }
        }
    }

    let Some(cache) = app
        .session
        .ui
        .craft_resolution_cache
        .as_ref()
        .filter(|cache| cache.dataset == dataset_id)
    else {
        return;
    };
    for signal in &cache.invocation.assessment.clear_signals {
        let x = x_to_screen(
            signal.chemical_shift_ppm,
            plot,
            fig.x.min,
            fig.x.span(),
            fig.x.reversed,
        );
        if x < plot.left || x > plot.right() {
            continue;
        }
        painter.line_segment(
            [
                Pos2::new(x, plot.bottom() - 8.0),
                Pos2::new(x, plot.bottom()),
            ],
            Stroke::new(1.0_f32, chrome.selection_stroke),
        );
    }
    let mut prioritized = cache
        .invocation
        .assessment
        .clear_signals
        .iter()
        .collect::<Vec<_>>();
    prioritized.sort_by(|left, right| {
        right
            .prominence_sigma
            .total_cmp(&left.prominence_sigma)
            .then_with(|| right.height_sigma.total_cmp(&left.height_sigma))
    });
    let mut labelled_x = Vec::<f32>::new();
    for signal in prioritized {
        let x = x_to_screen(
            signal.chemical_shift_ppm,
            plot,
            fig.x.min,
            fig.x.span(),
            fig.x.reversed,
        );
        if x >= plot.left
            && x <= plot.right()
            && labelled_x
                .iter()
                .all(|existing| (x - existing).abs() >= 52.0)
        {
            painter.text(
                Pos2::new(x, plot.bottom() - 10.0),
                egui::Align2::CENTER_BOTTOM,
                format!("{:.3}", signal.chemical_shift_ppm),
                egui::FontId::proportional(9.0),
                chrome.selection_stroke,
            );
            labelled_x.push(x);
        }
    }
    if let Some(pointer) = painter
        .ctx()
        .pointer_hover_pos()
        .filter(|pointer| plot_contains(plot, *pointer))
        && let Some(signal) = cache
            .invocation
            .assessment
            .clear_signals
            .iter()
            .map(|signal| {
                let x = x_to_screen(
                    signal.chemical_shift_ppm,
                    plot,
                    fig.x.min,
                    fig.x.span(),
                    fig.x.reversed,
                );
                (signal, (x - pointer.x).abs())
            })
            .filter(|(_, distance)| *distance <= 8.0)
            .min_by(|left, right| left.1.total_cmp(&right.1))
            .map(|(signal, _)| signal)
    {
        painter.text(
            Pos2::new(pointer.x, plot.top + 6.0),
            egui::Align2::CENTER_TOP,
            format!("Suggested signal {:.5} ppm", signal.chemical_shift_ppm),
            egui::FontId::proportional(10.0),
            chrome.selection_stroke,
        );
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
