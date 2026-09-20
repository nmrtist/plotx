//! Import, reset and property defaults use the same checked acquisition evidence.

use super::*;
use nmr::{acquisition::GroupDelayState, axis::AxisDomain};
use plotx_io::{nmr_series::NmrSeriesSource, nmr_view::NmrSource};

pub(crate) fn default_group_delay_correct(source: &NmrSource) -> bool {
    source.dataset().as_raw().is_some_and(|raw| {
        raw.descriptor().axes().last().is_some_and(|axis| {
            axis.domain() == AxisDomain::Time
                && matches!(
                    axis.group_delay(),
                    GroupDelayState::Pending(_) | GroupDelayState::NotApplicable
                )
        })
    })
}

pub(crate) fn default_nmr_pipeline(source: &NmrSource) -> AxisPipeline {
    if source.axes()[0].domain == AxisDomain::Time {
        return if default_group_delay_correct(source) {
            AxisPipeline::default_1d()
        } else {
            AxisPipeline { steps: Vec::new() }
        };
    }
    let mut pipeline = AxisPipeline::frequency_1d();
    disable_scalar_phase(&mut pipeline, source, 0);
    pipeline
}

pub(crate) fn default_nmr_params(data: &NmrSeriesSource, preset: Preset2D) -> Params2D {
    let source = data.source_dataset();
    let mut params = Params2D::default_for(preset);
    for (axis, pipeline) in [(1, &mut params.f2), (0, &mut params.f1)] {
        match source.axes()[axis].domain {
            AxisDomain::Frequency => {
                *pipeline = AxisPipeline::frequency_2d(axis == 1);
                disable_scalar_phase(pipeline, source, axis);
            }
            AxisDomain::Parameter => pipeline.steps.clear(),
            _ => {}
        }
    }
    if source.axes()[0].domain == AxisDomain::Parameter {
        params.layout = plotx_processing::Layout2D::Stack;
    }
    if !default_group_delay_correct(source)
        && data.direct.domain == AxisDomain::Time
        && data.nus.is_none()
    {
        params.f2.steps.clear();
        params.f1.steps.clear();
    }
    params
}

fn disable_scalar_phase(pipeline: &mut AxisPipeline, source: &NmrSource, axis: usize) {
    if !source.has_imaginary(axis) {
        for step in &mut pipeline.steps {
            if matches!(step.kind, StepKind::Phase(_)) {
                step.enabled = false;
            }
        }
    }
}
