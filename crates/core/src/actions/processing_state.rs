use super::*;
use crate::state::PhaseAxis;
use plotx_processing::ProcessingStep;

impl DatasetProcessingState {
    pub(crate) fn axis_pipeline_mut(&mut self, axis: PhaseAxis) -> Option<&mut AxisPipeline> {
        match self {
            Self::Nmr { pipeline, .. } if axis == PhaseAxis::Direct => Some(pipeline),
            Self::Nmr2D { params, .. } => match axis {
                PhaseAxis::F2 => Some(&mut params.f2),
                PhaseAxis::F1 => Some(&mut params.f1),
                PhaseAxis::Direct => None,
            },
            Self::Nmr { .. } | Self::Table | Self::Electrophysiology(_) | Self::Afm => None,
        }
    }

    pub(crate) fn group_delay_correct_mut(&mut self) -> Option<&mut bool> {
        match self {
            Self::Nmr {
                group_delay_correct,
                ..
            }
            | Self::Nmr2D {
                group_delay_correct,
                ..
            } => Some(group_delay_correct),
            Self::Table | Self::Electrophysiology(_) | Self::Afm => None,
        }
    }

    pub fn from_dataset(dataset: &Dataset) -> Self {
        match dataset {
            Dataset::Nmr(n) => Self::Nmr {
                pipeline: n.pipeline.clone(),
                group_delay_correct: n.group_delay_correct,
            },
            Dataset::Nmr2D(n) => Self::Nmr2D {
                params: n.params.clone(),
                preset: n.preset,
                group_delay_correct: n.group_delay_correct,
            },
            Dataset::Table(_) => Self::Table,
            Dataset::Electrophysiology(d) => Self::Electrophysiology(d.processing),
            Dataset::Afm(_) => Self::Afm,
        }
    }

    /// Every step of every axis this recipe carries.
    ///
    /// A caller that holds a `StepId` wants the step, not the dimension it
    /// happens to sit in: step identity is owner-local and stable, while the
    /// axis split is a detail of how a recipe is stored. Answering it here keeps
    /// that detail with the type that owns the variants instead of copying the
    /// split into every editor that addresses a step.
    pub fn steps_mut(&mut self) -> impl Iterator<Item = &mut ProcessingStep> {
        let pipelines: Vec<&mut AxisPipeline> = match self {
            Self::Nmr { pipeline, .. } => vec![pipeline],
            Self::Nmr2D { params, .. } => vec![&mut params.f2, &mut params.f1],
            Self::Table | Self::Electrophysiology(_) | Self::Afm => Vec::new(),
        };
        pipelines
            .into_iter()
            .flat_map(|pipeline| pipeline.steps.iter_mut())
    }

    /// Apply this recipe to a canonical dataset and rebuild only as much cached
    /// processing state as the recipe change requires. UI actions and headless
    /// workflows share this path so a scheme has identical numerical semantics.
    pub fn apply_to(
        &self,
        dataset: &mut Dataset,
    ) -> Result<ProcessingRebuild, ProcessingStateError> {
        match (dataset, self) {
            (
                Dataset::Nmr(n),
                Self::Nmr {
                    pipeline,
                    group_delay_correct,
                },
            ) => {
                pipeline.output_domain(n.data.domain).map_err(|error| {
                    ProcessingStateError::InvalidPipeline {
                        axis: "direct",
                        details: error.to_string(),
                    }
                })?;
                let full = plotx_processing::needs_retransform(
                    pipeline,
                    &n.pipeline,
                    *group_delay_correct,
                    n.group_delay_correct,
                );
                n.pipeline = pipeline.clone();
                n.repair_step_allocator();
                n.group_delay_correct = *group_delay_correct;
                let rebuild = if full {
                    n.retransform();
                    ProcessingRebuild::Retransformed
                } else {
                    n.rebuild();
                    ProcessingRebuild::Rebuilt
                };
                n.recompute_integrals();
                Ok(rebuild)
            }
            (
                Dataset::Nmr2D(n),
                Self::Nmr2D {
                    params,
                    preset,
                    group_delay_correct,
                },
            ) => {
                params.f2.output_domain(n.data.domain).map_err(|error| {
                    ProcessingStateError::InvalidPipeline {
                        axis: "F2",
                        details: error.to_string(),
                    }
                })?;
                params.f1.output_domain(n.data.domain).map_err(|error| {
                    ProcessingStateError::InvalidPipeline {
                        axis: "F1",
                        details: error.to_string(),
                    }
                })?;
                let full = plotx_processing::needs_retransform_2d(params, &n.params);
                let full = full || *group_delay_correct != n.group_delay_correct;
                n.params = params.clone();
                n.repair_step_allocator();
                n.preset = *preset;
                n.group_delay_correct = *group_delay_correct;
                if full {
                    n.retransform();
                    Ok(ProcessingRebuild::Retransformed)
                } else {
                    n.rebuild();
                    Ok(ProcessingRebuild::Rebuilt)
                }
            }
            (Dataset::Table(_), Self::Table) => Ok(ProcessingRebuild::Unchanged),
            (Dataset::Electrophysiology(data), Self::Electrophysiology(processing)) => {
                data.processing = *processing;
                Ok(ProcessingRebuild::Rebuilt)
            }
            (Dataset::Afm(_), Self::Afm) => Ok(ProcessingRebuild::Unchanged),
            (dataset, state) => Err(ProcessingStateError::KindMismatch {
                dataset_kind: dataset.kind_label(),
                state_kind: state.kind_label(),
            }),
        }
    }

    fn kind_label(&self) -> &'static str {
        match self {
            Self::Nmr { .. } => "NMR 1D",
            Self::Nmr2D { .. } => "NMR 2D",
            Self::Table => "Data Table",
            Self::Electrophysiology(_) => "Electrophysiology",
            Self::Afm => "AFM",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProcessingRebuild {
    Unchanged,
    Rebuilt,
    Retransformed,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ProcessingStateError {
    #[error("cannot apply {state_kind} processing state to {dataset_kind} dataset")]
    KindMismatch {
        dataset_kind: &'static str,
        state_kind: &'static str,
    },
    #[error("cannot apply invalid {axis} processing pipeline: {details}")]
    InvalidPipeline { axis: &'static str, details: String },
}
