use super::*;
use plotx_analysis::alignment::reference_peak;
use plotx_processing::align::apply_reference_shift;

#[derive(Clone, Copy, PartialEq)]
pub enum AlignTargetMode {
    ReferencePeak,
    Custom(f64),
}

#[derive(Clone)]
pub enum AlignOutcome {
    Peak { ppm: f64, shift: Option<f64> },
    Skip(String),
}

#[derive(Clone)]
pub struct AlignRow {
    pub dataset: usize,
    pub outcome: AlignOutcome,
}

#[derive(Clone)]
pub struct AlignPlan {
    pub reference: Option<usize>,
    pub target_ppm: Option<f64>,
    pub rows: Vec<AlignRow>,
}

impl AlignPlan {
    pub fn shift_count(&self) -> usize {
        self.rows
            .iter()
            .filter(|r| matches!(r.outcome, AlignOutcome::Peak { shift: Some(_), .. }))
            .count()
    }
}

impl PlotxApp {
    /// The datasets an alignment run considers: the Data-list multi-selection
    /// when it holds two or more, otherwise every dataset.
    pub fn align_scope(&self) -> Vec<usize> {
        let sel = &self.session.ui.data_selection;
        if sel.len() >= 2 {
            let mut scope = sel.to_vec();
            scope.sort_unstable();
            scope.dedup();
            scope
        } else {
            (0..self.doc.datasets.len()).collect()
        }
    }

    fn align_candidates(&self) -> Vec<usize> {
        self.align_scope()
            .into_iter()
            .filter(|&di| {
                self.doc
                    .datasets
                    .get(di)
                    .and_then(Dataset::as_nmr)
                    .is_some_and(|n| !n.spectrum.is_empty())
            })
            .collect()
    }

    pub fn can_align_spectra(&self) -> bool {
        self.align_candidates().len() >= 2
    }

    /// The spectrum the others align to: the selection lead when eligible,
    /// otherwise the first eligible dataset in scope.
    pub fn align_reference(&self) -> Option<usize> {
        let candidates = self.align_candidates();
        self.active_dataset()
            .filter(|di| candidates.contains(di))
            .or_else(|| candidates.first().copied())
    }

    pub fn plan_spectrum_alignment(&self, lo: f64, hi: f64, target: AlignTargetMode) -> AlignPlan {
        let reference = self.align_reference();
        let ref_nucleus = reference
            .and_then(|di| self.doc.datasets.get(di))
            .and_then(Dataset::as_nmr)
            .map(|n| n.spectrum.nucleus.trim().to_owned())
            .unwrap_or_default();

        let mut rows: Vec<AlignRow> = self
            .align_scope()
            .into_iter()
            .map(|di| {
                let outcome = match self.doc.datasets.get(di) {
                    Some(Dataset::Nmr(n)) if n.spectrum.is_empty() => {
                        AlignOutcome::Skip("Empty spectrum.".into())
                    }
                    Some(Dataset::Nmr(n)) if n.spectrum.nucleus.trim() != ref_nucleus => {
                        AlignOutcome::Skip(format!(
                            "Nucleus {} differs from {}.",
                            n.spectrum.nucleus, ref_nucleus
                        ))
                    }
                    Some(Dataset::Nmr(n)) => {
                        match reference_peak(&n.spectrum.ppm, &n.spectrum.real(), lo, hi) {
                            Some(ppm) => AlignOutcome::Peak { ppm, shift: None },
                            None => AlignOutcome::Skip("No significant peak in the window.".into()),
                        }
                    }
                    _ => AlignOutcome::Skip("Not a 1D spectrum.".into()),
                };
                AlignRow {
                    dataset: di,
                    outcome,
                }
            })
            .collect();

        let target_ppm = match target {
            AlignTargetMode::Custom(t) => Some(t),
            AlignTargetMode::ReferencePeak => rows
                .iter()
                .find(|r| Some(r.dataset) == reference)
                .and_then(|r| match r.outcome {
                    AlignOutcome::Peak { ppm, .. } => Some(ppm),
                    _ => None,
                }),
        };
        if let Some(t) = target_ppm {
            for row in &mut rows {
                if let AlignOutcome::Peak { ppm, shift } = &mut row.outcome {
                    *shift = Some(t - *ppm);
                }
            }
        }
        AlignPlan {
            reference,
            target_ppm,
            rows,
        }
    }

    /// Apply a plan as one undoable step: each aligned dataset's referencing
    /// step absorbs its shift, all wrapped in a single composite action.
    pub fn apply_spectrum_alignment(&mut self, plan: &AlignPlan) {
        let Some(target) = plan.target_ppm else {
            self.session.status = "Alignment needs a target position.".into();
            return;
        };
        if self.has_pending_processing() {
            self.session.status =
                "Resolve the paused processing edit in the panel before aligning.".into();
            return;
        }
        let mut actions = Vec::new();
        let mut aligned = 0usize;
        for row in &plan.rows {
            let AlignOutcome::Peak { ppm, .. } = row.outcome else {
                continue;
            };
            let Some(n) = self.doc.datasets.get(row.dataset).and_then(Dataset::as_nmr) else {
                continue;
            };
            aligned += 1;
            if (target - ppm).abs() < 1e-9 {
                continue;
            }
            let before = DatasetProcessingState::from_dataset(&self.doc.datasets[row.dataset]);
            let mut pipeline = n.pipeline.clone();
            let group_delay_correct = n.group_delay_correct;
            // `pipeline` is a copy of a live recipe, so an appended referencing
            // step needs a real identity from this dataset's allocator rather
            // than template-local numbering. Reserving it unconditionally is
            // safe: the allocator is a monotone high-water mark and gaps are
            // expected (it deliberately never rolls back on undo).
            let Some(dataset) = self
                .doc
                .datasets
                .get_mut(row.dataset)
                .and_then(Dataset::as_nmr_mut)
            else {
                continue;
            };
            let step_id = dataset.allocate_step_id();
            let dataset_id = dataset.resource_id;
            apply_reference_shift(&mut pipeline, ppm, target, step_id);
            let after = DatasetProcessingState::Nmr {
                pipeline,
                group_delay_correct,
            };
            actions.push(Action::update_dataset_processing(dataset_id, before, after));
        }
        if aligned == 0 {
            self.session.status = "No spectrum had a usable peak in the window.".into();
            return;
        }
        let skipped = plan.rows.len() - aligned;
        if !actions.is_empty() {
            self.execute_action(Action::Composite(actions));
        }
        self.session.status =
            format!("Aligned {aligned} spectra to {target:.3} ppm; skipped {skipped}.");
    }
}
