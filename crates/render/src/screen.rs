pub use crate::screen_stats::RenderStats;
use crate::screen_stats::visible_source_len;
use crate::{
    AXIS_LINE_WIDTH, Document, DocumentItem, DocumentObject, DocumentOverlay, DocumentViewport,
    LegendMark, OUTER_PAD, OverlayAlign, OverlayKind, OverlayShape, OverlayShapeKind, OverlayText,
    Projector, Rect, TICK_LABEL_PAD, TICK_LENGTH, arrow_head, axis_layout, error_bar_segments,
    heatmap_cells, integral, legend_entries, polygon_outline, projection_points,
};
use egui::{Align2, Color32, FontId, Pos2, Sense, Shape, Stroke, StrokeKind, Ui, Vec2};
use plotx_figure::{AxisFrame, AxisTrace, Color, Figure, SeriesKind};
use std::borrow::Cow;

/// Bounds on a pooled line's column grid.
const MIN_LINE_COLUMNS: usize = 2_048;
const MAX_LINE_COLUMNS: usize = 16_384;

fn col(c: Color) -> Color32 {
    Color32::from_rgb(c.r, c.g, c.b)
}

/// Allocate space in `ui` and paint the whole figure.
pub fn show(ui: &mut Ui, fig: &Figure) {
    let avail = ui.available_size();
    let desired = Vec2::new(avail.x.max(320.0), avail.y.max(240.0));
    let (response, painter) = ui.allocate_painter(desired, Sense::hover());
    let r = response.rect;
    let outer = Rect::new(r.left(), r.top(), r.width(), r.height());
    paint(&painter, outer, fig, 1.0);
}

/// Paint a figure into an explicit rectangle of an existing painter. `scale` is
/// the single page→screen factor: `outer` is already scaled, and every intrinsic
/// size (margins, fonts, strokes, offsets) is a page-unit constant multiplied by
/// it here, so the whole figure stays proportional at any zoom.
pub fn paint(painter: &egui::Painter, outer: Rect, fig: &Figure, scale: f32) {
    paint_with_stats(painter, outer, fig, scale, None);
}

pub fn paint_with_stats(
    painter: &egui::Painter,
    outer: Rect,
    fig: &Figure,
    scale: f32,
    mut stats: Option<&mut RenderStats>,
) {
    let ty = fig.typography;
    let layout = axis_layout(fig, outer.width / scale, outer.height / scale);
    let margins = layout.margins.scaled(scale);
    let proj = Projector::new(fig, outer, &margins);
    let plot = proj.plot;

    let to_pos = |x: f32, y: f32| Pos2::new(x, y);

    painter.rect_filled(
        egui::Rect::from_min_size(
            Pos2::new(outer.left, outer.top),
            Vec2::new(outer.width, outer.height),
        ),
        0.0,
        col(fig.background),
    );

    if !fig.title.trim().is_empty() {
        painter.text(
            Pos2::new(
                outer.left + outer.width / 2.0,
                outer.top + (OUTER_PAD + ty.title_pt * 0.5) * scale,
            ),
            Align2::CENTER_CENTER,
            &fig.title,
            FontId::proportional(ty.title_pt * scale),
            col(Color::BLACK),
        );
    }

    let hidden_frame = fig.axis_frame == AxisFrame::Hidden;
    let (x_ticks, y_ticks) = (layout.x_ticks, layout.y_ticks);

    if fig.show_grid && !hidden_frame {
        let grid_stroke = Stroke::new(1.0 * scale, col(Color::GRID));
        for &xt in &x_ticks.values {
            let (px, _) = proj.project([xt, fig.y.min]);
            painter.line_segment(
                [to_pos(px, plot.top), to_pos(px, plot.bottom())],
                grid_stroke,
            );
        }
        for &yt in &y_ticks.values {
            let (_, py) = proj.project([fig.x.min, yt]);
            painter.line_segment(
                [to_pos(plot.left, py), to_pos(plot.right(), py)],
                grid_stroke,
            );
        }
    }

    let axis_stroke = Stroke::new(AXIS_LINE_WIDTH * scale, col(Color::AXIS));
    let y_axis_x = proj.left_band.map(|band| band.left).unwrap_or(plot.left);
    match fig.axis_frame {
        AxisFrame::Open => {
            painter.line_segment(
                [
                    to_pos(plot.left, plot.bottom()),
                    to_pos(plot.right(), plot.bottom()),
                ],
                axis_stroke,
            );
            painter.line_segment(
                [to_pos(y_axis_x, plot.top), to_pos(y_axis_x, plot.bottom())],
                axis_stroke,
            );
        }
        AxisFrame::Box => {
            painter.rect_stroke(
                egui::Rect::from_min_size(
                    Pos2::new(plot.left, plot.top),
                    Vec2::new(plot.width, plot.height),
                ),
                0.0,
                axis_stroke,
                StrokeKind::Inside,
            );
            if (y_axis_x - plot.left).abs() > f32::EPSILON {
                painter.line_segment(
                    [to_pos(y_axis_x, plot.top), to_pos(y_axis_x, plot.bottom())],
                    axis_stroke,
                );
            }
        }
        AxisFrame::Hidden => {}
    }

    for (&xt, label) in x_ticks.values.iter().zip(&x_ticks.labels) {
        let (px, _) = proj.project([xt, fig.y.min]);
        painter.line_segment(
            [
                to_pos(px, plot.bottom()),
                to_pos(px, plot.bottom() + TICK_LENGTH * scale),
            ],
            axis_stroke,
        );
        if fig.x.show_tick_labels {
            painter.text(
                Pos2::new(
                    px,
                    plot.bottom() + (TICK_LENGTH + TICK_LABEL_PAD + ty.tick_pt * 0.5) * scale,
                ),
                Align2::CENTER_CENTER,
                label,
                FontId::proportional(ty.tick_pt * scale),
                col(Color::AXIS),
            );
        }
    }
    // A left projection band sits between the contour and its ppm scale, so nudge
    // the F1 tick numbers out past the band to keep them clear of the trace.
    let y_tick_x = y_axis_x - (TICK_LENGTH + TICK_LABEL_PAD) * scale;
    for (&yt, label) in y_ticks.values.iter().zip(&y_ticks.labels) {
        let (_, py) = proj.project([fig.x.min, yt]);
        painter.line_segment(
            [
                to_pos(y_axis_x - TICK_LENGTH * scale, py),
                to_pos(y_axis_x, py),
            ],
            axis_stroke,
        );
        if fig.y.show_tick_labels {
            painter.text(
                Pos2::new(y_tick_x, py),
                Align2::RIGHT_CENTER,
                label,
                FontId::proportional(ty.tick_pt * scale),
                col(Color::AXIS),
            );
        }
    }

    if fig.y.show_tick_labels
        && let Some(multiplier) = y_ticks.multiplier()
    {
        painter.text(
            Pos2::new(y_axis_x, plot.top - TICK_LABEL_PAD * scale),
            Align2::LEFT_BOTTOM,
            multiplier,
            FontId::proportional(ty.tick_pt * scale),
            col(Color::AXIS),
        );
    }
    if fig.x.show_tick_labels
        && let Some(multiplier) = x_ticks.multiplier()
    {
        painter.text(
            Pos2::new(
                plot.right(),
                outer.bottom() - (OUTER_PAD + ty.tick_pt * 0.5) * scale,
            ),
            Align2::RIGHT_CENTER,
            multiplier,
            FontId::proportional(ty.tick_pt * scale),
            col(Color::AXIS),
        );
    }

    if !hidden_frame && fig.x.show_label {
        let multiplier_clearance = if fig.x.show_tick_labels {
            x_ticks.multiplier_clearance(ty.tick_pt)
        } else {
            0.0
        };
        painter.text(
            Pos2::new(
                (plot.left + plot.right()) / 2.0,
                outer.bottom() - (OUTER_PAD + multiplier_clearance + ty.label_pt * 0.5) * scale,
            ),
            Align2::CENTER_CENTER,
            &fig.x.label,
            FontId::proportional(ty.label_pt * scale),
            col(Color::AXIS),
        );
    }
    if !hidden_frame && fig.y.show_label {
        let galley = painter.layout_no_wrap(
            fig.y.label.clone(),
            FontId::proportional(ty.label_pt * scale),
            col(Color::AXIS),
        );
        let galley_size = galley.size();
        let mut y_label = egui::epaint::TextShape::new(
            Pos2::new(-galley_size.x * 0.5, -galley_size.y * 0.5),
            galley,
            col(Color::AXIS),
        )
        .with_angle_and_anchor(-std::f32::consts::FRAC_PI_2, Align2::CENTER_CENTER);
        y_label.pos += Vec2::new(
            outer.left + (OUTER_PAD + ty.label_pt * 0.5) * scale,
            (plot.top + plot.bottom()) * 0.5,
        );
        painter.add(y_label);
    }

    let clip = egui::Rect::from_min_size(
        Pos2::new(plot.left, plot.top),
        Vec2::new(plot.width, plot.height),
    );
    let clipped = painter.with_clip_rect(clip);

    if let Some(grid) = &fig.heatmap {
        for (cell, color) in heatmap_cells(&proj, grid) {
            // Expand a hair so adjacent cells cannot show anti-aliasing seams.
            let rect = egui::Rect::from_min_size(
                Pos2::new(cell.left, cell.top),
                Vec2::new(cell.width, cell.height),
            )
            .expand(0.5);
            clipped.rect_filled(rect, 0.0, col(color));
        }
    }

    for poly in &fig.polygons {
        let Some(outline) = polygon_outline(&proj, poly) else {
            continue;
        };
        let pts: Vec<Pos2> = outline.iter().map(|&(x, y)| to_pos(x, y)).collect();
        let fill = Color32::from_rgba_unmultiplied(
            poly.fill.r,
            poly.fill.g,
            poly.fill.b,
            (poly.opacity.clamp(0.0, 1.0) * 255.0).round() as u8,
        );
        let stroke = poly
            .stroke
            .map(|(c, w)| Stroke::new(w * scale, col(c)))
            .unwrap_or(Stroke::NONE);
        clipped.add(Shape::convex_polygon(pts, fill, stroke));
    }

    for contour in &fig.contours {
        let stroke = Stroke::new(contour.width * scale, col(contour.color));
        for seg in &contour.segments {
            let (ax, ay) = proj.project(seg[0]);
            let (bx, by) = proj.project(seg[1]);
            clipped.line_segment([to_pos(ax, ay), to_pos(bx, by)], stroke);
        }
    }

    paint_error_bars(&clipped, &proj, fig, scale, false);

    for series in &fig.series {
        if series.points.is_empty() {
            continue;
        }
        match series.kind {
            SeriesKind::Line if series.points.len() >= 2 => {
                if let Some(stats) = stats.as_deref_mut() {
                    stats.line_series_visited += 1;
                }
                let columns = line_columns(plot.width, painter.ctx().pixels_per_point());
                let visible = screen_line_points(
                    &series.points,
                    fig.x.min.min(fig.x.max),
                    fig.x.min.max(fig.x.max),
                    columns,
                );
                if let Some(stats) = stats.as_deref_mut() {
                    if matches!(visible, Cow::Owned(_)) {
                        stats.line_source_points_scanned += visible_source_len(
                            &series.points,
                            fig.x.min.min(fig.x.max),
                            fig.x.min.max(fig.x.max),
                        );
                    }
                    stats.line_points_emitted += visible.len();
                }
                let pts: Vec<Pos2> = visible
                    .iter()
                    .map(|p| {
                        let (px, py) = proj.project(*p);
                        to_pos(px, py)
                    })
                    .collect();
                clipped.add(Shape::line(
                    pts,
                    Stroke::new(series.width * scale, col(series.color)),
                ));
            }
            SeriesKind::Points => {
                for p in &series.points {
                    let (px, py) = proj.project(*p);
                    clipped.circle_filled(to_pos(px, py), series.width * scale, col(series.color));
                }
            }
            SeriesKind::Line => {}
        }
    }
    paint_error_bars(&clipped, &proj, fig, scale, true);

    for curve in integral::layout(fig, plot, scale) {
        if curve.points.len() >= 2 {
            let points = curve.points.iter().map(|&(x, y)| to_pos(x, y)).collect();
            clipped.add(Shape::line(
                points,
                Stroke::new(curve.width * scale, col(curve.color)),
            ));
        }
        let galley = clipped.layout_no_wrap(
            curve.label.text,
            FontId::proportional(curve.label.font_size),
            col(curve.label.color),
        );
        let size = galley.size();
        let mut label = egui::epaint::TextShape::new(
            Pos2::new(-size.x * 0.5, -size.y * 0.5),
            galley,
            col(curve.label.color),
        )
        .with_angle_and_anchor(-std::f32::consts::FRAC_PI_2, Align2::CENTER_CENTER);
        label.pos += Vec2::new(curve.label.position.0, curve.label.position.1);
        clipped.add(label);
    }

    for a in &fig.annotations {
        let (px, py) = proj.project(a.at);
        clipped.text(
            Pos2::new(px, py),
            Align2::CENTER_BOTTOM,
            &a.text,
            FontId::proportional(a.size * scale),
            col(a.color),
        );
    }

    if let (Some(trace), Some(band)) = (&fig.top_projection, proj.top_band) {
        paint_projection(painter, fig, trace, plot, band, true, scale);
    }
    if let (Some(trace), Some(band)) = (&fig.left_projection, proj.left_band) {
        paint_projection(painter, fig, trace, plot, band, false, scale);
    }

    paint_legend(painter, plot, fig, scale);
}

fn paint_error_bars(
    painter: &egui::Painter,
    proj: &Projector<'_>,
    fig: &Figure,
    scale: f32,
    draw_over_data: bool,
) {
    for error_bar in &fig.error_bars {
        if error_bar.draw_over_data != draw_over_data {
            continue;
        }
        let Some(segments) = error_bar_segments(proj, error_bar, scale) else {
            continue;
        };
        let stroke = Stroke::new(error_bar.width * scale, col(error_bar.color));
        for [start, end] in segments {
            painter.line_segment(
                [Pos2::new(start.0, start.1), Pos2::new(end.0, end.1)],
                stroke,
            );
        }
    }
}

/// Two columns per physical pixel, so a pooled bucket stays sub-pixel.
/// `plot_width` is in egui points; without the conversion a HiDPI screen would
/// silently render at half its resolution.
fn line_columns(plot_width: f32, pixels_per_point: f32) -> usize {
    let physical = (plot_width * pixels_per_point.max(1.0)).max(1.0) as usize;
    physical
        .saturating_mul(2)
        .clamp(MIN_LINE_COLUMNS, MAX_LINE_COLUMNS)
}

/// Clip a line to the viewport, then pool it to `columns` min/max buckets when
/// the visible samples exceed the screen-space output budget. The envelope
/// preserves each bucket's extrema in source order; it is visually equivalent
/// at this sub-pixel density while avoiding tessellating invisible detail.
fn screen_line_points(
    points: &[[f64; 2]],
    x_min: f64,
    x_max: f64,
    columns: usize,
) -> Cow<'_, [[f64; 2]]> {
    // Keep one neighbour on each side for continuity. Handles ascending traces
    // (time) and descending ones (NMR ppm); a flat or non-monotonic series keeps
    // its whole extent, which is safe but less selective.
    let first_x = points.first().map(|p| p[0]);
    let last_x = points.last().map(|p| p[0]);
    let (start, end) = match (first_x, last_x) {
        (Some(first), Some(last)) if first < last => {
            let start = points
                .partition_point(|point| point[0] < x_min)
                .saturating_sub(1);
            let end = points
                .partition_point(|point| point[0] <= x_max)
                .saturating_add(1)
                .min(points.len());
            (start.min(end), end)
        }
        (Some(first), Some(last)) if first > last => {
            let start = points
                .partition_point(|point| point[0] > x_max)
                .saturating_sub(1);
            let end = points
                .partition_point(|point| point[0] >= x_min)
                .saturating_add(1)
                .min(points.len());
            (start.min(end), end)
        }
        _ => (0, points.len()),
    };
    let visible = &points[start..end];
    if visible.len() <= columns.saturating_mul(2) {
        return Cow::Borrowed(visible);
    }

    let bucket_count = columns.max(1);
    let bucket_size = visible.len().div_ceil(bucket_count);
    let mut pooled = Vec::with_capacity(bucket_count * 2 + 2);
    pooled.push(visible[0]);
    for bucket in visible.chunks(bucket_size) {
        let mut min_index = 0;
        let mut max_index = 0;
        for index in 1..bucket.len() {
            if bucket[index][1] < bucket[min_index][1] {
                min_index = index;
            }
            if bucket[index][1] > bucket[max_index][1] {
                max_index = index;
            }
        }
        if min_index <= max_index {
            pooled.push(bucket[min_index]);
            if max_index != min_index {
                pooled.push(bucket[max_index]);
            }
        } else {
            pooled.push(bucket[max_index]);
            pooled.push(bucket[min_index]);
        }
    }
    if let Some(last) = visible.last()
        && pooled.last() != Some(last)
    {
        pooled.push(*last);
    }
    Cow::Owned(pooled)
}

fn paint_projection(
    painter: &egui::Painter,
    fig: &Figure,
    trace: &AxisTrace,
    plot: Rect,
    band: Rect,
    along_x: bool,
    scale: f32,
) {
    let band_rect = egui::Rect::from_min_size(
        Pos2::new(band.left, band.top),
        Vec2::new(band.width, band.height),
    );
    // A hairline seats the band against the contour's shared edge.
    let seam = Stroke::new(0.75 * scale, col(Color::AXIS));
    if along_x {
        painter.line_segment(
            [
                Pos2::new(band.left, band.bottom()),
                Pos2::new(band.right(), band.bottom()),
            ],
            seam,
        );
    } else {
        painter.line_segment(
            [
                Pos2::new(band.right(), band.top),
                Pos2::new(band.right(), band.bottom()),
            ],
            seam,
        );
    }
    let pts: Vec<Pos2> = projection_points(fig, trace, plot, band, along_x)
        .into_iter()
        .map(|(x, y)| Pos2::new(x, y))
        .collect();
    if pts.len() < 2 {
        return;
    }
    painter.with_clip_rect(band_rect).add(Shape::line(
        pts,
        Stroke::new(trace.width * scale, col(trace.color)),
    ));
}

fn paint_legend(painter: &egui::Painter, plot: Rect, fig: &Figure, scale: f32) {
    let entries = legend_entries(fig);
    if !fig.show_legend || entries.len() < 2 {
        return;
    }
    let (row, sw, pad, font) = (15.0 * scale, 16.0 * scale, 6.0 * scale, 11.0 * scale);
    let chars = entries
        .iter()
        .map(|(n, _, _)| n.chars().count())
        .max()
        .unwrap_or(0);
    let box_w = sw + 5.0 * scale + chars as f32 * font * 0.6 + pad * 2.0;
    let box_h = entries.len() as f32 * row + pad * 2.0;
    let bx = (plot.right() - box_w - 8.0 * scale).max(plot.left + 2.0 * scale);
    let by = plot.top + 8.0 * scale;
    let box_rect = egui::Rect::from_min_size(Pos2::new(bx, by), Vec2::new(box_w, box_h));
    painter.rect_filled(box_rect, 3.0 * scale, Color32::from_white_alpha(217));
    painter.rect_stroke(
        box_rect,
        3.0 * scale,
        Stroke::new(0.75 * scale, col(Color::AXIS)),
        StrokeKind::Inside,
    );
    let font_id = FontId::proportional(font);
    for (i, (name, color, mark)) in entries.iter().enumerate() {
        let ly = by + pad + row * i as f32 + row * 0.5;
        let lx = bx + pad;
        match mark {
            LegendMark::Line => {
                painter.line_segment(
                    [Pos2::new(lx, ly), Pos2::new(lx + sw, ly)],
                    Stroke::new(2.0 * scale, col(*color)),
                );
            }
            LegendMark::Points => {
                painter.circle_filled(Pos2::new(lx + sw * 0.5, ly), 3.0 * scale, col(*color));
            }
            LegendMark::Rect => {
                painter.rect_filled(
                    egui::Rect::from_min_size(
                        Pos2::new(lx, ly - 4.0 * scale),
                        Vec2::new(sw, 8.0 * scale),
                    ),
                    1.0 * scale,
                    col(*color),
                );
            }
        }
        painter.text(
            Pos2::new(lx + sw + 5.0 * scale, ly),
            Align2::LEFT_CENTER,
            name,
            font_id.clone(),
            col(Color::AXIS),
        );
    }
}

/// Paint a fixed-size page document through a screen viewport. Page geometry is
/// left untouched; zoom/pan only affect the screen projection.
pub fn paint_document(
    painter: &egui::Painter,
    screen: Rect,
    document: &Document<'_>,
    viewport: DocumentViewport,
) {
    paint_document_with_stats(painter, screen, document, viewport, None);
}

pub fn paint_document_with_stats(
    painter: &egui::Painter,
    screen: Rect,
    document: &Document<'_>,
    viewport: DocumentViewport,
    mut stats: Option<&mut RenderStats>,
) {
    if let Some(stats) = stats.as_deref_mut() {
        stats.documents_painted += 1;
    }
    let page = Rect::new(
        screen.left + viewport.pan[0],
        screen.top + viewport.pan[1],
        document.width * viewport.zoom,
        document.height * viewport.zoom,
    );
    let page_rect = egui::Rect::from_min_size(
        Pos2::new(page.left, page.top),
        Vec2::new(page.width, page.height),
    );
    // Screen documents are page-clipped. Besides matching physical-page
    // semantics, this makes the page body the complete culling bound used by
    // the board; SVG and EMF paths are intentionally unaffected.
    let painter = painter.with_clip_rect(page_rect);
    painter.rect_filled(page_rect, 0.0, col(document.background));

    for item in &document.items {
        match item {
            DocumentItem::Plot(object) => {
                paint_document_object(&painter, page, object, viewport, stats.as_deref_mut())
            }
            DocumentItem::Overlay(overlay) => {
                paint_document_overlay(&painter, page, overlay, viewport)
            }
        }
    }
}

fn paint_document_object(
    painter: &egui::Painter,
    page: Rect,
    object: &DocumentObject,
    viewport: DocumentViewport,
    stats: Option<&mut RenderStats>,
) {
    if !object.visible {
        return;
    }
    let frame = Rect::new(
        page.left + object.frame.left * viewport.zoom,
        page.top + object.frame.top * viewport.zoom,
        object.frame.width * viewport.zoom,
        object.frame.height * viewport.zoom,
    );
    paint_with_stats(painter, frame, object.figure, viewport.zoom, stats);
    if let Some(title) = &object.title {
        let pos = Pos2::new(
            frame.left + title.position[0] * viewport.zoom,
            frame.top + title.position[1] * viewport.zoom,
        );
        let font = FontId::proportional((title.font_size * viewport.zoom).max(6.0));
        painter.text(
            pos,
            Align2::LEFT_TOP,
            &title.text,
            font.clone(),
            col(Color::BLACK),
        );
        painter.text(
            pos + Vec2::new(0.6, 0.0),
            Align2::LEFT_TOP,
            &title.text,
            font,
            col(Color::BLACK),
        );
    }
}

fn paint_document_overlay(
    painter: &egui::Painter,
    page: Rect,
    overlay: &DocumentOverlay,
    viewport: DocumentViewport,
) {
    if !overlay.visible {
        return;
    }
    let frame = Rect::new(
        page.left + overlay.frame.left * viewport.zoom,
        page.top + overlay.frame.top * viewport.zoom,
        overlay.frame.width * viewport.zoom,
        overlay.frame.height * viewport.zoom,
    );
    match &overlay.kind {
        OverlayKind::Text(t) => paint_overlay_text(painter, frame, t, viewport.zoom),
        OverlayKind::Shape(s) => paint_overlay_shape(painter, frame, s, viewport.zoom),
    }
}

fn paint_overlay_text(painter: &egui::Painter, frame: Rect, t: &OverlayText, zoom: f32) {
    if t.text.trim().is_empty() {
        return;
    }
    let size = (t.font_size * zoom).max(6.0);
    let (x, anchor) = match t.align {
        OverlayAlign::Left => (frame.left, Align2::LEFT_TOP),
        OverlayAlign::Center => (frame.left + frame.width * 0.5, Align2::CENTER_TOP),
        OverlayAlign::Right => (frame.right(), Align2::RIGHT_TOP),
    };
    let font = FontId::proportional(size);
    let mut y = frame.top;
    for line in t.text.lines() {
        painter.text(Pos2::new(x, y), anchor, line, font.clone(), col(t.color));
        if t.bold {
            // egui has no bold weight; a hair-offset second pass fakes it.
            painter.text(
                Pos2::new(x + 0.6, y),
                anchor,
                line,
                font.clone(),
                col(t.color),
            );
        }
        y += size * 1.25;
    }
}

fn paint_overlay_shape(painter: &egui::Painter, frame: Rect, s: &OverlayShape, zoom: f32) {
    let stroke = Stroke::new((s.stroke_width * zoom).max(0.5), col(s.stroke));
    let rect = egui::Rect::from_min_size(
        Pos2::new(frame.left, frame.top),
        Vec2::new(frame.width, frame.height),
    );
    match s.shape {
        OverlayShapeKind::Rect => {
            if let Some(fill) = s.fill {
                painter.rect_filled(rect, 0.0, col(fill));
            }
            painter.rect_stroke(rect, 0.0, stroke, StrokeKind::Inside);
        }
        OverlayShapeKind::Ellipse => {
            let pts = ellipse_points(frame);
            if let Some(fill) = s.fill {
                painter.add(Shape::convex_polygon(pts.clone(), col(fill), Stroke::NONE));
            }
            painter.add(Shape::closed_line(pts, stroke));
        }
        OverlayShapeKind::Line => {
            painter.line_segment(
                [
                    Pos2::new(frame.left, frame.top),
                    Pos2::new(frame.right(), frame.bottom()),
                ],
                stroke,
            );
        }
        OverlayShapeKind::Arrow => {
            let origin = (frame.left, frame.top);
            let tip = (frame.right(), frame.bottom());
            let [b1, b2] = arrow_head(origin, tip, zoom);
            painter.line_segment(
                [Pos2::new(origin.0, origin.1), Pos2::new(tip.0, tip.1)],
                stroke,
            );
            for barb in [b1, b2] {
                painter.line_segment([Pos2::new(tip.0, tip.1), Pos2::new(barb.0, barb.1)], stroke);
            }
        }
    }
}

fn ellipse_points(frame: Rect) -> Vec<Pos2> {
    let cx = frame.left + frame.width * 0.5;
    let cy = frame.top + frame.height * 0.5;
    let rx = frame.width * 0.5;
    let ry = frame.height * 0.5;
    (0..48)
        .map(|i| {
            let a = i as f32 / 48.0 * std::f32::consts::TAU;
            Pos2::new(cx + rx * a.cos(), cy + ry * a.sin())
        })
        .collect()
}

#[cfg(test)]
#[path = "screen_tests.rs"]
mod tests;
