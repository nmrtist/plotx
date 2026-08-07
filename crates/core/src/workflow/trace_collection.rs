use crate::state::{DataBinding, Dataset, MM_TO_PT, OVERLAY_PALETTE, StackSpec};
use plotx_figure::Figure;

pub(super) fn initial_figure(
    dataset: &Dataset,
    binding: &DataBinding,
    size_mm: [f32; 2],
    fallback: Figure,
) -> Figure {
    let field = match dataset {
        Dataset::Electrophysiology(recording) => recording
            .field_key(recording.selected_channel)
            .and_then(|key| recording.field_catalog.id_for_key(key)),
        _ => dataset.default_field_id(),
    };
    let Some(field) = field else {
        return fallback;
    };
    let active = binding
        .series
        .iter()
        .filter(|series| series.source.resource == dataset.resource_id())
        .filter(|series| series.source.field == field)
        .collect::<Vec<_>>();
    if !active.iter().any(|series| series.source.item.is_some()) {
        return fallback;
    }
    let parts = active
        .into_iter()
        .filter_map(|series| dataset.trace_item_figure(series.source.field, series.source.item?))
        .collect::<Vec<_>>();
    let Some(first) = parts.first() else {
        return fallback;
    };
    let mut figure = first.clone();
    figure.series.clear();
    let peak = parts
        .iter()
        .flat_map(|part| &part.series)
        .flat_map(|series| &series.points)
        .fold(0.0_f64, |peak, point| peak.max(point[1].abs()));
    let (mut y_min, mut y_max) = (f64::INFINITY, f64::NEG_INFINITY);
    for (index, part) in parts.into_iter().enumerate() {
        for mut series in part.series {
            for point in &mut series.points {
                point[1] += index as f64 * StackSpec::default().spacing_y * peak;
                y_min = y_min.min(point[1]);
                y_max = y_max.max(point[1]);
            }
            series.color = OVERLAY_PALETTE[index % OVERLAY_PALETTE.len()];
            figure.series.push(series);
        }
    }
    if y_min.is_finite() && y_max.is_finite() {
        figure.y.min = y_min;
        figure.y.max = y_max;
    }
    figure.series_colors_are_semantic = true;
    figure.width = size_mm[0] * MM_TO_PT;
    figure.height = size_mm[1] * MM_TO_PT;
    figure
}
