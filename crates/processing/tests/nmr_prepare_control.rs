//! Host cancellation must reach the production NUS preparation path.
use nmr::axis::{AxisCoordinates, AxisDomain, AxisUnit};
use nmr::execution::ExecutionStage;
use nmr::raw::*;
use nmr::{CancellationToken, Complex64, ExecutionContext};
use plotx_io::nmr_view::NmrSource;
use plotx_processing::nmr_bridge::{DelayPolicy, RecipeRange};
use plotx_processing::nmr_execution::{NusRequest, execute_2d};
use plotx_processing::{
    AxisPipeline, Layout2D, Params2D, ProcessingStep, StepId, StepKind, StepSource,
};
use std::sync::Arc;

fn input() -> NmrSource {
    let axis = |kind, points| {
        RawAxis::new(
            kind,
            AxisDomain::Time,
            Some(AxisUnit::Second),
            points,
            AxisCoordinates::Uniform {
                start: 0.0,
                step: 0.001,
            },
        )
        .unwrap()
    };
    let axes = vec![
        axis(
            RawAxisKind::Indirect(IndirectComponents::Cartesian(
                ComponentEvidence::user_constructed(),
            )),
            4,
        ),
        axis(RawAxisKind::Direct(DirectSamples::Complex), 3),
    ];
    let coordinates: Vec<_> = [3, 0, 1]
        .into_iter()
        .map(|i| SamplingCoordinate::new(vec![i]))
        .collect();
    let traces = coordinates
        .iter()
        .enumerate()
        .map(|(ordinal, coordinate)| {
            SparseTrace::new(
                ObservationOrdinal::new(ordinal),
                coordinate.clone(),
                vec![Complex64::new(1.0, 0.0); 6],
            )
        })
        .collect();
    let raw = RawDatasetBuilder::new(axes, RawMetadata::default())
        .unwrap()
        .sparse(traces, SamplingSchedule::new(vec![4], coordinates).unwrap())
        .unwrap();
    NmrSource::new(Arc::new(raw.into())).unwrap()
}

#[test]
fn production_nus_can_cancel_before_and_during_preparation() {
    let input = input();
    let params = Params2D {
        layout: Layout2D::Ft,
        f2: AxisPipeline {
            steps: vec![ProcessingStep::new(
                StepId::new(1),
                StepKind::Fft,
                StepSource::User,
            )],
        },
        f1: AxisPipeline { steps: vec![] },
    };
    for pre_cancelled in [true, false] {
        let token = CancellationToken::new();
        let cancel = token.clone();
        if pre_cancelled {
            token.cancel();
        }
        let mut saw_preparation = false;
        let mut progress = |event: nmr::execution::ProgressEvent| {
            if event.stage == ExecutionStage::Preflight && event.completed > 0 {
                saw_preparation = true;
                cancel.cancel();
            }
        };
        let mut context = ExecutionContext::default()
            .with_cancellation(token)
            .with_progress(&mut progress);
        let error = execute_2d(
            &input,
            &params,
            DelayPolicy::Disabled,
            RecipeRange::Base,
            Some(NusRequest {
                max_iterations: 1000,
                noise_standard_deviation: Some(0.0),
            }),
            &mut context,
        )
        .unwrap_err();
        assert!(error.is_cancelled(), "{error}");
        assert_eq!(context.ledger().used(), 0);
        if !pre_cancelled {
            assert!(saw_preparation);
        }
    }
}
