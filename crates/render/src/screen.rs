pub use crate::screen_stats::RenderStats;
use crate::{
    AXIS_LINE_WIDTH, Document, DocumentItem, DocumentObject, DocumentOverlay, DocumentViewport,
    OUTER_PAD, OverlayAlign, OverlayKind, OverlayShape, OverlayShapeKind, OverlayText, Projector,
    Rect, TICK_LABEL_PAD, TICK_LENGTH, arrow_head, axis_layout, error_bar_segments, heatmap_cells,
    integral, polygon_outline, projection_points,
    screen_contours::paint_contours,
    screen_lod::{line_columns, screen_line_points},
};
use egui::{Align2, Color32, FontId, Pos2, Sense, Shape, Stroke, StrokeKind, Ui, Vec2};
use plotx_figure::{AxisFrame, AxisTrace, Color, Figure, SeriesKind};

mod color_scale;
mod legend;
mod sticks;

/// Screen geometry fidelity. Interactive mode bounds costly geometry by pixels.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ScreenRenderDetail {
    #[default]
    Full,
    /// Temporarily omit sub-pixel detail during workspace camera movement.
    Interactive,
}

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

/// Paint a figure into an existing painter at the supplied page-to-screen scale.
pub fn paint(painter: &egui::Painter, outer: Rect, fig: &Figure, scale: f32) {
    paint_with_stats(painter, outer, fig, scale, None);
}

pub fn paint_with_stats(
    painter: &egui::Painter,
    outer: Rect,
    fig: &Figure,
    scale: f32,
    stats: Option<&mut RenderStats>,
) {
    paint_with_detail_and_stats(painter, outer, fig, scale, ScreenRenderDetail::Full, stats);
}

/// Paint one figure with an explicit screen detail level and optional counters.
pub fn paint_with_detail_and_stats(
    painter: &egui::Painter,
    outer: Rect,
    fig: &Figure,
    scale: f32,
    detail: ScreenRenderDetail,
    stats: Option<&mut RenderStats>,
) {
    paint_with_detail_and_stats_impl(painter, outer, fig, scale, detail, None, stats);
}

fn paint_with_detail_and_stats_impl(
    painter: &egui::Painter,
    outer: Rect,
    fig: &Figure,
    scale: f32,
    detail: ScreenRenderDetail,
    geometry_generation: Option<u64>,
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
    // Keep F1 tick numbers outside a left projection band.
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

    crate::screen_annotations::paint(&clipped, fig, &proj, plot, scale);

    paint_contours(
        painter,
        &clipped,
        fig,
        &proj,
        plot,
        scale,
        detail,
        geometry_generation,
        &mut stats,
    );

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
                let columns = line_columns(detail, plot.width, painter.ctx().pixels_per_point());
                let visible = screen_line_points(
                    &series.points,
                    fig.x.min.min(fig.x.max),
                    fig.x.min.max(fig.x.max),
                    columns,
                );
                if let Some(stats) = stats.as_deref_mut() {
                    stats.record_line(
                        visible.source_points_visited,
                        visible.points.len(),
                        visible.pooled,
                    );
                }
                let pts: Vec<Pos2> = visible
                    .points
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
            SeriesKind::Stick => sticks::paint(&clipped, &proj, series, scale),
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

    legend::paint(painter, plot, fig, scale);
    color_scale::paint(painter, plot, fig, scale);
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

/// Paint a fixed-size page document through a zoomable screen viewport.
pub fn paint_document(
    painter: &egui::Painter,
    screen: Rect,
    document: &Document<'_>,
    viewport: DocumentViewport,
) {
    paint_document_impl(
        painter,
        screen,
        document,
        viewport,
        true,
        ScreenRenderDetail::Full,
        None,
    );
}

/// Paint the editable board representation. The page background stays bounded,
/// while document items may remain visible outside the page so users can
/// recover and reposition temporarily overflowing content.
pub fn paint_document_for_editor(
    painter: &egui::Painter,
    screen: Rect,
    document: &Document<'_>,
    viewport: DocumentViewport,
) {
    paint_document_for_editor_with_detail(
        painter,
        screen,
        document,
        viewport,
        ScreenRenderDetail::Full,
    );
}

/// Paint an editable document using the requested screen detail level.
pub fn paint_document_for_editor_with_detail(
    painter: &egui::Painter,
    screen: Rect,
    document: &Document<'_>,
    viewport: DocumentViewport,
    detail: ScreenRenderDetail,
) {
    paint_document_for_editor_with_detail_and_stats(
        painter, screen, document, viewport, detail, None,
    );
}

/// Paint an editable document with explicit detail and optional counters.
pub fn paint_document_for_editor_with_detail_and_stats(
    painter: &egui::Painter,
    screen: Rect,
    document: &Document<'_>,
    viewport: DocumentViewport,
    detail: ScreenRenderDetail,
    stats: Option<&mut RenderStats>,
) {
    paint_document_impl(painter, screen, document, viewport, false, detail, stats);
}

pub fn paint_document_with_stats(
    painter: &egui::Painter,
    screen: Rect,
    document: &Document<'_>,
    viewport: DocumentViewport,
    stats: Option<&mut RenderStats>,
) {
    paint_document_impl(
        painter,
        screen,
        document,
        viewport,
        true,
        ScreenRenderDetail::Full,
        stats,
    );
}

fn paint_document_impl(
    painter: &egui::Painter,
    screen: Rect,
    document: &Document<'_>,
    viewport: DocumentViewport,
    clip_items_to_page: bool,
    detail: ScreenRenderDetail,
    mut stats: Option<&mut RenderStats>,
) {
    if let Some(stats) = stats.as_deref_mut() {
        stats.record_document(detail);
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
    let page_painter = painter.with_clip_rect(page_rect);
    page_painter.rect_filled(page_rect, 0.0, col(document.background));
    let item_painter = if clip_items_to_page {
        page_painter
    } else {
        painter.clone()
    };

    for item in &document.items {
        match item {
            DocumentItem::Plot(object) => paint_document_object(
                &item_painter,
                page,
                object,
                viewport,
                detail,
                stats.as_deref_mut(),
            ),
            DocumentItem::Overlay(overlay) => {
                paint_document_overlay(&item_painter, page, overlay, viewport)
            }
            DocumentItem::Raster(raster) => {
                paint_document_raster(&item_painter, page, raster, viewport)
            }
            DocumentItem::PanelLabel {
                frame,
                text,
                visible,
            } if *visible => {
                let pos = Pos2::new(
                    page.left + (frame.left + text.position[0]) * viewport.zoom,
                    page.top + (frame.top + text.position[1]) * viewport.zoom,
                );
                let font = FontId::proportional((text.font_size * viewport.zoom).max(6.0));
                let color = crate::screen_raster::contrasting_label_color(
                    &document.items,
                    frame,
                    text,
                    document.background,
                );
                item_painter.text(pos, Align2::LEFT_TOP, &text.text, font.clone(), col(color));
                item_painter.text(
                    pos + Vec2::new(0.6, 0.0),
                    Align2::LEFT_TOP,
                    &text.text,
                    font,
                    col(color),
                );
            }
            DocumentItem::PanelLabel { .. } => {}
        }
    }
}

use crate::screen_raster::paint_document_raster;

fn paint_document_object(
    painter: &egui::Painter,
    page: Rect,
    object: &DocumentObject,
    viewport: DocumentViewport,
    detail: ScreenRenderDetail,
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
    paint_with_detail_and_stats_impl(
        painter,
        frame,
        object.figure,
        viewport.zoom,
        detail,
        object.geometry_generation,
        stats,
    );
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
