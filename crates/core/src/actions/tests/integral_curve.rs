use super::{push_canvas, sample_app};
use crate::actions::Action;
use crate::state::{IntegralDrag, Interaction, RegionDragKind, Tool};
use crate::{DisplayModeLabel, IntegralResult};

fn sample_integral(id: u64, normalized_area: f64, reference_value: Option<f64>) -> IntegralResult {
    let app = sample_app();
    let spectrum = &app.doc.datasets[0].as_nmr().unwrap().spectrum;
    let (lo, hi) = spectrum.ppm_bounds();
    IntegralResult {
        id,
        start_ppm: lo,
        end_ppm: hi,
        area: normalized_area,
        normalized_area,
        mode: DisplayModeLabel::Real,
        reference_value,
    }
}

#[test]
fn set_integrals_apply_undo_redo_keeps_all_primary_figures_synced() {
    let mut app = sample_app();
    push_canvas(&mut app, 0, "second", [120.0, 80.0]);
    let integral = sample_integral(7, 3.0, Some(3.0));
    app.execute_action(Action::set_integrals(0, Vec::new(), vec![integral]));
    assert!(app.doc.canvases.iter().all(|canvas| {
        let curve = &canvas.objects[0].plot().unwrap().figure.integral_curves;
        curve.len() == 1 && curve[0].label == "3.000"
    }));

    app.undo();
    assert!(app.doc.canvases.iter().all(|canvas| {
        canvas.objects[0]
            .plot()
            .unwrap()
            .figure
            .integral_curves
            .is_empty()
    }));
    app.redo();
    assert!(app.doc.canvases.iter().all(|canvas| {
        canvas.objects[0]
            .plot()
            .unwrap()
            .figure
            .integral_curves
            .len()
            == 1
    }));
}

#[test]
fn cancelling_live_integral_edit_restores_curve_description() {
    let mut app = sample_app();
    let before = vec![sample_integral(3, 3.0, Some(3.0))];
    app.set_integrals(0, &before);
    let object = app.doc.canvases[0].objects[0].id;
    app.set_tool(Tool::Integrate);
    app.set_interaction(Interaction::Integral(IntegralDrag {
        canvas: 0,
        object,
        dataset: 0,
        kind: RegionDragKind::Move,
        integral_id: Some(3),
        before: before.clone(),
        anchor_ppm: 0.0,
        grab_lo: before[0].start_ppm,
        grab_hi: before[0].end_ppm,
        current_ppm: 0.0,
    }));
    app.doc.datasets[0].as_nmr_mut().unwrap().integrals[0].normalized_area = 9.0;
    app.sync_integral_curves_for(0);
    assert_eq!(
        app.doc.canvases[0].objects[0]
            .plot()
            .unwrap()
            .figure
            .integral_curves[0]
            .label,
        "9.000"
    );

    app.cancel_interaction();
    assert_eq!(app.doc.datasets[0].as_nmr().unwrap().integrals, before);
    assert_eq!(
        app.doc.canvases[0].objects[0]
            .plot()
            .unwrap()
            .figure
            .integral_curves[0]
            .label,
        "3.000"
    );
}

#[test]
fn integral_curve_json_has_no_duplicate_points() {
    let mut figure = plotx_figure::Figure::new(
        "",
        plotx_figure::Axis::new("x", 0.0, 1.0),
        plotx_figure::Axis::new("y", 0.0, 1.0),
    );
    figure.integral_curves = vec![plotx_figure::IntegralCurve {
        start_ppm: 0.0,
        end_ppm: 1.0,
        normalized_area: 1.0,
        label: "1.000".to_owned(),
        color: plotx_figure::Color::TRACE,
        width: 1.0,
        source_series: 0,
    }];
    let value = serde_json::to_value(&figure).unwrap();
    assert!(value["integral_curves"][0].get("points").is_none());
}

#[test]
fn additive_integral_fields_default_when_absent() {
    let mut value = serde_json::to_value(sample_integral(7, 1.0, Some(1.0))).unwrap();
    value.as_object_mut().unwrap().remove("id");
    value.as_object_mut().unwrap().remove("reference_value");

    let restored: IntegralResult = serde_json::from_value(value).unwrap();

    assert_eq!(restored.id, 0);
    assert_eq!(restored.reference_value, None);
}

#[test]
fn figure_without_integral_curves_deserializes_with_an_empty_layer() {
    let figure = plotx_figure::Figure::new(
        "",
        plotx_figure::Axis::new("x", 0.0, 1.0),
        plotx_figure::Axis::new("y", 0.0, 1.0),
    );
    let mut value = serde_json::to_value(figure).unwrap();
    value.as_object_mut().unwrap().remove("integral_curves");

    let restored: plotx_figure::Figure = serde_json::from_value(value).unwrap();

    assert!(restored.integral_curves.is_empty());
}

#[test]
fn overlay_only_dataset_does_not_contribute_integrals() {
    use crate::state::{DataBinding, SeriesBinding, StackSpec};
    let mut app = sample_app();
    let mut secondary = app.doc.datasets[0].clone();
    secondary.as_nmr_mut().unwrap().integrals = vec![sample_integral(4, 2.0, None)];
    app.doc.datasets.push(secondary);
    let binding = DataBinding {
        series: vec![SeriesBinding::new(0), SeriesBinding::new(1)],
    };
    let fig = app.build_stacked_figure(&binding, &StackSpec::default(), [120.0, 80.0]);
    assert!(fig.integral_curves.is_empty());

    app.doc.datasets[0].as_nmr_mut().unwrap().integrals = vec![sample_integral(2, 3.0, Some(3.0))];
    let fig = app.build_stacked_figure(&binding, &StackSpec::default(), [120.0, 80.0]);
    assert_eq!(fig.integral_curves.len(), 1);
    assert_eq!(fig.integral_curves[0].source_series, 0);
}

#[test]
fn lightweight_sync_respects_hidden_primary_series() {
    let mut app = sample_app();
    app.set_integrals(0, &[sample_integral(2, 3.0, Some(3.0))]);
    let plot = app.doc.canvases[0].objects[0].plot_mut().unwrap();
    plot.binding.series[0].visible = false;
    assert_eq!(plot.figure.integral_curves.len(), 1);

    app.sync_integral_curves_for(0);

    assert!(
        app.doc.canvases[0].objects[0]
            .plot()
            .unwrap()
            .figure
            .integral_curves
            .is_empty()
    );
}

#[test]
fn one_dimensional_processing_commit_recomputes_integral_and_curve() {
    let mut app = sample_app();
    let mut integral = sample_integral(8, 999.0, Some(3.0));
    integral.area = 999.0;
    app.doc.datasets[0].as_nmr_mut().unwrap().integrals = vec![integral];

    app.apply_dataset_edit(0);

    let recomputed = app.doc.datasets[0].as_nmr().unwrap().integrals[0];
    assert_ne!(recomputed.area, 999.0);
    assert_eq!(recomputed.normalized_area, 3.0);
    let curve = &app.doc.canvases[0].objects[0]
        .plot()
        .unwrap()
        .figure
        .integral_curves[0];
    assert_eq!(curve.normalized_area, recomputed.normalized_area);
    assert_eq!(curve.start_ppm, recomputed.start_ppm);
    assert_eq!(curve.end_ppm, recomputed.end_ppm);
}

#[test]
fn processing_action_apply_undo_and_redo_recompute_integrals() {
    use crate::actions::DatasetProcessingState;
    use plotx_processing::{PhaseParams, StepKind};

    let mut app = sample_app();
    app.doc.datasets[0].as_nmr_mut().unwrap().integrals =
        vec![sample_integral(8, 999.0, Some(3.0))];
    let before = DatasetProcessingState::from_dataset(&app.doc.datasets[0]);
    let mut after = before.clone();
    let DatasetProcessingState::Nmr { pipeline, .. } = &mut after else {
        unreachable!();
    };
    for step in &mut pipeline.steps {
        if let StepKind::Phase(params) = &mut step.kind {
            *params = PhaseParams {
                phase0: 0.5,
                ..PhaseParams::MANUAL_ZERO
            };
        }
    }

    app.execute_action(Action::update_dataset_processing(0, before, after));
    assert_ne!(
        app.doc.datasets[0].as_nmr().unwrap().integrals[0].area,
        999.0
    );

    app.doc.datasets[0].as_nmr_mut().unwrap().integrals[0].area = 777.0;
    app.doc.datasets[0].as_nmr_mut().unwrap().integrals[0].normalized_area = 777.0;
    app.undo();
    let restored = app.doc.datasets[0].as_nmr().unwrap().integrals[0];
    assert_ne!(restored.area, 777.0);
    assert_eq!(restored.normalized_area, 3.0);
    assert_eq!(
        app.doc.canvases[0].objects[0]
            .plot()
            .unwrap()
            .figure
            .integral_curves[0]
            .label,
        "3.000"
    );

    app.doc.datasets[0].as_nmr_mut().unwrap().integrals[0].area = 555.0;
    app.redo();
    assert_ne!(
        app.doc.datasets[0].as_nmr().unwrap().integrals[0].area,
        555.0
    );
}

#[test]
fn reference_accepts_arbitrary_target_without_plot_marker() {
    let mut app = sample_app();
    app.set_integrals(0, &[sample_integral(5, 0.25, None)]);

    app.set_integral_reference(0, 5, 100.0);

    let integral = app.doc.datasets[0].as_nmr().unwrap().integrals[0];
    assert_eq!(integral.reference_value, Some(100.0));
    assert_eq!(integral.normalized_area, 100.0);
    let curve = &app.doc.canvases[0].objects[0]
        .plot()
        .unwrap()
        .figure
        .integral_curves[0];
    assert_eq!(curve.label, "100.000");
    assert_eq!(curve.color, plotx_figure::Color::rgb(0x2b, 0x6c, 0xb0));
}
