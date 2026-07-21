//! Matrix charts over the whole table (rows × columns as a dense grid): the
//! colormapped heatmap and the fixed-view 3D surface. The surface projects at
//! build time (orthographic azimuth/elevation + painter's sort) and emits
//! plain filled triangles, so every render back-end works unchanged.

use super::super::ChartContext;
use super::super::table::TableDataset;
use super::{darkened, empty_axes_figure, row_labels, title_of};
use plotx_figure::{Axis, AxisFrame, Figure, HeatmapGrid, Polygon};

/// Upper bound on drawn heatmap cells; rows are strided beyond it so a huge
/// table cannot stall the painters (each cell is one filled rect per export).
const MAX_HEATMAP_CELLS: usize = 200_000;

/// Surface grids denser than this are strided down: beyond ~128 quads per axis
/// individual facets are subpixel anyway, and the polygon count explodes.
const MAX_SURFACE_DIM: usize = 128;

struct TableMatrix {
    rows: usize,
    cols: usize,
    /// Row-major, non-finite for missing cells.
    values: Vec<f32>,
    lo: f32,
    hi: f32,
    row_labels: Vec<String>,
    column_names: Vec<String>,
    row_axis_label: String,
}

/// Read a bounded dense view through stable presentation bindings. The typed
/// reader strides rows while decoding and columns are sampled afterwards, so
/// neither chart can force an unbounded materialization of the table.
fn table_matrix(dataset: &TableDataset, max_rows: usize, max_cols: usize) -> Option<TableMatrix> {
    let plot = dataset.typed_plot_data(max_rows.max(1)).ok()?;
    let column_stride = plot.series.len().div_ceil(max_cols.max(1)).max(1);
    let columns: Vec<_> = plot.series.iter().step_by(column_stride).collect();
    let (rows, cols) = (plot.x.len(), columns.len());
    if rows == 0 || cols == 0 {
        return None;
    }
    let mut values = Vec::with_capacity(rows * cols);
    let (mut lo, mut hi) = (f32::INFINITY, f32::NEG_INFINITY);
    for r in 0..rows {
        for col in &columns {
            let v = col.y.get(r).copied().unwrap_or(f64::NAN) as f32;
            if v.is_finite() {
                lo = lo.min(v);
                hi = hi.max(v);
            }
            values.push(v);
        }
    }
    if !lo.is_finite() {
        return None;
    }
    if lo == hi {
        // A flat matrix still needs a non-empty colormap range.
        lo -= 0.5;
        hi += 0.5;
    }
    let row_labels = row_labels(&plot.x);
    let column_names = columns.iter().map(|column| column.name.clone()).collect();
    Some(TableMatrix {
        rows,
        cols,
        values,
        lo,
        hi,
        row_labels,
        column_names,
        row_axis_label: plot.x_label,
    })
}

pub(crate) fn heatmap_figure(dataset: &TableDataset, ctx: &ChartContext) -> Figure {
    let columns = dataset.series_bindings.len().max(1);
    let max_rows = MAX_HEATMAP_CELLS.div_euclid(columns).max(1);
    let Some(matrix) = table_matrix(dataset, max_rows, columns) else {
        return empty_axes_figure(dataset, "", "");
    };

    let x = Axis::categorical("", matrix.column_names.clone());
    // Reversed so table row 0 reads at the top, like the sheet itself.
    let y =
        Axis::categorical(matrix.row_axis_label.clone(), matrix.row_labels.clone()).reversed(true);
    let mut fig = Figure::new(title_of(dataset), x, y);
    fig.axis_frame = AxisFrame::Box;
    fig.heatmap = Some(HeatmapGrid {
        rows: matrix.rows,
        cols: matrix.cols,
        values: matrix.values,
        x_bounds: [-0.5, matrix.cols as f64 - 0.5],
        y_bounds: [-0.5, matrix.rows as f64 - 0.5],
        colormap: ctx.colormap,
        value_range: [matrix.lo, matrix.hi],
    });
    fig
}

pub(crate) fn surface_figure(dataset: &TableDataset, ctx: &ChartContext) -> Figure {
    let matrix = match table_matrix(dataset, MAX_SURFACE_DIM, MAX_SURFACE_DIM) {
        // A surface needs a quad grid: at least 2×2 samples.
        Some(m) if m.rows >= 2 && m.cols >= 2 => m,
        _ => return empty_axes_figure(dataset, "", ""),
    };

    let (azimuth, elevation) = (
        (ctx.view_angles[0] as f64).to_radians(),
        (ctx.view_angles[1] as f64).to_radians(),
    );
    let (sin_az, cos_az) = azimuth.sin_cos();
    let (sin_el, cos_el) = elevation.sin_cos();
    let span = (matrix.hi - matrix.lo) as f64;

    // Normalize the grid to a unit cube centered on the origin, then project
    // orthographically: u spans the screen-horizontal axis, v the vertical,
    // depth orders facets for the painter's algorithm (larger = closer).
    let project = |r: usize, c: usize, z: f64| -> (f64, f64, f64) {
        let x = c as f64 / (matrix.cols - 1) as f64 - 0.5;
        let y = r as f64 / (matrix.rows - 1) as f64 - 0.5;
        let z = (z - matrix.lo as f64) / span - 0.5;
        let along = x * cos_az + y * sin_az;
        let u = -x * sin_az + y * cos_az;
        let v = -along * sin_el + z * cos_el;
        let depth = along * cos_el + z * sin_el;
        (u, v, depth)
    };

    struct Facet {
        depth: f64,
        points: Vec<[f64; 2]>,
        t_color: f32,
    }
    let mut facets: Vec<Facet> = Vec::new();
    let value = |r: usize, c: usize| matrix.values[r * matrix.cols + c] as f64;
    for r in 0..matrix.rows - 1 {
        for c in 0..matrix.cols - 1 {
            let corners = [(r, c), (r, c + 1), (r + 1, c + 1), (r + 1, c)];
            let zs: Vec<f64> = corners.iter().map(|&(rr, cc)| value(rr, cc)).collect();
            if zs.iter().any(|z| !z.is_finite()) {
                continue;
            }
            let mean_z = zs.iter().sum::<f64>() / 4.0;
            let t_color = ((mean_z - matrix.lo as f64) / span) as f32;
            let projected: Vec<(f64, f64, f64)> = corners
                .iter()
                .zip(&zs)
                .map(|(&(rr, cc), &z)| project(rr, cc, z))
                .collect();
            // A saddle cell's four projected corners can form a concave or
            // self-intersecting quad, breaking the renderer's convex-polygon
            // invariant; two triangles are convex by construction and each
            // sorts on its own depth. Both share the cell colour so the cell
            // still reads as one facet.
            for tri in [[0usize, 1, 2], [0, 2, 3]] {
                let depth = tri.iter().map(|&i| projected[i].2).sum::<f64>() / 3.0;
                let points = tri
                    .iter()
                    .map(|&i| [projected[i].0, projected[i].1])
                    .collect();
                facets.push(Facet {
                    depth,
                    points,
                    t_color,
                });
            }
        }
    }
    if facets.is_empty() {
        return empty_axes_figure(dataset, "", "");
    }
    // Painter's algorithm: farthest facets first.
    facets.sort_by(|a, b| a.depth.total_cmp(&b.depth));

    let (mut ulo, mut uhi, mut vlo, mut vhi) = (
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::INFINITY,
        f64::NEG_INFINITY,
    );
    for q in &facets {
        for p in &q.points {
            ulo = ulo.min(p[0]);
            uhi = uhi.max(p[0]);
            vlo = vlo.min(p[1]);
            vhi = vhi.max(p[1]);
        }
    }
    let ur = (uhi - ulo).max(f64::MIN_POSITIVE);
    let vr = (vhi - vlo).max(f64::MIN_POSITIVE);
    let mut fig = Figure::new(
        title_of(dataset),
        Axis::new("", ulo - 0.04 * ur, uhi + 0.04 * ur),
        Axis::new("", vlo - 0.04 * vr, vhi + 0.04 * vr),
    );
    fig.axis_frame = AxisFrame::Hidden;
    fig.lock_aspect = true;
    for q in facets {
        let fill = ctx.colormap.sample(q.t_color);
        fig.polygons.push(
            Polygon::new(String::new(), q.points, fill).with_stroke(darkened(fill, 0.55), 0.3),
        );
    }
    fig
}

#[cfg(test)]
mod tests {
    use super::super::super::{
        ChartContext, FloatSeries, TableDataset, materialized_float_series_table,
    };
    use plotx_figure::AxisFrame;

    fn grid_table(rows: usize, cols: usize) -> TableDataset {
        let series = (0..cols)
            .map(|c| FloatSeries {
                name: format!("col {c}"),
                unit: String::new(),
                values: (0..rows).map(|r| Some((r * cols + c) as f64)).collect(),
                uncertainty: None,
                fit: None,
            })
            .collect();
        materialized_float_series_table(
            (
                "Time".into(),
                "s".into(),
                (0..rows).map(|i| Some(i as f64)).collect(),
            ),
            series,
            "plotx.test.matrix-table.v1",
        )
        .unwrap()
    }

    #[test]
    fn heatmap_maps_the_whole_table_with_categorical_axes() {
        let fig = super::heatmap_figure(&grid_table(4, 3), &ChartContext::default());
        let grid = fig.heatmap.as_ref().expect("heatmap grid");
        assert_eq!((grid.rows, grid.cols), (4, 3));
        assert_eq!(grid.values.len(), 12);
        assert_eq!(grid.value_range, [0.0, 11.0]);
        assert_eq!(fig.x.categories.as_ref().unwrap().len(), 3);
        assert_eq!(fig.y.categories.as_ref().unwrap().len(), 4);
        assert!(fig.y.reversed, "row 0 reads at the top");
        assert_eq!(fig.axis_frame, AxisFrame::Box);
    }

    #[test]
    fn heatmap_strides_rows_beyond_the_cell_budget() {
        // 150k rows × 2 cols = 300k cells → stride 2 → 75k rows.
        let fig = super::heatmap_figure(&grid_table(150_000, 2), &ChartContext::default());
        let grid = fig.heatmap.as_ref().unwrap();
        assert!(grid.rows * grid.cols <= super::MAX_HEATMAP_CELLS);
        assert_eq!(grid.rows, 75_000);
    }

    #[test]
    fn surface_projects_sorted_triangles_with_hidden_axes() {
        let fig = super::surface_figure(&grid_table(5, 4), &ChartContext::default());
        // 4×3 cells, each split into two convex triangles.
        assert_eq!(fig.polygons.len(), 4 * 3 * 2);
        assert!(fig.polygons.iter().all(|p| p.points.len() == 3));
        assert_eq!(fig.axis_frame, AxisFrame::Hidden);
        assert!(fig.lock_aspect);
        assert!(fig.polygons.iter().all(|p| p.stroke.is_some()));
    }

    #[test]
    fn surface_caps_both_grid_dimensions() {
        // 300 columns exceed the per-axis cap → column stride 3 → 100 columns.
        let fig = super::surface_figure(&grid_table(3, 300), &ChartContext::default());
        assert_eq!(fig.polygons.len(), 2 * 2 * 99);
        // And a long table strides rows the same way.
        let fig = super::surface_figure(&grid_table(300, 3), &ChartContext::default());
        assert_eq!(fig.polygons.len(), 2 * 99 * 2);
    }

    #[test]
    fn degenerate_tables_fall_back_to_empty_axes() {
        let empty = materialized_float_series_table(
            ("x".into(), "".into(), Vec::new()),
            Vec::new(),
            "plotx.test.empty-table.v1",
        )
        .unwrap();
        assert!(
            super::heatmap_figure(&empty, &ChartContext::default())
                .heatmap
                .is_none()
        );
        // One row cannot form surface quads.
        let flat = grid_table(1, 3);
        assert!(
            super::surface_figure(&flat, &ChartContext::default())
                .polygons
                .is_empty()
        );
    }
}
