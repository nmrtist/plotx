use super::*;
use crate::actions::{Action, DatasetProcessingState};
use crate::state::DatasetId;
use std::collections::HashSet;

const SCHEME_VERSION: u32 = 1;

/// Written to a single `*.plotxproc` file so a recipe can travel between
/// datasets and workspaces.
#[derive(Serialize, Deserialize)]
pub struct ProcessingScheme {
    pub schema_version: u32,
    pub dimension_count: usize,
    pub pipelines: Vec<AxisPipelineDto>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub layout: Option<String>,
    #[serde(default = "scheme_gd_default")]
    pub group_delay_correct: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SchemeApplicationPolicy {
    #[default]
    StrictAll,
    CompatibleOnly,
}

#[derive(Clone, Debug, PartialEq)]
pub enum SchemeTargetResult {
    /// A dataset that accepts the scheme. The identity lives here rather than
    /// beside the result because only this variant can have one: an
    /// incompatible target may be a stale index with no dataset behind it.
    Compatible {
        dataset_id: DatasetId,
        before: DatasetProcessingState,
        after: DatasetProcessingState,
    },
    Incompatible {
        reason: String,
    },
}

impl SchemeTargetResult {
    pub fn is_compatible(&self) -> bool {
        matches!(self, Self::Compatible { .. })
    }

    pub fn incompatibility_reason(&self) -> Option<&str> {
        match self {
            Self::Compatible { .. } => None,
            Self::Incompatible { reason } => Some(reason),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct SchemeApplicationTarget {
    pub dataset: usize,
    pub result: SchemeTargetResult,
}

/// Full-selection preflight. No compatibility work is deferred to commit time.
#[derive(Clone, Debug, PartialEq)]
pub struct SchemeApplicationPlan {
    targets: Vec<SchemeApplicationTarget>,
}

impl SchemeApplicationPlan {
    pub fn targets(&self) -> &[SchemeApplicationTarget] {
        &self.targets
    }

    pub fn compatible_count(&self) -> usize {
        self.targets
            .iter()
            .filter(|target| target.result.is_compatible())
            .count()
    }

    pub fn incompatible_count(&self) -> usize {
        self.targets.len() - self.compatible_count()
    }

    /// Materialize an infallible commit transaction from preflight results.
    pub fn prepare(&self, policy: SchemeApplicationPolicy) -> Option<PreparedSchemeApplication> {
        if policy == SchemeApplicationPolicy::StrictAll && self.incompatible_count() != 0 {
            return None;
        }
        let mut actions = Vec::new();
        let mut applied_targets = Vec::new();
        let mut skipped_targets = Vec::new();
        for target in &self.targets {
            match &target.result {
                SchemeTargetResult::Compatible {
                    dataset_id,
                    before,
                    after,
                } => {
                    applied_targets.push(target.dataset);
                    actions.push(Action::update_dataset_processing(
                        *dataset_id,
                        before.clone(),
                        after.clone(),
                    ));
                }
                SchemeTargetResult::Incompatible { .. } => skipped_targets.push(target.dataset),
            }
        }
        if actions.is_empty() {
            return None;
        }
        Some(PreparedSchemeApplication {
            action: Action::Composite(actions),
            applied_targets,
            skipped_targets,
        })
    }
}

#[derive(Clone)]
pub struct PreparedSchemeApplication {
    pub action: Action,
    pub applied_targets: Vec<usize>,
    pub skipped_targets: Vec<usize>,
}

/// Preflight the complete selection. Duplicate indices are normalized in
/// first-seen order; stale indices remain visible as incompatible targets.
pub fn plan_scheme_application(
    scheme: &ProcessingScheme,
    datasets: &[Dataset],
    selected: &[usize],
) -> SchemeApplicationPlan {
    let mut seen = HashSet::new();
    let targets = selected
        .iter()
        .copied()
        .filter(|dataset| seen.insert(*dataset))
        .map(|dataset| {
            let result = match datasets.get(dataset) {
                Some(target) => match apply_scheme(scheme, target) {
                    Ok(after) => SchemeTargetResult::Compatible {
                        dataset_id: target.resource_id(),
                        before: DatasetProcessingState::from_dataset(target),
                        after,
                    },
                    Err(error) => SchemeTargetResult::Incompatible {
                        reason: error.to_string(),
                    },
                },
                None => SchemeTargetResult::Incompatible {
                    reason: "selected dataset no longer exists".to_owned(),
                },
            };
            SchemeApplicationTarget { dataset, result }
        })
        .collect();
    SchemeApplicationPlan { targets }
}

fn scheme_gd_default() -> bool {
    true
}

/// Every step is marked user-authored, since to a new dataset it is a
/// deliberate choice.
pub fn save_scheme(path: &Path, dataset: &Dataset) -> std::io::Result<()> {
    let scheme = scheme_from_dataset(dataset).ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "dataset has no processing pipeline",
        )
    })?;
    let json = serde_json::to_vec_pretty(&scheme).map_err(std::io::Error::other)?;
    std::fs::write(path, json)
}

pub fn load_scheme(path: &Path) -> Result<ProcessingScheme> {
    let bytes = std::fs::read(path)?;
    let scheme: ProcessingScheme = serde_json::from_slice(&bytes)?;
    if scheme.schema_version != SCHEME_VERSION {
        return Err(ProjectError::Unsupported(format!(
            "processing scheme version {}",
            scheme.schema_version
        )));
    }
    Ok(scheme)
}

pub fn apply_scheme(
    scheme: &ProcessingScheme,
    dataset: &Dataset,
) -> Result<DatasetProcessingState> {
    match dataset {
        Dataset::Nmr(_) => {
            if scheme.dimension_count != 1 {
                return Err(incompatible("a 1D spectrum needs a single-axis scheme"));
            }
            let dto = scheme
                .pipelines
                .first()
                .ok_or_else(|| incompatible("scheme carries no pipeline"))?;
            require_fft(dto)?;
            let mut pipeline = pipeline_from_dto(dto);
            remint_pipeline(&mut pipeline, &mut dataset_next_step_id(dataset));
            Ok(DatasetProcessingState::Nmr {
                pipeline,
                group_delay_correct: scheme.group_delay_correct,
            })
        }
        Dataset::Nmr2D(n) => {
            if scheme.dimension_count != 2 {
                return Err(incompatible("a 2D spectrum needs a two-axis scheme"));
            }
            let f2 = scheme
                .pipelines
                .first()
                .ok_or_else(|| incompatible("scheme carries no direct-axis pipeline"))?;
            let f1 = scheme
                .pipelines
                .get(1)
                .ok_or_else(|| incompatible("scheme carries no indirect-axis pipeline"))?;
            require_fft(f2)?;
            let layout = scheme
                .layout
                .as_deref()
                .map(layout_from_str)
                .unwrap_or(n.params.layout);
            let mut params = Params2D {
                layout,
                f2: pipeline_from_dto(f2),
                f1: pipeline_from_dto(f1),
            };
            let mut next = dataset_next_step_id(dataset);
            remint_pipeline(&mut params.f2, &mut next);
            remint_pipeline(&mut params.f1, &mut next);
            Ok(DatasetProcessingState::Nmr2D {
                params,
                preset: n.preset,
            })
        }
        Dataset::Table(_) => Err(incompatible("a data table has no processing pipeline")),
        Dataset::Electrophysiology(_) => Err(incompatible(
            "this processing scheme contains no steps applicable to the selected dataset",
        )),
        Dataset::Afm(_) => Err(incompatible(
            "an AFM dataset has no spectral processing pipeline",
        )),
    }
}

fn dataset_next_step_id(dataset: &Dataset) -> u64 {
    match dataset {
        Dataset::Nmr(dataset) => dataset.next_step_id,
        Dataset::Nmr2D(dataset) => dataset.next_step_id,
        _ => 0,
    }
}

/// Renumber a pipeline that is about to be adopted by `dataset`, so its steps
/// take identities the owner's allocator has not handed out.
fn remint_pipeline(pipeline: &mut AxisPipeline, next: &mut u64) {
    for step in &mut pipeline.steps {
        step.id = StepId::new(*next);
        *next = next.checked_add(1).expect("step id overflow");
    }
}

pub fn reset_processing(dataset: &Dataset) -> Option<DatasetProcessingState> {
    let mut state = match dataset {
        Dataset::Nmr(_) => Some(DatasetProcessingState::Nmr {
            pipeline: AxisPipeline::default_1d(),
            group_delay_correct: true,
        }),
        Dataset::Nmr2D(n) => Some(DatasetProcessingState::Nmr2D {
            params: Params2D::default_for(n.preset),
            preset: n.preset,
        }),
        Dataset::Table(_) => None,
        Dataset::Electrophysiology(_) => None,
        Dataset::Afm(_) => None,
    }?;
    let mut next = dataset_next_step_id(dataset);
    match &mut state {
        DatasetProcessingState::Nmr { pipeline, .. } => remint_pipeline(pipeline, &mut next),
        DatasetProcessingState::Nmr2D { params, .. } => {
            remint_pipeline(&mut params.f2, &mut next);
            remint_pipeline(&mut params.f1, &mut next);
        }
        _ => {}
    }
    Some(state)
}

fn scheme_from_dataset(dataset: &Dataset) -> Option<ProcessingScheme> {
    match dataset {
        Dataset::Nmr(n) => {
            let mut dto = pipeline_to_dto(&n.pipeline);
            force_user(&mut dto);
            strip_step_identities(&mut dto);
            Some(ProcessingScheme {
                schema_version: SCHEME_VERSION,
                dimension_count: 1,
                pipelines: vec![dto],
                layout: None,
                group_delay_correct: n.group_delay_correct,
            })
        }
        Dataset::Nmr2D(n) => {
            let mut f2 = pipeline_to_dto(&n.params.f2);
            let mut f1 = pipeline_to_dto(&n.params.f1);
            force_user(&mut f2);
            force_user(&mut f1);
            strip_step_identities(&mut f2);
            strip_step_identities(&mut f1);
            Some(ProcessingScheme {
                schema_version: SCHEME_VERSION,
                dimension_count: 2,
                pipelines: vec![f2, f1],
                layout: Some(layout_to_str(n.params.layout).to_owned()),
                group_delay_correct: n.group_delay_correct,
            })
        }
        Dataset::Table(_) => None,
        Dataset::Electrophysiology(_) => None,
        Dataset::Afm(_) => None,
    }
}

fn force_user(dto: &mut AxisPipelineDto) {
    for step in &mut dto.steps {
        step.source = StepSourceDto::User;
    }
}

fn require_fft(dto: &AxisPipelineDto) -> Result<()> {
    if dto.steps.iter().any(|s| matches!(s.kind, StepKindDto::Fft)) {
        Ok(())
    } else {
        Err(incompatible("scheme pipeline is missing its FFT anchor"))
    }
}

fn incompatible(msg: &str) -> ProjectError {
    ProjectError::Unsupported(format!("incompatible processing scheme: {msg}"))
}

#[cfg(test)]
mod plan_tests {
    use super::*;

    fn state() -> DatasetProcessingState {
        DatasetProcessingState::Nmr {
            pipeline: AxisPipeline::default_1d(),
            group_delay_correct: true,
        }
    }

    #[test]
    fn strict_blocks_all_while_compatible_only_prepares_one_composite() {
        let plan = SchemeApplicationPlan {
            targets: vec![
                SchemeApplicationTarget {
                    dataset: 2,
                    result: SchemeTargetResult::Compatible {
                        dataset_id: DatasetId::new(),
                        before: state(),
                        after: state(),
                    },
                },
                SchemeApplicationTarget {
                    dataset: 4,
                    result: SchemeTargetResult::Incompatible {
                        reason: "wrong dimension".to_owned(),
                    },
                },
            ],
        };
        assert!(plan.prepare(SchemeApplicationPolicy::StrictAll).is_none());
        let prepared = plan
            .prepare(SchemeApplicationPolicy::CompatibleOnly)
            .unwrap();
        assert_eq!(prepared.applied_targets, vec![2]);
        assert_eq!(prepared.skipped_targets, vec![4]);
        assert!(matches!(prepared.action, Action::Composite(actions) if actions.len() == 1));
    }

    #[test]
    fn preflight_keeps_a_stale_selection_visible_and_deduplicates_it() {
        let scheme = ProcessingScheme {
            schema_version: SCHEME_VERSION,
            dimension_count: 1,
            pipelines: Vec::new(),
            layout: None,
            group_delay_correct: true,
        };
        let plan = plan_scheme_application(&scheme, &[], &[9, 9]);
        assert_eq!(plan.targets().len(), 1);
        assert_eq!(
            plan.targets()[0].result.incompatibility_reason(),
            Some("selected dataset no longer exists")
        );
    }
}
