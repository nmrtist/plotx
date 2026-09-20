//! Execute the historical signal-quality fixtures through the production bridge.
use super::*;
use crate::nmr_bridge::{DelayPolicy, RecipeRange};
use plotx_io::nmr_view::NmrSource;
fn run(
    data: &NmrData,
    pipeline: &AxisPipeline,
    delay: bool,
    range: RecipeRange,
) -> Result<Processed1D, String> {
    let source = NmrSource::try_from(data.clone()).map_err(|e| e.to_string())?;
    crate::nmr_execution::execute_1d(
        &source,
        pipeline,
        if delay {
            DelayPolicy::AxisEvidence
        } else {
            DelayPolicy::Disabled
        },
        range,
        &mut nmr::ExecutionContext::default(),
    )
    .map(|out| out.view)
    .map_err(|e| e.to_string())
}
pub fn process(
    data: &NmrData,
    pipeline: &AxisPipeline,
    delay: bool,
) -> Result<Processed1D, String> {
    run(data, pipeline, delay, RecipeRange::All)
}
pub fn process_output(
    data: &NmrData,
    pipeline: &AxisPipeline,
    delay: bool,
) -> Result<Processed1D, String> {
    process(data, pipeline, delay)
}
pub fn transform_base(data: &NmrData, pipeline: &AxisPipeline, delay: bool) -> Spectrum {
    match run(data, pipeline, delay, RecipeRange::Base).unwrap() {
        Processed1D::Frequency(s) => s,
        _ => panic!("test requires FFT"),
    }
}
pub fn process_up_to(
    data: &NmrData,
    pipeline: &AxisPipeline,
    delay: bool,
    step: StepId,
) -> Processed1D {
    run(data, pipeline, delay, RecipeRange::Through(step)).unwrap()
}
pub fn apply_phase(spectrum: &mut Spectrum, method: AutoPhaseMethod) {
    let axis = nmr::processed::ProcessedAxis::new(
        nmr::axis::AxisRole::Signal,
        nmr::axis::AxisDomain::Frequency,
        Some(spectrum.unit),
        spectrum.len(),
        nmr::axis::AxisCoordinates::Explicit(spectrum.ppm.clone()),
        nmr::processed::ComponentBasis::Cartesian,
    )
    .unwrap();
    let input = nmr::processed::ProcessedDataset::from_complex_trace(
        axis,
        spectrum.values.clone(),
        nmr::processed::ProcessedProvenance::new(
            nmr::processed::ProcessedOrigin::Unknown,
            Vec::new(),
        )
        .unwrap(),
    )
    .unwrap();
    let source = NmrSource::new(std::sync::Arc::new(input.into())).unwrap();
    let pipeline = AxisPipeline {
        steps: vec![ProcessingStep::new(
            StepId::new(0),
            StepKind::Phase(PhaseParams {
                auto: Some(method),
                ..PhaseParams::MANUAL_ZERO
            }),
            StepSource::User,
        )],
    };
    let out = crate::nmr_execution::execute_1d(
        &source,
        &pipeline,
        DelayPolicy::Disabled,
        RecipeRange::All,
        &mut nmr::ExecutionContext::default(),
    )
    .unwrap();
    spectrum.values = out.view.as_frequency().unwrap().values.clone();
}
