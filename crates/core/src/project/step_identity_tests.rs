use super::{load_project, save_project};
use crate::project::tests::{sample_app, synthetic_1d, temp_project};
use crate::state::{Dataset, NmrDataset, PhaseAxis, PlotxApp};
use plotx_processing::{Apodization, ProcessingStep, ReferenceParams, StepKind, StepSource};

#[test]
fn step_ids_and_allocator_survive_project_roundtrip() {
    let app = sample_app();
    let before: Vec<_> = app.doc.datasets[0]
        .axis_pipeline(PhaseAxis::Direct)
        .unwrap()
        .steps
        .iter()
        .map(|step| step.id)
        .collect();
    let next_before = app.doc.datasets[0].as_nmr().unwrap().next_step_id;
    let path = temp_project("step-id-roundtrip");
    let _ = std::fs::remove_file(&path);

    save_project(&app, &path, false).unwrap();
    let loaded = load_project(&path).unwrap();
    let _ = std::fs::remove_file(&path);

    let after: Vec<_> = loaded.doc.datasets[0]
        .axis_pipeline(PhaseAxis::Direct)
        .unwrap()
        .steps
        .iter()
        .map(|step| step.id)
        .collect();
    assert_eq!(after, before);
    assert_eq!(
        loaded.doc.datasets[0].as_nmr().unwrap().next_step_id,
        next_before
    );
}

#[test]
fn project_roundtrip_preserves_custom_pipeline_steps() {
    let mut app = PlotxApp::new();
    let mut dataset = NmrDataset::load(synthetic_1d());
    // One time-side step (an exponential window before the FFT) and one
    // frequency-side step (a referencing shift) that must both survive.
    let fft_pos = dataset
        .pipeline
        .steps
        .iter()
        .position(|s| matches!(s.kind, StepKind::Fft))
        .unwrap();
    let apodize_id = dataset.allocate_step_id();
    dataset.pipeline.steps.insert(
        fft_pos,
        ProcessingStep::new(
            apodize_id,
            StepKind::Apodize(Apodization::Exponential { lb_hz: 8.0 }),
            StepSource::User,
        ),
    );
    let reference_id = dataset.allocate_step_id();
    dataset.pipeline.steps.push(ProcessingStep::new(
        reference_id,
        StepKind::Reference(ReferenceParams {
            at_ppm: 2.0,
            target_ppm: 2.5,
        }),
        StepSource::User,
    ));
    dataset.retransform();
    app.doc.datasets.push(Dataset::Nmr(Box::new(dataset)));

    let path = temp_project("pipeline");
    let _ = std::fs::remove_file(&path);
    save_project(&app, &path, false).unwrap();
    let loaded = load_project(&path).unwrap();
    let _ = std::fs::remove_file(&path);

    let Dataset::Nmr(n) = &loaded.doc.datasets[0] else {
        panic!("expected a 1D NMR dataset");
    };
    assert!(n.pipeline.steps.iter().any(|s| matches!(
        &s.kind,
        StepKind::Apodize(Apodization::Exponential { lb_hz }) if (*lb_hz - 8.0).abs() < 1e-9
    )));
    assert!(n.pipeline.steps.iter().any(|s| matches!(
        &s.kind,
        StepKind::Reference(r) if (r.target_ppm - 2.5).abs() < 1e-9 && (r.at_ppm - 2.0).abs() < 1e-9
    )));
}
