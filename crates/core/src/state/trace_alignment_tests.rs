use super::*;

fn alignment_recording_app() -> (PlotxApp, CanvasId, ObjectId, Vec<SeriesId>) {
    let mut dataset = recording("pA", None);
    let recording = dataset.as_electrophysiology_mut().unwrap();
    recording.processing.gaussian_lowpass_enabled = false;
    recording.data.sweeps[1].channels[0] = vec![f64::NAN, 3.0, 4.0];
    let mut app = PlotxApp::new();
    app.doc.datasets.push(dataset);
    let mut canvas = CanvasDocument::new("alignment".into(), [120.0, 80.0]);
    let id = canvas.allocate_object_id();
    let mut object = app.build_plot_object(
        0,
        ObjectFrame::new(0.0, 0.0, 340.0, 220.0),
        id,
        "Traces".into(),
    );
    object.plot_mut().unwrap().mint_series_ids();
    let ids = object
        .plot()
        .unwrap()
        .binding
        .series
        .iter()
        .map(|series| series.id)
        .collect();
    canvas.objects.push(object);
    canvas.selected_object = Some(id);
    let canvas_id = canvas.resource_id;
    app.doc.canvases.push(canvas);
    app.session.active_canvas = Some(0);
    app.rebuild_canvas(0);
    (app, canvas_id, id, ids)
}

#[test]
fn trace_start_composes_absolute_shifts_as_one_undo_step() {
    let (mut app, canvas, object, ids) = alignment_recording_app();
    let source_before = source_bits(&app.doc.datasets[0]);
    let binding_before = {
        let plot = app.doc.canvases[0]
            .object_mut(object)
            .unwrap()
            .plot_mut()
            .unwrap();
        assert!(plot.binding.series[1].set_line_x_shift(0.5));
        plot.binding.clone()
    };
    app.rebuild_canvas(0);
    let request = TraceAlignmentRequest {
        canvas,
        object,
        reference: ids[0],
        method: TraceAlignmentMethod::TraceStart,
    };
    let viewport_before = {
        let plot = app.doc.canvases[0]
            .object_mut(object)
            .unwrap()
            .plot_mut()
            .unwrap();
        plot.viewport.view_x = AxisRange::new(0.00002, 0.00008);
        plot.apply_viewport();
        plot.viewport.clone()
    };
    let plan = app.plan_trace_alignment(request).unwrap();
    let TraceAlignmentOutcome::Align {
        anchor,
        delta,
        resulting_shift,
    } = &plan.rows[1].outcome
    else {
        panic!("second trace should align: {:?}", plan.rows[1].outcome)
    };
    assert!((*anchor - 0.5001).abs() < 1e-12);
    assert!((*delta + 0.5001).abs() < 1e-12);
    assert!((*resulting_shift + 0.0001).abs() < 1e-12);

    let history = app.session.undo_stack.len();
    assert_eq!(app.apply_trace_alignment(request).unwrap(), 1);
    assert_eq!(app.session.undo_stack.len(), history + 1);
    assert!(matches!(
        app.session.undo_stack.last(),
        Some(Action::SetSeriesPresentation { .. })
    ));
    assert_eq!(
        app.doc.canvases[0]
            .object(object)
            .unwrap()
            .plot()
            .unwrap()
            .viewport
            .view_x,
        viewport_before.view_x
    );
    let plot = app.doc.canvases[0].object(object).unwrap().plot().unwrap();
    assert_eq!(
        plot.binding.series.iter().map(|s| s.id).collect::<Vec<_>>(),
        ids
    );
    assert!((plot.binding.series[1].line_x_shift().unwrap() + 0.0001).abs() < 1e-12);
    assert!(
        (plot.figure().series[0].points[0][0] - plot.figure().series[1].points[0][0]).abs() < 1e-12
    );
    assert_eq!(source_bits(&app.doc.datasets[0]), source_before);

    let repeat = app.plan_trace_alignment(request).unwrap();
    let TraceAlignmentOutcome::Align { delta, .. } = repeat.rows[1].outcome else {
        panic!("second trace should remain alignable")
    };
    assert!(delta.abs() < 1e-12);
    app.undo();
    assert_eq!(
        app.doc.canvases[0]
            .object(object)
            .unwrap()
            .plot()
            .unwrap()
            .binding,
        binding_before
    );
    assert_eq!(
        app.doc.canvases[0]
            .object(object)
            .unwrap()
            .plot()
            .unwrap()
            .viewport
            .view_x,
        viewport_before.view_x
    );
    app.redo();
    assert!(
        (app.doc.canvases[0]
            .object(object)
            .unwrap()
            .plot()
            .unwrap()
            .binding
            .series[1]
            .line_x_shift()
            .unwrap()
            + 0.0001)
            .abs()
            < 1e-12
    );
    assert_eq!(
        app.doc.canvases[0]
            .object(object)
            .unwrap()
            .plot()
            .unwrap()
            .viewport
            .view_x,
        viewport_before.view_x
    );
}

#[test]
fn manual_shift_round_trips_in_strict_v1_project() {
    let (mut app, _, object, _) = app_for_dataset(recording("pA", None));
    assert!(
        app.doc.canvases[0]
            .object_mut(object)
            .unwrap()
            .plot_mut()
            .unwrap()
            .binding
            .series[0]
            .set_line_x_shift(0.125)
    );
    app.rebuild_canvas(0);
    let path = std::env::temp_dir().join(format!(
        "plotx-trace-alignment-{}.plotx",
        uuid::Uuid::new_v4()
    ));
    crate::project::save_project(&app, &path, false).unwrap();
    let loaded = crate::project::load_project(&path).unwrap();
    let _ = std::fs::remove_file(path);
    assert_eq!(
        loaded.doc.canvases[0]
            .object(object)
            .unwrap()
            .plot()
            .unwrap()
            .binding
            .series[0]
            .line_x_shift(),
        Some(0.125)
    );
}

#[test]
fn hidden_and_stale_requests_are_atomic() {
    let (mut app, canvas, object, ids) = alignment_recording_app();
    app.doc.canvases[0]
        .object_mut(object)
        .unwrap()
        .plot_mut()
        .unwrap()
        .binding
        .series[1]
        .visible = false;
    let before = app.doc.canvases[0]
        .object(object)
        .unwrap()
        .plot()
        .unwrap()
        .binding
        .clone();
    let request = TraceAlignmentRequest {
        canvas,
        object,
        reference: ids[0],
        method: TraceAlignmentMethod::TraceStart,
    };
    let plan = app.plan_trace_alignment(request).unwrap();
    assert!(matches!(
        plan.rows[1].outcome,
        TraceAlignmentOutcome::Skipped(_)
    ));
    assert!(app.apply_trace_alignment(request).is_err());
    assert_eq!(
        app.doc.canvases[0]
            .object(object)
            .unwrap()
            .plot()
            .unwrap()
            .binding,
        before
    );
    assert!(app.session.undo_stack.is_empty());
    assert!(
        app.plan_trace_alignment(TraceAlignmentRequest {
            reference: SeriesId::new(999),
            ..request
        })
        .is_err()
    );
    assert_eq!(
        app.doc.canvases[0]
            .object(object)
            .unwrap()
            .plot()
            .unwrap()
            .binding,
        before
    );
}

#[test]
fn manual_shift_moves_single_trace_and_preserves_provider_bounds() {
    let dataset = recording("pA", None);
    let mut binding = DataBinding {
        series: vec![DataBinding::single(&dataset).series[0].clone()],
    };
    assert!(binding.series[0].set_line_x_shift(2.5));
    let mut app = PlotxApp::new();
    app.doc.datasets.push(dataset);
    let build = |app: &mut PlotxApp| {
        app.build_binding_figure(
            &binding,
            &ChartSpec::default_for(DataDomain::Electrophysiology),
            &StackSpec::default(),
            [120.0, 80.0],
        )
    };
    let figure = build(&mut app);
    assert_eq!(figure.series[0].points[0][0], 2.5);
    assert_eq!([figure.x.min, figure.x.max], [2.5, 2.5002]);
    assert_eq!(build(&mut app).series[0].points[0][0], 2.5);
}

#[test]
fn continuous_manual_shift_commits_one_presentation_action() {
    let (mut app, _, object, _) = alignment_recording_app();
    let before = app.doc.canvases[0]
        .object(object)
        .unwrap()
        .plot()
        .unwrap()
        .binding
        .clone();
    let view_x = {
        let plot = app.doc.canvases[0]
            .object_mut(object)
            .unwrap()
            .plot_mut()
            .unwrap();
        plot.viewport.view_x = AxisRange::new(0.00002, 0.00008);
        plot.apply_viewport();
        plot.viewport.view_x
    };
    let history = app.session.undo_stack.len();
    let revision = app.doc.automation_revision;

    app.begin_series_presentation_edit(0, object);
    for shift in [0.1, 0.2, 0.3] {
        let mut after = app.doc.canvases[0]
            .object(object)
            .unwrap()
            .plot()
            .unwrap()
            .binding
            .clone();
        assert!(after.series[0].set_line_x_shift(shift));
        app.set_series_presentation_value(0, object, &after);
    }
    assert_eq!(app.session.undo_stack.len(), history);
    assert_eq!(app.doc.automation_revision, revision);
    app.finish_series_presentation_edit();

    assert_eq!(app.session.undo_stack.len(), history + 1);
    assert_eq!(app.doc.automation_revision, revision + 1);
    assert!(matches!(
        app.session.undo_stack.last(),
        Some(Action::SetSeriesPresentation { .. })
    ));
    let after = app.doc.canvases[0]
        .object(object)
        .unwrap()
        .plot()
        .unwrap()
        .binding
        .clone();
    assert_eq!(after.series[0].line_x_shift(), Some(0.3));
    assert_eq!(
        app.doc.canvases[0]
            .object(object)
            .unwrap()
            .plot()
            .unwrap()
            .viewport
            .view_x,
        view_x
    );

    app.undo();
    assert_eq!(
        app.doc.canvases[0]
            .object(object)
            .unwrap()
            .plot()
            .unwrap()
            .binding,
        before
    );
    app.redo();
    assert_eq!(
        app.doc.canvases[0]
            .object(object)
            .unwrap()
            .plot()
            .unwrap()
            .binding,
        after
    );
}

#[test]
fn stacked_shift_bounds_union_each_provider_range() {
    let dataset = recording("pA", None);
    let mut binding = DataBinding::single(&dataset);
    assert!(binding.series[0].set_line_x_shift(-1.0));
    assert!(binding.series[1].set_line_x_shift(2.0));
    let mut app = PlotxApp::new();
    app.doc.datasets.push(dataset);
    let chart = ChartSpec::default_for(DataDomain::Electrophysiology);
    let parts: Vec<_> = binding
        .series
        .iter()
        .map(|series| {
            app.build_binding_figure(
                &DataBinding {
                    series: vec![series.clone()],
                },
                &chart,
                &StackSpec::default(),
                [120.0, 80.0],
            )
        })
        .collect();
    let stacked = app.build_binding_figure(&binding, &chart, &StackSpec::default(), [120.0, 80.0]);
    assert_eq!(stacked.x.min, parts[0].x.min.min(parts[1].x.min));
    assert_eq!(stacked.x.max, parts[0].x.max.max(parts[1].x.max));
}

#[test]
fn pseudo_increment_uses_the_same_plot_owned_plan() {
    let dataset = Dataset::Nmr2D(Box::new(Nmr2DDataset::load(pseudo_data())));
    let field = dataset.field_catalog().id_for_key("nmr.stack").unwrap();
    let mut app = PlotxApp::new();
    app.doc.datasets.push(dataset);
    let mut canvas = CanvasDocument::new("pseudo alignment".into(), [120.0, 80.0]);
    let object = canvas.allocate_object_id();
    let mut plot_object = app.build_plot_object(
        0,
        ObjectFrame::new(0.0, 0.0, 340.0, 220.0),
        object,
        "Pseudo traces".into(),
    );
    let plot = plot_object.plot_mut().unwrap();
    plot.binding = DataBinding {
        series: SeriesBinding::from_field_all(&app.doc.datasets[0], field)[..2].to_vec(),
    };
    plot.mint_series_ids();
    let ids: Vec<_> = plot.binding.series.iter().map(|series| series.id).collect();
    assert!(plot.binding.series[1].set_line_x_shift(0.25));
    canvas.objects.push(plot_object);
    let canvas_id = canvas.resource_id;
    app.doc.canvases.push(canvas);
    app.rebuild_canvas(0);
    let plan = app
        .plan_trace_alignment(TraceAlignmentRequest {
            canvas: canvas_id,
            object,
            reference: ids[0],
            method: TraceAlignmentMethod::TraceStart,
        })
        .unwrap();
    let TraceAlignmentOutcome::Align {
        resulting_shift, ..
    } = plan.rows[1].outcome
    else {
        panic!("pseudo increment should align")
    };
    assert!(resulting_shift.abs() < 1e-12);
}

#[test]
fn peak_window_uses_displayed_coordinates() {
    let mut dataset = recording("pA", None);
    let recording = dataset.as_electrophysiology_mut().unwrap();
    recording.processing.gaussian_lowpass_enabled = false;
    recording.data.sweeps[0].channels[0] = vec![0.0, 0.0, 8.0, 0.0, 0.0, 0.0, 0.0];
    recording.data.sweeps[1].channels[0] = vec![0.0, 0.0, 0.0, 0.0, 0.0, 8.0, 0.0];
    let (mut app, canvas, object, ids) = app_for_dataset(dataset);
    let plan = app
        .plan_trace_alignment(TraceAlignmentRequest {
            canvas,
            object,
            reference: ids[0],
            method: TraceAlignmentMethod::PeakInWindow {
                lo: 0.0,
                hi: 0.001,
                polarity: plotx_analysis::alignment::PeakPolarity::Positive,
            },
        })
        .unwrap();
    let TraceAlignmentOutcome::Align { delta, .. } = plan.rows[1].outcome else {
        panic!("second peak should align")
    };
    assert!((delta + 0.0003).abs() < 1e-12);
}

#[test]
fn selected_channel_projection_preserves_other_channel_bindings() {
    let dataset = multichannel_recording(["mV", "pA"], 1, "channels.abf");
    let (mut app, canvas, object, _) = app_for_dataset(dataset);
    let active_field = app.doc.datasets[0].active_trace_collection_field().unwrap();
    let persisted_before = app.doc.canvases[0]
        .object(object)
        .unwrap()
        .plot()
        .unwrap()
        .binding
        .clone();
    let displayed = {
        let plot = app.doc.canvases[0].object(object).unwrap().plot().unwrap();
        app.display_binding(plot.display_owner, &plot.binding)
    };
    assert_eq!(displayed.series.len(), 2);
    assert!(
        displayed
            .series
            .iter()
            .all(|series| series.source.field == active_field)
    );
    app.apply_trace_alignment(TraceAlignmentRequest {
        canvas,
        object,
        reference: displayed.series[0].id,
        method: TraceAlignmentMethod::TraceStart,
    })
    .unwrap();
    let persisted_after = &app.doc.canvases[0]
        .object(object)
        .unwrap()
        .plot()
        .unwrap()
        .binding;
    for before in persisted_before
        .series
        .iter()
        .filter(|series| series.source.field != active_field)
    {
        assert_eq!(
            persisted_after
                .series
                .iter()
                .find(|series| series.id == before.id),
            Some(before)
        );
    }
}

#[test]
fn automatic_alignment_skips_incompatible_x_units() {
    let (mut app, canvas, object, ids) = alignment_recording_app();
    let pseudo = Dataset::Nmr2D(Box::new(Nmr2DDataset::load(pseudo_data())));
    let field = pseudo.field_catalog().id_for_key("nmr.stack").unwrap();
    let mut extra = SeriesBinding::from_field_all(&pseudo, field)[0].clone();
    extra.id = SeriesId::new(99);
    app.doc.datasets.push(pseudo);
    app.doc.canvases[0]
        .object_mut(object)
        .unwrap()
        .plot_mut()
        .unwrap()
        .binding
        .series
        .push(extra);
    let plan = app
        .plan_trace_alignment(TraceAlignmentRequest {
            canvas,
            object,
            reference: ids[0],
            method: TraceAlignmentMethod::TraceStart,
        })
        .unwrap();
    assert!(matches!(
        plan.rows.last().unwrap().outcome,
        TraceAlignmentOutcome::Skipped(ref reason) if reason.contains("unit differs")
    ));
}

#[test]
fn provider_line_x_units_describe_plotted_x_axes() {
    for response_unit in ["pA", "mV"] {
        let dataset = recording(response_unit, None);
        let field = dataset.active_trace_collection_field().unwrap();
        assert_eq!(
            dataset.field_descriptor(field).unwrap().line_x_unit(),
            Some("s")
        );
    }
    let pseudo = Dataset::Nmr2D(Box::new(Nmr2DDataset::load(pseudo_data())));
    let field = pseudo.field_catalog().id_for_key("nmr.stack").unwrap();
    assert_eq!(
        pseudo.field_descriptor(field).unwrap().line_x_unit(),
        Some("ppm")
    );
}

fn scalar_nmr(source: &str, carrier_ppm: f64) -> Dataset {
    Dataset::Nmr(Box::new(NmrDataset::load(plotx_io::NmrData {
        points: vec![num_complex::Complex64::new(1.0, 0.0); 8],
        domain: plotx_io::Domain::Frequency,
        spectral_width_hz: 4_000.0,
        observe_freq_mhz: 400.0,
        carrier_ppm,
        nucleus: "1H".to_owned(),
        source: source.to_owned(),
        group_delay: 0.0,
    })))
}

#[test]
fn ordinary_scalar_line_stack_uses_the_same_alignment_planner() {
    let mut app = PlotxApp::new();
    app.doc.datasets.push(scalar_nmr("before", 4.0));
    app.doc.datasets.push(scalar_nmr("after", 5.0));

    let mut canvas = CanvasDocument::new("scalar alignment".into(), [120.0, 80.0]);
    let object = canvas.allocate_object_id();
    let mut plot_object = app.build_plot_object(
        0,
        ObjectFrame::new(0.0, 0.0, 340.0, 220.0),
        object,
        "NMR comparison".into(),
    );
    let plot = plot_object.plot_mut().unwrap();
    plot.display_owner = None;
    plot.binding = DataBinding {
        series: app
            .doc
            .datasets
            .iter()
            .map(|dataset| DataBinding::single(dataset).series.remove(0))
            .collect(),
    };
    plot.mint_series_ids();
    let reference = plot.binding.series[0].id;
    assert!(
        plot.binding
            .series
            .iter()
            .all(|series| series.source.item.is_none())
    );
    canvas.objects.push(plot_object);
    canvas.selected_object = Some(object);
    let canvas_id = canvas.resource_id;
    app.doc.canvases.push(canvas);
    app.session.active_canvas = Some(0);
    app.rebuild_canvas(0);

    let raw_before = app
        .doc
        .datasets
        .iter()
        .map(|dataset| {
            dataset
                .as_nmr()
                .unwrap()
                .data
                .points
                .iter()
                .map(|point| (point.re.to_bits(), point.im.to_bits()))
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    let binding_before = app.doc.canvases[0]
        .object(object)
        .unwrap()
        .plot()
        .unwrap()
        .binding
        .clone();
    assert_eq!(app.trace_alignment_target(), Some((canvas_id, object)));
    let request = TraceAlignmentRequest {
        canvas: canvas_id,
        object,
        reference,
        method: TraceAlignmentMethod::TraceStart,
    };
    let plan = app.plan_trace_alignment(request).unwrap();
    assert_eq!(plan.x_unit, "ppm");
    assert_eq!(plan.alignment_count(), 1);

    let history = app.session.undo_stack.len();
    assert_eq!(app.apply_trace_alignment(request).unwrap(), 1);
    assert_eq!(app.session.undo_stack.len(), history + 1);
    assert!(matches!(
        app.session.undo_stack.last(),
        Some(Action::SetSeriesPresentation { .. })
    ));
    let binding_after = app.doc.canvases[0]
        .object(object)
        .unwrap()
        .plot()
        .unwrap()
        .binding
        .clone();
    assert_ne!(binding_after, binding_before);
    let figure = app.doc.canvases[0]
        .object(object)
        .unwrap()
        .plot()
        .unwrap()
        .figure();
    assert!((figure.series[0].points[0][0] - figure.series[1].points[0][0]).abs() < 1e-12);
    assert_eq!(
        app.doc
            .datasets
            .iter()
            .map(|dataset| {
                dataset
                    .as_nmr()
                    .unwrap()
                    .data
                    .points
                    .iter()
                    .map(|point| (point.re.to_bits(), point.im.to_bits()))
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>(),
        raw_before
    );

    let path = std::env::temp_dir().join(format!(
        "plotx-scalar-line-alignment-{}.plotx",
        uuid::Uuid::new_v4()
    ));
    crate::project::save_project(&app, &path, false).unwrap();
    let loaded = crate::project::load_project(&path).unwrap();
    let _ = std::fs::remove_file(path);
    assert_eq!(
        loaded.doc.canvases[0]
            .object(object)
            .unwrap()
            .plot()
            .unwrap()
            .binding,
        binding_after
    );

    app.undo();
    assert_eq!(
        app.doc.canvases[0]
            .object(object)
            .unwrap()
            .plot()
            .unwrap()
            .binding,
        binding_before
    );
    app.redo();
    assert_eq!(
        app.doc.canvases[0]
            .object(object)
            .unwrap()
            .plot()
            .unwrap()
            .binding,
        binding_after
    );
}

fn app_for_dataset(dataset: Dataset) -> (PlotxApp, CanvasId, ObjectId, Vec<SeriesId>) {
    let mut app = PlotxApp::new();
    app.doc.datasets.push(dataset);
    let mut canvas = CanvasDocument::new("alignment".into(), [120.0, 80.0]);
    let object = canvas.allocate_object_id();
    let mut plot_object = app.build_plot_object(
        0,
        ObjectFrame::new(0.0, 0.0, 340.0, 220.0),
        object,
        "Traces".into(),
    );
    plot_object.plot_mut().unwrap().mint_series_ids();
    let ids = plot_object
        .plot()
        .unwrap()
        .binding
        .series
        .iter()
        .map(|series| series.id)
        .collect();
    canvas.objects.push(plot_object);
    let canvas_id = canvas.resource_id;
    app.doc.canvases.push(canvas);
    app.rebuild_canvas(0);
    (app, canvas_id, object, ids)
}

fn source_bits(dataset: &Dataset) -> Vec<u64> {
    dataset
        .as_electrophysiology()
        .unwrap()
        .data
        .sweeps
        .iter()
        .flat_map(|sweep| &sweep.channels)
        .flat_map(|channel| channel.iter().map(|value| value.to_bits()))
        .collect()
}
