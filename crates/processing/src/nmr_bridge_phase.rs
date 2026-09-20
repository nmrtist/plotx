//! Representative selection is application policy; estimation and rotation belong to nmr.

use super::*;
use nmr::processing::PhaseMethod;

#[derive(Clone, Debug)]
pub struct PhaseReport {
    pub step: StepId,
    pub axis: usize,
    pub points: usize,
    pub display_pivot: usize,
    pub method: PhaseMethod,
    pub correction: PhaseCorrection,
    pub objective: f64,
    pub evaluations: usize,
    pub input: nmr::provenance::CanonicalDatasetDigests,
    pub representative: Option<RepresentativeTrace>,
}

impl PhaseReport {
    /// Convert the library's exp(+i phase), i/N convention to the recipe's
    /// exp(-i phase), i/(N-1) convention without estimating another correction.
    pub fn recipe_parameters(&self) -> (f64, f64, f64) {
        let scale = if self.points > 1 {
            (self.points - 1) as f64 / self.points as f64
        } else {
            1.0
        };
        let phase1 = -self.correction.p1_degrees().to_radians() * scale;
        let pivot = if self.points > 1 {
            self.display_pivot as f64 / (self.points - 1) as f64
        } else {
            0.0
        };
        let phase0 = -self.correction.p0_degrees().to_radians()
            + phase1 * (pivot - self.correction.pivot_fraction() / scale);
        (phase0, phase1, pivot)
    }
}

/// Logical selection bound to `PhaseReport::input`, not application collection positions.
#[derive(Clone, Debug)]
pub struct RepresentativeTrace {
    pub removed_axis: usize,
    pub index: usize,
    pub component: usize,
    pub input: nmr::provenance::CanonicalDatasetDigests,
}

pub(super) fn apply_auto(
    input: &Dataset,
    axis: usize,
    step: StepId,
    method: PhaseMethod,
    options: ProcessingOptions,
    context: &mut ExecutionContext<'_>,
) -> Result<(Dataset, PhaseReport), ProcessingError> {
    let processed = input
        .as_processed()
        .ok_or(ProcessingError::InvalidParameter(
            "automatic phase requires processed input",
        ))?;
    let axes = processed.descriptor().axes();
    if axes.len() == 1 {
        let estimate = method
            .prepare(input, axis, options)?
            .estimate_with_context(context)?;
        let output = estimate.apply_with_context(input, options, context)?;
        let mut pivot = 0;
        let mut peak = -1.0_f64;
        for point in 0..axes[0].points() {
            if point % 4096 == 0 {
                context.check_cancelled()?;
            }
            let re = processed
                .data()
                .get(&[point], &[0])
                .map_err(|_| ProcessingError::InvalidParameter("phase pivot"))?;
            let im = processed
                .data()
                .get(&[point], &[1])
                .map_err(|_| ProcessingError::InvalidParameter("phase pivot"))?;
            if re.hypot(im) > peak {
                pivot = point;
                peak = re.hypot(im);
            }
        }
        return Ok((output, report(input, axis, step, &estimate, None, pivot)));
    }
    if axes.len() != 2 || axis >= 2 {
        return Err(ProcessingError::InvalidParameter("automatic phase axis"));
    }
    // Choose the trace through the strongest Cartesian component. Selecting by
    // peak amplitude avoids overflow from summing squared, unscaled samples and
    // also handles an exactly zero real plane in hypercomplex input.
    let other = 1 - axis;
    let shared_pair = axes.iter().any(|axis| {
        matches!(
            axis.component_basis(),
            nmr::processed::ComponentBasis::SharedComplex { .. }
        )
    });
    let data = processed.data();
    let mut peak = -1.0_f64;
    let mut selection = (0, 0);
    let mut display_pivot = 0;
    for index in 0..axes[other].points() {
        context.check_cancelled()?;
        for component in 0..axes[other].component_count() {
            for point in 0..axes[axis].points() {
                if point % 4096 == 0 {
                    context.check_cancelled()?;
                }
                let mut coordinates = [0; 2];
                coordinates[axis] = point;
                coordinates[other] = index;
                for channel in 0..axes[axis].component_count() {
                    let mut components = [0; 2];
                    components[axis] = channel;
                    components[other] = component;
                    let value = data.get(&coordinates, &components).map_err(|_| {
                        ProcessingError::InvalidParameter("representative phase component")
                    })?;
                    if value.abs() > peak {
                        peak = value.abs();
                        // Shared fields form one complex trace even when its
                        // imaginary field is strongest; Slice selects that pair at zero.
                        selection = (index, if shared_pair { 0 } else { component });
                        display_pivot = point;
                    }
                }
            }
        }
    }
    let representative = ProcessingPlan::new(vec![spectrum_op(
        other,
        SpectrumOperation::Slice {
            index: selection.0,
            component: selection.1,
        },
    )])?
    .apply_with_context(input, options, context)?;
    let phase = method
        .prepare(&representative, 0, options)?
        .estimate_with_context(context)?;
    let output = ProcessingPlan::new(vec![Op::PhaseCorrection {
        axis,
        correction: phase.correction(),
    }])?
    .apply_with_context(input, options, context)?;
    let representative = RepresentativeTrace {
        removed_axis: other,
        index: selection.0,
        component: selection.1,
        input: representative.canonical_digests(),
    };
    Ok((
        output,
        report(
            input,
            axis,
            step,
            &phase,
            Some(representative),
            display_pivot,
        ),
    ))
}

fn report(
    input: &Dataset,
    axis: usize,
    step: StepId,
    phase: &nmr::processing::PhaseEstimate,
    representative: Option<RepresentativeTrace>,
    display_pivot: usize,
) -> PhaseReport {
    PhaseReport {
        step,
        axis,
        points: input
            .as_processed()
            .expect("phase input is processed")
            .descriptor()
            .axes()[axis]
            .points(),
        display_pivot,
        method: phase.method(),
        correction: phase.correction(),
        objective: phase.objective(),
        evaluations: phase.evaluations(),
        input: input.canonical_digests(),
        representative,
    }
}
