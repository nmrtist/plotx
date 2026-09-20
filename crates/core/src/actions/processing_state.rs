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
            Self::Nmr { .. }
            | Self::Table
            | Self::Electrophysiology(_)
            | Self::Afm
            | Self::Xrd(_)
            | Self::Xps { .. } => None,
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
            Self::Table
            | Self::Electrophysiology(_)
            | Self::Afm
            | Self::Xrd(_)
            | Self::Xps { .. } => None,
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
                nus_request: n.nus_request,
                group_delay_correct: n.group_delay_correct,
            },
            Dataset::Table(_) => Self::Table,
            Dataset::Electrophysiology(d) => Self::Electrophysiology(d.processing),
            Dataset::Afm(_) => Self::Afm,
            Dataset::MassSpec(_) => Self::Table,
            Dataset::Xrd(data) => Self::Xrd(data.params),
            Dataset::Xps(xps) => Self::Xps {
                active_region: xps.active_region,
                measurement_shifts: xps.measurement_shifts.clone(),
                region_recipes: xps.region_recipes.clone(),
                fit_workspaces: xps.fit_workspaces.clone(),
                fits: xps.fits.clone(),
                next_step_id: xps.next_step_id,
            },
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
            Self::Table
            | Self::Electrophysiology(_)
            | Self::Afm
            | Self::Xrd(_)
            | Self::Xps { .. } => Vec::new(),
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
                pipeline.output_domain(n.input_domain()).map_err(|error| {
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
                let mut next = (**n).clone();
                next.pipeline = pipeline.clone();
                next.repair_step_allocator();
                next.group_delay_correct = *group_delay_correct;
                let result = if full {
                    next.retransform()
                } else {
                    next.rebuild()
                };
                result.map_err(|details| ProcessingStateError::InvalidPipeline {
                    axis: "direct",
                    details,
                })?;
                next.recompute_integrals();
                **n = next;
                let rebuild = if full {
                    ProcessingRebuild::Retransformed
                } else {
                    ProcessingRebuild::Rebuilt
                };
                Ok(rebuild)
            }
            (
                Dataset::Nmr2D(n),
                Self::Nmr2D {
                    params,
                    preset,
                    nus_request,
                    group_delay_correct,
                },
            ) => {
                plotx_processing::nmr_execution::validate_2d_domains(&n.data, params).map_err(
                    |details| ProcessingStateError::InvalidPipeline {
                        axis: "2D",
                        details,
                    },
                )?;
                let full = plotx_processing::needs_retransform_2d(params, &n.params)
                    || *group_delay_correct != n.group_delay_correct
                    || *nus_request != n.nus_request;
                let mut next = (**n).clone();
                next.params = params.clone();
                next.repair_step_allocator();
                next.preset = *preset;
                next.nus_request = *nus_request;
                next.group_delay_correct = *group_delay_correct;
                let result = if full {
                    next.retransform()
                } else {
                    next.rebuild()
                };
                result.map_err(|details| ProcessingStateError::InvalidPipeline {
                    axis: "2D",
                    details,
                })?;
                **n = next;
                Ok(if full {
                    ProcessingRebuild::Retransformed
                } else {
                    ProcessingRebuild::Rebuilt
                })
            }
            (Dataset::Table(_), Self::Table) => Ok(ProcessingRebuild::Unchanged),
            (Dataset::Electrophysiology(data), Self::Electrophysiology(processing)) => {
                data.processing = *processing;
                Ok(ProcessingRebuild::Rebuilt)
            }
            (Dataset::Afm(_), Self::Afm) => Ok(ProcessingRebuild::Unchanged),
            (Dataset::Xrd(data), Self::Xrd(processing)) => {
                data.apply_processing(*processing).map_err(|error| {
                    ProcessingStateError::InvalidPipeline {
                        axis: "2theta",
                        details: error.to_string(),
                    }
                })?;
                Ok(ProcessingRebuild::Rebuilt)
            }
            (
                Dataset::Xps(xps),
                Self::Xps {
                    active_region,
                    measurement_shifts,
                    region_recipes,
                    fit_workspaces,
                    fits,
                    next_step_id,
                },
            ) => {
                if xps.region(*active_region).is_none() {
                    return Err(ProcessingStateError::InvalidXps(
                        "the selected XPS region no longer exists".into(),
                    ));
                }
                let expected_measurements = xps
                    .experiment
                    .measurements
                    .iter()
                    .map(|measurement| measurement.id)
                    .collect::<std::collections::BTreeSet<_>>();
                let expected_workspaces = xps
                    .experiment
                    .regions
                    .iter()
                    .filter(|region| region.binding_energy_ev.is_some())
                    .map(|region| region.id)
                    .collect::<std::collections::BTreeSet<_>>();
                if measurement_shifts
                    .keys()
                    .copied()
                    .collect::<std::collections::BTreeSet<_>>()
                    != expected_measurements
                {
                    return Err(ProcessingStateError::InvalidXps(
                        "XPS measurement shift identities do not match the experiment".into(),
                    ));
                }
                let expected_regions = xps
                    .experiment
                    .regions
                    .iter()
                    .map(|region| region.id)
                    .collect::<std::collections::BTreeSet<_>>();
                if region_recipes
                    .keys()
                    .copied()
                    .collect::<std::collections::BTreeSet<_>>()
                    != expected_regions
                {
                    return Err(ProcessingStateError::InvalidXps(
                        "XPS region recipe identities do not match the experiment".into(),
                    ));
                }
                if measurement_shifts.values().any(|shift| !shift.is_finite()) {
                    return Err(ProcessingStateError::InvalidXps(
                        "XPS measurement energy shifts must be finite".into(),
                    ));
                }
                let current_workspaces = fit_workspaces
                    .keys()
                    .copied()
                    .collect::<std::collections::BTreeSet<_>>();
                if current_workspaces != expected_workspaces
                    || fits
                        .keys()
                        .any(|region| !expected_workspaces.contains(region))
                {
                    return Err(ProcessingStateError::InvalidXps(
                        "XPS fitting workspace identities do not match the experiment".into(),
                    ));
                }
                let mut step_ids = std::collections::BTreeSet::new();
                for recipe in region_recipes.values() {
                    for step in &recipe.steps {
                        if !step_ids.insert(step.id) || step.id.get() >= *next_step_id {
                            return Err(ProcessingStateError::InvalidXps(
                                "XPS processing step identities or allocator are invalid".into(),
                            ));
                        }
                    }
                }
                if *next_step_id == 0 {
                    return Err(ProcessingStateError::InvalidXps(
                        "XPS processing step allocator is invalid".into(),
                    ));
                }
                for region in &xps.experiment.regions {
                    let Some(shift) = measurement_shifts.get(&region.measurement) else {
                        return Err(ProcessingStateError::InvalidXps(format!(
                            "measurement {} has no energy shift",
                            region.measurement.0
                        )));
                    };
                    let Some(recipe) = region_recipes.get(&region.id) else {
                        return Err(ProcessingStateError::InvalidXps(format!(
                            "region {} has no processing recipe",
                            region.id.0
                        )));
                    };
                    let (energy, applied_shift) = region
                        .binding_energy_ev
                        .as_ref()
                        .map_or((&region.native_energy_ev, 0.0), |binding| (binding, *shift));
                    plotx_processing::xps::process_region(
                        energy,
                        &region.intensity_cps,
                        applied_shift,
                        recipe,
                    )
                    .map_err(|message| ProcessingStateError::InvalidXps(message.into()))?;
                    if region.binding_energy_ev.is_some() {
                        let workspace = fit_workspaces.get(&region.id).ok_or_else(|| {
                            ProcessingStateError::InvalidXps(format!(
                                "region {} has no fit workspace",
                                region.id.0
                            ))
                        })?;
                        plotx_analysis::xps::validate_xps_constraints(&workspace.invocation)
                            .map_err(|error| ProcessingStateError::InvalidXps(error.to_string()))?;
                        let next = workspace
                            .invocation
                            .peaks
                            .iter()
                            .map(|peak| peak.id.0)
                            .max()
                            .unwrap_or(0)
                            .saturating_add(1);
                        if workspace.next_component_id < next {
                            return Err(ProcessingStateError::InvalidXps(format!(
                                "region {} has an invalid component allocator",
                                region.id.0
                            )));
                        }
                        for fit in fits.get(&region.id).into_iter().flatten() {
                            if fit.region != region.id {
                                return Err(ProcessingStateError::InvalidXps(format!(
                                    "region {} has mismatched fit provenance",
                                    region.id.0
                                )));
                            }
                            plotx_analysis::xps::validate_xps_fit_summary(
                                &fit.invocation,
                                &fit.result,
                            )
                            .map_err(|error| {
                                ProcessingStateError::InvalidXps(format!(
                                    "region {} has an invalid fit: {error}",
                                    region.id.0
                                ))
                            })?;
                        }
                    }
                }
                xps.active_region = *active_region;
                xps.measurement_shifts = measurement_shifts.clone();
                xps.region_recipes = region_recipes.clone();
                xps.fit_workspaces = fit_workspaces.clone();
                xps.fits = fits.clone();
                xps.next_step_id = *next_step_id;
                Ok(ProcessingRebuild::Rebuilt)
            }
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
            Self::Xrd(_) => "XRD",
            Self::Xps { .. } => "XPS",
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
    #[error("cannot apply invalid XPS processing state: {0}")]
    InvalidXps(String),
}
