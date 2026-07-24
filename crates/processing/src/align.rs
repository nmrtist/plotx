//! Pipeline shifting for multi-spectrum reference alignment.

use crate::{AxisPipeline, ProcessingStep, ReferenceParams, StepId, StepKind, StepSource};

/// Shift the axis so the point at `at_ppm` reads `target_ppm`, through the
/// pipeline's referencing step: the last enabled Reference step absorbs the
/// extra translation, or a new user step is appended when none exists.
///
/// `new_step_id` is only consumed in that append case, and it must come from
/// the owning dataset's allocator: `pipe` is a live recipe, so a step minted
/// with template-local numbering would collide with the steps already there.
/// This crate sits below `plotx-core` and cannot reach the owner itself, so the
/// identity is supplied by the caller.
pub fn apply_reference_shift(
    pipe: &mut AxisPipeline,
    at_ppm: f64,
    target_ppm: f64,
    new_step_id: StepId,
) {
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
            new_step_id,
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
        apply_reference_shift(&mut pipe, 2.0, 2.5, StepId::new(7));
        assert_eq!(refs(&pipe).len(), 1);
        assert_eq!(
            refs(&pipe)[0],
            ReferenceParams {
                at_ppm: 2.0,
                target_ppm: 2.5
            }
        );

        apply_reference_shift(&mut pipe, 4.0, 4.25, StepId::new(8));
        let all = refs(&pipe);
        assert_eq!(all.len(), 1);
        assert!((all[0].target_ppm - all[0].at_ppm - 0.75).abs() < 1e-12);
    }

    /// The appended step takes the caller-supplied identity verbatim: the
    /// pipeline it lands in already numbers its own steps from zero, so a
    /// template-local id would alias an existing row.
    #[test]
    fn an_appended_reference_step_uses_the_supplied_identity() {
        let mut pipe = AxisPipeline::frequency_1d();
        apply_reference_shift(&mut pipe, 2.0, 2.5, StepId::new(9));
        let appended = pipe.steps.last().expect("a step was appended");
        assert_eq!(appended.id, StepId::new(9));
        assert!(pipe.steps.iter().filter(|s| s.id == StepId::new(9)).count() == 1);
    }
}
