use nmr::axis::{AxisCoordinates, AxisDomain, AxisUnit};
use nmr::raw::{
    ComponentEvidence, DirectSamples, IndirectComponents, RawAxis, RawAxisKind, RawDatasetBuilder,
    RawMetadata,
};
use nmr::{Complex64, ExecutionContext};
use plotx_io::{nmr_bridge::snapshot, nmr_view::NmrSource};
use plotx_processing::nmr_bridge::{DelayPolicy, RecipeRange};
use plotx_processing::{
    AutoPhaseMethod, AxisPipeline, Layout2D, Params2D, PhaseParams, Processed2D, ProcessingStep,
    StepId, StepKind, StepSource, nmr_execution,
};
use std::{f64::consts::TAU, sync::Arc};

fn shared_tone() -> NmrSource {
    let axis = |kind, points| {
        RawAxis::new(
            kind,
            AxisDomain::Time,
            Some(AxisUnit::Second),
            points,
            AxisCoordinates::Uniform {
                start: 0.0,
                step: 1.0 / 32.0,
            },
        )
        .unwrap()
    };
    let input = RawDatasetBuilder::new(
        vec![
            axis(
                RawAxisKind::Indirect(IndirectComponents::SharedComplex {
                    conjugated: true,
                    evidence: ComponentEvidence::user_constructed(),
                }),
                8,
            ),
            axis(RawAxisKind::Direct(DirectSamples::Complex), 16),
        ],
        RawMetadata::default(),
    )
    .unwrap()
    .dense(
        (0..8)
            .flat_map(|row| {
                (0..16).map(move |col| {
                    Complex64::from_polar(1.0, TAU * (-row as f64 / 8.0 + 3.0 * col as f64 / 16.0))
                })
            })
            .collect::<Vec<_>>(),
    )
    .unwrap();
    NmrSource::new(Arc::new(input.into())).unwrap()
}

#[test]
fn shared_auto_phase_selects_the_pair_when_the_imaginary_field_is_strongest() {
    let input = nmr_execution::execute_2d(
        &shared_tone(),
        &Params2D {
            layout: Layout2D::Ft,
            f2: pipeline(0, 1.0),
            f1: pipeline(2, 0.0),
        },
        DelayPolicy::Disabled,
        RecipeRange::All,
        None,
        &mut ExecutionContext::default(),
    )
    .unwrap();
    for axis in 0..2 {
        let pipe = AxisPipeline {
            steps: vec![ProcessingStep::new(
                StepId::new(9),
                StepKind::Phase(PhaseParams {
                    auto: Some(AutoPhaseMethod::AbsorptivePeak),
                    ..PhaseParams::MANUAL_ZERO
                }),
                StepSource::User,
            )],
        };
        let output = plotx_processing::nmr_bridge::compile(
            input.source.dataset().clone(),
            &pipe,
            axis,
            DelayPolicy::Disabled,
            RecipeRange::Frequency,
        )
        .unwrap()
        .execute_with_report(Default::default(), &mut ExecutionContext::default())
        .unwrap();
        let report = &output.phases[0];
        assert_eq!(report.representative.as_ref().unwrap().component, 0);
        let processed = output.dataset.as_processed().unwrap();
        let re = processed.data().get(&[5, 11], &[0, 0]).unwrap();
        let im = processed.data().get(&[5, 11], &[0, 1]).unwrap();
        assert!((re - 128.0).abs() < 1e-10);
        assert!(im.abs() < 1e-10);
        let (phase0, phase1, pivot_frac) = report.recipe_parameters();
        let mut manual = pipe.clone();
        manual.steps[0].kind = StepKind::Phase(PhaseParams {
            phase0,
            phase1,
            pivot_frac,
            auto: None,
        });
        let replayed = plotx_processing::nmr_bridge::compile(
            input.source.dataset().clone(),
            &manual,
            axis,
            DelayPolicy::Disabled,
            RecipeRange::Frequency,
        )
        .unwrap()
        .execute(Default::default(), &mut ExecutionContext::default())
        .unwrap();
        for (actual, expected) in replayed
            .as_processed()
            .unwrap()
            .data()
            .samples()
            .iter()
            .zip(processed.data().samples())
        {
            assert!((actual - expected).abs() < 1e-10);
        }
    }
}

fn pipeline(start: u64, phase: f64) -> AxisPipeline {
    AxisPipeline {
        steps: vec![
            ProcessingStep::new(StepId::new(start), StepKind::Fft, StepSource::User),
            ProcessingStep::new(
                StepId::new(start + 1),
                StepKind::Phase(PhaseParams {
                    phase0: phase,
                    ..PhaseParams::MANUAL_ZERO
                }),
                StepSource::User,
            ),
        ],
    }
}

#[test]
fn shared_pair_keeps_phase_capability_signed_peak_and_magnitude_through_snapshot() {
    let input = shared_tone();
    assert!(input.has_imaginary(0));
    assert!(input.has_imaginary(1));
    assert!(!input.has_imaginary(2));
    let output = nmr_execution::execute_2d(
        &input,
        &Params2D {
            layout: Layout2D::Ft,
            f2: pipeline(0, 0.2),
            f1: pipeline(2, 0.5),
        },
        DelayPolicy::Disabled,
        RecipeRange::All,
        None,
        &mut ExecutionContext::default(),
    )
    .unwrap();
    assert!(output.source.has_imaginary(0));
    assert!(output.source.has_imaginary(1));
    assert_eq!(
        output
            .source
            .dataset()
            .as_processed()
            .unwrap()
            .descriptor()
            .component_counts(),
        [1, 2]
    );
    let Processed2D::Ft(view) = &output.view else {
        panic!("expected frequency plane");
    };
    assert_eq!(view.f1_ppm[5], 4.0);
    assert_eq!(view.f2_ppm[11], 6.0);
    // F1's imaginary orientation reverses its phase rotation on the stored pair.
    let expected = Complex64::from_polar(128.0, 0.5 - 0.2);
    assert!((view.data[5 * 16 + 11] - expected).norm() < 1e-10);
    assert!(view.data[3 * 16 + 11].norm() < 1e-10);
    let magnitude = view.magnitude_plane.as_ref().unwrap();
    assert!((magnitude[5 * 16 + 11] - 128.0).abs() < 1e-10);
    assert!(magnitude[3 * 16 + 11] < 1e-10);

    let mut bytes = Vec::new();
    snapshot::write(
        output.source.dataset(),
        &mut bytes,
        Default::default(),
        &mut ExecutionContext::default(),
    )
    .unwrap();
    let restored = NmrSource::new(
        snapshot::read(
            &mut bytes.as_slice(),
            Default::default(),
            &mut ExecutionContext::default(),
        )
        .unwrap(),
    )
    .unwrap();
    assert!(restored.has_imaginary(0));
    assert!(restored.has_imaginary(1));
    let Processed2D::Ft(restored_view) =
        nmr_execution::view_2d(&restored, Layout2D::Ft, &mut ExecutionContext::default()).unwrap()
    else {
        panic!("expected restored frequency plane");
    };
    assert_eq!(restored_view.data, view.data);
    assert_eq!(restored_view.magnitude_plane, view.magnitude_plane);
    assert_eq!(restored_view.f1_ppm, view.f1_ppm);
    assert_eq!(restored_view.f2_ppm, view.f2_ppm);
}

#[test]
fn shared_reference_and_slices_retain_complex_values_and_axis_coordinates() {
    use nmr::processing::{
        FrequencyFrame, PhaseMethod, ProcessingOperation as Op, ProcessingPlan, ReferenceSource,
    };
    use plotx_processing::{ReferenceParams, SliceKind, slice::Reduction};

    let output = nmr_execution::execute_2d(
        &shared_tone(),
        &Params2D {
            layout: Layout2D::Ft,
            f2: pipeline(0, 0.2),
            f1: pipeline(2, 0.5),
        },
        DelayPolicy::Disabled,
        RecipeRange::All,
        None,
        &mut ExecutionContext::default(),
    )
    .unwrap();
    let input = ProcessingPlan::new(
        [100.0, 400.0]
            .into_iter()
            .enumerate()
            .map(|(axis, mhz)| Op::ResolveFrequencyFrame {
                axis,
                frame: FrequencyFrame::Ppm(ReferenceSource::Explicit(
                    nmr::raw::ChemicalShiftReference::user_constructed(4.0, mhz).unwrap(),
                )),
            })
            .collect(),
    )
    .unwrap()
    .apply(output.source.dataset())
    .unwrap();
    let input = NmrSource::new(Arc::new(input)).unwrap();
    let original = input.dataset().canonical_digests();
    for (axis, kind, index) in [(0, SliceKind::Column, 11), (1, SliceKind::Row, 5)] {
        let pipe = AxisPipeline {
            steps: vec![ProcessingStep::new(
                StepId::new(4),
                StepKind::Reference(ReferenceParams {
                    at_ppm: 1.0,
                    target_ppm: 1.1,
                }),
                StepSource::User,
            )],
        };
        let shifted = plotx_processing::nmr_bridge::compile(
            input.dataset().clone(),
            &pipe,
            axis,
            DelayPolicy::Disabled,
            RecipeRange::Frequency,
        )
        .unwrap()
        .execute(Default::default(), &mut ExecutionContext::default())
        .unwrap();
        let shifted = NmrSource::new(shifted).unwrap();
        assert_eq!(
            shifted.dataset().as_processed().unwrap().data().samples(),
            input.dataset().as_processed().unwrap().data().samples()
        );
        for a in 0..2 {
            let delta = if a == axis { 0.1 } else { 0.0 };
            for (before, after) in input.axes()[a]
                .coordinate_values()
                .unwrap()
                .iter()
                .zip(shifted.axes()[a].coordinate_values().unwrap())
            {
                assert!((after - before - delta).abs() < 1e-12);
            }
        }
        let (slice_source, slice) =
            plotx_processing::slice::extract(&shifted, kind, Reduction::Slice(index)).unwrap();
        assert!(slice_source.has_imaginary(0));
        assert_eq!(
            slice.coordinates,
            shifted.axes()[axis].coordinate_values().unwrap()
        );
        assert_eq!(slice.reference_freq_mhz, Some([100.0, 400.0][axis]));
        let data = shifted.dataset().as_processed().unwrap().data();
        for (point, actual) in slice.values.iter().enumerate() {
            let coordinates = if axis == 0 {
                [point, index]
            } else {
                [index, point]
            };
            let expected = Complex64::new(
                data.get(&coordinates, &[0, 0]).unwrap(),
                data.get(&coordinates, &[0, 1]).unwrap(),
            );
            assert_eq!(*actual, if axis == 0 { expected.conj() } else { expected });
        }
        PhaseMethod::AbsorptivePeak
            .prepare(slice_source.dataset(), 0, Default::default())
            .unwrap()
            .estimate()
            .unwrap();
        assert_eq!(input.dataset().canonical_digests(), original);
    }
}
