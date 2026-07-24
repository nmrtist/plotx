//! Processing-recipe commits: the pause gate and on-plot phase seeding.

use super::*;

impl PlotxApp {
    /// Record an action whose final state is already live, without applying it
    /// again. Multi-surface processing sessions use this for their final commit.
    fn record_applied_processing_action(&mut self, action: Action) -> Result<(), ActionApplyError> {
        if action.is_noop() {
            return Ok(());
        }
        validate_action(self, &action, &mut ValidationShape::from_app(self))?;
        self.session.undo_stack.push(action);
        if self.session.undo_stack.len() > self.session.history_limit {
            self.session.undo_stack.remove(0);
        }
        self.session.redo_stack.clear();
        self.doc.dirty = true;
        self.doc.automation_revision = self.doc.automation_revision.saturating_add(1);
        Ok(())
    }

    /// Write a recipe into the live dataset without recomputing its spectra — the
    /// paused edit path, where the display refreshes later on Apply.
    fn set_recipe_no_recompute(&mut self, dataset: usize, state: &DatasetProcessingState) {
        let Some(current) = self.doc.datasets.get_mut(dataset) else {
            return;
        };
        match (current, state) {
            (
                Dataset::Nmr(n),
                DatasetProcessingState::Nmr {
                    pipeline,
                    group_delay_correct,
                },
            ) => {
                n.pipeline = pipeline.clone();
                n.group_delay_correct = *group_delay_correct;
            }
            (Dataset::Nmr2D(n), DatasetProcessingState::Nmr2D { params, preset }) => {
                n.params = params.clone();
                n.preset = *preset;
            }
            _ => {}
        }
    }

    /// Commit a processing edit through the pause gate: recompute now when
    /// unpaused, or stash the recipe and defer the recompute to [`Self::apply_paused_processing`].
    pub fn commit_processing_edit(
        &mut self,
        dataset: usize,
        before: DatasetProcessingState,
        after: DatasetProcessingState,
    ) {
        let Some(dataset_id) = self.doc.datasets.get(dataset).map(Dataset::resource_id) else {
            return;
        };
        if self
            .session
            .ui
            .processing_session
            .as_ref()
            .is_some_and(|edit| edit.dataset == dataset_id)
            && !self.session.ui.proc_paused
        {
            if DatasetProcessingState::from_dataset(&self.doc.datasets[dataset]) != after {
                self.set_dataset_processing_state(dataset, &after);
            }
            return;
        }
        if self.session.ui.proc_paused {
            self.set_recipe_no_recompute(dataset, &after);
            if self.session.ui.proc_pending.is_none() {
                self.session.ui.proc_pending = Some((dataset_id, before));
            }
        } else {
            self.execute_action(Action::update_dataset_processing(dataset_id, before, after));
        }
    }

    pub fn has_pending_processing(&self) -> bool {
        self.session.ui.proc_pending.is_some()
    }

    /// Start a processing transaction whose live edits may come from multiple UI
    /// surfaces. Re-entering the same dataset preserves the original snapshot.
    pub fn begin_processing_session(&mut self, dataset: usize) {
        let Some(dataset_id) = self.doc.datasets.get(dataset).map(Dataset::resource_id) else {
            return;
        };
        if self
            .session
            .ui
            .processing_session
            .as_ref()
            .is_some_and(|edit| edit.dataset == dataset_id)
        {
            return;
        }
        self.finish_processing_session();
        if let Some(current) = self.doc.datasets.get(dataset) {
            self.session.ui.processing_session = Some(PendingProcessingEdit {
                dataset: dataset_id,
                before: DatasetProcessingState::from_dataset(current),
            });
        }
    }

    /// End the current multi-surface processing transaction, recording its
    /// already-applied final state without recomputing it.
    pub fn finish_processing_session(&mut self) {
        let Some(edit) = self.session.ui.processing_session.take() else {
            return;
        };
        let Some(dataset_index) = self.doc.dataset_index(edit.dataset) else {
            return;
        };
        let dataset = &self.doc.datasets[dataset_index];
        let after = DatasetProcessingState::from_dataset(dataset);
        let action = Action::update_dataset_processing(edit.dataset, edit.before, after);
        if let Err(error) = self.record_applied_processing_action(action) {
            self.session.status = error.to_string();
        }
    }

    pub fn apply_paused_processing(&mut self) {
        let Some((dataset, before)) = self.session.ui.proc_pending.take() else {
            return;
        };
        let Some(dataset_index) = self.doc.dataset_index(dataset) else {
            return;
        };
        let after = DatasetProcessingState::from_dataset(&self.doc.datasets[dataset_index]);
        // Restore the pre-edit recipe so the commit sees a real diff and picks
        // retransform vs rebuild correctly, then apply the accumulated recipe.
        self.set_recipe_no_recompute(dataset_index, &before);
        self.execute_action(Action::update_dataset_processing(dataset, before, after));
    }

    /// Abandon the staged recipe and restore the exact pre-edit state without
    /// creating history. Paused edits never recompute, so restoring the recipe is
    /// sufficient to restore the state before the edit run.
    pub fn discard_paused_processing(&mut self) {
        let Some((dataset, before)) = self.session.ui.proc_pending.take() else {
            return;
        };
        let Some(dataset) = self.doc.dataset_index(dataset) else {
            return;
        };
        self.set_recipe_no_recompute(dataset, &before);
        self.session.status = "Discarded pending processing changes.".to_owned();
    }

    /// Whether the Processing panel currently has a Phase step's editor expanded
    /// for the active dataset — the signal that the user is phasing and the canvas
    /// should show the pivot and take on-plot drags.
    pub fn phase_editor_open(&self) -> bool {
        self.phase_editor_dataset().is_some()
    }

    fn phase_editor_dataset(&self) -> Option<usize> {
        let (owner, id) = self.session.ui.proc_expanded_step?;
        let di = self.active_dataset()?;
        let dataset = self.doc.datasets.get(di)?;
        // The expanded row belongs to one dataset. Without this check a step
        // that merely shares its owner-local number would read as "the user is
        // phasing" the moment the active dataset changes.
        if dataset.resource_id() != owner {
            return None;
        }
        dataset
            .phase_axes()
            .iter()
            .any(|&axis| {
                dataset.axis_pipeline(axis).is_some_and(|pipe| {
                    pipe.steps.iter().any(|s| {
                        s.id == id && matches!(s.kind, plotx_processing::StepKind::Phase(_))
                    })
                })
            })
            .then_some(di)
    }

    /// Hold the canvas in on-plot phase mode exactly while a Phase step's editor is
    /// open, so expanding it shows the pivot and drags phase, and collapsing it
    /// hands the plot back to the Home (Select) tool. Edge-detected against `phase_edit_active`
    /// so a deliberate mid-phasing tool switch is respected rather than fought.
    pub fn sync_phase_interaction(&mut self) {
        let phase_dataset = self.phase_editor_dataset();
        let phasing = phase_dataset.is_some();
        let session_matches = phase_dataset.is_some_and(|dataset| {
            self.session
                .ui
                .processing_session
                .as_ref()
                .is_some_and(|edit| {
                    self.doc
                        .datasets
                        .get(dataset)
                        .is_some_and(|value| value.resource_id() == edit.dataset)
                })
        });
        if phasing == self.session.ui.phase_edit_active && (!phasing || session_matches) {
            return;
        }
        self.session.ui.phase_edit_active = phasing;
        if phasing {
            if let Some(dataset) = phase_dataset {
                self.begin_processing_session(dataset);
            }
            self.set_tool(crate::state::Tool::ManualPhase);
        } else {
            self.finish_processing_session();
            if self.session.tool == crate::state::Tool::ManualPhase {
                self.set_tool(crate::state::Tool::Select);
            }
        }
    }

    /// Switch the first enabled Phase step on `axis` to manual, seeding it from the
    /// phase the auto method currently yields so the display does not jump. Used by
    /// the on-plot phase grab and the panel's Manual/Auto switch.
    pub fn seed_manual_phase(&mut self, dataset: usize, axis: crate::state::PhaseAxis) {
        use plotx_processing::StepKind;
        let seed = self
            .doc
            .datasets
            .get(dataset)
            .and_then(|dataset| dataset.automatic_phase_params(axis));
        let Some(pipe) = self
            .doc
            .datasets
            .get_mut(dataset)
            .and_then(|d| d.axis_pipeline_mut(axis))
        else {
            return;
        };
        for step in pipe.steps.iter_mut().filter(|s| s.enabled) {
            if let StepKind::Phase(p) = &mut step.kind {
                if p.auto.is_some() {
                    if let Some((p0, p1, piv)) = seed {
                        p.phase0 = p0;
                        p.phase1 = p1;
                        p.pivot_frac = piv;
                    }
                    p.auto = None;
                }
                break;
            }
        }
    }
}
