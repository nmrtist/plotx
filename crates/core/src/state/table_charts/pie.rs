//! Pie chart of one column: each table row becomes a wedge, labeled by its x
//! ruler value. Axis-less (hidden frame) with a locked aspect so the disc
//! stays circular at any frame shape.

use super::super::ChartContext;
use super::super::table::{TableDataset, series_color};
use super::{WHITE, empty_axes_figure, row_labels, title_of};
use plotx_figure::{Annotation, Axis, AxisFrame, Figure, Polygon};

const CENTER: [f64; 2] = [0.5, 0.5];
const RADIUS: f64 = 0.40;
/// Arc tessellation step. Wedges are additionally split so no sub-polygon
/// subtends more than 90°, keeping every emitted polygon convex (the
/// renderers' fill invariant).
const ARC_STEP_DEG: f64 = 6.0;

pub(crate) fn pie_figure(dataset: &TableDataset, ctx: &ChartContext) -> Figure {
    let plot = dataset.typed_plot_data(20_000).unwrap_or_default();
    let column = ctx
        .column
        .and_then(|column| {
            plot.series
                .iter()
                .position(|series| series.binding.value_column == column)
        })
        .unwrap_or(0);
    let Some(col) = plot.series.get(column) else {
        return empty_axes_figure(dataset, "", "");
    };
    let labels = row_labels(&plot.x);
    // Pies show non-negative parts of a whole; negative and non-finite rows
    // are excluded from the disc.
    let slices: Vec<(usize, f64)> = col
        .y
        .iter()
        .enumerate()
        .filter_map(|(row, &v)| (v.is_finite() && v > 0.0).then_some((row, v)))
        .collect();
    let total: f64 = slices.iter().map(|(_, v)| v).sum();
    if total <= 0.0 {
        return empty_axes_figure(dataset, "", "");
    }

    let mut fig = Figure::new(
        title_of(dataset),
        Axis::new("", 0.0, 1.0),
        Axis::new("", 0.0, 1.0),
    );
    fig.axis_frame = AxisFrame::Hidden;
    fig.lock_aspect = true;
    fig.show_legend = slices.len() >= 2;

    // Clockwise from 12 o'clock, the familiar spreadsheet convention.
    let mut start = 0.0f64;
    for (row, value) in &slices {
        let fraction = value / total;
        let sweep = fraction * 360.0;
        let color = series_color(*row);
        let name = labels
            .get(*row)
            .cloned()
            .unwrap_or_else(|| format!("row {row}"));

        // ≤ 90° convex fans per polygon; same name so the legend lists one entry.
        let mut sub_start = start;
        while sub_start < start + sweep - 1e-9 {
            let sub_end = (sub_start + 90.0).min(start + sweep);
            let mut points = vec![CENTER];
            let steps = ((sub_end - sub_start) / ARC_STEP_DEG).ceil().max(1.0) as usize;
            for k in 0..=steps {
                let angle = sub_start + (sub_end - sub_start) * k as f64 / steps as f64;
                points.push(arc_point(angle));
            }
            fig.polygons.push(Polygon::new(name.clone(), points, color));
            sub_start = sub_end;
        }

        // Percentage callout at the wedge centroid angle; slivers stay clean.
        if fraction >= 0.04 {
            let mid = start + sweep * 0.5;
            let [ax, ay] = arc_point_at(mid, RADIUS * 0.62);
            fig.annotations.push(Annotation {
                text: format!("{:.1}%", fraction * 100.0),
                at: [ax, ay],
                color: WHITE,
                size: 8.0,
            });
        }
        start += sweep;
    }
    fig
}

fn arc_point(angle_deg: f64) -> [f64; 2] {
    arc_point_at(angle_deg, RADIUS)
}

/// Clockwise-from-12-o'clock polar → figure coordinates.
fn arc_point_at(angle_deg: f64, radius: f64) -> [f64; 2] {
    let rad = angle_deg.to_radians();
    [
        CENTER[0] + radius * rad.sin(),
        CENTER[1] + radius * rad.cos(),
    ]
}

#[cfg(test)]
mod tests {
    use super::super::super::{
        ChartContext, FloatSeries, TableDataset, materialized_float_series_table,
    };
    use plotx_figure::AxisFrame;

    fn share_table(values: Vec<f64>) -> TableDataset {
        let n = values.len();
        materialized_float_series_table(
            (
                "Region".into(),
                "".into(),
                (1..=n).map(|i| Some(i as f64 * 10.0)).collect(),
            ),
            vec![FloatSeries {
                name: "share".to_owned(),
                unit: String::new(),
                values: values.into_iter().map(Some).collect(),
                uncertainty: None,
                fit: None,
            }],
            "plotx.test.pie-table.v1",
        )
        .unwrap()
    }

    #[test]
    fn wedges_cover_the_disc_with_convex_fans_and_percent_labels() {
        let fig = super::pie_figure(
            &share_table(vec![50.0, 30.0, 20.0]),
            &ChartContext::default(),
        );
        assert_eq!(fig.axis_frame, AxisFrame::Hidden);
        assert!(fig.lock_aspect && fig.show_legend);
        // 50% (180°) splits into 2 fans, 30% (108°) into 2, 20% (72°) into 1.
        assert_eq!(fig.polygons.len(), 5);
        let names: Vec<&str> = fig.polygons.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, vec!["10", "10", "20", "20", "30"]);
        assert_eq!(fig.annotations.len(), 3);
        assert_eq!(fig.annotations[0].text, "50.0%");
        // Every fan vertex stays inside the unit square.
        assert!(fig.polygons.iter().all(|p| {
            p.points
                .iter()
                .all(|pt| (0.0..=1.0).contains(&pt[0]) && (0.0..=1.0).contains(&pt[1]))
        }));
    }

    #[test]
    fn negative_zero_and_nan_rows_are_excluded() {
        let fig = super::pie_figure(
            &share_table(vec![60.0, -5.0, 0.0, f64::NAN, 40.0]),
            &ChartContext::default(),
        );
        let mut names: Vec<&str> = fig.polygons.iter().map(|p| p.name.as_str()).collect();
        names.dedup();
        assert_eq!(names, vec!["10", "50"]);
    }

    #[test]
    fn all_invalid_values_fall_back_to_empty_axes() {
        let fig = super::pie_figure(&share_table(vec![-1.0, 0.0]), &ChartContext::default());
        assert!(fig.polygons.is_empty());
        assert_eq!(fig.axis_frame, AxisFrame::Open);
    }
}
