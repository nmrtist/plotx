use super::col;
use crate::{
    LegendMark, Rect, legend_entries, legend_entry_origin, legend_layout, legend_rect,
    renders_legend,
};
use egui::{Align2, Color32, FontId, Pos2, Stroke, StrokeKind, Vec2};
use plotx_figure::{Color, Figure};

pub(super) fn paint(painter: &egui::Painter, plot: Rect, fig: &Figure, scale: f32) {
    let entries = legend_entries(fig);
    if !renders_legend(fig) {
        return;
    }
    let layout = legend_layout(fig, &entries);
    let font = fig.typography.legend_pt * scale;
    let sw = layout.swatch * scale;
    let Some(box_geometry) = legend_rect(fig, plot, scale) else {
        return;
    };
    let (bx, by) = (box_geometry.left, box_geometry.top);
    let box_rect = egui::Rect::from_min_size(
        Pos2::new(bx, by),
        Vec2::new(box_geometry.width, box_geometry.height),
    );
    painter.rect_filled(box_rect, 3.0 * scale, Color32::from_white_alpha(217));
    painter.rect_stroke(
        box_rect,
        3.0 * scale,
        Stroke::new(0.75 * scale, col(Color::AXIS)),
        StrokeKind::Inside,
    );
    let font_id = FontId::proportional(font);
    if !fig.guide_title.trim().is_empty() {
        painter.text(
            Pos2::new(bx + layout.padding * scale, by + layout.padding * scale),
            Align2::LEFT_TOP,
            &fig.guide_title,
            font_id.clone(),
            col(fig.typography.legend_color),
        );
    }
    for (i, (name, color, mark)) in entries.iter().enumerate() {
        let (ox, oy) = legend_entry_origin(&layout, i);
        let ly = by + oy * scale;
        let lx = bx + ox * scale;
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
            LegendMark::LinePoints => {
                painter.line_segment(
                    [Pos2::new(lx, ly), Pos2::new(lx + sw, ly)],
                    Stroke::new(2.0 * scale, col(*color)),
                );
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
            col(fig.typography.legend_color),
        );
    }
}
