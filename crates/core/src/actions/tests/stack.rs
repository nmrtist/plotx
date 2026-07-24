use super::*;

#[test]
fn stacked_figure_is_domain_generic_with_offset_scale_and_hide() {
    use crate::state::{ChartSpec, DataBinding, DataDomain, SeriesBinding, StackMode, StackSpec};
    let size = [120.0, 80.0];

    // NMR 1D and Table domains exercise the same generic stacking path.
    let mut nmr = sample_app();
    nmr.doc
        .datasets
        .push(Dataset::Nmr(Box::new(NmrDataset::load(synthetic_1d()))));
    let (mut table, _) = table_app_with_sigma(vec![0.1, 0.1, 0.1]);
    let second = second_table_with_sigma(vec![0.2, 0.2, 0.2]);
    table.doc.datasets.push(Dataset::Table(Box::new(second)));

    for (app, domain) in [(&nmr, DataDomain::Nmr1d), (&table, DataDomain::Table)] {
        let chart = ChartSpec::default_for(domain);
        let binding = DataBinding {
            series: vec![
                SeriesBinding::new(app.doc.datasets[0].resource_id()),
                SeriesBinding::new(app.doc.datasets[1].resource_id()),
            ],
        };
        let sup = app.build_binding_figure(&binding, &chart, &StackSpec::default(), size);
        assert!(sup.show_legend, "a combined figure shows a legend");
        assert!(sup.series.len() >= 2, "both traces are drawn");
        if domain == DataDomain::Table {
            assert_eq!(sup.error_bars.len(), 6, "both tables keep their errors");
        }

        let stacked = StackSpec {
            mode: StackMode::Offset,
            spacing_y: 0.5,
            ..StackSpec::default()
        };
        let stk = app.build_binding_figure(&binding, &chart, &stacked, size);
        assert!(
            stk.y.max > sup.y.max,
            "the vertical offset extends the y-range upward"
        );

        let mut hidden = binding.clone();
        hidden.series[1].visible = false;
        let fig_h = app.build_binding_figure(&hidden, &chart, &stacked, size);
        assert!(
            fig_h.series.len() < stk.series.len(),
            "hidden trace is dropped"
        );
        assert!(!fig_h.series.is_empty(), "visible trace remains");
        if domain == DataDomain::Table {
            assert_eq!(fig_h.error_bars.len(), 3, "hidden errors are dropped");
        }

        let mut scaled = binding.clone();
        scaled.series[0].scale = 3.0;
        let fig_s = app.build_binding_figure(&scaled, &chart, &StackSpec::default(), size);
        assert!(fig_s.y.max > sup.y.max, "scaling up raises the peak");
        if domain == DataDomain::Table {
            assert!((fig_s.error_bars[0].negative - 0.3).abs() < 1e-9);
        }
    }
}

#[test]
fn field_overlay_stacks_two_2d_contours_in_distinct_colors() {
    use crate::state::{ChartSpec, DataBinding, DataDomain, SeriesBinding, StackMode, StackSpec};
    let mut app = sample_app();
    app.doc
        .datasets
        .push(Dataset::Nmr2D(Box::new(crate::state::Nmr2DDataset::load(
            synthetic_2d(),
        ))));
    app.doc
        .datasets
        .push(Dataset::Nmr2D(Box::new(crate::state::Nmr2DDataset::load(
            synthetic_2d(),
        ))));
    let (a, b) = (app.doc.datasets.len() - 2, app.doc.datasets.len() - 1);
    let binding = DataBinding {
        series: vec![
            SeriesBinding::new(app.doc.datasets[a].resource_id()),
            SeriesBinding::new(app.doc.datasets[b].resource_id()),
        ],
    };
    let chart = ChartSpec::default_for(DataDomain::Nmr2d);
    let stack = StackSpec {
        mode: StackMode::ColorOverlay,
        ..StackSpec::default()
    };
    let fig = app.build_binding_figure(&binding, &chart, &stack, [120.0, 80.0]);

    assert!(fig.show_legend, "a Field overlay shows a legend");
    assert!(
        fig.contours.len() >= 2,
        "each 2D dataset contributes its own contour"
    );
    assert_ne!(
        fig.contours[0].color, fig.contours[1].color,
        "overlaid datasets get distinct palette colours"
    );
}

#[test]
fn plain_then_ctrl_click_selects_two_datasets_for_stacking() {
    let mut app = sample_app();
    app.doc
        .datasets
        .push(Dataset::Nmr(Box::new(NmrDataset::load(synthetic_1d()))));

    // The first click must count toward the stack: plain-click A then Ctrl-click
    // B yields a two-item selection (no "Ctrl the first item" trap).
    app.toggle_selection(0, false);
    app.toggle_selection(1, true);

    assert_eq!(app.session.ui.data_selection, vec![0, 1]);
    assert_eq!(app.stackable_selection(), Some(vec![0, 1]));
    assert!(app.stackable_selection().is_some(), "can_stack is true");
    assert_eq!(app.active_dataset(), Some(1));
}

#[test]
fn ctrl_clicking_two_identical_1d_datasets_enables_stack() {
    let mut app = sample_app();
    app.doc
        .datasets
        .push(Dataset::Nmr(Box::new(NmrDataset::load(synthetic_1d()))));

    app.clear_selection();
    app.toggle_selection(0, true);
    app.toggle_selection(1, true);
    assert_eq!(app.stackable_selection(), Some(vec![0, 1]));

    // Toggling one back off drops below the ≥2 threshold and disables the command,
    // leaving the remaining item active.
    app.toggle_selection(1, true);
    assert_eq!(app.session.ui.data_selection, vec![0]);
    assert!(app.stackable_selection().is_none());
    assert_eq!(app.active_dataset(), Some(0));
}

#[test]
fn selecting_canvas_populates_data_selection_with_its_datasets() {
    let mut app = sample_app();
    app.doc
        .datasets
        .push(Dataset::Nmr(Box::new(NmrDataset::load(synthetic_1d()))));
    let object = app.doc.canvases[0].objects[0].id;
    let binding = crate::state::DataBinding {
        series: vec![
            crate::state::SeriesBinding::new(app.doc.datasets[0].resource_id()),
            crate::state::SeriesBinding::new(app.doc.datasets[1].resource_id()),
        ],
    };
    app.execute_action(Action::set_data_binding(
        0,
        object,
        crate::state::DataBinding::single(app.doc.datasets[0].resource_id()),
        binding,
    ));

    let mut dataset_indices = app.doc.canvases[0]
        .dataset_ids()
        .into_iter()
        .filter_map(|id| app.doc.dataset_index(id))
        .collect::<Vec<_>>();
    dataset_indices.sort_unstable();
    assert_eq!(dataset_indices, vec![0, 1]);

    // Selecting the canvas mirrors its datasets into the Data-list multi-select,
    // so a qualifying page can be stacked immediately.
    app.session.ui.data_selection = dataset_indices;
    assert_eq!(app.stackable_selection(), Some(vec![0, 1]));
}

#[test]
fn plot_object_reports_every_bound_dataset_for_selection_mirroring() {
    use crate::state::{DataBinding, SeriesBinding};
    let mut app = sample_app();
    app.doc
        .datasets
        .push(Dataset::Nmr(Box::new(NmrDataset::load(synthetic_1d()))));
    let object = app.doc.canvases[0].objects[0].id;
    let binding = DataBinding {
        series: vec![
            SeriesBinding::new(app.doc.datasets[0].resource_id()),
            SeriesBinding::new(app.doc.datasets[1].resource_id()),
        ],
    };
    app.execute_action(Action::set_data_binding(
        0,
        object,
        DataBinding::single(app.doc.datasets[0].resource_id()),
        binding,
    ));

    // The board-click handler mirrors this into the Data list, so a stacked plot
    // must surface both of its datasets, not just the primary.
    let obj = app.doc.canvases[0].object(object).unwrap();
    assert_eq!(
        obj.dataset_ids(),
        vec![
            app.doc.datasets[0].resource_id(),
            app.doc.datasets[1].resource_id()
        ]
    );
    assert_eq!(obj.dataset(), Some(app.doc.datasets[0].resource_id()));
}

#[test]
fn shear_sign_flips_the_pseudo_3d_lean_direction() {
    use crate::state::{ChartSpec, DataBinding, DataDomain, SeriesBinding, StackMode, StackSpec};
    let (mut table, _) = table_app();
    table
        .doc
        .datasets
        .push(Dataset::Table(Box::new(second_table())));
    let chart = ChartSpec::default_for(DataDomain::Table);
    let binding = DataBinding {
        series: vec![
            SeriesBinding::new(table.doc.datasets[0].resource_id()),
            SeriesBinding::new(table.doc.datasets[1].resource_id()),
        ],
    };
    let size = [120.0, 80.0];
    let right = StackSpec {
        mode: StackMode::Offset,
        shear_x: 0.3,
        ..StackSpec::default()
    };
    let left = StackSpec {
        shear_x: -0.3,
        ..right
    };
    let fr = table.build_binding_figure(&binding, &chart, &right, size);
    let fl = table.build_binding_figure(&binding, &chart, &left, size);
    assert!(
        fr.x.max > fl.x.max,
        "positive shear leans successive traces toward larger x"
    );
    assert!(
        fl.x.min < fr.x.min,
        "negative shear leans successive traces toward smaller x"
    );
}

#[test]
fn multi_selecting_pages_in_the_workspace_populates_data_for_stacking() {
    use crate::state::FrameRef;
    let mut app = sample_app();
    app.doc
        .datasets
        .push(Dataset::Nmr(Box::new(NmrDataset::load(synthetic_1d()))));
    push_canvas(&mut app, 1, "second canvas", [120.0, 80.0]);

    app.session.ui.frame_selection = vec![FrameRef::Page(0)];
    crate::state::toggle_frame_selection_synced(&mut app, FrameRef::Page(1));

    // Both pages' datasets become the Data-list selection, so the stack command
    // enables straight from the workspace multi-select — no Data-list re-picking.
    assert_eq!(app.session.ui.data_selection, vec![0, 1]);
    assert_eq!(app.stackable_selection(), Some(vec![0, 1]));

    crate::state::toggle_frame_selection_synced(&mut app, FrameRef::Page(1));
    assert_eq!(app.session.ui.data_selection, vec![0]);
}

#[test]
fn selecting_one_page_pulls_active_into_the_set_so_no_phantom_highlight() {
    use crate::state::FrameRef;
    let mut app = sample_app();
    app.doc
        .datasets
        .push(Dataset::Nmr(Box::new(NmrDataset::load(synthetic_1d()))));
    push_canvas(&mut app, 1, "second canvas", [120.0, 80.0]);

    // A stale active dataset (0) points outside the frame about to be selected.
    // Selecting only page 1 must reconcile active into the one-item set, so the
    // Data list shows a single selection — not a phantom two — and Stack stays
    // correctly disabled until a genuine second dataset is added.
    app.focus_single(0);
    app.session.ui.frame_selection.clear();
    crate::state::toggle_frame_selection_synced(&mut app, FrameRef::Page(1));

    assert_eq!(app.session.ui.data_selection, vec![1]);
    assert_eq!(app.active_dataset(), Some(1));
    assert!(app.stackable_selection().is_none());
}

#[test]
fn every_selection_mutator_keeps_active_inside_the_set() {
    let mut app = sample_app();
    app.doc
        .datasets
        .push(Dataset::Nmr(Box::new(NmrDataset::load(synthetic_1d()))));

    let holds = |a: &PlotxApp| {
        a.active_dataset()
            .is_none_or(|di| a.session.ui.data_selection.contains(&di))
    };

    app.focus_single(1);
    assert!(holds(&app));
    app.add_to_selection(0);
    assert!(holds(&app));
    app.toggle_selection(1, true);
    assert!(holds(&app));
    app.toggle_selection(0, false);
    assert!(holds(&app));
    app.focus_datasets(&[0, 1], Some(0));
    assert!(holds(&app));
    app.clear_selection();
    assert!(holds(&app) && app.active_dataset().is_none());
}

fn second_table() -> crate::state::TableDataset {
    second_table_with_sigma(Vec::new())
}

fn second_table_with_sigma(sigma: Vec<f64>) -> crate::state::TableDataset {
    use crate::state::{FloatSeries, materialized_float_series_table};
    materialized_float_series_table(
        (
            "Gradient".into(),
            "mT/m".into(),
            vec![Some(0.0), Some(1.0), Some(2.0)],
        ),
        vec![FloatSeries {
            name: "b".to_owned(),
            unit: String::new(),
            values: vec![Some(6.0), Some(4.0), Some(2.0)],
            uncertainty: (!sigma.is_empty()).then(|| sigma.into_iter().map(Some).collect()),
            fit: None,
        }],
        "plotx.test.second-table.v1",
    )
    .unwrap()
}
