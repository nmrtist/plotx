use super::escape;
use crate::{
    LegendMark, Rect, legend_entries, legend_entry_origin, legend_layout, legend_rect,
    renders_legend,
};
use plotx_figure::Figure;
use std::fmt::Write;

pub(super) fn write(s: &mut String, fig: &Figure, plot: Rect) {
    let entries = legend_entries(fig);
    if !renders_legend(fig) {
        return;
    }
    let font = fig.typography.legend_pt;
    let layout = legend_layout(fig, &entries);
    let sw = layout.swatch;
    let Some(box_geometry) = legend_rect(fig, plot, 1.0) else {
        return;
    };
    let (bx, by, box_w, box_h) = (
        box_geometry.left,
        box_geometry.top,
        box_geometry.width,
        box_geometry.height,
    );
    let _ = write!(
        s,
        r#"<rect x="{bx:.2}" y="{by:.2}" width="{box_w:.2}" height="{box_h:.2}" rx="3" fill="white" fill-opacity="0.85" stroke="{axis}" stroke-width="0.75"/>"#,
        axis = plotx_figure::Color::AXIS.to_hex(),
    );
    if !fig.guide_title.trim().is_empty() {
        let _ = write!(
            s,
            r#"<text x="{x:.2}" y="{y:.2}" font-size="{font}" font-weight="bold" fill="{axis}" dominant-baseline="hanging">{title}</text>"#,
            x = bx + layout.padding,
            y = by + layout.padding,
            axis = fig.typography.legend_color.to_hex(),
            title = escape(&fig.guide_title),
        );
    }
    for (i, (name, color, mark)) in entries.iter().enumerate() {
        let (ox, oy) = legend_entry_origin(&layout, i);
        let ly = by + oy;
        let lx = bx + ox;
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
            LegendMark::LinePoints => {
                let _ = write!(
                    s,
                    r#"<line x1="{lx:.2}" y1="{ly:.2}" x2="{x2:.2}" y2="{ly:.2}" stroke="{col}" stroke-width="2"/><circle cx="{cx:.2}" cy="{ly:.2}" r="3" fill="{col}"/>"#,
                    x2 = lx + sw,
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
            axis = fig.typography.legend_color.to_hex(),
            txt = escape(name),
        );
    }
}
