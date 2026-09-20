use super::*;
use plotx_core::state::{Dataset, NmrDataset, PhaseAxis};
use plotx_io::{Domain, NmrData};
use plotx_processing::{ProcessingStep, ReferenceParams, StepId, StepKind, StepSource};

fn synthetic_app() -> PlotxApp {
    use num_complex::Complex64;
    use std::f64::consts::TAU;
    let npoints = 256;
    let (sw, obs, carrier) = (4000.0, 400.0, 5.0);
    let dt = 1.0 / sw;
    let points = (0..npoints)
        .map(|k| {
            let t = k as f64 * dt;
            let decay = (-t / 0.25f64).exp();
            let freq_hz = (2.0 - carrier) * obs;
            Complex64::from_polar(decay, TAU * freq_hz * t)
        })
        .collect();
    let data = NmrData {
        points,
        domain: Domain::Time,
        spectral_width_hz: sw,
        observe_freq_mhz: obs,
        carrier_ppm: carrier,
        nucleus: "1H".to_owned(),
        source: "synthetic".to_owned(),
        group_delay: 0.0,
    };
    let mut app = PlotxApp::new();
    app.doc
        .datasets
        .push(Dataset::Nmr(Box::new(NmrDataset::load(data).unwrap())));
    app.focus_single(0);
    app
}

fn add_reference_step(app: &mut PlotxApp, params: ReferenceParams) -> StepId {
    let pipe = app.doc.datasets[0]
        .axis_pipeline_mut(PhaseAxis::Direct)
        .unwrap();
    let id = StepId::new(pipe.steps.iter().map(|s| s.id.get()).max().unwrap_or(0) + 1);
    pipe.steps.push(ProcessingStep::new(
        id,
        StepKind::Reference(params),
        StepSource::User,
    ));
    id
}

fn reference_params(app: &PlotxApp, step: StepId) -> ReferenceParams {
    let StepKind::Reference(params) = app.doc.datasets[0]
        .axis_pipeline(PhaseAxis::Direct)
        .unwrap()
        .steps
        .iter()
        .find(|s| s.id == step)
        .unwrap()
        .kind
    else {
        panic!("the step must stay a Reference step");
    };
    params
}

/// A pick on the displayed axis must land the picked feature exactly on
/// `target_ppm` after the recompute: the step's own current offset is removed
/// from the picked coordinate before it becomes the new `at_ppm`.
#[test]
fn commit_converts_the_displayed_pick_into_step_coordinates() {
    let mut app = synthetic_app();
    let step = add_reference_step(
        &mut app,
        ReferenceParams {
            at_ppm: 1.0,
            target_ppm: 2.0,
        },
    );
    let dataset = app.doc.datasets[0].resource_id();
    app.session.ui.proc_expanded_step = Some((dataset, step));
    app.toggle_reference_pick(dataset, step);
    let resolved = app.resolve_reference_pick().expect("armed and expanded");

    // The step currently applies +1.0 ppm, so a feature displayed at 5.0 sits
    // at 4.0 on the axis entering the step.
    commit_reference_pick(&mut app, &resolved, 5.0);

    let params = reference_params(&app, step);
    assert!((params.at_ppm - 4.0).abs() < 1e-12);
    assert_eq!(params.target_ppm, 2.0);
    // One-shot: the pick disarms on commit, and the edit is one undo step.
    assert!(app.session.ui.reference_pick.is_none());
    app.undo();
    assert_eq!(reference_params(&app, step).at_ppm, 1.0);
}

/// A fresh step (zero offset) stores the picked coordinate verbatim.
#[test]
fn commit_on_a_fresh_step_stores_the_picked_position() {
    let mut app = synthetic_app();
    let step = add_reference_step(
        &mut app,
        ReferenceParams {
            at_ppm: 0.0,
            target_ppm: 0.0,
        },
    );
    let dataset = app.doc.datasets[0].resource_id();
    app.session.ui.proc_expanded_step = Some((dataset, step));
    app.toggle_reference_pick(dataset, step);
    let resolved = app.resolve_reference_pick().expect("armed and expanded");

    commit_reference_pick(&mut app, &resolved, 3.25);

    assert!((reference_params(&app, step).at_ppm - 3.25).abs() < 1e-12);
}
