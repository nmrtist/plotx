use super::*;
use crate::state::{
    DataDomain, DatasetLineage, DerivationKind, LineShapeKind, SeriesBinding, StackMode,
    StoredFittedPeak, StoredLineFit,
};
use plotx_processing::Slice1D;

fn lorentz(x0: f64, h: f64, w: f64, x: f64) -> f64 {
    let hw2 = (w / 2.0) * (w / 2.0);
    h * hw2 / ((x - x0) * (x - x0) + hw2)
}

fn two_lorentzian_dataset(name: &str) -> Dataset {
    let ppm: Vec<f64> = (0..600).map(|i| 10.0 * i as f64 / 599.0).collect();
    let values = ppm
        .iter()
        .map(|&x| {
            Complex64::new(
                0.2 + lorentz(3.0, 5.0, 0.3, x) + lorentz(6.0, 3.0, 0.4, x),
                0.0,
            )
        })
        .collect();
    let slice = Slice1D {
        ppm,
        values,
        nucleus: "1H".to_owned(),
        observe_freq_mhz: 400.0,
        position_ppm: None,
    };
    Dataset::Nmr(Box::new(NmrDataset::from_slice(slice, name.to_owned())))
}

fn two_lorentzian_app() -> PlotxApp {
    let mut app = PlotxApp::new();
    app.doc.save_include_view_snapshots = false;
    app.doc.datasets.push(two_lorentzian_dataset("two peaks"));
    push_canvas(&mut app, 0, "spectrum", [120.0, 80.0]);
    app.focus_single(0);
    app.session.active_canvas = Some(0);
    app
}

fn stored_sample(id: u64) -> StoredLineFit {
    StoredLineFit {
        id,
        lo: 1.0,
        hi: 5.0,
        shape: LineShapeKind::Lorentzian,
        peaks: vec![StoredFittedPeak {
            position: 3.0,
            height: 5.0,
            fwhm: 0.3,
            eta: None,
            area: 2.35,
            position_sigma: Some(0.01),
            height_sigma: Some(0.02),
            fwhm_sigma: Some(0.01),
            eta_sigma: None,
            area_sigma: Some(0.05),
        }],
        offset: 0.2,
        offset_sigma: Some(0.001),
        r2: 0.999,
    }
}

#[test]
fn run_line_fit_stores_inline_result_and_materializes_on_request() {
    let mut app = two_lorentzian_app();
    let canvases_before = app.doc.canvases.len();

    app.run_line_fit(0, 0.5, 9.5, LineShapeKind::Lorentzian)
        .expect("fit succeeds");

    let n = app.doc.datasets[0].as_nmr().unwrap();
    assert_eq!(n.line_fits.len(), 1);
    assert_eq!(n.next_line_fit_id, 1);
    let fit = &n.line_fits[0];
    assert_eq!(fit.peaks.len(), 2);
    assert!((fit.peaks[0].position - 3.0).abs() < 0.05);
    assert!((fit.peaks[1].position - 6.0).abs() < 0.05);
    assert!(fit.r2 > 0.99);

    assert_eq!(app.doc.datasets.len(), 1);
    assert_eq!(app.doc.canvases.len(), canvases_before);

    app.add_line_fit_result_to_board(0, fit.id)
        .expect("result can be added to board");
    assert_eq!(app.doc.datasets.len(), 2);
    assert_eq!(app.doc.canvases.len(), canvases_before + 1);
    let table = app.doc.datasets[1].as_table().unwrap();
    let rows = table.typed_rows(2, &[]).unwrap();
    assert_eq!(
        rows.columns[0].values,
        vec![
            plotx_data::ScalarValue::Float64(1.0),
            plotx_data::ScalarValue::Float64(2.0)
        ]
    );
    assert_eq!(rows.columns[0].schema.name, "peak");
    let names: Vec<&str> = table
        .series_bindings
        .iter()
        .map(|binding| {
            table
                .typed_state
                .envelope
                .revision
                .snapshot
                .schema
                .column(binding.value_column)
                .unwrap()
                .name
                .as_str()
        })
        .collect();
    assert_eq!(
        names,
        vec!["position (ppm)", "height", "fwhm (ppm)", "area"]
    );
    assert!(table.provenance.is_none());
    assert_eq!(
        app.doc.datasets[1].lineage(),
        Some(&DatasetLineage::new(DerivationKind::LineFitTable, [0]))
    );

    let series_names: Vec<&str> = app.doc.canvases[0].objects[0]
        .plot()
        .unwrap()
        .figure
        .series
        .iter()
        .map(|s| s.name.as_str())
        .collect();
    assert_eq!(series_names, vec!["real", "peak 1", "peak 2", "fit"]);

    app.undo();
    assert_eq!(app.doc.datasets[0].as_nmr().unwrap().line_fits.len(), 1);
    assert_eq!(app.doc.datasets.len(), 1);
    assert_eq!(app.doc.canvases.len(), canvases_before);

    app.undo();
    assert!(app.doc.datasets[0].as_nmr().unwrap().line_fits.is_empty());
    assert_eq!(
        app.doc.canvases[0].objects[0]
            .plot()
            .unwrap()
            .figure
            .series
            .len(),
        1
    );

    app.redo();
    assert_eq!(app.doc.datasets[0].as_nmr().unwrap().line_fits.len(), 1);
    assert_eq!(app.doc.datasets.len(), 1);
    app.redo();
    assert_eq!(app.doc.datasets.len(), 2);
    assert_eq!(app.doc.canvases.len(), canvases_before + 1);
}

#[test]
fn run_line_fit_rejects_bad_targets_without_side_effects() {
    let mut app = two_lorentzian_app();
    assert!(
        app.run_line_fit(5, 0.0, 10.0, LineShapeKind::Gaussian)
            .is_err()
    );
    assert!(
        app.run_line_fit(0, 20.0, 30.0, LineShapeKind::Gaussian)
            .is_err()
    );
    assert_eq!(app.doc.datasets.len(), 1);
    assert!(!app.can_undo());
}

#[test]
fn run_line_fit_uses_pseudo_voigt_eta_column() {
    let mut app = two_lorentzian_app();
    app.run_line_fit(0, 0.5, 9.5, LineShapeKind::PseudoVoigt)
        .expect("fit succeeds");
    app.add_line_fit_result_to_board(0, 0)
        .expect("result can be added to board");
    let table = app.doc.datasets[1].as_table().unwrap();
    assert_eq!(
        table
            .typed_state
            .envelope
            .revision
            .snapshot
            .schema
            .columns
            .last()
            .unwrap()
            .name,
        "eta"
    );
}

#[test]
fn set_line_fits_applies_reverts_and_skips_noops() {
    let mut app = two_lorentzian_app();
    let fit = stored_sample(0);

    app.execute_action(Action::set_line_fits(0, Vec::new(), vec![fit.clone()]));
    assert_eq!(
        app.doc.datasets[0].as_nmr().unwrap().line_fits,
        vec![fit.clone()]
    );

    app.undo();
    assert!(app.doc.datasets[0].as_nmr().unwrap().line_fits.is_empty());
    app.redo();
    assert_eq!(app.doc.datasets[0].line_fits(), std::slice::from_ref(&fit));

    let undo_before = app.session.undo_stack.len();
    app.execute_action(Action::set_line_fits(0, vec![fit.clone()], vec![fit]));
    assert_eq!(app.session.undo_stack.len(), undo_before);
}

#[test]
fn remove_line_fit_deletes_by_id_and_is_undoable() {
    let mut app = two_lorentzian_app();
    app.execute_action(Action::set_line_fits(
        0,
        Vec::new(),
        vec![stored_sample(0), stored_sample(1)],
    ));

    app.remove_line_fit(0, 0);
    let ids: Vec<u64> = app.doc.datasets[0]
        .line_fits()
        .iter()
        .map(|f| f.id)
        .collect();
    assert_eq!(ids, vec![1]);

    app.remove_line_fit(0, 99);
    assert_eq!(app.doc.datasets[0].line_fits().len(), 1);

    app.undo();
    assert_eq!(app.doc.datasets[0].line_fits().len(), 2);
}

#[test]
fn background_fit_rebuilds_undo_snapshot_at_completion() {
    let mut app = two_lorentzian_app();
    app.execute_action(Action::set_line_fits(
        0,
        Vec::new(),
        vec![stored_sample(5), stored_sample(6)],
    ));

    app.start_line_fit(0, 0.5, 9.5, LineShapeKind::Lorentzian)
        .expect("fit starts");
    assert!(
        app.start_line_fit(0, 0.5, 9.5, LineShapeKind::Lorentzian)
            .is_err()
    );

    app.remove_line_fit(0, 5);
    assert_eq!(app.doc.datasets[0].line_fits().len(), 1);

    while app.poll_line_fit() {
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    let ids: Vec<u64> = app.doc.datasets[0]
        .line_fits()
        .iter()
        .map(|f| f.id)
        .collect();
    assert_eq!(ids, vec![6, 0]);

    app.undo();
    let ids: Vec<u64> = app.doc.datasets[0]
        .line_fits()
        .iter()
        .map(|f| f.id)
        .collect();
    assert_eq!(ids, vec![6]);

    app.redo();
    assert_eq!(app.doc.datasets[0].line_fits().len(), 2);
}

#[test]
fn background_fit_result_dropped_when_dataset_vanishes() {
    let mut app = two_lorentzian_app();
    app.start_line_fit(0, 0.5, 9.5, LineShapeKind::Lorentzian)
        .expect("fit starts");
    app.doc.datasets.clear();
    while app.poll_line_fit() {
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    assert!(app.doc.datasets.is_empty());
    assert!(!app.can_undo());
    assert!(app.session.status.contains("discarded"));
}

#[test]
fn background_fit_can_be_cancelled_without_applying_a_result() {
    let mut app = two_lorentzian_app();
    app.start_line_fit(0, 0.5, 9.5, LineShapeKind::Lorentzian)
        .expect("fit starts");
    assert!(app.line_fit_progress().is_some());
    assert!(app.cancel_line_fit());
    assert!(app.line_fit_progress().is_none());
    assert!(!app.poll_line_fit());
    assert!(app.doc.datasets[0].line_fits().is_empty());
    assert!(app.session.status.contains("cancelled"));
}

#[test]
fn background_fit_result_dropped_when_datasets_shift_mid_fit() {
    let mut app = two_lorentzian_app();
    app.start_line_fit(0, 0.5, 9.5, LineShapeKind::Lorentzian)
        .expect("fit starts");
    let table = crate::state::materialized_float_series_table(
        ("x".into(), "".into(), vec![Some(0.0), Some(1.0)]),
        Vec::new(),
        "plotx.test.shift-table.v1",
    )
    .unwrap();
    let insert = Action::insert_dataset_with_default_canvas(
        &app,
        Dataset::Table(Box::new(table)),
        "sheet".to_owned(),
        [100.0, 80.0],
    );
    app.execute_action(insert);
    app.undo();
    while app.poll_line_fit() {
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    assert!(app.doc.datasets[0].line_fits().is_empty());
    assert!(app.session.status.contains("discarded"));
}

#[test]
fn single_table_figure_gains_line_fit_overlays() {
    let mut app = two_lorentzian_app();
    let table = crate::state::materialized_float_series_table(
        (
            "x".into(),
            "".into(),
            vec![Some(1.0), Some(2.0), Some(3.0), Some(4.0), Some(5.0)],
        ),
        vec![crate::state::FloatSeries {
            name: "y".to_owned(),
            unit: String::new(),
            values: vec![Some(0.2), Some(1.0), Some(5.0), Some(1.0), Some(0.2)],
            uncertainty: None,
            fit: None,
        }],
        "plotx.test.line-overlay-table.v1",
    )
    .unwrap();
    app.doc.datasets.push(Dataset::Table(Box::new(table)));
    app.execute_action(Action::set_line_fits(1, Vec::new(), vec![stored_sample(0)]));
    assert_eq!(app.doc.datasets[1].line_fits().len(), 1);

    let fig = app.build_binding_figure(
        &DataBinding::single(1),
        &ChartSpec::default_for(DataDomain::Table),
        &StackSpec::default(),
        [120.0, 80.0],
    );
    let names: Vec<&str> = fig.series.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(names, vec!["y", "peak 1", "fit"]);
}

#[test]
fn non_line_table_charts_skip_line_fit_overlays() {
    let mut app = two_lorentzian_app();
    let table = crate::state::materialized_float_series_table(
        (
            "x".into(),
            "".into(),
            vec![Some(1.0), Some(2.0), Some(3.0), Some(4.0), Some(5.0)],
        ),
        vec![crate::state::FloatSeries {
            name: "y".to_owned(),
            unit: String::new(),
            values: vec![Some(0.2), Some(1.0), Some(5.0), Some(1.0), Some(0.2)],
            uncertainty: None,
            fit: None,
        }],
        "plotx.test.non-line-table.v1",
    )
    .unwrap();
    app.doc.datasets.push(Dataset::Table(Box::new(table)));
    app.execute_action(Action::set_line_fits(1, Vec::new(), vec![stored_sample(0)]));

    // The stored fit lives in the table's x/y space; distribution and part-of-
    // whole charts draw in other coordinates where the curve is unrelated ink.
    for id in [
        "table_histogram",
        "table_box",
        "table_violin",
        "table_bar_grouped",
        "table_heatmap",
        "table_pie",
        "table_surface",
    ] {
        let fig = app.build_binding_figure(
            &DataBinding::single(1),
            &ChartSpec {
                type_id: id.to_owned(),
                ..ChartSpec::default()
            },
            &StackSpec::default(),
            [120.0, 80.0],
        );
        assert!(
            fig.series.iter().all(|s| s.name != "fit"),
            "{id} must not draw fit overlays"
        );
    }
}

#[test]
fn stacked_figures_exclude_line_fit_overlays() {
    let mut app = two_lorentzian_app();
    app.doc.datasets.push(two_lorentzian_dataset("second"));
    app.execute_action(Action::set_line_fits(0, Vec::new(), vec![stored_sample(0)]));

    let binding = DataBinding {
        series: vec![SeriesBinding::new(0), SeriesBinding::new(1)],
    };
    let stack = StackSpec {
        mode: StackMode::Offset,
        ..StackSpec::default()
    };
    let fig = app.build_stacked_figure(&binding, &stack, [120.0, 80.0]);
    assert_eq!(fig.series.len(), 2);
}

#[test]
fn single_plot_color_override_leaves_overlay_colors_alone() {
    use plotx_figure::Color;
    let mut app = two_lorentzian_app();
    app.execute_action(Action::set_line_fits(0, Vec::new(), vec![stored_sample(0)]));

    let override_color = Color::rgb(0x11, 0x22, 0x33);
    let mut binding = DataBinding::single(0);
    binding.series[0].color = Some(override_color);
    let fig = app.build_binding_figure(
        &binding,
        &ChartSpec::default_for(DataDomain::Nmr1d),
        &StackSpec::default(),
        [120.0, 80.0],
    );

    assert_eq!(fig.series.len(), 3);
    assert_eq!(fig.series[0].color, override_color);
    assert!(fig.series[1..].iter().all(|s| s.color != override_color));
}
