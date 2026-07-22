use crate::{
    AXIS_LINE_WIDTH, Document, DocumentItem, DocumentObject, DocumentOverlay, LegendMark,
    OUTER_PAD, OverlayAlign, OverlayKind, OverlayShapeKind, Projector, Rect, TICK_LABEL_PAD,
    TICK_LENGTH, arrow_head, axis_layout, error_bar_segments, heatmap_cells, integral,
    legend_entries, polygon_outline, projection_points,
};
use plotx_figure::{AxisFrame, AxisTrace, Figure, SeriesKind};
use std::fmt::Write as _;

/// Render a [`Figure`] to a standalone SVG document string.
pub fn export(fig: &Figure) -> String {
    let w = fig.width;
    let h = fig.height;
    let outer = Rect::new(0.0, 0.0, w, h);
    let mut s = String::new();
    let _ = write!(
        s,
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{w}" height="{h}" viewBox="0 0 {w} {h}" font-family="sans-serif">"#
    );
    write_figure(&mut s, fig, outer, "plot");
    let _ = write!(s, "</svg>");
    s
}

/// Render a page document to SVG using page points as the geometry space.
pub fn export_document(document: &Document<'_>) -> String {
    let w = document.width;
    let h = document.height;
    let mut s = String::new();
    let _ = write!(
        s,
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{w}pt" height="{h}pt" viewBox="0 0 {w} {h}" font-family="sans-serif">"#
    );
    let _ = write!(
        s,
        r#"<rect x="0" y="0" width="{w}" height="{h}" fill="{}"/>"#,
        document.background.to_hex()
    );
    for item in &document.items {
        match item {
            DocumentItem::Plot(object) => write_document_object(&mut s, object),
            DocumentItem::Overlay(overlay) => {
                if overlay.visible {
                    write_overlay(&mut s, overlay);
                }
            }
        }
    }
    let _ = write!(s, "</svg>");
    s
}

fn write_document_object(s: &mut String, object: &DocumentObject<'_>) {
    if !object.visible {
        return;
    }
    let id = escape_id(&object.id);
    let _ = write!(
        s,
        r#"<g id="{id}" transform="translate({x:.2},{y:.2})">"#,
        x = object.frame.left,
        y = object.frame.top,
    );
    write_figure(
        s,
        object.figure,
        Rect::new(0.0, 0.0, object.frame.width, object.frame.height),
        &format!("{id}_clip"),
    );
    if let Some(title) = &object.title {
        write_panel_letter(s, &title.text, title.position, title.font_size);
    }
    let _ = write!(s, "</g>");
}

fn write_overlay(s: &mut String, overlay: &DocumentOverlay<'_>) {
    let f = overlay.frame;
    match &overlay.kind {
        OverlayKind::Text(t) => {
            if t.text.trim().is_empty() {
                return;
            }
            let (x, anchor) = match t.align {
                OverlayAlign::Left => (f.left, "start"),
                OverlayAlign::Center => (f.left + f.width * 0.5, "middle"),
                OverlayAlign::Right => (f.right(), "end"),
            };
            let weight = if t.bold { "bold" } else { "normal" };
            let _ = write!(
                s,
                r#"<text x="{x:.2}" y="{y:.2}" text-anchor="{anchor}" font-size="{sz:.2}" font-weight="{weight}" fill="{col}">"#,
                y = f.top + t.font_size,
                sz = t.font_size,
                col = t.color.to_hex(),
            );
            for (i, line) in t.text.lines().enumerate() {
                let dy = if i == 0 { 0.0 } else { t.font_size * 1.25 };
                let _ = write!(
                    s,
                    r#"<tspan x="{x:.2}" dy="{dy:.2}">{}</tspan>"#,
                    escape(line)
                );
            }
            let _ = write!(s, "</text>");
        }
        OverlayKind::Shape(sh) => {
            let fill = sh
                .fill
                .map(|c| c.to_hex())
                .unwrap_or_else(|| "none".to_owned());
            let stroke = sh.stroke.to_hex();
            let sw = sh.stroke_width.max(0.5);
            match sh.shape {
                OverlayShapeKind::Rect => {
                    let _ = write!(
                        s,
                        r#"<rect x="{l:.2}" y="{t:.2}" width="{w:.2}" height="{h:.2}" fill="{fill}" stroke="{stroke}" stroke-width="{sw}"/>"#,
                        l = f.left,
                        t = f.top,
                        w = f.width,
                        h = f.height,
                    );
                }
                OverlayShapeKind::Ellipse => {
                    let _ = write!(
                        s,
                        r#"<ellipse cx="{cx:.2}" cy="{cy:.2}" rx="{rx:.2}" ry="{ry:.2}" fill="{fill}" stroke="{stroke}" stroke-width="{sw}"/>"#,
                        cx = f.left + f.width * 0.5,
                        cy = f.top + f.height * 0.5,
                        rx = f.width * 0.5,
                        ry = f.height * 0.5,
                    );
                }
                OverlayShapeKind::Line => {
                    let _ = write!(
                        s,
                        r#"<line x1="{x1:.2}" y1="{y1:.2}" x2="{x2:.2}" y2="{y2:.2}" stroke="{stroke}" stroke-width="{sw}"/>"#,
                        x1 = f.left,
                        y1 = f.top,
                        x2 = f.right(),
                        y2 = f.bottom(),
                    );
                }
                OverlayShapeKind::Arrow => {
                    let origin = (f.left, f.top);
                    let tip = (f.right(), f.bottom());
                    let [b1, b2] = arrow_head(origin, tip, 1.0);
                    let _ = write!(
                        s,
                        r#"<path d="M{ox:.2} {oy:.2}L{tx:.2} {ty:.2}M{tx:.2} {ty:.2}L{b1x:.2} {b1y:.2}M{tx:.2} {ty:.2}L{b2x:.2} {b2y:.2}" fill="none" stroke="{stroke}" stroke-width="{sw}"/>"#,
                        ox = origin.0,
                        oy = origin.1,
                        tx = tip.0,
                        ty = tip.1,
                        b1x = b1.0,
                        b1y = b1.1,
                        b2x = b2.0,
                        b2y = b2.1,
                    );
                }
            }
        }
    }
}

fn write_figure(s: &mut String, fig: &Figure, outer: Rect, clip_id: &str) {
    let ty = fig.typography;
    let w = outer.width;
    let h = outer.height;
    let layout = axis_layout(fig, outer.width, outer.height);
    let margins = layout.margins;
    let proj = Projector::new(fig, outer, &margins);
    let plot = proj.plot;

    let _ = write!(
        s,
        r#"<rect x="{x}" y="{y}" width="{w}" height="{h}" fill="{}"/>"#,
        fig.background.to_hex(),
        x = outer.left,
        y = outer.top
    );

    if !fig.title.trim().is_empty() {
        let _ = write!(
            s,
            r#"<text x="{cx}" y="{y}" text-anchor="middle" font-size="{font}" fill="{col}">{title}</text>"#,
            cx = outer.left + w / 2.0,
            y = outer.top + OUTER_PAD + ty.title_pt,
            font = ty.title_pt,
            col = plotx_figure::Color::BLACK.to_hex(),
            title = escape(&fig.title),
        );
    }

    let hidden_frame = fig.axis_frame == AxisFrame::Hidden;
    let (x_ticks, y_ticks) = (layout.x_ticks, layout.y_ticks);

    if fig.show_grid && !hidden_frame {
        let grid = plotx_figure::Color::GRID.to_hex();
        for &xt in &x_ticks.values {
            let (px, _) = proj.project([xt, fig.y.min]);
            let _ = write!(
                s,
                r#"<line x1="{px:.2}" y1="{t:.2}" x2="{px:.2}" y2="{b:.2}" stroke="{grid}" stroke-width="1"/>"#,
                t = plot.top,
                b = plot.bottom(),
            );
        }
        for &yt in &y_ticks.values {
            let (_, py) = proj.project([fig.x.min, yt]);
            let _ = write!(
                s,
                r#"<line x1="{l:.2}" y1="{py:.2}" x2="{r:.2}" y2="{py:.2}" stroke="{grid}" stroke-width="1"/>"#,
                l = plot.left,
                r = plot.right(),
            );
        }
    }

    let axis = plotx_figure::Color::AXIS.to_hex();
    let y_axis_x = proj.left_band.map(|band| band.left).unwrap_or(plot.left);
    match fig.axis_frame {
        AxisFrame::Open => {
            let _ = write!(
                s,
                r#"<path d="M{l:.2} {b:.2}H{r:.2}M{yl:.2} {t:.2}V{b:.2}" fill="none" stroke="{axis}" stroke-width="{width}"/>"#,
                l = plot.left,
                r = plot.right(),
                yl = y_axis_x,
                t = plot.top,
                b = plot.bottom(),
                width = AXIS_LINE_WIDTH,
            );
        }
        AxisFrame::Box => {
            let _ = write!(
                s,
                r#"<rect x="{l:.2}" y="{t:.2}" width="{w:.2}" height="{h:.2}" fill="none" stroke="{axis}" stroke-width="{width}"/>"#,
                l = plot.left,
                t = plot.top,
                w = plot.width,
                h = plot.height,
                width = AXIS_LINE_WIDTH,
            );
            if (y_axis_x - plot.left).abs() > f32::EPSILON {
                let _ = write!(
                    s,
                    r#"<path d="M{yl:.2} {t:.2}V{b:.2}" stroke="{axis}" stroke-width="{width}"/>"#,
                    yl = y_axis_x,
                    t = plot.top,
                    b = plot.bottom(),
                    width = AXIS_LINE_WIDTH,
                );
            }
        }
        AxisFrame::Hidden => {}
    }

    for (&xt, label) in x_ticks.values.iter().zip(&x_ticks.labels) {
        let (px, _) = proj.project([xt, fig.y.min]);
        let _ = write!(
            s,
            r#"<path d="M{px:.2} {b:.2}v{tick}" stroke="{axis}" stroke-width="{width}"/><text x="{px:.2}" y="{y:.2}" text-anchor="middle" font-size="{font}" fill="{axis}">{lab}</text>"#,
            b = plot.bottom(),
            tick = TICK_LENGTH,
            width = AXIS_LINE_WIDTH,
            y = plot.bottom() + TICK_LENGTH + TICK_LABEL_PAD + ty.tick_pt,
            font = ty.tick_pt,
            lab = escape(label),
        );
    }
    let y_tick_x = y_axis_x - TICK_LENGTH - TICK_LABEL_PAD;
    for (&yt, label) in y_ticks.values.iter().zip(&y_ticks.labels) {
        let (_, py) = proj.project([fig.x.min, yt]);
        let _ = write!(
            s,
            r#"<path d="M{yl:.2} {py:.2}h{tick}" stroke="{axis}" stroke-width="{width}"/><text x="{x:.2}" y="{py:.2}" text-anchor="end" font-size="{font}" fill="{axis}" dominant-baseline="middle">{lab}</text>"#,
            yl = y_axis_x - TICK_LENGTH,
            tick = TICK_LENGTH,
            width = AXIS_LINE_WIDTH,
            x = y_tick_x,
            font = ty.tick_pt,
            lab = escape(label),
        );
    }
    if let Some(multiplier) = y_ticks.multiplier() {
        let _ = write!(
            s,
            r#"<text x="{x:.2}" y="{y:.2}" text-anchor="start" font-size="{font}" fill="{axis}">{label}</text>"#,
            x = y_axis_x,
            y = plot.top - TICK_LABEL_PAD,
            font = ty.tick_pt,
            label = escape(&multiplier),
        );
    }
    if let Some(multiplier) = x_ticks.multiplier() {
        let _ = write!(
            s,
            r#"<text x="{x:.2}" y="{y:.2}" text-anchor="end" font-size="{font}" fill="{axis}">{label}</text>"#,
            x = plot.right(),
            y = outer.top + h - OUTER_PAD,
            font = ty.tick_pt,
            label = escape(&multiplier),
        );
    }

    if !hidden_frame {
        let _ = write!(
            s,
            r#"<text x="{cx:.2}" y="{y:.2}" text-anchor="middle" font-size="{font}" fill="{axis}">{lab}</text>"#,
            cx = (plot.left + plot.right()) / 2.0,
            y = outer.top + h - OUTER_PAD - x_ticks.multiplier_clearance(ty.tick_pt),
            font = ty.label_pt,
            lab = escape(&fig.x.label),
        );
        let _ = write!(
            s,
            r#"<text transform="translate({x:.2},{cy:.2}) rotate(-90)" text-anchor="middle" font-size="{font}" fill="{axis}">{lab}</text>"#,
            x = outer.left + OUTER_PAD + ty.label_pt * 0.5,
            cy = (plot.top + plot.bottom()) / 2.0,
            font = ty.label_pt,
            lab = escape(&fig.y.label),
        );
    }

    let _ = write!(
        s,
        r#"<clipPath id="{clip_id}"><rect x="{l:.2}" y="{t:.2}" width="{pw:.2}" height="{ph:.2}"/></clipPath>"#,
        l = plot.left,
        t = plot.top,
        pw = plot.width,
        ph = plot.height,
    );
    let _ = write!(s, r#"<g clip-path="url(#{clip_id})">"#);
    if let Some(grid) = &fig.heatmap {
        // crispEdges keeps abutting cells seam-free in SVG viewers.
        let _ = write!(s, r#"<g shape-rendering="crispEdges">"#);
        for (cell, color) in heatmap_cells(&proj, grid) {
            let _ = write!(
                s,
                r#"<rect x="{x:.2}" y="{y:.2}" width="{w:.2}" height="{h:.2}" fill="{col}"/>"#,
                x = cell.left,
                y = cell.top,
                w = cell.width,
                h = cell.height,
                col = color.to_hex(),
            );
        }
        let _ = write!(s, "</g>");
    }
    for poly in &fig.polygons {
        let Some(outline) = polygon_outline(&proj, poly) else {
            continue;
        };
        let mut pts = String::new();
        for (px, py) in outline {
            let _ = write!(pts, "{px:.2},{py:.2} ");
        }
        let opacity = if poly.opacity < 1.0 {
            format!(r#" fill-opacity="{:.3}""#, poly.opacity)
        } else {
            String::new()
        };
        let stroke = match poly.stroke {
            Some((c, w)) => format!(r#" stroke="{}" stroke-width="{w:.2}""#, c.to_hex()),
            None => String::new(),
        };
        let _ = write!(
            s,
            r#"<polygon points="{pts}" fill="{col}"{opacity}{stroke}/>"#,
            col = poly.fill.to_hex(),
        );
    }
    for contour in &fig.contours {
        let mut path = String::new();
        for seg in &contour.segments {
            let (ax, ay) = proj.project(seg[0]);
            let (bx, by) = proj.project(seg[1]);
            let _ = write!(path, "M{ax:.2} {ay:.2}L{bx:.2} {by:.2}");
        }
        let _ = write!(
            s,
            r#"<path d="{path}" fill="none" stroke="{col}" stroke-width="{w}"/>"#,
            col = contour.color.to_hex(),
            w = contour.width,
        );
    }
    write_error_bars(s, fig, &proj, false);
    for series in &fig.series {
        if series.points.is_empty() {
            continue;
        }
        match series.kind {
            SeriesKind::Line if series.points.len() >= 2 => {
                let mut pts = String::new();
                for p in &series.points {
                    let (px, py) = proj.project(*p);
                    let _ = write!(pts, "{px:.2},{py:.2} ");
                }
                let _ = write!(
                    s,
                    r#"<polyline points="{pts}" fill="none" stroke="{col}" stroke-width="{w}"/>"#,
                    col = series.color.to_hex(),
                    w = series.width,
                );
            }
            SeriesKind::Points => {
                for p in &series.points {
                    let (px, py) = proj.project(*p);
                    let _ = write!(
                        s,
                        r#"<circle cx="{px:.2}" cy="{py:.2}" r="{r:.2}" fill="{col}"/>"#,
                        r = series.width,
                        col = series.color.to_hex(),
                    );
                }
            }
            SeriesKind::Line => {}
        }
    }
    write_error_bars(s, fig, &proj, true);
    for curve in integral::layout(fig, plot, 1.0) {
        let mut points = String::new();
        for (x, y) in curve.points {
            let _ = write!(points, "{x:.2},{y:.2} ");
        }
        let _ = write!(
            s,
            r#"<polyline class="integral-curve" points="{points}" fill="none" stroke="{color}" stroke-width="{width}"/>"#,
            color = curve.color.to_hex(),
            width = curve.width,
        );
        let _ = write!(
            s,
            r#"<text class="integral-label" x="{x:.2}" y="{y:.2}" text-anchor="middle" dominant-baseline="middle" font-size="{font:.2}" fill="{color}" transform="rotate(-90 {x:.2} {y:.2})">{text}</text>"#,
            x = curve.label.position.0,
            y = curve.label.position.1,
            font = curve.label.font_size,
            color = curve.label.color.to_hex(),
            text = escape(&curve.label.text),
        );
    }
    for a in &fig.annotations {
        let (px, py) = proj.project(a.at);
        let _ = write!(
            s,
            r#"<text x="{px:.2}" y="{py:.2}" text-anchor="middle" font-size="{sz}" fill="{col}">{txt}</text>"#,
            sz = a.size,
            col = a.color.to_hex(),
            txt = escape(&a.text),
        );
    }
    let _ = write!(s, r#"</g>"#);

    if let (Some(trace), Some(band)) = (&fig.top_projection, proj.top_band) {
        write_projection(s, fig, trace, plot, band, true, clip_id);
    }
    if let (Some(trace), Some(band)) = (&fig.left_projection, proj.left_band) {
        write_projection(s, fig, trace, plot, band, false, clip_id);
    }

    write_legend(s, fig, plot);
}

fn write_error_bars(s: &mut String, fig: &Figure, proj: &Projector<'_>, draw_over_data: bool) {
    for error_bar in &fig.error_bars {
        if error_bar.draw_over_data != draw_over_data {
            continue;
        }
        let Some(segments) = error_bar_segments(proj, error_bar, 1.0) else {
            continue;
        };
        let mut path = String::new();
        for [start, end] in segments {
            let _ = write!(
                path,
                "M{:.2} {:.2}L{:.2} {:.2}",
                start.0, start.1, end.0, end.1
            );
        }
        let _ = write!(
            s,
            r#"<path class="error-bar" d="{path}" fill="none" stroke="{col}" stroke-width="{w}"/>"#,
            col = error_bar.color.to_hex(),
            w = error_bar.width,
        );
    }
}

fn write_projection(
    s: &mut String,
    fig: &Figure,
    trace: &AxisTrace,
    plot: Rect,
    band: Rect,
    along_x: bool,
    clip_id: &str,
) {
    let axis = plotx_figure::Color::AXIS.to_hex();
    let (x1, y1, x2, y2) = if along_x {
        (band.left, band.bottom(), band.right(), band.bottom())
    } else {
        (band.right(), band.top, band.right(), band.bottom())
    };
    let _ = write!(
        s,
        r#"<line x1="{x1:.2}" y1="{y1:.2}" x2="{x2:.2}" y2="{y2:.2}" stroke="{axis}" stroke-width="0.75"/>"#,
    );
    let pts = projection_points(fig, trace, plot, band, along_x);
    if pts.len() < 2 {
        return;
    }
    let clip = format!("{clip_id}_band_{}", if along_x { "top" } else { "left" });
    let _ = write!(
        s,
        r#"<clipPath id="{clip}"><rect x="{l:.2}" y="{t:.2}" width="{w:.2}" height="{h:.2}"/></clipPath>"#,
        l = band.left,
        t = band.top,
        w = band.width,
        h = band.height,
    );
    let mut poly = String::new();
    for (x, y) in pts {
        let _ = write!(poly, "{x:.2},{y:.2} ");
    }
    let _ = write!(
        s,
        r#"<polyline points="{poly}" fill="none" stroke="{col}" stroke-width="{w}" clip-path="url(#{clip})"/>"#,
        col = trace.color.to_hex(),
        w = trace.width,
    );
}

fn write_legend(s: &mut String, fig: &Figure, plot: Rect) {
    let entries = legend_entries(fig);
    if !fig.show_legend || entries.len() < 2 {
        return;
    }
    let (row, sw, pad, font) = (15.0f32, 16.0f32, 6.0f32, 11.0f32);
    let chars = entries
        .iter()
        .map(|(n, _, _)| n.chars().count())
        .max()
        .unwrap_or(0);
    let box_w = sw + 5.0 + chars as f32 * font * 0.6 + pad * 2.0;
    let box_h = entries.len() as f32 * row + pad * 2.0;
    let bx = (plot.right() - box_w - 8.0).max(plot.left + 2.0);
    let by = plot.top + 8.0;
    let _ = write!(
        s,
        r#"<rect x="{bx:.2}" y="{by:.2}" width="{box_w:.2}" height="{box_h:.2}" rx="3" fill="white" fill-opacity="0.85" stroke="{axis}" stroke-width="0.75"/>"#,
        axis = plotx_figure::Color::AXIS.to_hex(),
    );
    for (i, (name, color, mark)) in entries.iter().enumerate() {
        let ly = by + pad + row * i as f32 + row * 0.5;
        let lx = bx + pad;
        match mark {
            LegendMark::Line => {
                let _ = write!(
                    s,
                    r#"<line x1="{lx:.2}" y1="{ly:.2}" x2="{x2:.2}" y2="{ly:.2}" stroke="{col}" stroke-width="2"/>"#,
                    x2 = lx + sw,
                    col = color.to_hex(),
                );
            }
            LegendMark::Points => {
                let _ = write!(
                    s,
                    r#"<circle cx="{cx:.2}" cy="{ly:.2}" r="3" fill="{col}"/>"#,
                    cx = lx + sw * 0.5,
                    col = color.to_hex(),
                );
            }
            LegendMark::Rect => {
                let _ = write!(
                    s,
                    r#"<rect x="{lx:.2}" y="{y:.2}" width="{sw:.2}" height="8" rx="1" fill="{col}"/>"#,
                    y = ly - 4.0,
                    col = color.to_hex(),
                );
            }
        }
        let _ = write!(
            s,
            r#"<text x="{tx:.2}" y="{ly:.2}" font-size="{font}" fill="{axis}" dominant-baseline="middle">{txt}</text>"#,
            tx = lx + sw + 5.0,
            axis = plotx_figure::Color::AXIS.to_hex(),
            txt = escape(name),
        );
    }
}

fn write_panel_letter(s: &mut String, text: &str, position: [f32; 2], font_size: f32) {
    if text.trim().is_empty() {
        return;
    }
    let col = plotx_figure::Color::BLACK.to_hex();
    let _ = write!(
        s,
        r#"<text x="{x:.2}" y="{y:.2}" text-anchor="start" font-size="{font_size:.2}" font-weight="bold" fill="{col}">{txt}</text>"#,
        x = position[0],
        y = position[1] + font_size,
        txt = escape(text),
    );
}

fn escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn escape_id(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use plotx_figure::{Axis, AxisFrame, Color, ErrorBar, Figure, IntegralCurve, Series};

    #[test]
    fn exports_wellformed_ish_svg_with_polyline() {
        let fig = Figure::new(
            "Demo",
            Axis::new("ppm", 0.0, 10.0).reversed(true),
            Axis::new("intensity", 0.0, 1.0),
        )
        .with_series(Series::line(
            "trace",
            vec![[0.0, 0.0], [5.0, 1.0], [10.0, 0.0]],
        ));
        let out = export(&fig);
        assert!(out.starts_with("<svg"));
        assert!(out.trim_end().ends_with("</svg>"));
        assert!(out.contains("<polyline"));
        assert!(out.contains("Demo"));
    }

    #[test]
    fn exports_integral_result_curve_and_label() {
        let mut fig = Figure::new(
            "",
            Axis::new("ppm", 0.0, 2.0).reversed(true),
            Axis::new("intensity", 0.0, 1.0),
        )
        .with_series(Series::line(
            "trace",
            vec![[0.0, 1.0], [1.0, 2.0], [2.0, 1.0]],
        ));
        fig.integral_curves.push(IntegralCurve {
            start_ppm: 0.0,
            end_ppm: 2.0,
            normalized_area: 3.0,
            label: "3.000".to_owned(),
            color: Color::rgb(0x22, 0x8b, 0x57),
            width: 1.5,
            source_series: 0,
        });
        let out = export(&fig);
        assert!(out.contains("class=\"integral-curve\""));
        assert!(out.contains("class=\"integral-label\""));
        assert!(out.contains("3.000"));
        assert!(!out.contains("(ref)"));
        assert!(out.contains("rotate(-90"));
        assert!(!out.contains("integral-selection"));
    }

    #[test]
    fn escapes_xml_special_chars() {
        let fig = Figure::new(
            "A & B <test>",
            Axis::categorical("x", vec!["A & B".into(), "<ctrl>".into()]),
            Axis::categorical("y", vec!["north & south".into(), "<root>".into()]),
        );
        let out = export(&fig);
        assert!(out.contains("A &amp; B &lt;test&gt;"));
        assert!(out.contains("A &amp; B"));
        assert!(out.contains("&lt;ctrl&gt;"));
        assert!(out.contains("north &amp; south"));
        assert!(out.contains("&lt;root&gt;"));
        assert!(!out.contains(">A & B<"));
        assert!(!out.contains("><ctrl><"));
    }

    #[test]
    fn exports_error_bar_stem_and_caps_inside_the_plot_clip() {
        let fig = Figure::new("", Axis::new("x", 0.0, 1.0), Axis::new("y", 0.0, 2.0))
            .with_error_bar(ErrorBar::symmetric([0.5, 1.0], 0.25));
        let out = export(&fig);
        assert_eq!(out.matches("class=\"error-bar\"").count(), 1);
        assert!(out.contains("clip-path=\"url(#plot)\""));
    }

    #[test]
    fn foreground_error_bar_is_written_after_its_data_series() {
        let fig = Figure::new("", Axis::new("x", 0.0, 1.0), Axis::new("y", 0.0, 2.0))
            .with_series(Series::line("bar", vec![[0.5, 0.0], [0.5, 1.0]]))
            .with_error_bar(ErrorBar::symmetric([0.5, 1.0], 0.25).over_data());
        let out = export(&fig);
        assert!(out.find("<polyline").unwrap() < out.find("class=\"error-bar\"").unwrap());
    }

    #[test]
    fn exports_contextual_ticks_and_one_axis_multiplier() {
        let fig = Figure::new(
            "",
            Axis::new("ppm", 76.5, 78.1).reversed(true),
            Axis::new("intensity", -500.0, 17_000.0),
        );
        let out = export(&fig);
        assert!(out.contains(">77.8</text>"));
        assert!(out.contains("×10⁴"));
        assert!(!out.contains("e3</text>"));
        assert!(!out.contains("e4</text>"));
    }

    #[test]
    fn frame_style_is_open_for_1d_and_boxed_for_2d() {
        let open = export(&Figure::new(
            "",
            Axis::new("x", 0.0, 1.0),
            Axis::new("y", 0.0, 1.0),
        ));
        let open_rects = open.matches("<rect ").count();

        let boxed = export(
            &Figure::new("", Axis::new("x", 0.0, 1.0), Axis::new("y", 0.0, 1.0))
                .with_axis_frame(AxisFrame::Box),
        );
        assert_eq!(boxed.matches("<rect ").count(), open_rects + 1);
        assert!(boxed.contains("fill=\"none\" stroke=\"#272727\""));
    }

    #[test]
    fn tiny_figure_keeps_axis_lines_after_ticks_are_dropped() {
        let mut fig = Figure::new("", Axis::new("x", 0.0, 1.0), Axis::new("y", 0.0, 1.0));
        fig.width = 24.0;
        fig.height = 24.0;
        let outer = Rect::new(0.0, 0.0, fig.width, fig.height);
        let layout = axis_layout(&fig, fig.width, fig.height);
        assert!(layout.x_ticks.labels.is_empty() && layout.y_ticks.labels.is_empty());
        let plot = Projector::new(&fig, outer, &layout.margins).plot;
        let axis_path = format!(
            r##"<path d="M{l:.2} {b:.2}H{r:.2}M{l:.2} {t:.2}V{b:.2}" fill="none" stroke="#272727""##,
            l = plot.left,
            r = plot.right(),
            t = plot.top,
            b = plot.bottom(),
        );
        let svg = export(&fig);
        assert!(
            svg.contains(&axis_path),
            "missing open-axis path: {axis_path}"
        );
        assert_eq!(svg.matches("stroke=\"#272727\"").count(), 1);
    }
}
