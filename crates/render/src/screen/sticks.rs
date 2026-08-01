use super::*;

pub(super) fn paint(
    painter: &egui::Painter,
    projector: &Projector<'_>,
    series: &plotx_figure::Series,
    scale: f32,
) {
    let stroke = Stroke::new(series.width * scale, col(series.color));
    for point in &series.points {
        let (x, baseline) = projector.project([point[0], 0.0]);
        let (_, y) = projector.project(*point);
        painter.line_segment([Pos2::new(x, baseline), Pos2::new(x, y)], stroke);
    }
}
