use super::*;
use crate::{AutoPhaseMethod, BaselineMethod, BinMethod, BinParams, ProcessingStep, StepSource};
use nmr::Complex64;
use nmr::axis::{AxisCoordinates, AxisRole, AxisUnit};
use nmr::processed::{
    ComponentBasis, ProcessedAxis, ProcessedDataset, ProcessedOrigin, ProcessedProvenance,
};

fn spectrum(values: Vec<Complex64>) -> Arc<Dataset> {
    Arc::new(
        ProcessedDataset::from_complex_trace(
            ProcessedAxis::new(
                AxisRole::Signal,
                AxisDomain::Frequency,
                Some(AxisUnit::Ppm),
                values.len(),
                AxisCoordinates::Uniform {
                    start: 0.0,
                    step: 1.0,
                },
                ComponentBasis::Cartesian,
            )
            .unwrap(),
            values,
            ProcessedProvenance::new(ProcessedOrigin::Unknown, vec![]).unwrap(),
        )
        .unwrap()
        .into(),
    )
}

fn pipe(kinds: impl IntoIterator<Item = StepKind>) -> AxisPipeline {
    AxisPipeline {
        steps: kinds
            .into_iter()
            .enumerate()
            .map(|(i, kind)| {
                ProcessingStep::new(StepId::new(71 + i as u64 * 3), kind, StepSource::User)
            })
            .collect(),
    }
}

fn run(input: Arc<Dataset>, pipeline: &AxisPipeline) -> Result<Arc<Dataset>, RecipeError> {
    compile(input, pipeline, 0, DelayPolicy::Disabled, RecipeRange::All)?
        .execute(ProcessingOptions::new(), &mut ExecutionContext::default())
}

#[test]
fn bin_then_manual_phase_resolves_the_actual_output_length() {
    let params = PhaseParams {
        phase0: 0.4,
        phase1: 1.2,
        pivot_frac: 0.3,
        auto: None,
    };
    let result = run(
        spectrum(vec![Complex64::new(1.0, 2.0); 5]),
        &pipe([
            StepKind::Bin(BinParams {
                width: 2.0,
                method: BinMethod::Mean,
            }),
            StepKind::Phase(params),
        ]),
    )
    .unwrap();
    let processed = result.as_processed().unwrap();
    assert_eq!(
        processed.descriptor().axes()[0]
            .coordinate_iter()
            .unwrap()
            .collect::<Vec<_>>(),
        [0.5, 2.5, 4.0]
    );
    for i in 0..3 {
        let expected = Complex64::new(1.0, 2.0)
            * Complex64::from_polar(1.0, -(0.4 + 1.2 * (i as f64 / 2.0 - 0.3)));
        let actual = Complex64::new(
            processed.data().get(&[i], &[0]).unwrap(),
            processed.data().get(&[i], &[1]).unwrap(),
        );
        assert!((actual - expected).norm() < 1e-13);
    }
}

#[test]
fn estimator_errors_keep_the_recipe_identity_and_leave_the_input_unchanged() {
    let input = spectrum(vec![Complex64::new(1.0, 2.0); 8]);
    let digest = input.canonical_digests();
    let pipeline = pipe([
        StepKind::Invert,
        StepKind::Phase(PhaseParams {
            auto: Some(AutoPhaseMethod::PeakRegression),
            ..PhaseParams::MANUAL_ZERO
        }),
        StepKind::Baseline(BaselineMethod::Offset),
    ]);
    let error = run(Arc::clone(&input), &pipeline).unwrap_err();
    assert!(
        matches!(error, RecipeError::Library { step: Some(id), .. } if id == pipeline.steps[1].id)
    );
    assert_eq!(input.canonical_digests(), digest);
}

#[test]
fn estimator_and_later_segments_replay_and_survive_an_offline_snapshot() {
    let input = spectrum(
        (0..128)
            .map(|i| {
                let d = (i as f64 - 64.0) / 3.0;
                Complex64::new(1.0, d) / (1.0 + d * d) * Complex64::from_polar(1.0, 0.7)
            })
            .collect(),
    );
    let pipeline = pipe([
        StepKind::Phase(PhaseParams {
            auto: Some(AutoPhaseMethod::AbsorptivePeak),
            ..PhaseParams::MANUAL_ZERO
        }),
        StepKind::Phase(PhaseParams {
            phase0: 0.2,
            ..PhaseParams::MANUAL_ZERO
        }),
        StepKind::Baseline(BaselineMethod::Offset),
        StepKind::Invert,
    ]);
    let output = run(Arc::clone(&input), &pipeline).unwrap();
    let history = output
        .as_processed()
        .unwrap()
        .provenance()
        .history()
        .unwrap();
    let replay = history
        .replay(&[input.as_ref()], ProcessingOptions::new())
        .unwrap();
    assert_eq!(replay.canonical_digests(), output.canonical_digests());
    let mut bytes = Vec::new();
    plotx_io::nmr_bridge::snapshot::write(
        &output,
        &mut bytes,
        Default::default(),
        &mut ExecutionContext::default(),
    )
    .unwrap();
    let restored = plotx_io::nmr_bridge::snapshot::read(
        &mut bytes.as_slice(),
        Default::default(),
        &mut ExecutionContext::default(),
    )
    .unwrap();
    assert_eq!(restored.canonical_digests(), output.canonical_digests());
    let after = run(restored, &pipe([StepKind::Invert])).unwrap();
    for (a, b) in after
        .as_dense_processed()
        .unwrap()
        .samples()
        .iter()
        .zip(output.as_dense_processed().unwrap().samples())
    {
        assert_eq!(*a, -*b);
    }
}

#[test]
fn axis_magnitude_retains_the_other_cartesian_component() {
    let axis = || {
        ProcessedAxis::new(
            AxisRole::Signal,
            AxisDomain::Frequency,
            Some(AxisUnit::Ppm),
            1,
            AxisCoordinates::Explicit(vec![1.0]),
            ComponentBasis::Cartesian,
        )
        .unwrap()
    };
    let input = Arc::new(
        ProcessedDataset::from_dense_samples(
            nmr::processed::ProcessedDescriptor::new(vec![axis(), axis()]).unwrap(),
            vec![3.0, 4.0, 5.0, 12.0],
            ProcessedProvenance::new(ProcessedOrigin::Unknown, vec![]).unwrap(),
        )
        .unwrap()
        .into(),
    );
    let output = compile(
        input,
        &pipe([StepKind::Magnitude]),
        1,
        DelayPolicy::Disabled,
        RecipeRange::All,
    )
    .unwrap()
    .execute(ProcessingOptions::new(), &mut ExecutionContext::default())
    .unwrap();
    assert_eq!(
        output
            .as_processed()
            .unwrap()
            .descriptor()
            .component_counts(),
        [2, 1]
    );
    assert_eq!(output.as_dense_processed().unwrap().samples(), [5.0, 13.0]);
    let rotated = run(
        output,
        &pipe([StepKind::Phase(PhaseParams {
            phase0: std::f64::consts::FRAC_PI_2,
            ..PhaseParams::MANUAL_ZERO
        })]),
    )
    .unwrap();
    let values = rotated.as_dense_processed().unwrap().samples();
    assert!((values[0] - 13.0).abs() < 1e-12);
    assert!((values[1] + 5.0).abs() < 1e-12);
}

#[test]
fn series_phase_uses_one_representative_and_preserves_parameter_coordinates() {
    let parameter = ProcessedAxis::new(
        AxisRole::ArrayParameter,
        AxisDomain::Parameter,
        Some(AxisUnit::Second),
        3,
        AxisCoordinates::Explicit(vec![0.003, 0.001, 0.001]),
        ComponentBasis::Scalar,
    )
    .unwrap();
    let signal = ProcessedAxis::new(
        AxisRole::Signal,
        AxisDomain::Frequency,
        Some(AxisUnit::Ppm),
        8,
        AxisCoordinates::Uniform {
            start: 4.0,
            step: -0.5,
        },
        ComponentBasis::Cartesian,
    )
    .unwrap();
    let samples: Vec<f64> = [1.0, 3.0, -2.0]
        .into_iter()
        .flat_map(|scale| {
            (0..8).flat_map(move |point| {
                let value = Complex64::from_polar(scale * if point == 3 { 10.0 } else { 0.1 }, 0.7);
                [value.re, value.im]
            })
        })
        .collect();
    let input = Arc::new(
        ProcessedDataset::from_dense_samples(
            nmr::processed::ProcessedDescriptor::new(vec![parameter, signal]).unwrap(),
            samples,
            ProcessedProvenance::new(ProcessedOrigin::Unknown, vec![]).unwrap(),
        )
        .unwrap()
        .into(),
    );
    let output = compile(
        Arc::clone(&input),
        &pipe([StepKind::Phase(PhaseParams {
            auto: Some(AutoPhaseMethod::AbsorptivePeak),
            ..PhaseParams::MANUAL_ZERO
        })]),
        1,
        DelayPolicy::Disabled,
        RecipeRange::All,
    )
    .unwrap()
    .execute_with_report(ProcessingOptions::new(), &mut ExecutionContext::default())
    .unwrap();
    let report = &output.phases[0];
    assert_eq!(report.step, StepId::new(71));
    assert_eq!(report.method, nmr::processing::PhaseMethod::AbsorptivePeak);
    assert_eq!(report.input, input.canonical_digests());
    let selection = report.representative.as_ref().unwrap();
    assert_eq!(
        (selection.removed_axis, selection.index, selection.component),
        (0, 1, 0)
    );
    let processed = output.dataset.as_processed().unwrap();
    let replay = processed
        .provenance()
        .history()
        .unwrap()
        .replay(&[input.as_ref()], ProcessingOptions::new())
        .unwrap();
    assert_eq!(
        replay.canonical_digests(),
        output.dataset.canonical_digests()
    );
    assert_eq!(
        processed.descriptor().axes()[0].coordinates(),
        &AxisCoordinates::Explicit(vec![0.003, 0.001, 0.001])
    );
    for (row, expected) in [10.0, 30.0, -20.0].into_iter().enumerate() {
        assert!((processed.data().get(&[row, 3], &[0, 0]).unwrap() - expected).abs() < 1e-12);
        assert!(processed.data().get(&[row, 3], &[0, 1]).unwrap().abs() < 1e-12);
    }
}
