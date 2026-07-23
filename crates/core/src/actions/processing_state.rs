use super::*;

impl DatasetProcessingState {
    pub fn from_dataset(dataset: &Dataset) -> Self {
        match dataset {
            Dataset::Nmr(n) => Self::Nmr {
                pipeline: n.pipeline.clone(),
                group_delay_correct: n.group_delay_correct,
            },
            Dataset::Nmr2D(n) => Self::Nmr2D {
                params: n.params.clone(),
                preset: n.preset,
            },
            Dataset::Table(_) => Self::Table,
            Dataset::Electrophysiology(d) => Self::Electrophysiology(d.processing),
            Dataset::Afm(_) => Self::Afm,
        }
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
                let full = plotx_processing::needs_retransform(
                    pipeline,
                    &n.pipeline,
                    *group_delay_correct,
                    n.group_delay_correct,
                );
                n.pipeline = pipeline.clone();
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
            (Dataset::Nmr2D(n), Self::Nmr2D { params, preset }) => {
                let full = plotx_processing::needs_retransform_2d(params, &n.params);
                n.params = params.clone();
                n.preset = *preset;
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
            (dataset, state) => Err(ProcessingStateError {
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
#[error("cannot apply {state_kind} processing state to {dataset_kind} dataset")]
pub struct ProcessingStateError {
    pub dataset_kind: &'static str,
    pub state_kind: &'static str,
}
