//! Recipe compilation and staged estimation through the nmr public API.

use crate::{Apodization, AxisPipeline, PhaseParams, StepId, StepKind, ZeroFill};
use nmr::acquisition::GroupDelayState;
use nmr::axis::AxisDomain;
use nmr::processing::{
    DelaySource, DigitalFilterCorrection, FourierTransform, PhaseCorrection, ProcessingError,
    ProcessingErrorCode, ProcessingOperation as Op, ProcessingOptions, ProcessingPlan,
    SpectrumOperation, Window,
};
use nmr::{Dataset, ExecutionContext, dataset::DescriptorRef};
use std::{collections::BTreeSet, sync::Arc};

#[path = "nmr_bridge_phase.rs"]
mod phase;
pub use phase::{PhaseReport, RepresentativeTrace};

pub struct RecipeExecution {
    pub dataset: Arc<Dataset>,
    /// Estimation provenance for the owning recipe. Series history records the
    /// shared explicit correction; this report also identifies its representative.
    pub phases: Vec<PhaseReport>,
}

#[derive(Debug, thiserror::Error)]
pub enum RecipeError {
    #[error("invalid recipe: {0}")]
    Invalid(String),
    #[error("NMR processing failed at step {step:?}: {source}")]
    Library {
        step: Option<StepId>,
        #[source]
        source: ProcessingError,
    },
}

impl RecipeError {
    pub fn is_cancelled(&self) -> bool {
        matches!(self, Self::Library { source, .. } if source.code() == ProcessingErrorCode::Cancelled)
    }
}

#[derive(Clone, Copy, Debug)]
pub enum RecipeRange {
    All,
    /// Time prefix including the enabled FFT; cache this result before phasing.
    Base,
    /// Frequency suffix applied to an already cached base.
    Frequency,
    /// Preview through this stable step; disabled steps remain skipped.
    Through(StepId),
}

/// An invocation override, not a second persisted source of delay state.
#[derive(Clone, Copy, Debug)]
pub enum DelayPolicy {
    Disabled,
    AxisEvidence,
    Explicit(f64),
}

/// Bound to immutable input so a prepared recipe cannot accidentally execute
/// against a replacement dataset. Local operation positions never escape as IDs.
pub struct CompiledRecipe {
    input: Arc<Dataset>,
    segments: Vec<Segment>,
    step_ids: Vec<StepId>,
}

enum Segment {
    Setup(ProcessingPlan),
    Plan {
        plan: ProcessingPlan,
        ids: Vec<StepId>,
    },
    Phase {
        axis: usize,
        params: PhaseParams,
        id: StepId,
    },
}

impl CompiledRecipe {
    /// A deterministic prefix for the library NUS executor. Estimators require
    /// their staged input and cannot be flattened into this prefix.
    pub fn deterministic_plan(&self) -> Result<ProcessingPlan, RecipeError> {
        let mut operations = Vec::new();
        for segment in &self.segments {
            match segment {
                Segment::Setup(plan) | Segment::Plan { plan, .. } => {
                    operations.extend_from_slice(plan.operations())
                }
                Segment::Phase { .. } => {
                    return Err(RecipeError::Invalid(
                        "NUS direct prefix cannot contain phase estimation".into(),
                    ));
                }
            }
        }
        ProcessingPlan::new(operations)
            .map_err(|source| RecipeError::Library { step: None, source })
    }
    pub fn step_id(&self, local_index: usize) -> Option<StepId> {
        self.step_ids.get(local_index).copied()
    }

    pub fn execute(
        &self,
        options: ProcessingOptions,
        context: &mut ExecutionContext<'_>,
    ) -> Result<Arc<Dataset>, RecipeError> {
        self.execute_with_report(options, context)
            .map(|result| result.dataset)
    }

    pub fn execute_with_report(
        &self,
        options: ProcessingOptions,
        context: &mut ExecutionContext<'_>,
    ) -> Result<RecipeExecution, RecipeError> {
        context
            .check_cancelled()
            .map_err(|error| self.locate(error.into()))?;
        let mut output = Arc::clone(&self.input);
        let mut phases = Vec::new();
        for segment in &self.segments {
            output = Arc::new(match segment {
                Segment::Setup(plan) => plan
                    .apply_with_context(&output, options, context)
                    .map_err(|source| RecipeError::Library { step: None, source })?,
                Segment::Plan { plan, ids } => plan
                    .apply_with_context(&output, options, context)
                    .map_err(|source| RecipeError::Library {
                    step: source.step_index().and_then(|i| ids.get(i).copied()),
                    source,
                })?,
                Segment::Phase { axis, params, id } => {
                    let result = if let Some(method) = params.auto {
                        phase::apply_auto(
                            &output,
                            *axis,
                            *id,
                            phase_method(method),
                            options,
                            context,
                        )
                        .map(|(dataset, report)| {
                            phases.push(report);
                            dataset
                        })
                    } else {
                        apply_phase(&output, *axis, *params, options, context)
                    };
                    result.map_err(|source| RecipeError::Library {
                        step: Some(*id),
                        source,
                    })?
                }
            });
        }
        Ok(RecipeExecution {
            dataset: output,
            phases,
        })
    }

    fn locate(&self, source: ProcessingError) -> RecipeError {
        RecipeError::Library {
            step: source.step_index().and_then(|index| self.step_id(index)),
            source,
        }
    }
}

pub fn compile(
    input: Arc<Dataset>,
    pipeline: &AxisPipeline,
    axis: usize,
    delay: DelayPolicy,
    range: RecipeRange,
) -> Result<CompiledRecipe, RecipeError> {
    let mut seen = BTreeSet::new();
    if pipeline.steps.iter().any(|step| !seen.insert(step.id)) {
        return Err(RecipeError::Invalid("duplicate StepId".into()));
    }
    let end = match range {
        RecipeRange::Through(id) => pipeline
            .steps
            .iter()
            .position(|step| step.id == id)
            .map(|index| index + 1)
            .ok_or_else(|| RecipeError::Invalid(format!("preview step {id:?} does not exist")))?,
        _ => pipeline.steps.len(),
    };
    let (mut points, mut domain) = match input.descriptor() {
        DescriptorRef::Raw(descriptor) => descriptor
            .axes()
            .get(axis)
            .map(|axis| (axis.points(), axis.domain())),
        DescriptorRef::Processed(descriptor) => descriptor
            .axes()
            .get(axis)
            .map(|axis| (axis.points(), axis.domain())),
        _ => None,
    }
    .ok_or_else(|| RecipeError::Invalid(format!("axis {axis} does not exist")))?;
    let mut ops = Vec::new();
    let mut ids = Vec::new();
    let mut segments = Vec::new();
    if !matches!(range, RecipeRange::Frequency)
        && let Some(raw) = input.as_raw()
    {
        let decoding: Vec<_> = raw
            .descriptor()
            .axes()
            .iter()
            .enumerate()
            .filter_map(|(axis, value)| {
                matches!(
                    value.kind(),
                    nmr::raw::RawAxisKind::Indirect(nmr::raw::IndirectComponents::Encoded(_))
                )
                .then_some(Op::ComponentTransform { axis })
            })
            .collect();
        if !decoding.is_empty() {
            segments
                .push(Segment::Setup(ProcessingPlan::new(decoding).map_err(
                    |source| RecipeError::Library { step: None, source },
                )?));
        }
    }
    let mut step_ids = Vec::new();
    for step in &pipeline.steps[..end] {
        if !step.enabled {
            continue;
        }
        match range {
            RecipeRange::Frequency if step.kind.at_or_before_fft() => continue,
            RecipeRange::Base if !step.kind.at_or_before_fft() => break,
            _ => {}
        }
        let required = match step.kind.input_domain() {
            plotx_io::Domain::Time => AxisDomain::Time,
            plotx_io::Domain::Frequency => AxisDomain::Frequency,
        };
        if domain != required {
            return Err(RecipeError::Invalid(format!(
                "step {:?} ({}) requires {required:?}, found {domain:?}",
                step.id,
                step.kind.label()
            )));
        }
        let operation = match step.kind {
            StepKind::Apodize(Apodization::None) | StepKind::ZeroFill(ZeroFill::None) => None,
            StepKind::Apodize(Apodization::CosineBell) => Some(Op::Window {
                axis,
                window: Window::SineBell {
                    offset: 0.5,
                    end: 1.0,
                    power: 1.0,
                    first_point_scale: 1.0,
                },
            }),
            StepKind::Apodize(Apodization::Exponential { lb_hz }) => Some(Op::Window {
                axis,
                window: Window::Exponential { lb_hz },
            }),
            StepKind::Apodize(Apodization::Gaussian { lb_hz, gb_hz }) => Some(Op::Window {
                axis,
                window: Window::lorentz_to_gauss(lb_hz, gb_hz).map_err(|source| {
                    RecipeError::Library {
                        step: Some(step.id),
                        source,
                    }
                })?,
            }),
            StepKind::ZeroFill(fill) => {
                points = zero_fill_target(fill, points).map_err(|source| RecipeError::Library {
                    step: Some(step.id),
                    source,
                })?;
                Some(Op::ZeroFill {
                    axis,
                    zero_fill: nmr::processing::ZeroFill::new(points).map_err(|source| {
                        RecipeError::Library {
                            step: Some(step.id),
                            source,
                        }
                    })?,
                })
            }
            StepKind::Fft => {
                domain = AxisDomain::Frequency;
                Some(Op::FourierTransform {
                    axis,
                    transform: FourierTransform::default(),
                })
            }
            StepKind::Phase(params) => {
                flush_plan(&mut segments, &mut ops, &mut ids)?;
                segments.push(Segment::Phase {
                    axis,
                    params,
                    id: step.id,
                });
                step_ids.push(step.id);
                None
            }
            StepKind::Baseline(method) => Some(spectrum_op(
                axis,
                SpectrumOperation::Baseline(baseline(method)),
            )),
            StepKind::Magnitude => Some(spectrum_op(axis, SpectrumOperation::Magnitude)),
            StepKind::Reference(reference) => Some(spectrum_op(
                axis,
                SpectrumOperation::Reference {
                    delta_ppm: reference.target_ppm - reference.at_ppm,
                },
            )),
            StepKind::Smooth(method) => Some(spectrum_op(
                axis,
                match method {
                    crate::SmoothMethod::MovingAverage { window } => {
                        SpectrumOperation::MovingAverage {
                            window: usize::from(window),
                        }
                    }
                    crate::SmoothMethod::SavitzkyGolay { window, poly_order } => {
                        SpectrumOperation::SavitzkyGolay {
                            window: usize::from(window),
                            order: usize::from(poly_order),
                        }
                    }
                },
            )),
            StepKind::Normalize(method) => Some(spectrum_op(
                axis,
                SpectrumOperation::Normalize(match method {
                    crate::NormalizeMethod::MaxPeak => nmr::processing::Normalization::MaxPeak,
                    crate::NormalizeMethod::TotalArea => {
                        nmr::processing::Normalization::TotalArea {
                            singleton_width: None,
                        }
                    }
                    crate::NormalizeMethod::Constant { divisor } => {
                        nmr::processing::Normalization::Constant(divisor)
                    }
                }),
            )),
            StepKind::Bin(bin) => Some(spectrum_op(
                axis,
                SpectrumOperation::Bin {
                    width: bin.width,
                    aggregation: match bin.method {
                        crate::BinMethod::Sum => nmr::processing::BinAggregation::Sum,
                        crate::BinMethod::Mean => nmr::processing::BinAggregation::Mean,
                    },
                },
            )),
            StepKind::Reverse => Some(spectrum_op(axis, SpectrumOperation::Reverse)),
            StepKind::Invert => Some(spectrum_op(axis, SpectrumOperation::Invert)),
        };
        if let Some(operation) = operation {
            ops.push(operation);
            ids.push(step.id);
            step_ids.push(step.id);
        }
        if matches!(step.kind, StepKind::Fft) {
            if let Some(correction) =
                delay_correction(&input, axis, delay).map_err(|source| RecipeError::Library {
                    step: Some(step.id),
                    source,
                })?
            {
                ops.push(Op::DigitalFilterCorrection { axis, correction });
                ids.push(step.id);
                step_ids.push(step.id);
            }
            let has_reference = input
                .as_raw()
                .and_then(|raw| raw.descriptor().axes().get(axis))
                .is_some_and(|axis| axis.chemical_shift_reference().is_some())
                || input
                    .as_processed()
                    .and_then(|processed| processed.axis_evidence(axis))
                    .is_some_and(|evidence| evidence.chemical_shift_reference().is_some());
            if has_reference {
                ops.push(Op::ResolveFrequencyFrame {
                    axis,
                    frame: nmr::processing::FrequencyFrame::Ppm(
                        nmr::processing::ReferenceSource::AxisEvidence,
                    ),
                });
                ids.push(step.id);
                step_ids.push(step.id);
            }
            if matches!(range, RecipeRange::Base) {
                break;
            }
        }
    }
    flush_plan(&mut segments, &mut ops, &mut ids)?;
    Ok(CompiledRecipe {
        input,
        segments,
        step_ids,
    })
}

fn flush_plan(
    segments: &mut Vec<Segment>,
    ops: &mut Vec<Op>,
    ids: &mut Vec<StepId>,
) -> Result<(), RecipeError> {
    if !ops.is_empty() {
        let plan = ProcessingPlan::new(std::mem::take(ops))
            .map_err(|source| RecipeError::Library { step: None, source })?;
        segments.push(Segment::Plan {
            plan,
            ids: std::mem::take(ids),
        });
    }
    Ok(())
}

fn spectrum_op(axis: usize, operation: SpectrumOperation) -> Op {
    Op::Spectrum { axis, operation }
}

fn baseline(method: crate::BaselineMethod) -> nmr::processing::RealBaseline {
    match method {
        crate::BaselineMethod::Offset => nmr::processing::RealBaseline::Offset,
        crate::BaselineMethod::Polynomial { order } => nmr::processing::RealBaseline::Polynomial {
            order: usize::from(order),
        },
        crate::BaselineMethod::AsymmetricLeastSquares {
            smoothness,
            asymmetry,
            iterations,
        } => nmr::processing::RealBaseline::Asls {
            lambda: smoothness,
            asymmetry,
            iterations: usize::from(iterations),
        },
    }
}

fn phase_method(method: crate::AutoPhaseMethod) -> nmr::processing::PhaseMethod {
    match method {
        crate::AutoPhaseMethod::AbsorptivePeak => nmr::processing::PhaseMethod::AbsorptivePeak,
        crate::AutoPhaseMethod::Entropy => nmr::processing::PhaseMethod::Entropy,
        crate::AutoPhaseMethod::NegativeMinimization => {
            nmr::processing::PhaseMethod::NegativeMinimization
        }
        crate::AutoPhaseMethod::PeakRegression => nmr::processing::PhaseMethod::PeakRegression,
        crate::AutoPhaseMethod::RobustConsensus => nmr::processing::PhaseMethod::RobustConsensus,
    }
}

fn apply_phase(
    input: &Dataset,
    axis: usize,
    params: PhaseParams,
    options: ProcessingOptions,
    context: &mut ExecutionContext<'_>,
) -> Result<Dataset, ProcessingError> {
    // A previous bin may have changed the length; resolve the endpoint convention
    // from the actual intermediate descriptor, never the original input shape.
    let points = input
        .as_processed()
        .and_then(|p| p.descriptor().axes().get(axis))
        .ok_or(ProcessingError::InvalidParameter("phase input axis"))?
        .points();
    ProcessingPlan::new(vec![Op::PhaseCorrection {
        axis,
        correction: manual_phase(params, points)?,
    }])?
    .apply_with_context(input, options, context)
}

fn zero_fill_target(fill: ZeroFill, points: usize) -> Result<usize, ProcessingError> {
    match fill {
        ZeroFill::None => Ok(points),
        ZeroFill::Size(size) => Ok(size.max(points)),
        ZeroFill::Factor(factor) => points
            .checked_next_power_of_two()
            .and_then(|base| {
                1usize
                    .checked_shl(u32::from(factor.saturating_sub(1)))
                    .and_then(|multiplier| base.checked_mul(multiplier))
            })
            .ok_or(ProcessingError::SizeOverflow),
    }
}

/// PlotX uses exp(-i phi), radians and i/(N-1). nmr uses exp(+i phi),
/// degrees and i/N. Pivot is an index fraction regardless of coordinate direction.
fn manual_phase(params: PhaseParams, points: usize) -> Result<PhaseCorrection, ProcessingError> {
    if !params.pivot_frac.is_finite() || !(0.0..=1.0).contains(&params.pivot_frac) {
        return Err(ProcessingError::InvalidParameter("phase pivot"));
    }
    if points == 1 {
        PhaseCorrection::new(
            (-params.phase0 + params.phase1 * params.pivot_frac).to_degrees(),
            0.0,
            0.0,
        )
    } else {
        let scale = points as f64 / (points - 1) as f64;
        PhaseCorrection::new(
            -params.phase0.to_degrees(),
            -params.phase1.to_degrees() * scale,
            params.pivot_frac / scale,
        )
    }
}

fn delay_correction(
    input: &Dataset,
    axis: usize,
    policy: DelayPolicy,
) -> Result<Option<DigitalFilterCorrection>, ProcessingError> {
    use nmr::processed::ProcessedGroupDelay;
    let state = input
        .as_raw()
        .and_then(|raw| raw.descriptor().axes().get(axis))
        .map(|axis| match axis.group_delay() {
            GroupDelayState::NotApplicable => ProcessedGroupDelay::NotApplicable,
            GroupDelayState::Pending(delay) => ProcessedGroupDelay::Pending(delay),
            _ => ProcessedGroupDelay::Unknown,
        })
        .or_else(|| {
            input
                .as_processed()?
                .axis_evidence(axis)
                .map(|e| e.group_delay())
        });
    let correction = match policy {
        DelayPolicy::Disabled => return Ok(None),
        DelayPolicy::Explicit(0.0) => {
            // Explicitly disabling correction is different from certifying unknown
            // hardware delay as zero. nmr requires established zero evidence.
            DigitalFilterCorrection::AcknowledgeZeroDelayV1
        }
        DelayPolicy::Explicit(value) => {
            DigitalFilterCorrection::FrequencyDomainPhaseRampV1(DelaySource::Explicit(value))
        }
        DelayPolicy::AxisEvidence => match state {
            Some(ProcessedGroupDelay::NotApplicable | ProcessedGroupDelay::Corrected { .. }) => {
                return Ok(None);
            }
            Some(ProcessedGroupDelay::Pending(delay)) if delay.delay_points() == 0.0 => {
                DigitalFilterCorrection::AcknowledgeZeroDelayV1
            }
            _ => DigitalFilterCorrection::FrequencyDomainPhaseRampV1(DelaySource::AxisEvidence),
        },
    };
    Ok(Some(correction))
}

#[cfg(test)]
#[path = "nmr_bridge_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "nmr_bridge_staged_tests.rs"]
mod staged_tests;
