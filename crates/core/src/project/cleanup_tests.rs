use super::tests::{synthetic_1d, temp_project, temp_scheme};
use super::*;
use crate::state::Dataset;

#[test]
fn project_and_scheme_roundtrips_preserve_cleanup_steps() {
    let mut app = PlotxApp::new();
    let mut dataset = NmrDataset::load(synthetic_1d());
    let cleanup = [
        StepKind::Smooth(SmoothMethod::SavitzkyGolay {
            window: 11,
            poly_order: 4,
        }),
        StepKind::Normalize(NormalizeMethod::TotalArea),
        StepKind::Bin(BinParams {
            width: 0.02,
            method: BinMethod::Mean,
        }),
        StepKind::Reverse,
        StepKind::Invert,
    ];
    for kind in &cleanup {
        let id = dataset.allocate_step_id();
        dataset
            .pipeline
            .steps
            .push(ProcessingStep::new(id, kind.clone(), StepSource::User));
    }
    dataset.retransform();
    let expected: Vec<StepKind> = cleanup.to_vec();
    app.doc.datasets.push(Dataset::Nmr(Box::new(dataset)));

    let path = temp_project("cleanup");
    let _ = std::fs::remove_file(&path);
    save_project(&app, &path, false).unwrap();
    let loaded = load_project(&path).unwrap();
    let _ = std::fs::remove_file(&path);
    let Dataset::Nmr(n) = &loaded.doc.datasets[0] else {
        panic!("expected a 1D NMR dataset");
    };
    let tail: Vec<StepKind> = n.pipeline.steps[n.pipeline.steps.len() - 5..]
        .iter()
        .map(|s| s.kind.clone())
        .collect();
    assert_eq!(tail, expected);

    let scheme_path = temp_scheme("cleanup");
    let _ = std::fs::remove_file(&scheme_path);
    save_scheme(&scheme_path, &loaded.doc.datasets[0]).unwrap();
    let scheme = load_scheme(&scheme_path).unwrap();
    let _ = std::fs::remove_file(&scheme_path);
    let target = Dataset::Nmr(Box::new(NmrDataset::load(synthetic_1d())));
    let crate::actions::DatasetProcessingState::Nmr { pipeline, .. } =
        apply_scheme(&scheme, &target).unwrap()
    else {
        panic!("expected a 1D processing state");
    };
    let tail: Vec<StepKind> = pipeline.steps[pipeline.steps.len() - 5..]
        .iter()
        .map(|s| s.kind.clone())
        .collect();
    assert_eq!(tail, expected);
}

#[test]
fn applying_a_scheme_reports_an_invalid_stored_smoothing_window() {
    let target = Dataset::Nmr(Box::new(NmrDataset::load(synthetic_1d())));
    let mut pipeline = AxisPipeline::default_1d();
    pipeline.steps.push(ProcessingStep::new(
        StepId::new(99),
        StepKind::Smooth(SmoothMethod::SavitzkyGolay {
            window: 8,
            poly_order: 7,
        }),
        StepSource::User,
    ));
    let scheme = ProcessingScheme {
        schema_version: 1,
        dimension_count: 1,
        pipelines: vec![pipeline_to_dto(&pipeline)],
        layout: None,
        group_delay_correct: true,
    };
    let error = apply_scheme(&scheme, &target)
        .expect_err("an even persisted window must be diagnosed at the load boundary");
    let message = error.to_string();
    assert!(message.contains("stored smoothing window 8"), "{message}");
    assert!(message.contains("odd value between 3 and 201"), "{message}");
}
