//! Chart builders for the generic Table domain beyond the basic line chart:
//! bars, distributions (histogram / box / violin), matrix views (heatmap /
//! 3D surface), and pie. Each builder lowers a [`TableDataset`] plus its
//! [`ChartContext`](super::ChartContext) into the plain render IR, so no
//! renderer knows about chart types.

mod bars;
mod dist;
mod matrix;
mod pie;

pub(crate) use bars::grouped_bar_figure;
pub(crate) use dist::{box_figure, histogram_figure, violin_figure};
pub(crate) use matrix::{heatmap_figure, surface_figure};
pub(crate) use pie::pie_figure;

use super::table::TableDataset;
use plotx_figure::{Axis, Color, Figure};

pub(crate) const WHITE: Color = Color::rgb(255, 255, 255);

fn title_of(dataset: &TableDataset) -> String {
    dataset
        .name
        .clone()
        .unwrap_or_else(|| "Data table".to_owned())
}

/// A placeholder with labeled unit axes for tables that cannot produce the
/// requested chart (no columns, no finite data); keeps the canvas object
/// rendering instead of vanishing.
fn empty_axes_figure(dataset: &TableDataset, x_label: &str, y_label: &str) -> Figure {
    Figure::new(
        title_of(dataset),
        Axis::new(x_label, 0.0, 1.0),
        Axis::new(y_label, 0.0, 1.0),
    )
}

/// The finite observations of one column, unpaired from the x ruler (for
/// distribution charts, which summarize the column as a sample).
fn finite_values(values: &[f64]) -> Vec<f64> {
    values.iter().copied().filter(|v| v.is_finite()).collect()
}

/// One label per table row from the x ruler, using the shortest round-trip
/// float formatting so `0.05` stays `0.05`. Non-finite rows label as `–`.
fn row_labels(x: &[f64]) -> Vec<String> {
    x.iter()
        .map(|&v| {
            if v.is_finite() {
                format!("{v}")
            } else {
                "–".to_owned()
            }
        })
        .collect()
}

/// Darken a fill for use as its outline stroke.
fn darkened(c: Color, factor: f32) -> Color {
    let scale = |channel: u8| (channel as f32 * factor).round().clamp(0.0, 255.0) as u8;
    Color::rgb(scale(c.r), scale(c.g), scale(c.b))
}

#[cfg(test)]
mod tests {
    use crate::state::{
        ChartSpec, DataDomain, Dataset, FloatSeries, chart_types_for,
        materialized_float_series_table,
    };

    /// End-to-end sweep: every registered table chart builds from a realistic
    /// table (sigma, a NaN hole, a negative value) through the registry
    /// dispatch and survives the SVG exporter — the same path screen paint and
    /// EMF export share via the render IR.
    #[test]
    fn every_table_chart_builds_and_exports_svg() {
        let table = materialized_float_series_table(
            (
                "Time".into(),
                "s".into(),
                vec![Some(0.0), Some(0.5), Some(1.0), Some(1.5), Some(2.0)],
            ),
            vec![
                FloatSeries {
                    name: "signal".to_owned(),
                    unit: String::new(),
                    values: vec![Some(1.0), Some(2.0), None, Some(4.0), Some(3.0)],
                    uncertainty: Some(vec![Some(0.1), Some(0.2), Some(0.1), Some(0.3), Some(0.2)]),
                    fit: None,
                },
                FloatSeries {
                    name: "baseline".to_owned(),
                    unit: String::new(),
                    values: vec![Some(0.5), Some(-0.5), Some(1.5), Some(0.5), Some(2.5)],
                    uncertainty: None,
                    fit: None,
                },
            ],
            "plotx.test.chart-table.v1",
        )
        .unwrap();
        let dataset = Dataset::Table(Box::new(table));

        for chart in chart_types_for(DataDomain::Table) {
            for stacked in [false, true] {
                let spec = ChartSpec {
                    type_id: chart.id.to_owned(),
                    bins: Some(4),
                    stacked,
                    ..ChartSpec::default()
                };
                let fig = (chart.build)(&dataset, &spec.context(&dataset))
                    .unwrap_or_else(|| panic!("{} failed to build", chart.id));
                let svg = plotx_render::svg::export(&fig);
                assert!(
                    svg.starts_with("<svg") && svg.trim_end().ends_with("</svg>"),
                    "{} produced malformed SVG",
                    chart.id
                );
                // Every chart draws real content for this table, not bare axes.
                let has_marks =
                    !fig.series.is_empty() || !fig.polygons.is_empty() || fig.heatmap.is_some();
                assert!(has_marks, "{} drew nothing", chart.id);
            }
        }
    }
}
