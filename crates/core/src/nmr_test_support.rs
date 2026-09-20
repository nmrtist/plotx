//! Explicit manual recipes for presentation and application-state fixtures.
use crate::state::{Nmr2DDataset, NmrDataset};
use plotx_processing::{AxisPipeline, Params2D, PhaseParams, StepKind};
fn manual(pipeline: &mut AxisPipeline) {
    for step in &mut pipeline.steps {
        if let StepKind::Phase(ref mut phase) = step.kind {
            *phase = PhaseParams::MANUAL_ZERO;
        }
    }
}
pub(crate) fn load_1d(input: plotx_io::NmrData) -> Result<NmrDataset, String> {
    let mut pipeline = match input.domain {
        plotx_io::Domain::Time => AxisPipeline::default_1d(),
        plotx_io::Domain::Frequency => AxisPipeline::frequency_1d(),
    };
    manual(&mut pipeline);
    NmrDataset::load_with_pipeline(input, Some(pipeline), None)
}
pub(crate) fn load_2d(input: plotx_io::NmrData2D) -> Result<Nmr2DDataset, String> {
    let source = plotx_io::nmr_series::NmrSeriesSource::try_from(input)
        .map_err(|error| error.to_string())?;
    let preset = plotx_processing::recommend_preset(&source);
    let mut params = if source.direct.domain == nmr::axis::AxisDomain::Time {
        Params2D::default_for(preset)
    } else {
        Params2D::frequency_domain(preset)
    };
    if source.indirect.domain == nmr::axis::AxisDomain::Parameter {
        params.layout = plotx_processing::Layout2D::Stack;
        params.f1.steps.clear();
    }
    manual(&mut params.f2);
    manual(&mut params.f1);
    for (axis, pipeline) in [(1, &mut params.f2), (0, &mut params.f1)] {
        if source.source_dataset().axes()[axis].domain == nmr::axis::AxisDomain::Frequency
            && !source.source_dataset().has_imaginary(axis)
        {
            for step in &mut pipeline.steps {
                if matches!(step.kind, StepKind::Phase(_)) {
                    step.enabled = false;
                }
            }
        }
    }
    Nmr2DDataset::load_with_pipeline(source, Some(params), None, None, true)
}
