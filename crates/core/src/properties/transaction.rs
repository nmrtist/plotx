//! The writable document snapshots a property provider selects.
//!
//! Providers decide which typed storage they need; the service merely executes
//! the completed action. That keeps a document property, a binding property,
//! and a future app preference from being dispatched by `ScopeKind` in the
//! planner.

use crate::actions::{Action, DatasetProcessingState};
use crate::state::{DataBinding, DatasetId, ObjectId, PlotxApp};
use plotx_figure::FigureTypography;

/// Working copies of all typed stores a catalog edit touches.
#[derive(Default)]
pub(crate) struct PropertyTransaction {
    bindings: BindingPlan,
    typography: Option<(FigureTypography, FigureTypography)>,
    processing: Vec<(DatasetId, DatasetProcessingState, DatasetProcessingState)>,
    /// The working-copy state a single provider edit started from. It is
    /// deliberately per target, rather than one transaction-wide dirty bit:
    /// two series can share a binding, and a later no-op edit must not inherit
    /// the first series' change as a false success.
    target_before: Vec<TargetSnapshot>,
}

enum TargetSnapshot {
    Binding {
        canvas: usize,
        object: ObjectId,
        before: DataBinding,
    },
    Typography(FigureTypography),
    Processing {
        dataset: DatasetId,
        before: DatasetProcessingState,
    },
}

impl PropertyTransaction {
    /// Start measuring one provider operation. The service calls this around
    /// every target so it can report a same-value write instead of claiming an
    /// empty action was applied.
    pub(crate) fn begin_target(&mut self) {
        self.target_before.clear();
    }

    /// Whether the current provider operation changed one of the typed
    /// working copies it selected.
    pub(crate) fn target_changed(&self) -> bool {
        self.target_before.iter().any(|snapshot| match snapshot {
            TargetSnapshot::Binding {
                canvas,
                object,
                before,
            } => self
                .bindings
                .entries
                .iter()
                .find(|entry| entry.0 == *canvas && entry.1 == *object)
                .is_some_and(|entry| entry.3 != *before),
            TargetSnapshot::Typography(before) => {
                self.typography.is_some_and(|(_, after)| after != *before)
            }
            TargetSnapshot::Processing { dataset, before } => self
                .processing
                .iter()
                .find(|(candidate, _, _)| candidate == dataset)
                .is_some_and(|(_, _, after)| after != before),
        })
    }

    /// A provider may discover a target is inapplicable after selecting a
    /// working copy. Restore that operation's local snapshot before the
    /// service records its skip, so a failed target cannot leak a mutation
    /// into another compatible target's atomic commit.
    pub(crate) fn rollback_target(&mut self) {
        for snapshot in &self.target_before {
            match snapshot {
                TargetSnapshot::Binding {
                    canvas,
                    object,
                    before,
                } => {
                    if let Some(entry) = self
                        .bindings
                        .entries
                        .iter_mut()
                        .find(|entry| entry.0 == *canvas && entry.1 == *object)
                    {
                        entry.3 = before.clone();
                    }
                }
                TargetSnapshot::Typography(before) => {
                    if let Some((_, after)) = self.typography.as_mut() {
                        *after = *before;
                    }
                }
                TargetSnapshot::Processing { dataset, before } => {
                    if let Some((_, _, after)) = self
                        .processing
                        .iter_mut()
                        .find(|(candidate, _, _)| candidate == dataset)
                    {
                        *after = before.clone();
                    }
                }
            }
        }
    }

    /// Select the binding of one plot object for mutation. Repeated edits to
    /// series on the same object share its one before/after snapshot.
    pub(crate) fn data_binding(
        &mut self,
        app: &PlotxApp,
        canvas: usize,
        object: ObjectId,
    ) -> Result<&mut DataBinding, super::PropertyError> {
        let binding = self.bindings.entry(app, canvas, object)?;
        if !self.target_before.iter().any(|snapshot| {
            matches!(snapshot, TargetSnapshot::Binding { canvas: candidate_canvas, object: candidate_object, .. } if *candidate_canvas == canvas && *candidate_object == object)
        }) {
            self.target_before.push(TargetSnapshot::Binding {
                canvas,
                object,
                before: binding.clone(),
            });
        }
        Ok(binding)
    }

    /// Select the document's figure typography for mutation. The action stores
    /// the complete typed value because that is the existing undo boundary.
    pub(crate) fn figure_typography(&mut self, app: &PlotxApp) -> &mut FigureTypography {
        let typography = &mut self
            .typography
            .get_or_insert_with(|| {
                let value = app.doc.style_library.figure_typography;
                (value, value)
            })
            .1;
        if !self
            .target_before
            .iter()
            .any(|snapshot| matches!(snapshot, TargetSnapshot::Typography(_)))
        {
            self.target_before
                .push(TargetSnapshot::Typography(*typography));
        }
        typography
    }

    /// Select one dataset's existing processing snapshot for mutation. The
    /// provider chooses this store; the service never switches on scope to do
    /// so. Multiple component edits to one dataset still become one action.
    pub(crate) fn processing_state(
        &mut self,
        app: &PlotxApp,
        dataset: DatasetId,
    ) -> Result<&mut DatasetProcessingState, super::PropertyError> {
        let index = if let Some(index) = self
            .processing
            .iter()
            .position(|(candidate, _, _)| *candidate == dataset)
        {
            index
        } else {
            let current = app
                .doc
                .dataset_by_id(dataset)
                .ok_or_else(|| super::PropertyError::UnknownTarget(dataset.to_string()))?;
            let state = DatasetProcessingState::from_dataset(current);
            self.processing.push((dataset, state.clone(), state));
            self.processing.len() - 1
        };
        if !self.target_before.iter().any(|snapshot| {
            matches!(snapshot, TargetSnapshot::Processing { dataset: candidate, .. } if *candidate == dataset)
        }) {
            self.target_before.push(TargetSnapshot::Processing {
                dataset,
                before: self.processing[index].2.clone(),
            });
        }
        Ok(&mut self.processing[index].2)
    }

    pub(crate) fn into_action(self) -> Action {
        let mut actions = self.bindings.into_actions();
        if let Some((before, after)) = self.typography
            && before != after
        {
            actions.push(Action::set_figure_typography(before, after));
        }
        actions.extend(
            self.processing
                .into_iter()
                .filter(|(_, before, after)| before != after)
                .map(|(dataset, before, after)| {
                    Action::update_dataset_processing(dataset, before, after)
                }),
        );
        Action::Composite(actions)
    }
}

#[derive(Default)]
struct BindingPlan {
    entries: Vec<(usize, ObjectId, DataBinding, DataBinding)>,
}

impl BindingPlan {
    fn entry(
        &mut self,
        app: &PlotxApp,
        canvas: usize,
        object: ObjectId,
    ) -> Result<&mut DataBinding, super::PropertyError> {
        if let Some(index) = self
            .entries
            .iter()
            .position(|entry| entry.0 == canvas && entry.1 == object)
        {
            return Ok(&mut self.entries[index].3);
        }
        let binding = app
            .doc
            .canvases
            .get(canvas)
            .and_then(|canvas| canvas.object(object))
            .and_then(|object| object.plot())
            .map(|plot| plot.binding.clone())
            .ok_or_else(|| super::PropertyError::UnknownTarget(object.to_string()))?;
        self.entries
            .push((canvas, object, binding.clone(), binding));
        let index = self.entries.len() - 1;
        Ok(&mut self.entries[index].3)
    }

    fn into_actions(self) -> Vec<Action> {
        self.entries
            .into_iter()
            .filter(|(_, _, before, after)| before != after)
            .map(|(canvas, object, before, after)| {
                Action::set_data_binding(canvas, object, before, after)
            })
            .collect()
    }
}
