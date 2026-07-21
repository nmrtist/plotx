use super::*;
use plotx_figure::{Axis, Figure};

#[test]
fn ticks_are_nice_and_bounded() {
    let t = ticks(0.0, 10.0, 5);
    assert!(t.contains(&0.0));
    assert!(t.iter().all(|&v| (0.0..=10.0).contains(&v)));
    assert!(t.len() >= 3 && t.len() <= 12);
}

#[test]
fn ticks_never_exceed_range() {
    // Regression: the loop used to emit a tick up to half a step beyond max,
    // whose label spilled past the plot border (left side on reversed NMR axes).
    for &(lo, hi) in &[(0.5, 8.6), (-3.3, 4.1), (0.0, 10.0), (1.0, 100.0)] {
        let t = ticks(lo, hi, 8);
        assert!(
            t.iter().all(|&v| v >= lo - 1e-6 && v <= hi + 1e-6),
            "ticks({lo}, {hi}) produced out-of-range value: {t:?}"
        );
    }
}

#[test]
fn narrow_axis_labels_keep_the_precision_carried_by_the_ticks() {
    let ticks = axis_ticks(76.5, 78.1, 8);
    assert!(ticks.labels.iter().any(|label| label == "77.8"));
    assert!(ticks.labels.iter().any(|label| label == "77.0"));
    assert_eq!(ticks.scale_exponent, None);
}

#[test]
fn large_axis_uses_one_shared_scientific_multiplier() {
    let ticks = axis_ticks(-500.0, 17_000.0, 5);
    assert_eq!(ticks.scale_exponent, Some(4));
    assert_eq!(ticks.multiplier().as_deref(), Some("×10⁴"));
    assert!(ticks.labels.iter().any(|label| label == "0.0"));
    assert!(ticks.labels.iter().any(|label| label == "1.5"));
    assert!(ticks.labels.iter().all(|label| !label.contains('e')));
}

#[test]
fn margins_follow_tick_width_and_keep_a_compact_label_gap() {
    let compact = Figure::new("", Axis::new("x", 0.0, 1.0), Axis::new("y", 0.0, 1.0));
    let wide_ticks = Figure::new(
        "",
        Axis::new("x", 0.0, 1.0),
        Axis::new("y", -9_000.0, 9_000.0),
    );
    let compact_margins = Margins::for_figure(&compact);
    let wide_margins = Margins::for_figure(&wide_ticks);

    assert!(compact_margins.left < 36.0);
    assert!(wide_margins.left > compact_margins.left);
    assert!(wide_margins.left < 45.0);
    assert!(compact_margins.bottom < 30.0);

    let labels = axis_ticks(wide_ticks.y.min, wide_ticks.y.max, 5);
    let widest = labels
        .labels
        .iter()
        .map(|label| estimated_text_width(label, wide_ticks.typography.tick_pt))
        .fold(0.0, f32::max);
    let tick_text_left = wide_margins.left - TICK_LENGTH - TICK_LABEL_PAD - widest;
    let y_title_right = OUTER_PAD + wide_ticks.typography.label_pt;
    assert!((tick_text_left - y_title_right - AXIS_LABEL_GAP).abs() < 1e-3);
}

#[test]
fn multipliers_get_their_own_text_rows() {
    let plain = Figure::new("t", Axis::new("x", 0.0, 1.0), Axis::new("y", 0.0, 1.0));
    let scientific = Figure::new(
        "t",
        Axis::new("x", 0.0, 90_000.0),
        Axis::new("y", 0.0, 90_000.0),
    );
    let plain_margins = Margins::for_figure(&plain);
    let sci_margins = Margins::for_figure(&scientific);
    let row = plain.typography.tick_pt + AXIS_LABEL_GAP;
    assert!((sci_margins.top - plain_margins.top - row).abs() < 1e-3);
    assert!((sci_margins.bottom - plain_margins.bottom - row).abs() < 1e-3);
}

#[test]
fn projection_bands_shrink_plot_and_share_edges() {
    use plotx_figure::AxisTrace;
    let mut fig = Figure::new(
        "t",
        Axis::new("x", 0.0, 10.0).reversed(true),
        Axis::new("y", 0.0, 20.0).reversed(true),
    );
    let outer = Rect::new(0.0, 0.0, 400.0, 300.0);
    let m = Margins::default();
    let (bare_w, bare_h) = {
        let bare = Projector::new(&fig, outer, &m);
        assert!(bare.top_band.is_none() && bare.left_band.is_none());
        (bare.plot.width, bare.plot.height)
    };

    let trace = AxisTrace {
        points: vec![[0.0, 0.0], [5.0, 1.0], [10.0, 0.0]],
        color: Color::TRACE,
        width: 1.0,
    };
    fig.top_projection = Some(trace.clone());
    fig.left_projection = Some(trace);
    let proj = Projector::new(&fig, outer, &m);
    let top = proj.top_band.expect("top band reserved");
    let left = proj.left_band.expect("left band reserved");
    // Bands hug the (now smaller) plot: top sits directly above it, left flush
    // to its left, sharing the plot's along-axis extent.
    assert!(proj.plot.width < bare_w && proj.plot.height < bare_h);
    assert!((top.bottom() - proj.plot.top).abs() < 1e-3);
    assert!((left.right() - proj.plot.left).abs() < 1e-3);
    assert!((top.width - proj.plot.width).abs() < 1e-3);

    // The trace lands inside its band.
    let pts = projection_points(
        &fig,
        fig.top_projection.as_ref().unwrap(),
        proj.plot,
        top,
        true,
    );
    assert!(
        pts.iter()
            .all(|(_, y)| *y >= top.top - 1e-3 && *y <= top.bottom() + 1e-3)
    );
}

#[test]
fn reversed_x_axis_flips_projection() {
    let fig = Figure::new(
        "t",
        Axis::new("ppm", 0.0, 10.0).reversed(true),
        Axis::new("i", 0.0, 1.0),
    );
    let outer = Rect::new(0.0, 0.0, 200.0, 100.0);
    let m = Margins::default();
    let proj = Projector::new(&fig, outer, &m);
    let (x_at_max, _) = proj.project([10.0, 0.5]);
    let (x_at_min, _) = proj.project([0.0, 0.5]);
    assert!(
        x_at_max < x_at_min,
        "reversed axis should put max on the left"
    );
}

#[test]
fn categorical_axis_labels_slots_and_thins_when_dense() {
    let axis = Axis::categorical("group", vec!["a".into(), "b".into(), "c".into()]);
    let t = axis_ticks_for(&axis, 8);
    assert_eq!(t.values, vec![0.0, 1.0, 2.0]);
    assert_eq!(t.labels, vec!["a", "b", "c"]);
    assert_eq!(t.scale_exponent, None);

    let many: Vec<String> = (0..40).map(|i| format!("c{i}")).collect();
    let dense = axis_ticks_for(&Axis::categorical("group", many), 8);
    assert!(dense.values.len() <= 8, "dense: {:?}", dense.labels);
    assert_eq!(dense.labels[0], "c0");

    // A zoomed window only labels visible slots.
    let mut axis = Axis::categorical("group", vec!["a".into(), "b".into(), "c".into()]);
    axis.min = 0.5;
    axis.max = 2.5;
    let zoomed = axis_ticks_for(&axis, 8);
    assert_eq!(zoomed.labels, vec!["b", "c"]);
}

#[test]
fn hidden_frame_collapses_margins_to_outer_pad() {
    let mut fig = Figure::new("", Axis::new("x", 0.0, 1.0), Axis::new("y", 0.0, 1.0));
    fig.axis_frame = plotx_figure::AxisFrame::Hidden;
    let m = Margins::for_figure(&fig);
    assert_eq!(m.left, OUTER_PAD);
    assert_eq!(m.right, OUTER_PAD);
    assert_eq!(m.top, OUTER_PAD);
    assert_eq!(m.bottom, OUTER_PAD);
}

#[test]
fn legend_merges_series_and_named_polygons_once() {
    let mut fig = Figure::new("", Axis::new("x", 0.0, 1.0), Axis::new("y", 0.0, 1.0));
    fig.series.push(plotx_figure::Series::line(
        "trace",
        vec![[0.0, 0.0], [1.0, 1.0]],
    ));
    for _ in 0..3 {
        fig.polygons.push(plotx_figure::Polygon::rect(
            "bars",
            0.0,
            0.5,
            0.0,
            1.0,
            Color::TRACE,
        ));
    }
    fig.polygons.push(plotx_figure::Polygon::rect(
        "",
        0.0,
        0.1,
        0.0,
        0.1,
        Color::TRACE,
    ));
    let entries = legend_entries(&fig);
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0].0, "trace");
    assert_eq!(entries[1].0, "bars");
    assert!(matches!(entries[1].2, LegendMark::Rect));
}

#[test]
fn heatmap_cells_project_every_finite_cell() {
    let fig = Figure::new("", Axis::new("x", 0.0, 2.0), Axis::new("y", 0.0, 2.0));
    let grid = plotx_figure::HeatmapGrid {
        rows: 2,
        cols: 2,
        values: vec![0.0, 1.0, f32::NAN, 3.0],
        x_bounds: [0.0, 2.0],
        y_bounds: [0.0, 2.0],
        colormap: plotx_figure::ColormapId::Viridis,
        value_range: [0.0, 3.0],
    };
    let outer = Rect::new(0.0, 0.0, 200.0, 200.0);
    let proj = Projector::new(&fig, outer, &Margins::default());
    let cells = heatmap_cells(&proj, &grid);
    assert_eq!(cells.len(), 3, "NaN cell must be skipped");
    assert!(cells.iter().all(|(r, _)| r.width > 0.0 && r.height > 0.0));
}
