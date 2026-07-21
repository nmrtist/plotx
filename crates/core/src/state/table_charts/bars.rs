//! Bar charts: the single-column bar (rows on the numeric x ruler) and the
//! grouped / stacked multi-column bars (rows as categorical slots).

use super::super::ChartContext;
use super::super::table::{TableDataset, series_color};
use super::{empty_axes_figure, row_labels, title_of};
use plotx_figure::{Axis, ErrorBar, Figure, Polygon};

impl TableDataset {
    /// A bar chart of one column's values against the table's x ruler: each row
    /// is a filled rectangle from the zero baseline to its value, sized in data
    /// space so bars scale with zoom like every other mark.
    pub fn bar_figure(&self, column: Option<plotx_data::ColumnId>) -> Figure {
        let plot = self.typed_plot_data(20_000).unwrap_or_default();
        let column = column
            .and_then(|column| {
                plot.series
                    .iter()
                    .position(|series| series.binding.value_column == column)
            })
            .unwrap_or(0);
        let col = plot.series.get(column);
        let (mut xlo, mut xhi) = (f64::INFINITY, f64::NEG_INFINITY);
        for &v in &plot.x {
            if v.is_finite() {
                xlo = xlo.min(v);
                xhi = xhi.max(v);
            }
        }
        let (mut ylo, mut yhi) = (0.0f64, f64::NEG_INFINITY);
        if let Some(col) = col {
            for (row, (&px, &v)) in plot.x.iter().zip(&col.y).enumerate() {
                if px.is_finite() && v.is_finite() {
                    ylo = ylo.min(v);
                    yhi = yhi.max(v);
                    if let Some(sigma) = plot_sigma(col, row) {
                        ylo = ylo.min(v - sigma);
                        yhi = yhi.max(v + sigma);
                    }
                }
            }
        }
        if !xlo.is_finite() {
            (xlo, xhi) = (0.0, 1.0);
        }
        if !yhi.is_finite() {
            yhi = 1.0;
        }
        let xr = (xhi - xlo).max(f64::MIN_POSITIVE);
        let yr = (yhi - ylo).max(f64::MIN_POSITIVE);
        let y_label = col
            .map(|c| c.name.clone())
            .unwrap_or_else(|| "Value".to_owned());
        let x = Axis::new(plot.x_label.clone(), xlo - 0.06 * xr, xhi + 0.06 * xr);
        let y = Axis::new(y_label, ylo - 0.02 * yr, yhi + 0.08 * yr);
        let mut fig = Figure::new(title_of(self), x, y);
        if let Some(col) = col {
            let color = series_color(column);
            let half = 0.5 * bar_width(&plot.x, xr);
            for (row, (&px, &py)) in plot.x.iter().zip(&col.y).enumerate() {
                if !px.is_finite() || !py.is_finite() {
                    continue;
                }
                if let Some(sigma) = plot_sigma(col, row) {
                    fig = fig.with_error_bar(
                        ErrorBar::symmetric([px, py], sigma)
                            .colored(color)
                            .over_data(),
                    );
                }
                fig = fig.with_polygon(Polygon::rect(
                    col.name.clone(),
                    px - half,
                    px + half,
                    0.0,
                    py,
                    color,
                ));
            }
        }
        fig
    }
}

/// Data-space bar width: 80% of the smallest gap between adjacent finite x
/// positions, so unevenly spaced rulers never overlap bars.
fn bar_width(x: &[f64], x_range: f64) -> f64 {
    let mut xs: Vec<f64> = x.iter().copied().filter(|v| v.is_finite()).collect();
    xs.sort_by(f64::total_cmp);
    xs.dedup();
    let min_gap = xs
        .windows(2)
        .map(|pair| pair[1] - pair[0])
        .fold(f64::INFINITY, f64::min);
    if min_gap.is_finite() {
        0.8 * min_gap
    } else {
        // A single bar has no neighbours to collide with.
        0.5 * x_range.max(1.0)
    }
}

/// Multi-column bars over categorical row slots: grouped side-by-side, or
/// stacked (positive values up, negative values down) when `ctx.stacked`.
pub(crate) fn grouped_bar_figure(dataset: &TableDataset, ctx: &ChartContext) -> Figure {
    let plot = dataset.typed_plot_data(20_000).unwrap_or_default();
    let n_rows = plot.x.len();
    let n_cols = plot.series.len();
    if n_rows == 0 || n_cols == 0 {
        return empty_axes_figure(dataset, &plot.x_label, "Value");
    }

    let mut fig = Figure::new(
        title_of(dataset),
        Axis::categorical(plot.x_label.clone(), row_labels(&plot.x)),
        Axis::new("Value", 0.0, 1.0),
    );
    let (mut ylo, mut yhi) = (0.0f64, 0.0f64);

    if ctx.stacked {
        let width = 0.7;
        for row in 0..n_rows {
            let (mut up, mut down) = (0.0f64, 0.0f64);
            for (j, col) in plot.series.iter().enumerate() {
                let Some(&v) = col.y.get(row).filter(|v| v.is_finite()) else {
                    continue;
                };
                let (y0, y1) = if v >= 0.0 {
                    let seg = (up, up + v);
                    up += v;
                    seg
                } else {
                    let seg = (down + v, down);
                    down += v;
                    seg
                };
                fig.polygons.push(
                    Polygon::rect(
                        col.name.clone(),
                        row as f64 - width * 0.5,
                        row as f64 + width * 0.5,
                        y0,
                        y1,
                        series_color(j),
                    )
                    .with_stroke(fig.background, 0.5),
                );
            }
            ylo = ylo.min(down);
            yhi = yhi.max(up);
        }
    } else {
        let width = 0.8 / n_cols as f64;
        for (j, col) in plot.series.iter().enumerate() {
            let color = series_color(j);
            for row in 0..n_rows {
                let Some(&v) = col.y.get(row).filter(|v| v.is_finite()) else {
                    continue;
                };
                let x0 = row as f64 - 0.4 + j as f64 * width;
                fig.polygons.push(Polygon::rect(
                    col.name.clone(),
                    x0,
                    x0 + width,
                    0.0,
                    v,
                    color,
                ));
                ylo = ylo.min(v);
                yhi = yhi.max(v);
                if let Some(sigma) = plot_sigma(col, row) {
                    fig.error_bars.push(
                        ErrorBar::symmetric([x0 + width * 0.5, v], sigma)
                            .colored(color)
                            .over_data(),
                    );
                    ylo = ylo.min(v - sigma);
                    yhi = yhi.max(v + sigma);
                }
            }
        }
    }

    let yr = (yhi - ylo).max(f64::MIN_POSITIVE);
    fig.y.min = if ylo < 0.0 { ylo - 0.05 * yr } else { 0.0 };
    fig.y.max = yhi + 0.08 * yr;
    fig.show_legend = n_cols >= 2;
    fig
}

fn plot_sigma(series: &super::super::table_native::TypedPlotSeries, row: usize) -> Option<f64> {
    series
        .uncertainty
        .as_ref()?
        .get(row)
        .copied()
        .filter(|sigma| sigma.is_finite() && *sigma > 0.0)
}

#[cfg(test)]
mod tests {
    use super::super::super::{
        ChartContext, FloatSeries, TableDataset, materialized_float_series_table,
    };

    fn two_column_table() -> TableDataset {
        materialized_float_series_table(
            (
                "Gradient".into(),
                "mT/m".into(),
                vec![Some(0.0), Some(1.0), Some(2.0)],
            ),
            vec![
                FloatSeries {
                    name: "peak a".to_owned(),
                    unit: String::new(),
                    values: vec![Some(1.0), Some(0.5), Some(0.25)],
                    uncertainty: None,
                    fit: None,
                },
                FloatSeries {
                    name: "peak b".to_owned(),
                    unit: String::new(),
                    values: vec![Some(2.0), Some(1.0), Some(-0.5)],
                    uncertainty: Some(vec![Some(0.1), Some(0.2), Some(0.3)]),
                    fit: None,
                },
            ],
            "plotx.test.bar-table.v1",
        )
        .unwrap()
    }

    #[test]
    fn bar_figure_makes_one_rect_per_row_from_the_chosen_column() {
        let table = two_column_table();
        let fig = table.bar_figure(Some(table.series_bindings[1].value_column));
        assert_eq!(fig.polygons.len(), 3);
        assert!(fig.polygons.iter().all(|p| p.points.len() == 4));
        assert_eq!(fig.polygons[0].name, "peak b");
        assert_eq!(fig.y.label, "peak b");
        // Row 0 bar is centered on x = 0 and spans the zero baseline to 2.0.
        let p = &fig.polygons[0];
        assert!((p.points[0][0] + p.points[1][0]).abs() < 1e-9);
        assert_eq!(p.points[0][1], 0.0);
        assert_eq!(p.points[2][1], 2.0);
        // 80% of the min ruler gap (1.0).
        assert!((p.points[1][0] - p.points[0][0] - 0.8).abs() < 1e-9);
        assert_eq!(fig.error_bars.len(), 3);
        assert!(fig.error_bars.iter().all(|e| e.draw_over_data));
        assert!(fig.y.min < 0.0 && fig.y.max > 2.0);
    }

    #[test]
    fn grouped_bars_slot_columns_side_by_side_on_a_categorical_axis() {
        let fig = super::grouped_bar_figure(&two_column_table(), &ChartContext::default());
        let categories = fig.x.categories.as_ref().expect("categorical x");
        assert_eq!(categories, &["0", "1", "2"]);
        assert_eq!(fig.polygons.len(), 6);
        assert!(fig.show_legend);
        assert_eq!(fig.error_bars.len(), 3, "sigma only on the second column");
        // Both columns of row 0 stay within its slot [-0.4, 0.4].
        for p in fig.polygons.iter().filter(|p| p.points[0][0] < 0.5) {
            assert!(p.points.iter().all(|pt| pt[0] > -0.41 && pt[0] < 0.41));
        }
        assert!(fig.y.min < 0.0, "negative bar expands the range");
    }

    #[test]
    fn stacked_bars_accumulate_positive_up_and_negative_down() {
        let ctx = ChartContext {
            stacked: true,
            ..ChartContext::default()
        };
        let fig = super::grouped_bar_figure(&two_column_table(), &ctx);
        assert_eq!(fig.polygons.len(), 6);
        assert!(
            fig.error_bars.is_empty(),
            "stacked bars skip sigma whiskers"
        );
        // Row 0 stacks 1.0 then 2.0 on top → top of the second segment at 3.0.
        let row0: Vec<_> = fig
            .polygons
            .iter()
            .filter(|p| p.points[0][0].abs() < 0.5)
            .collect();
        let top = row0
            .iter()
            .flat_map(|p| p.points.iter().map(|pt| pt[1]))
            .fold(f64::NEG_INFINITY, f64::max);
        assert!((top - 3.0).abs() < 1e-9);
        // Row 2 has a negative segment reaching -0.5.
        let bottom = fig
            .polygons
            .iter()
            .flat_map(|p| p.points.iter().map(|pt| pt[1]))
            .fold(f64::INFINITY, f64::min);
        assert!((bottom + 0.5).abs() < 1e-9);
    }

    #[test]
    fn empty_table_still_yields_labeled_axes() {
        let dataset = materialized_float_series_table(
            ("x".into(), "".into(), Vec::new()),
            Vec::new(),
            "plotx.test.empty-table.v1",
        )
        .unwrap();
        let fig = super::grouped_bar_figure(&dataset, &ChartContext::default());
        assert!(fig.polygons.is_empty());
        assert_eq!(fig.x.label, "x");
    }
}
