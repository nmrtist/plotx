//! Pipeline shifting for multi-spectrum reference alignment.

use crate::{AxisPipeline, ProcessingStep, ReferenceParams, StepKind, StepSource};

/// Shift the axis so the point at `at_ppm` reads `target_ppm`, through the
/// pipeline's referencing step: the last enabled Reference step absorbs the
/// extra translation, or a new user step is appended when none exists.
pub fn apply_reference_shift(pipe: &mut AxisPipeline, at_ppm: f64, target_ppm: f64) {
    let existing = pipe
        .steps
        .iter_mut()
        .rev()
        .filter(|s| s.enabled)
        .find_map(|s| match &mut s.kind {
            StepKind::Reference(r) => Some(r),
            _ => None,
        });
    match existing {
        Some(r) => r.target_ppm += target_ppm - at_ppm,
        None => pipe.steps.push(ProcessingStep::new(
            StepKind::Reference(ReferenceParams { at_ppm, target_ppm }),
            StepSource::User,
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shift_appends_then_folds_into_existing_step() {
        let mut pipe = AxisPipeline::frequency_1d();
        let refs = |p: &AxisPipeline| {
            p.steps
                .iter()
                .filter_map(|s| match s.kind {
                    StepKind::Reference(r) => Some(r),
                    _ => None,
                })
                .collect::<Vec<_>>()
        };
        apply_reference_shift(&mut pipe, 2.0, 2.5);
        assert_eq!(refs(&pipe).len(), 1);
        assert_eq!(
            refs(&pipe)[0],
            ReferenceParams {
                at_ppm: 2.0,
                target_ppm: 2.5
            }
        );

        apply_reference_shift(&mut pipe, 4.0, 4.25);
        let all = refs(&pipe);
        assert_eq!(all.len(), 1);
        assert!((all[0].target_ppm - all[0].at_ppm - 0.75).abs() < 1e-12);
    }
}
