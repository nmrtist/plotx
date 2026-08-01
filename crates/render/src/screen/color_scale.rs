use super::col;
use crate::{Rect, color_scale_rect};
use egui::{Align2, FontId, Pos2, Stroke, StrokeKind, Vec2};
use plotx_figure::{Color, Figure};

pub(super) fn paint(painter: &egui::Painter, plot: Rect, fig: &Figure, scale: f32) {
    let (Some(heatmap), Some(rect)) = (&fig.heatmap, color_scale_rect(fig, plot, scale)) else {
        return;
    };
    let horizontal = rect.width > rect.height;
    const STEPS: usize = 64;
    for step in 0..STEPS {
        let q0 = step as f32 / STEPS as f32;
        let q1 = (step + 1) as f32 / STEPS as f32;
        let cell = if horizontal {
            egui::Rect::from_min_max(
                Pos2::new(rect.left + rect.width * q0, rect.top),
                Pos2::new(rect.left + rect.width * q1 + 0.5, rect.bottom()),
            )
        } else {
            egui::Rect::from_min_max(
                Pos2::new(rect.left, rect.top + rect.height * (1.0 - q1)),
                Pos2::new(rect.right(), rect.top + rect.height * (1.0 - q0) + 0.5),
            )
        };
        painter.rect_filled(cell, 0.0, col(heatmap.colormap.sample((q0 + q1) * 0.5)));
    }
    let border = egui::Rect::from_min_size(
        Pos2::new(rect.left, rect.top),
        Vec2::new(rect.width, rect.height),
    );
    painter.rect_stroke(
        border,
        0.0,
        Stroke::new(0.75 * scale, col(Color::AXIS)),
        StrokeKind::Inside,
    );
    let font = FontId::proportional(fig.typography.legend_pt * scale);
    let [min, max] = heatmap.value_range;
    let text_color = col(fig.typography.legend_color);
    if horizontal {
        painter.text(
            Pos2::new(rect.left, rect.bottom() + 2.0 * scale),
            Align2::LEFT_TOP,
            format_value(min),
            font.clone(),
            text_color,
        );
        painter.text(
            Pos2::new(rect.right(), rect.bottom() + 2.0 * scale),
            Align2::RIGHT_TOP,
            format_value(max),
            font,
            text_color,
        );
    } else {
        painter.text(
            Pos2::new(rect.right() + 3.0 * scale, rect.top),
            Align2::LEFT_TOP,
            format_value(max),
            font.clone(),
            text_color,
        );
        painter.text(
            Pos2::new(rect.right() + 3.0 * scale, rect.bottom()),
            Align2::LEFT_BOTTOM,
            format_value(min),
            font,
            text_color,
        );
    }
}

fn format_value(value: f32) -> String {
    if value.abs() >= 10_000.0 || (value != 0.0 && value.abs() < 0.001) {
        format!("{value:.2e}")
    } else {
        format!("{value:.3}")
            .trim_end_matches('0')
            .trim_end_matches('.')
            .to_owned()
    }
}
