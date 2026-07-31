use crate::{Projector, Rect, TextAnchor, range_label_layout};
use egui::{Align2, Color32, FontId, Pos2, Stroke};
use plotx_figure::Figure;

pub(crate) fn paint(
    painter: &egui::Painter,
    figure: &Figure,
    projector: &Projector,
    plot: Rect,
    scale: f32,
) {
    for annotation in &figure.range_annotations {
        let (x0, _) = projector.project([annotation.x0, figure.y.min]);
        let (x1, _) = projector.project([annotation.x1, figure.y.min]);
        let rect = egui::Rect::from_min_max(
            Pos2::new(x0.min(x1), plot.top),
            Pos2::new(x0.max(x1), plot.bottom()),
        );
        let color = Color32::from_rgb(annotation.color.r, annotation.color.g, annotation.color.b);
        let fill = Color32::from_rgba_unmultiplied(
            annotation.color.r,
            annotation.color.g,
            annotation.color.b,
            (annotation.fill_opacity.clamp(0.0, 1.0) * 255.0).round() as u8,
        );
        painter.rect_filled(rect, 0.0, fill);
        painter.rect_stroke(
            rect,
            0.0,
            Stroke::new(annotation.width * scale, color),
            egui::StrokeKind::Inside,
        );
        let Some(label) = range_label_layout(
            plot,
            rect.left(),
            rect.right(),
            figure.typography.tick_pt * scale,
            &annotation.label,
            annotation.label_position,
        ) else {
            continue;
        };
        painter.text(
            Pos2::new(label.x, label.top),
            match label.anchor {
                TextAnchor::Left => Align2::LEFT_TOP,
                TextAnchor::Center => Align2::CENTER_TOP,
                TextAnchor::Right => Align2::RIGHT_TOP,
            },
            label.text,
            FontId::proportional(figure.typography.tick_pt * scale),
            color,
        );
    }
}
