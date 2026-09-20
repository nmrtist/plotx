//! Every supported recipe operation executes through the production bridge.

use nmr::processed::{
    ComponentBasis, ProcessedAxis, ProcessedDataset, ProcessedOrigin, ProcessedProvenance,
};
use nmr::{
    Complex64, Dataset, ExecutionContext,
    axis::{AxisCoordinates, AxisDomain, AxisRole, AxisUnit},
};
use plotx_processing::nmr_bridge::{self, DelayPolicy, RecipeRange};
use plotx_processing::{
    Apodization, AutoPhaseMethod, AxisPipeline, BaselineMethod, BinParams, NormalizeMethod,
    PhaseParams, ProcessingStep, ReferenceParams, SmoothMethod, StepId, StepKind, StepSource,
};
use std::{error::Error, path::PathBuf, sync::Arc};

#[test]
fn supported_recipe_operations() -> Result<(), Box<dyn Error>> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../io/tests/fixtures/nmr");
    let raw =
        plotx_io::nmr_bridge::read(&root.join("bruker-1d"), &mut ExecutionContext::default())?;
    let axis = ProcessedAxis::new(
        AxisRole::Signal,
        AxisDomain::Frequency,
        Some(AxisUnit::Ppm),
        128,
        AxisCoordinates::Uniform {
            start: 1.0,
            step: 0.1,
        },
        ComponentBasis::Cartesian,
    )?;
    let spectrum = Arc::new(Dataset::from_processed(
        ProcessedDataset::from_complex_trace(
            axis,
            (0..128)
                .map(|i| {
                    let mut value = Complex64::new(0.0, 0.0);
                    for (center, height) in [(24.0, 1.0), (67.0, 0.7), (105.0, 0.5)] {
                        let d = (i as f64 - center) / 2.0;
                        value += Complex64::new(height, height * d) / (1.0 + d * d);
                    }
                    value * Complex64::from_polar(1.0, 0.3 + 0.4 * i as f64 / 127.0)
                })
                .collect(),
            ProcessedProvenance::new(ProcessedOrigin::Unknown, vec![])?,
        )?,
    ));
    let mut steps = vec![
        StepKind::Apodize(Apodization::Gaussian {
            lb_hz: 1.0,
            gb_hz: 2.0,
        }),
        StepKind::Baseline(BaselineMethod::Offset),
        StepKind::Baseline(BaselineMethod::Polynomial { order: 2 }),
        StepKind::Baseline(BaselineMethod::AUTO),
        StepKind::Reference(ReferenceParams {
            at_ppm: 1.0,
            target_ppm: 2.0,
        }),
        StepKind::Smooth(SmoothMethod::MovingAverage { window: 3 }),
        StepKind::Smooth(SmoothMethod::DEFAULT),
        StepKind::Normalize(NormalizeMethod::MaxPeak),
        StepKind::Normalize(NormalizeMethod::TotalArea),
        StepKind::Normalize(NormalizeMethod::Constant { divisor: 2.0 }),
        StepKind::Bin(BinParams::DEFAULT),
        StepKind::Reverse,
        StepKind::Invert,
    ];
    steps.extend(
        [
            AutoPhaseMethod::RobustConsensus,
            AutoPhaseMethod::AbsorptivePeak,
            AutoPhaseMethod::Entropy,
            AutoPhaseMethod::NegativeMinimization,
            AutoPhaseMethod::PeakRegression,
        ]
        .map(|method| {
            StepKind::Phase(PhaseParams {
                auto: Some(method),
                ..PhaseParams::MANUAL_ZERO
            })
        }),
    );
    for (index, kind) in steps.into_iter().enumerate() {
        let input = if matches!(kind, StepKind::Apodize(_)) {
            &raw
        } else {
            &spectrum
        };
        let label = format!("{kind:?}");
        let pipe = AxisPipeline {
            steps: vec![ProcessingStep::new(
                StepId::new(index as u64),
                kind,
                StepSource::User,
            )],
        };
        let result = nmr_bridge::compile(
            Arc::clone(input),
            &pipe,
            0,
            DelayPolicy::Disabled,
            RecipeRange::All,
        )
        .and_then(|recipe| {
            recipe.execute(
                nmr::processing::ProcessingOptions::new(),
                &mut ExecutionContext::default(),
            )
        });
        result.unwrap_or_else(|error| panic!("{label}: {error}"));
    }
    Ok(())
}

#[test]
fn sparse_preparation_preserves_observation_order() -> Result<(), Box<dyn Error>> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../io/tests/fixtures/nmr");
    let sparse =
        plotx_io::nmr_bridge::read(&root.join("bruker-nus"), &mut ExecutionContext::default())?;
    let plan = nmr::processing::ProcessingPlan::new(vec![
        nmr::processing::ProcessingOperation::ComponentTransform { axis: 0 },
        nmr::processing::ProcessingOperation::FourierTransform {
            axis: 1,
            transform: nmr::processing::FourierTransform::default(),
        },
    ])?;
    // This fixture's arbitrary integer samples are not a sparse-spectrum oracle.
    // Check preparation here; analytic reconstruction is verified separately.
    let prepared = nmr::processing::NusSettings {
        max_iterations: 2048,
        noise_standard_deviation: Some(0.0),
    }
    .prepare(&sparse, plan, nmr::processing::ProcessingOptions::new())?;
    assert_eq!(prepared.measured_indices(), [3, 1]);
    Ok(())
}
