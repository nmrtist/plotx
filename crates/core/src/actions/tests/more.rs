//! Chart-type, table-data, stacking, and axis-projection action tests.

use super::*;

#[test]
fn stacked_binding_builds_distinctly_coloured_series_with_legend() {
    let mut app = sample_app();
    app.doc
        .datasets
        .push(Dataset::Nmr(Box::new(NmrDataset::load(synthetic_1d()))));
    let object = app.doc.canvases[0].objects[0].id;
    let binding = crate::state::DataBinding {
        series: vec![
            crate::state::SeriesBinding::new(0),
            crate::state::SeriesBinding::new(1),
        ],
    };

    app.execute_action(Action::set_data_binding(
        0,
        object,
        crate::state::DataBinding::single(0),
        binding,
    ));

    let fig = &first_plot(&app).figure;
    assert!(fig.series.len() >= 2, "stack should draw both traces");
    assert!(fig.show_legend, "stack should show a legend");
    assert_ne!(
        fig.series[0].color, fig.series[1].color,
        "stacked traces must be distinctly coloured"
    );

    app.undo();
    assert_eq!(first_plot(&app).binding.series.len(), 1);
    assert!(!first_plot(&app).figure.show_legend);
}

#[test]
fn single_table_color_override_recolors_points_and_error_bars() {
    use crate::state::{ChartSpec, DataBinding, DataDomain, SeriesBinding, StackSpec};
    let (app, _) = table_app_with_sigma(vec![0.1, 0.1, 0.1]);
    let color = plotx_figure::Color::rgb(0xaa, 0x22, 0x44);
    let mut series = SeriesBinding::new(0);
    series.color = Some(color);
    let figure = app.build_binding_figure(
        &DataBinding {
            series: vec![series],
        },
        &ChartSpec::default_for(DataDomain::Table),
        &StackSpec::default(),
        [120.0, 80.0],
    );
    assert!(figure.series.iter().all(|series| series.color == color));
    assert!(
        figure
            .error_bars
            .iter()
            .all(|error_bar| error_bar.color == color)
    );
}

#[test]
fn single_table_color_override_recolors_bar_polygons() {
    use crate::state::{ChartSpec, DataBinding, SeriesBinding, StackSpec};
    let (app, _) = table_app_with_sigma(vec![0.1, 0.1, 0.1]);
    let color = plotx_figure::Color::rgb(0xaa, 0x22, 0x44);
    let mut series = SeriesBinding::new(0);
    series.color = Some(color);
    let figure = app.build_binding_figure(
        &DataBinding {
            series: vec![series],
        },
        &ChartSpec {
            type_id: "table_bar".to_owned(),
            ..ChartSpec::default()
        },
        &StackSpec::default(),
        [120.0, 80.0],
    );
    // The bar bodies are polygons; the override must reach them along with
    // their whiskers, or a rebuild silently resets a custom-coloured chart.
    assert!(!figure.polygons.is_empty());
    assert!(figure.polygons.iter().all(|polygon| polygon.fill == color));
    assert!(
        figure
            .error_bars
            .iter()
            .all(|error_bar| error_bar.color == color)
    );
}

#[test]
fn set_chart_type_switches_table_to_categorical_bars_and_undoes() {
    use crate::state::{AxisOverrides, AxisRange, ChartSpec};
    let (mut app, id) = table_app();

    let before = first_plot(&app).chart.clone();
    assert_eq!(before.type_id, "table_line");
    let line_series = first_plot(&app).figure.series.len();
    assert_eq!(line_series, 1, "line chart draws one series per column");
    let overrides = AxisOverrides {
        x_range: Some(AxisRange::new(1.0, 8.0)),
        ..AxisOverrides::default()
    };
    app.set_axis_overrides_value(0, id, &overrides);

    let after = ChartSpec {
        type_id: "table_bar_grouped".to_owned(),
        ..ChartSpec::default()
    };
    app.execute_action(Action::set_chart_type(0, id, before, after.clone()));
    assert_eq!(first_plot(&app).chart, after);
    // One-series grouped bars draw one filled rectangle per categorical row.
    assert_eq!(first_plot(&app).figure.polygons.len(), 3);
    assert!(first_plot(&app).figure.x.categories.is_some());
    assert_eq!(first_plot(&app).figure.x.min, -0.5);
    assert_eq!(first_plot(&app).figure.x.max, 2.5);
    assert_eq!(first_plot(&app).axis_overrides.x_range, overrides.x_range);

    app.undo();
    assert_eq!(first_plot(&app).chart.type_id, "table_line");
    assert_eq!(first_plot(&app).figure.series.len(), line_series);
    assert_eq!(first_plot(&app).viewport.full_x, AxisRange::new(1.0, 8.0));

    app.redo();
    assert_eq!(first_plot(&app).chart.type_id, "table_bar_grouped");
    assert_eq!(first_plot(&app).figure.polygons.len(), 3);
}

#[test]
fn stack_candidates_reject_incompatible_datasets() {
    let mut app = sample_app();
    app.doc
        .datasets
        .push(Dataset::Nmr(Box::new(NmrDataset::load(synthetic_1d()))));
    app.doc
        .datasets
        .push(Dataset::Nmr2D(Box::new(crate::state::Nmr2DDataset::load(
            synthetic_2d(),
        ))));
    let binding = crate::state::DataBinding::single(0);

    let candidates = app.stack_candidates(&binding);
    assert!(candidates.contains(&1), "the other 1D spectrum is eligible");
    assert!(
        !candidates.contains(&2),
        "a 2D dataset must not be a stack candidate"
    );

    // The 2D primary is a Field-stackable domain, but with no other 2D dataset
    // loaded there is nothing to overlay onto it.
    let two_d = crate::state::DataBinding::single(2);
    assert!(app.stack_candidates(&two_d).is_empty());
}

#[test]
fn axis_projections_attach_and_project_survive_undo() {
    use crate::state::{AxisProjection, AxisProjections, ProjectionSource};

    // dataset 0 = 1D (from sample_app), dataset 1 = a true-2D contour on canvas 1.
    let mut app = sample_app();
    app.doc
        .datasets
        .push(Dataset::Nmr2D(Box::new(crate::state::Nmr2DDataset::load(
            synthetic_2d(),
        ))));
    assert!(app.doc.datasets[1].as_nmr2d().unwrap().is_true_2d());
    push_canvas(&mut app, 1, "2d", [120.0, 80.0]);
    let ci = 1;
    let object = app.doc.canvases[ci].objects[0].id;

    // Top (F2) attaches the loaded 1D spectrum; Left (F1) is a sum projection.
    let before = AxisProjections::default();
    let after = AxisProjections {
        top: AxisProjection {
            source: ProjectionSource::Attached(0),
            visible: true,
        },
        left: AxisProjection {
            source: ProjectionSource::Sum,
            visible: true,
        },
    };
    app.execute_action(Action::SetAxisProjections {
        canvas: ci,
        object,
        before,
        after: after.clone(),
    });

    let fig = &app.doc.canvases[ci].objects[0].plot().unwrap().figure;
    let top = fig.top_projection.as_ref().expect("attached top trace");
    let left = fig.left_projection.as_ref().expect("sum left trace");
    let expected = app.doc.datasets[0].as_nmr().unwrap().spectrum.ppm.len();
    assert_eq!(top.points.len(), expected);
    let f1 = match &app.doc.datasets[1].as_nmr2d().unwrap().processed {
        plotx_processing::Processed2D::Ft(s) => s.f1_size,
        plotx_processing::Processed2D::Stack(_) => 0,
    };
    assert_eq!(left.points.len(), f1);
    assert_eq!(
        app.doc.canvases[ci].objects[0].plot().unwrap().projections,
        after
    );

    app.undo();
    let fig = &app.doc.canvases[ci].objects[0].plot().unwrap().figure;
    assert!(fig.top_projection.is_none() && fig.left_projection.is_none());
}

// Regression: an auto Phase step stores a placeholder pivot, so the on-plot pivot
// handle must report the peak the pass actually rotates about, not collapse onto a
// spectrum edge (which left manual phasing with nothing to grab).
#[test]
fn auto_phase_pivot_reports_the_peak_ppm() {
    use crate::state::PhaseAxis;

    let dataset = Dataset::Nmr(Box::new(NmrDataset::load(synthetic_1d())));
    let pivot = dataset.pivot_ppm(PhaseAxis::Direct).unwrap();
    assert!(
        (pivot - 2.0).abs() < 0.2,
        "pivot should sit on the ~2.0 ppm peak, got {pivot}"
    );
}

// The direct (F2) axis auto-phases by default, so its pivot follows the peak; the
// indirect (F1) axis defaults to zero phase, so its pivot stays at the frac-0 edge.
#[test]
fn auto_phase_pivot_reports_the_peak_ppm_2d() {
    use crate::state::PhaseAxis;

    let dataset = crate::state::Nmr2DDataset::load(synthetic_2d());
    let s = match &dataset.base {
        plotx_processing::Processed2D::Ft(s) => s,
        plotx_processing::Processed2D::Stack(_) => unreachable!("synthetic_2d is true-2D"),
    };
    let (f2_frac, _) = s.peak_pivot_fracs();
    let (lo2, hi2) = (s.f2_ppm[0], *s.f2_ppm.last().unwrap());
    let expect_f2 = lo2 + (hi2 - lo2) * f2_frac;
    assert!((dataset.pivot_ppm(PhaseAxis::F2).unwrap() - expect_f2).abs() < 1e-9);
    assert!((dataset.pivot_ppm(PhaseAxis::F1).unwrap() - s.f1_ppm[0]).abs() < 1e-9);
}

#[test]
fn manual_phase_inherits_the_automatic_solution() {
    use crate::state::PhaseAxis;

    let mut app = sample_app();
    let expected = app.doc.datasets[0]
        .automatic_phase_params(PhaseAxis::Direct)
        .unwrap();

    app.seed_manual_phase(0, PhaseAxis::Direct);

    let params = app.doc.datasets[0]
        .phase_params_mut(PhaseAxis::Direct)
        .unwrap();
    assert_eq!(params.auto, None);
    assert_eq!((params.phase0, params.phase1, params.pivot_frac), expected);
}

#[test]
fn manual_2d_phase_inherits_the_automatic_solution() {
    use crate::state::PhaseAxis;

    let mut app = PlotxApp::new();
    app.doc
        .datasets
        .push(Dataset::Nmr2D(Box::new(crate::state::Nmr2DDataset::load(
            synthetic_2d(),
        ))));
    let expected = app.doc.datasets[0]
        .automatic_phase_params(PhaseAxis::F2)
        .unwrap();

    app.seed_manual_phase(0, PhaseAxis::F2);

    let params = app.doc.datasets[0].phase_params_mut(PhaseAxis::F2).unwrap();
    assert_eq!(params.auto, None);
    assert_eq!((params.phase0, params.phase1, params.pivot_frac), expected);
}
