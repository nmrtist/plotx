use nmr::axis::{AxisCoordinates, AxisDomain, AxisUnit};
use nmr::processing::{
    FourierTransform, NusSettings, ProcessingOperation, ProcessingOptions, ProcessingPlan,
};
use nmr::raw::*;
use nmr::{Complex64, Dataset, ExecutionContext};
use plotx_processing::{AxisPipeline, ProcessingStep, StepId, StepKind, StepSource, nmr_bridge};

/// An independent 4x3 separable tone, with one missing indirect observation.
#[test]
fn sparse_tone_reconstructs_and_round_trips() -> Result<(), Box<dyn std::error::Error>> {
    let indirect = RawAxis::new(
        RawAxisKind::Indirect(IndirectComponents::Cartesian(
            ComponentEvidence::user_constructed(),
        )),
        AxisDomain::Time,
        Some(AxisUnit::Second),
        4,
        AxisCoordinates::Uniform {
            start: 0.0,
            step: 0.01,
        },
    )?;
    let direct = RawAxis::new(
        RawAxisKind::Direct(DirectSamples::Complex),
        AxisDomain::Time,
        Some(AxisUnit::Second),
        3,
        AxisCoordinates::Uniform {
            start: 0.0,
            step: 0.001,
        },
    )?;
    let indices = [3, 0, 1];
    let coordinates: Vec<_> = indices
        .iter()
        .map(|i| SamplingCoordinate::new(vec![*i]))
        .collect();
    let traces = indices
        .iter()
        .enumerate()
        .map(|(ordinal, i)| {
            let angle = std::f64::consts::TAU * *i as f64 / 4.0;
            let samples = [angle.cos(), angle.sin()]
                .into_iter()
                .flat_map(|lane| {
                    (0..3).map(move |j| {
                        Complex64::from_polar(lane, std::f64::consts::TAU * j as f64 / 3.0)
                    })
                })
                .collect();
            SparseTrace::new(
                ObservationOrdinal::new(ordinal),
                coordinates[ordinal].clone(),
                samples,
            )
        })
        .collect();
    let input: Dataset = RawDatasetBuilder::new(vec![indirect, direct], RawMetadata::default())?
        .sparse(traces, SamplingSchedule::new(vec![4], coordinates)?)?
        .into();
    let plan = ProcessingPlan::new(vec![ProcessingOperation::FourierTransform {
        axis: 1,
        transform: FourierTransform::default(),
    }])?;
    let mut context = ExecutionContext::default();
    let options = ProcessingOptions::new();
    let prepared = NusSettings {
        max_iterations: 1000,
        noise_standard_deviation: Some(0.0),
    }
    .prepare(&input, plan, options)?;
    if prepared.measured_indices() != indices {
        return Err("observation order changed".into());
    }
    let mixed = prepared.execute_with_context(&mut context)?;
    let data = mixed
        .as_dense_processed()
        .ok_or("missing reconstructed samples")?;
    for row in 0..4 {
        let angle = std::f64::consts::TAU * row as f64 / 4.0;
        for (lane, expected) in [3.0 * angle.cos(), 3.0 * angle.sin()]
            .into_iter()
            .enumerate()
        {
            if (data.get(&[row, 2], &[lane, 0])? - expected).abs() > 1e-5 {
                return Err("NUS tone reconstruction exceeded 1e-5 amplitude error".into());
            }
        }
    }
    let pipeline = AxisPipeline {
        steps: vec![ProcessingStep::new(
            StepId::new(77),
            StepKind::Fft,
            StepSource::User,
        )],
    };
    let frequency = nmr_bridge::compile(
        std::sync::Arc::new(mixed),
        &pipeline,
        0,
        nmr_bridge::DelayPolicy::Disabled,
        nmr_bridge::RecipeRange::All,
    )?
    .execute(options, &mut context)?;
    let values = frequency.as_dense_processed().ok_or("missing F1 output")?;
    if (values.get(&[3, 2], &[0, 0])? - 12.0).abs() > 1e-5 {
        return Err("incorrect 2D peak amplitude".into());
    }
    let mut bytes = vec![];
    plotx_io::nmr_bridge::snapshot::write(
        &frequency,
        &mut bytes,
        Default::default(),
        &mut context,
    )?;
    let restored = plotx_io::nmr_bridge::snapshot::read(
        &mut bytes.as_slice(),
        Default::default(),
        &mut context,
    )?;
    if restored.canonical_digests() != frequency.canonical_digests() {
        return Err("NUS snapshot identity changed".into());
    }
    Ok(())
}
