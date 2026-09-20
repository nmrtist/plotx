use super::*;
use crate::{ProcessingStep, StepSource};
use nmr::processed::{ComponentBasis, ProcessedAxis, ProcessedDataset};
use nmr::raw::{DirectSamples, RawAxis, RawAxisKind, RawDatasetBuilder, RawMetadata};
use nmr::resource::WorkLedger;
use nmr::{
    Complex64,
    axis::{AxisCoordinates, AxisRole, AxisUnit},
};

fn raw(points: usize, delay: GroupDelayState) -> Arc<Dataset> {
    let axis = RawAxis::new(
        RawAxisKind::Direct(DirectSamples::Complex),
        AxisDomain::Time,
        Some(AxisUnit::Second),
        points,
        AxisCoordinates::Uniform {
            start: 0.0,
            step: 0.001,
        },
    )
    .unwrap()
    .with_group_delay(delay)
    .unwrap();
    Arc::new(Dataset::from_raw(
        RawDatasetBuilder::new(vec![axis], RawMetadata::default())
            .unwrap()
            .dense(
                (0..points)
                    .map(|index| {
                        Complex64::from_polar(
                            1.0,
                            std::f64::consts::TAU * index as f64 / points as f64,
                        )
                    })
                    .collect(),
            )
            .unwrap(),
    ))
}

#[test]
fn processed_time_input_retains_reference_and_zero_filter_evidence_across_snapshot_and_fft() {
    let input = plotx_io::nmr_bridge::read(
        &std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../io/tests/fixtures/nmr/bruker-1d"),
        &mut ExecutionContext::default(),
    )
    .unwrap();
    // A time-domain operation creates processed state before the FFT recipe.
    let time = nmr::processing::ProcessingPlan::new(vec![Op::ZeroFill {
        axis: 0,
        zero_fill: nmr::processing::ZeroFill::new(8).unwrap(),
    }])
    .unwrap()
    .apply(&input)
    .unwrap();
    assert!(time.as_processed().is_some());
    let mut bytes = Vec::new();
    plotx_io::nmr_bridge::snapshot::write(
        &time,
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
    let output = compile(
        restored,
        &pipeline([StepKind::Fft]),
        0,
        DelayPolicy::AxisEvidence,
        RecipeRange::All,
    )
    .unwrap()
    .execute(ProcessingOptions::new(), &mut ExecutionContext::default())
    .unwrap();
    let source = plotx_io::nmr_view::NmrSource::new(output.clone()).unwrap();
    assert_eq!(source.axes()[0].unit, Some(AxisUnit::Ppm));
    assert_eq!(source.reference_frequency_mhz(0), Some(400.0));
    assert!(matches!(
        output
            .as_processed()
            .unwrap()
            .axis_evidence(0)
            .unwrap()
            .group_delay(),
        nmr::processed::ProcessedGroupDelay::Corrected {
            delay_points: 0.0,
            ..
        }
    ));
}

fn spectrum(points: usize, descending: bool) -> Arc<Dataset> {
    let axis = ProcessedAxis::new(
        AxisRole::Signal,
        AxisDomain::Frequency,
        Some(AxisUnit::Ppm),
        points,
        AxisCoordinates::Uniform {
            start: 4.0,
            step: if descending { -0.1 } else { 0.1 },
        },
        ComponentBasis::Cartesian,
    )
    .unwrap();
    Arc::new(Dataset::from_processed(
        ProcessedDataset::from_complex_trace(
            axis,
            vec![Complex64::new(1.0, 2.0); points],
            nmr::processed::ProcessedProvenance::new(
                nmr::processed::ProcessedOrigin::Unknown,
                vec![],
            )
            .unwrap(),
        )
        .unwrap(),
    ))
}

fn pipeline(kinds: impl IntoIterator<Item = StepKind>) -> AxisPipeline {
    AxisPipeline {
        steps: kinds
            .into_iter()
            .enumerate()
            .map(|(index, kind)| {
                ProcessingStep::new(StepId::new(100 + index as u64 * 3), kind, StepSource::User)
            })
            .collect(),
    }
}

fn execute(
    input: Arc<Dataset>,
    pipe: &AxisPipeline,
    range: RecipeRange,
) -> Result<Arc<Dataset>, RecipeError> {
    compile(input, pipe, 0, DelayPolicy::Disabled, range)?
        .execute(ProcessingOptions::new(), &mut ExecutionContext::default())
}

#[test]
fn phase_preserves_radians_sign_endpoint_pivot_singleton_and_axis_direction() {
    let params = PhaseParams {
        phase0: 0.7,
        phase1: -1.4,
        pivot_frac: 0.37,
        auto: None,
    };
    for points in [1, 2, 5, 6] {
        for descending in [false, true] {
            let output = execute(
                spectrum(points, descending),
                &pipeline([StepKind::Phase(params)]),
                RecipeRange::All,
            )
            .unwrap();
            let data = output.as_dense_processed().unwrap();
            for index in 0..points {
                let phi = params.phase0
                    + params.phase1
                        * (index as f64 / (points - 1).max(1) as f64 - params.pivot_frac);
                let expected = Complex64::new(1.0, 2.0) * Complex64::from_polar(1.0, -phi);
                let actual = Complex64::new(
                    data.get(&[index], &[0]).unwrap(),
                    data.get(&[index], &[1]).unwrap(),
                );
                assert!(
                    (actual - expected).norm() < 2e-14,
                    "{points}/{index}/{descending}"
                );
            }
        }
    }
}

#[test]
fn fft_matches_analytic_tone_and_integer_center_for_odd_and_even_lengths() {
    for points in [1, 5, 6] {
        let output = execute(
            raw(points, GroupDelayState::NotApplicable),
            &pipeline([StepKind::Fft]),
            RecipeRange::All,
        )
        .unwrap();
        let processed = output.as_processed().unwrap();
        let axis = &processed.descriptor().axes()[0];
        for index in 0..points {
            let q = index as isize - (points / 2) as isize;
            assert!(
                (axis.coordinate(index).unwrap() - q as f64 * 1000.0 / points as f64).abs() < 1e-12
            );
            let expected = if points == 1 || q == 1 {
                points as f64
            } else {
                0.0
            };
            assert!((processed.data().get(&[index], &[0]).unwrap() - expected).abs() < 1e-12);
            assert!(processed.data().get(&[index], &[1]).unwrap().abs() < 1e-12);
        }
    }
}

#[test]
fn reordered_windows_and_zero_fills_use_current_length_and_preview_identity() {
    let pipe = pipeline([
        StepKind::ZeroFill(ZeroFill::Size(5)),
        StepKind::Apodize(Apodization::CosineBell),
        StepKind::ZeroFill(ZeroFill::Factor(1)),
        StepKind::Fft,
    ]);
    let input = raw(3, GroupDelayState::NotApplicable);
    let preview = execute(
        Arc::clone(&input),
        &pipe,
        RecipeRange::Through(pipe.steps[2].id),
    )
    .unwrap();
    let data = preview.as_dense_processed().unwrap();
    assert_eq!(data.shape(), &[8]);
    let expected = Complex64::from_polar(
        std::f64::consts::FRAC_1_SQRT_2,
        4.0 * std::f64::consts::PI / 3.0,
    );
    assert!((data.get(&[2], &[0]).unwrap() - expected.re).abs() < 1e-14);
    assert!((data.get(&[2], &[1]).unwrap() - expected.im).abs() < 1e-14);
    assert_eq!(data.get(&[7], &[0]).unwrap(), 0.0);
    let output = execute(input, &pipe, RecipeRange::All).unwrap();
    assert_eq!(output.as_dense_processed().unwrap().shape(), &[8]);
}

#[test]
fn split_cache_matches_full_recipe_and_shares_work_budget() {
    let pipe = pipeline([
        StepKind::Apodize(Apodization::Exponential { lb_hz: 1.0 }),
        StepKind::Fft,
        StepKind::Phase(PhaseParams {
            phase0: 0.3,
            ..PhaseParams::MANUAL_ZERO
        }),
    ]);
    let input = raw(8, GroupDelayState::NotApplicable);
    let whole = execute(Arc::clone(&input), &pipe, RecipeRange::All).unwrap();
    let mut ledger = WorkLedger::processing_default();
    let mut context = ExecutionContext::new(&mut ledger);
    let base = compile(input, &pipe, 0, DelayPolicy::Disabled, RecipeRange::Base)
        .unwrap()
        .execute(ProcessingOptions::new(), &mut context)
        .unwrap();
    let output = compile(
        base,
        &pipe,
        0,
        DelayPolicy::Disabled,
        RecipeRange::Frequency,
    )
    .unwrap()
    .execute(ProcessingOptions::new(), &mut context)
    .unwrap();
    assert_eq!(whole.as_dense_processed(), output.as_dense_processed());
    assert!(ledger.used() > 0);
}

#[test]
fn disabling_fft_preserves_time_output_and_disabled_preview_endpoint() {
    let mut pipe = pipeline([
        StepKind::Apodize(Apodization::Exponential { lb_hz: 2.0 }),
        StepKind::Fft,
    ]);
    pipe.steps[1].enabled = false;
    let result = execute(
        raw(8, GroupDelayState::NotApplicable),
        &pipe,
        RecipeRange::Through(pipe.steps[1].id),
    )
    .unwrap();
    assert_eq!(
        result.as_processed().unwrap().descriptor().axes()[0].domain(),
        AxisDomain::Time
    );
}

#[test]
fn unknown_delay_is_not_zero_and_library_error_maps_to_fft_id() {
    let pipe = pipeline([StepKind::Apodize(Apodization::None), StepKind::Fft]);
    let input = raw(8, GroupDelayState::Unknown);
    let recipe = compile(input, &pipe, 0, DelayPolicy::AxisEvidence, RecipeRange::All).unwrap();
    assert_eq!(recipe.step_id(0), Some(pipe.steps[1].id));
    assert_eq!(recipe.step_id(1), Some(pipe.steps[1].id));
    let error = recipe
        .execute(ProcessingOptions::new(), &mut ExecutionContext::default())
        .unwrap_err();
    assert!(matches!(error, RecipeError::Library { step: Some(id), .. } if id == pipe.steps[1].id));
    assert!(!error.is_cancelled());
}

#[test]
fn new_spectrum_steps_execute_without_changing_the_input() {
    let input = spectrum(8, false);
    let before = input.canonical_digests();
    let output = execute(
        Arc::clone(&input),
        &pipeline([
            StepKind::Reference(crate::ReferenceParams {
                at_ppm: 4.0,
                target_ppm: 1.0,
            }),
            StepKind::Reverse,
            StepKind::Invert,
            StepKind::Baseline(crate::BaselineMethod::Offset),
        ]),
        RecipeRange::All,
    )
    .unwrap();
    assert_eq!(input.canonical_digests(), before);
    let processed = output.as_processed().unwrap();
    assert_eq!(processed.descriptor().axes()[0].coordinate(0).unwrap(), 1.0);
    for i in 0..8 {
        assert_eq!(processed.data().get(&[i], &[0]).unwrap(), 0.0);
        assert_eq!(processed.data().get(&[i], &[1]).unwrap(), -2.0);
    }
}

#[test]
fn cancellation_is_distinct_including_an_empty_recipe_and_budget_is_enforced() {
    let token = nmr::CancellationToken::new();
    token.cancel();
    let mut context = ExecutionContext::default().with_cancellation(token);
    let input = raw(8, GroupDelayState::NotApplicable);
    let recipe = compile(
        Arc::clone(&input),
        &pipeline([]),
        0,
        DelayPolicy::Disabled,
        RecipeRange::All,
    )
    .unwrap();
    assert!(
        recipe
            .execute(ProcessingOptions::new(), &mut context)
            .unwrap_err()
            .is_cancelled()
    );
    let mut ledger = WorkLedger::new(0);
    let recipe = compile(
        input,
        &pipeline([StepKind::Fft]),
        0,
        DelayPolicy::Disabled,
        RecipeRange::All,
    )
    .unwrap();
    let error = recipe
        .execute(
            ProcessingOptions::new(),
            &mut ExecutionContext::new(&mut ledger),
        )
        .unwrap_err();
    assert!(
        matches!(error, RecipeError::Library { source, .. } if source.code() == ProcessingErrorCode::ResourceLimit)
    );
}

#[test]
fn overflow_and_stale_preview_ids_are_recoverable() {
    let input = raw(8, GroupDelayState::NotApplicable);
    assert!(
        compile(
            Arc::clone(&input),
            &pipeline([StepKind::ZeroFill(ZeroFill::Factor(255))]),
            0,
            DelayPolicy::Disabled,
            RecipeRange::All
        )
        .is_err()
    );
    assert!(
        compile(
            input,
            &pipeline([StepKind::Fft]),
            0,
            DelayPolicy::Disabled,
            RecipeRange::Through(StepId::new(17))
        )
        .is_err()
    );
}

#[test]
fn magnitude_uses_library_scalar_projection_without_inventing_an_imaginary_channel() {
    let output = execute(
        spectrum(5, false),
        &pipeline([StepKind::Magnitude]),
        RecipeRange::All,
    )
    .unwrap();
    let data = output.as_processed().unwrap();
    assert_eq!(data.descriptor().component_counts(), [1]);
    assert!(
        data.data()
            .samples()
            .iter()
            .all(|value| (*value - 5.0f64.sqrt()).abs() < 1e-14)
    );
}

#[test]
fn explicit_fractional_delay_uses_signed_bins_and_conflicting_evidence_fails() {
    let pipe = pipeline([StepKind::Fft]);
    let input = raw(5, GroupDelayState::Unknown);
    let result = compile(
        input,
        &pipe,
        0,
        DelayPolicy::Explicit(0.25),
        RecipeRange::All,
    )
    .unwrap()
    .execute(ProcessingOptions::new(), &mut ExecutionContext::default())
    .unwrap();
    let expected = Complex64::from_polar(5.0, std::f64::consts::TAU * 0.25 / 5.0);
    let data = result.as_dense_processed().unwrap();
    assert!((data.get(&[3], &[0]).unwrap() - expected.re).abs() < 1e-12);
    assert!((data.get(&[3], &[1]).unwrap() - expected.im).abs() < 1e-12);
    let known = raw(
        5,
        GroupDelayState::Pending(nmr::raw::PendingGroupDelay::user_constructed(0.5).unwrap()),
    );
    let error = compile(
        known,
        &pipe,
        0,
        DelayPolicy::Explicit(0.25),
        RecipeRange::All,
    )
    .unwrap()
    .execute(ProcessingOptions::new(), &mut ExecutionContext::default())
    .unwrap_err();
    assert!(
        matches!(error, RecipeError::Library { source, .. } if matches!(source.root_cause(), ProcessingError::DelayEvidenceMismatch))
    );
}
