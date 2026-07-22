//! Enhanced-metafile (EMF) exporter: records the document through Win32 GDI so
//! Office/WPS paste it as an editable vector. Mirrors the [`crate::svg`]
//! traversal one function per function.

use crate::{
    AXIS_LINE_WIDTH, Document, DocumentItem, DocumentObject, DocumentOverlay, LegendMark,
    OUTER_PAD, OverlayAlign, OverlayKind, OverlayShapeKind, Projector, Rect, TICK_LABEL_PAD,
    TICK_LENGTH, arrow_head, axis_layout, error_bar_segments, heatmap_cells, integral,
    legend_entries, polygon_outline, projection_points,
};
use plotx_figure::{AxisFrame, AxisTrace, Color, Figure, SeriesKind};
use std::collections::HashMap;
use windows_sys::Win32::Foundation::RECT;
use windows_sys::Win32::Graphics::Gdi::{
    CloseEnhMetaFile, CreateEnhMetaFileW, DeleteEnhMetaFile, GM_ADVANCED, GetDC, GetDeviceCaps,
    GetEnhMetaFileBits, HENHMETAFILE, HORZRES, HORZSIZE, MM_ANISOTROPIC, ReleaseDC, SetBkMode,
    SetGraphicsMode, SetMapMode, SetViewportExtEx, SetWindowExtEx, TA_CENTER, TA_LEFT, TA_RIGHT,
    TRANSPARENT, VERTRES, VERTSIZE,
};

mod dc;

use dc::{Dc, TextStyle, blend, l};

#[derive(Debug)]
pub struct EmfError(pub String);

impl std::fmt::Display for EmfError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "EMF export failed: {}", self.0)
    }
}

impl std::error::Error for EmfError {}

/// Record `doc` into EMF bytes suitable for a `.emf` file or CF_ENHMETAFILE
/// (via `SetEnhMetaFileBits`). Page geometry is in points, as for SVG.
pub fn export_document_emf(doc: &Document<'_>) -> Result<Vec<u8>, EmfError> {
    let (w, h) = (doc.width, doc.height);
    if !(w > 0.0 && h > 0.0) {
        return Err(EmfError("empty page".into()));
    }
    unsafe {
        let screen = GetDC(std::ptr::null_mut());
        if screen.is_null() {
            return Err(EmfError("no reference DC".into()));
        }
        // rclFrame is in 0.01 mm and fixes the physical paste size: 1 pt = 1/72 in.
        let frame = RECT {
            left: 0,
            top: 0,
            right: (w as f64 * 2540.0 / 72.0).round() as i32,
            bottom: (h as f64 * 2540.0 / 72.0).round() as i32,
        };
        let hdc = CreateEnhMetaFileW(screen, std::ptr::null(), &frame, std::ptr::null());
        if hdc.is_null() {
            ReleaseDC(std::ptr::null_mut(), screen);
            return Err(EmfError("CreateEnhMetaFileW failed".into()));
        }
        SetGraphicsMode(hdc, GM_ADVANCED);
        SetMapMode(hdc, MM_ANISOTROPIC);
        SetWindowExtEx(hdc, l(w), l(h), std::ptr::null_mut());
        let dev = |res: i32, size_mm: i32, extent_01mm: i32| -> i32 {
            ((extent_01mm as i64 * res as i64) / (size_mm as i64 * 100).max(1)) as i32
        };
        SetViewportExtEx(
            hdc,
            dev(
                GetDeviceCaps(screen, HORZRES as i32),
                GetDeviceCaps(screen, HORZSIZE as i32),
                frame.right,
            )
            .max(1),
            dev(
                GetDeviceCaps(screen, VERTRES as i32),
                GetDeviceCaps(screen, VERTSIZE as i32),
                frame.bottom,
            )
            .max(1),
            std::ptr::null_mut(),
        );
        SetBkMode(hdc, TRANSPARENT as i32);

        {
            let mut dc = Dc {
                hdc,
                pens: HashMap::new(),
                brushes: HashMap::new(),
                fonts: HashMap::new(),
            };
            dc.rect(Rect::new(0.0, 0.0, w, h), Some(doc.background), None);
            for item in &doc.items {
                match item {
                    DocumentItem::Plot(object) => write_document_object(&mut dc, object),
                    DocumentItem::Overlay(overlay) => {
                        if overlay.visible {
                            write_overlay(&mut dc, overlay);
                        }
                    }
                }
            }
        }

        let hemf: HENHMETAFILE = CloseEnhMetaFile(hdc);
        ReleaseDC(std::ptr::null_mut(), screen);
        if hemf.is_null() {
            return Err(EmfError("CloseEnhMetaFile failed".into()));
        }
        let size = GetEnhMetaFileBits(hemf, 0, std::ptr::null_mut());
        if size == 0 {
            DeleteEnhMetaFile(hemf);
            return Err(EmfError("GetEnhMetaFileBits failed".into()));
        }
        let mut bytes = vec![0u8; size as usize];
        let copied = GetEnhMetaFileBits(hemf, size, bytes.as_mut_ptr());
        DeleteEnhMetaFile(hemf);
        if copied != size {
            return Err(EmfError("metafile readback truncated".into()));
        }
        Ok(bytes)
    }
}

fn write_document_object(dc: &mut Dc, object: &DocumentObject<'_>) {
    if !object.visible {
        return;
    }
    write_figure(dc, object.figure, object.frame);
    if let Some(title) = &object.title {
        write_panel_letter(
            dc,
            &title.text,
            [
                title.position[0] + object.frame.left,
                title.position[1] + object.frame.top,
            ],
            title.font_size,
        );
    }
}

fn write_overlay(dc: &mut Dc, overlay: &DocumentOverlay<'_>) {
    let f = overlay.frame;
    match &overlay.kind {
        OverlayKind::Text(t) => {
            if t.text.trim().is_empty() {
                return;
            }
            let (x, align) = match t.align {
                OverlayAlign::Left => (f.left, TA_LEFT),
                OverlayAlign::Center => (f.left + f.width * 0.5, TA_CENTER),
                OverlayAlign::Right => (f.right(), TA_RIGHT),
            };
            let mut style = TextStyle::new(t.font_size, t.color, align);
            if t.bold {
                style = style.bold();
            }
            let mut y = f.top + t.font_size;
            for line in t.text.lines() {
                dc.text(line, (x, y), style);
                y += t.font_size * 1.25;
            }
        }
        OverlayKind::Shape(sh) => {
            let stroke = Some((sh.stroke, sh.stroke_width.max(0.5)));
            match sh.shape {
                OverlayShapeKind::Rect => dc.rect(f, sh.fill, stroke),
                OverlayShapeKind::Ellipse => dc.ellipse(f, sh.fill, stroke),
                OverlayShapeKind::Line => dc.line(
                    (f.left, f.top),
                    (f.right(), f.bottom()),
                    sh.stroke,
                    sh.stroke_width.max(0.5),
                ),
                OverlayShapeKind::Arrow => {
                    let origin = (f.left, f.top);
                    let tip = (f.right(), f.bottom());
                    let [b1, b2] = arrow_head(origin, tip, 1.0);
                    let w = sh.stroke_width.max(0.5);
                    dc.segments(&[[origin, tip], [tip, b1], [tip, b2]], sh.stroke, w);
                }
            }
        }
    }
}

fn write_figure(dc: &mut Dc, fig: &Figure, outer: Rect) {
    let ty = fig.typography;
    let layout = axis_layout(fig, outer.width, outer.height);
    let margins = layout.margins;
    let proj = Projector::new(fig, outer, &margins);
    let plot = proj.plot;

    dc.rect(outer, Some(fig.background), None);

    if !fig.title.trim().is_empty() {
        dc.text(
            &fig.title,
            (
                outer.left + outer.width / 2.0,
                outer.top + OUTER_PAD + ty.title_pt,
            ),
            TextStyle::new(ty.title_pt, Color::BLACK, TA_CENTER),
        );
    }

    let hidden_frame = fig.axis_frame == AxisFrame::Hidden;
    let (x_ticks, y_ticks) = (layout.x_ticks, layout.y_ticks);

    if fig.show_grid && !hidden_frame {
        for &xt in &x_ticks.values {
            let (px, _) = proj.project([xt, fig.y.min]);
            dc.line((px, plot.top), (px, plot.bottom()), Color::GRID, 1.0);
        }
        for &yt in &y_ticks.values {
            let (_, py) = proj.project([fig.x.min, yt]);
            dc.line((plot.left, py), (plot.right(), py), Color::GRID, 1.0);
        }
    }

    let y_axis_x = proj.left_band.map(|band| band.left).unwrap_or(plot.left);
    match fig.axis_frame {
        AxisFrame::Open => {
            dc.line(
                (plot.left, plot.bottom()),
                (plot.right(), plot.bottom()),
                Color::AXIS,
                AXIS_LINE_WIDTH,
            );
            dc.line(
                (y_axis_x, plot.top),
                (y_axis_x, plot.bottom()),
                Color::AXIS,
                AXIS_LINE_WIDTH,
            );
        }
        AxisFrame::Box => {
            dc.rect(plot, None, Some((Color::AXIS, AXIS_LINE_WIDTH)));
            if (y_axis_x - plot.left).abs() > f32::EPSILON {
                dc.line(
                    (y_axis_x, plot.top),
                    (y_axis_x, plot.bottom()),
                    Color::AXIS,
                    AXIS_LINE_WIDTH,
                );
            }
        }
        AxisFrame::Hidden => {}
    }

    for (&xt, label) in x_ticks.values.iter().zip(&x_ticks.labels) {
        let (px, _) = proj.project([xt, fig.y.min]);
        dc.line(
            (px, plot.bottom()),
            (px, plot.bottom() + TICK_LENGTH),
            Color::AXIS,
            AXIS_LINE_WIDTH,
        );
        if fig.x.show_tick_labels {
            dc.text(
                label,
                (
                    px,
                    plot.bottom() + TICK_LENGTH + TICK_LABEL_PAD + ty.tick_pt,
                ),
                TextStyle::new(ty.tick_pt, Color::AXIS, TA_CENTER),
            );
        }
    }
    let y_tick_x = y_axis_x - TICK_LENGTH - TICK_LABEL_PAD;
    for (&yt, label) in y_ticks.values.iter().zip(&y_ticks.labels) {
        let (_, py) = proj.project([fig.x.min, yt]);
        dc.line(
            (y_axis_x - TICK_LENGTH, py),
            (y_axis_x, py),
            Color::AXIS,
            AXIS_LINE_WIDTH,
        );
        if fig.y.show_tick_labels {
            dc.text(
                label,
                (y_tick_x, py),
                TextStyle::new(ty.tick_pt, Color::AXIS, TA_RIGHT).middle(),
            );
        }
    }
    if fig.y.show_tick_labels
        && let Some(multiplier) = y_ticks.multiplier()
    {
        dc.text(
            &multiplier,
            (y_axis_x, plot.top - TICK_LABEL_PAD),
            TextStyle::new(ty.tick_pt, Color::AXIS, TA_LEFT),
        );
    }
    if fig.x.show_tick_labels
        && let Some(multiplier) = x_ticks.multiplier()
    {
        dc.text(
            &multiplier,
            (plot.right(), outer.top + outer.height - OUTER_PAD),
            TextStyle::new(ty.tick_pt, Color::AXIS, TA_RIGHT),
        );
    }

    if !hidden_frame && fig.x.show_label {
        let multiplier_clearance = if fig.x.show_tick_labels {
            x_ticks.multiplier_clearance(ty.tick_pt)
        } else {
            0.0
        };
        dc.text(
            &fig.x.label,
            (
                (plot.left + plot.right()) / 2.0,
                outer.top + outer.height - OUTER_PAD - multiplier_clearance,
            ),
            TextStyle::new(ty.label_pt, Color::AXIS, TA_CENTER),
        );
    }
    if !hidden_frame && fig.y.show_label {
        dc.text(
            &fig.y.label,
            (
                outer.left + OUTER_PAD + ty.label_pt * 0.5,
                (plot.top + plot.bottom()) / 2.0,
            ),
            TextStyle::new(ty.label_pt, Color::AXIS, TA_CENTER).rotated(),
        );
    }

    dc.clipped(plot, |dc| {
        if let Some(grid) = &fig.heatmap {
            for (cell, color) in heatmap_cells(&proj, grid) {
                dc.rect(cell, Some(color), None);
            }
        }
        for poly in &fig.polygons {
            let Some(outline) = polygon_outline(&proj, poly) else {
                continue;
            };
            // GDI has no per-primitive alpha; pre-blend against the background.
            let fill = blend(poly.fill, poly.opacity.clamp(0.0, 1.0), fig.background);
            dc.polygon(&outline, Some(fill), poly.stroke);
        }
        for contour in &fig.contours {
            let segs: Vec<[(f32, f32); 2]> = contour
                .segments
                .iter()
                .map(|seg| [proj.project(seg[0]), proj.project(seg[1])])
                .collect();
            dc.segments(&segs, contour.color, contour.width);
        }
        write_error_bars(dc, fig, &proj, false);
        for series in &fig.series {
            if series.points.is_empty() {
                continue;
            }
            match series.kind {
                SeriesKind::Line if series.points.len() >= 2 => {
                    let pts: Vec<(f32, f32)> =
                        series.points.iter().map(|p| proj.project(*p)).collect();
                    dc.polyline(&pts, series.color, series.width);
                }
                SeriesKind::Points => {
                    for p in &series.points {
                        let (px, py) = proj.project(*p);
                        let r = series.width;
                        dc.ellipse(
                            Rect::new(px - r, py - r, r * 2.0, r * 2.0),
                            Some(series.color),
                            None,
                        );
                    }
                }
                SeriesKind::Line => {}
            }
        }
        write_error_bars(dc, fig, &proj, true);
        for curve in integral::layout(fig, plot, 1.0) {
            dc.polyline(&curve.points, curve.color, curve.width);
            dc.text(
                &curve.label.text,
                curve.label.position,
                TextStyle::new(curve.label.font_size, curve.label.color, TA_CENTER)
                    .middle()
                    .rotated(),
            );
        }
        for a in &fig.annotations {
            let (px, py) = proj.project(a.at);
            dc.text(
                &a.text,
                (px, py),
                TextStyle::new(a.size, a.color, TA_CENTER),
            );
        }
    });

    if let (Some(trace), Some(band)) = (&fig.top_projection, proj.top_band) {
        write_projection(dc, fig, trace, plot, band, true);
    }
    if let (Some(trace), Some(band)) = (&fig.left_projection, proj.left_band) {
        write_projection(dc, fig, trace, plot, band, false);
    }

    write_legend(dc, fig, plot);
}

fn write_error_bars(dc: &mut Dc, fig: &Figure, proj: &Projector<'_>, draw_over_data: bool) {
    for error_bar in &fig.error_bars {
        if error_bar.draw_over_data != draw_over_data {
            continue;
        }
        if let Some(segments) = error_bar_segments(proj, error_bar, 1.0) {
            dc.segments(&segments, error_bar.color, error_bar.width);
        }
    }
}

fn write_projection(
    dc: &mut Dc,
    fig: &Figure,
    trace: &AxisTrace,
    plot: Rect,
    band: Rect,
    along_x: bool,
) {
    let (a, b) = if along_x {
        ((band.left, band.bottom()), (band.right(), band.bottom()))
    } else {
        ((band.right(), band.top), (band.right(), band.bottom()))
    };
    dc.line(a, b, Color::AXIS, 0.75);
    let pts = projection_points(fig, trace, plot, band, along_x);
    if pts.len() < 2 {
        return;
    }
    dc.clipped(band, |dc| dc.polyline(&pts, trace.color, trace.width));
}

fn write_legend(dc: &mut Dc, fig: &Figure, plot: Rect) {
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
    let box_fill = blend(Color::rgb(255, 255, 255), 0.85, fig.background);
    dc.round_rect(
        Rect::new(bx, by, box_w, box_h),
        3.0,
        Some(box_fill),
        Some((Color::AXIS, 0.75)),
    );
    for (i, (name, color, mark)) in entries.iter().enumerate() {
        let ly = by + pad + row * i as f32 + row * 0.5;
        let lx = bx + pad;
        match mark {
            LegendMark::Line => dc.line((lx, ly), (lx + sw, ly), *color, 2.0),
            LegendMark::Points => dc.ellipse(
                Rect::new(lx + sw * 0.5 - 3.0, ly - 3.0, 6.0, 6.0),
                Some(*color),
                None,
            ),
            LegendMark::Rect => dc.rect(Rect::new(lx, ly - 4.0, sw, 8.0), Some(*color), None),
        }
        dc.text(
            name,
            (lx + sw + 5.0, ly),
            TextStyle::new(font, Color::AXIS, TA_LEFT).middle(),
        );
    }
}

fn write_panel_letter(dc: &mut Dc, text: &str, position: [f32; 2], font_size: f32) {
    if text.trim().is_empty() {
        return;
    }
    dc.text(
        text,
        (position[0], position[1] + font_size),
        TextStyle::new(font_size, Color::BLACK, TA_LEFT).bold(),
    );
}

#[cfg(test)]
mod tests;
