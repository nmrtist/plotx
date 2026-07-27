//! The typed working copies a property provider selects.
//!
//! Providers decide which typed storage they need; the service merely executes
//! the completed action. That keeps a document property, a binding property,
//! and a future app preference from being dispatched by `ScopeKind` in the
//! planner.

use crate::actions::{Action, DatasetProcessingState, PageSizeState};
use crate::layout::PageLayout;
use crate::settings::Settings;
use crate::state::{
    AxisOverrides, CanvasId, DataBinding, DatasetId, ObjectId, PanelLabelStyle, PlotxApp,
};
use plotx_figure::FigureTypography;

#[path = "transaction_object.rs"]
mod object;
use object::{ObjectPlans, ObjectTargetSnapshot};

/// Persistence boundaries a provider can select.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum StorageClass {
    Document,
    AppPreferences,
}

impl StorageClass {
    const fn label(self) -> &'static str {
        match self {
            Self::Document => "document",
            Self::AppPreferences => "app preferences",
        }
    }
}

/// Working copies of all typed stores a catalog edit touches.
#[derive(Default)]
pub(crate) struct PropertyTransaction {
    bindings: BindingPlan,
    canvases: Vec<CanvasPlan>,
    axis_overrides: Vec<AxisOverridesPlan>,
    objects: ObjectPlans,
    typography: Option<(FigureTypography, FigureTypography)>,
    processing: Vec<(DatasetId, DatasetProcessingState, DatasetProcessingState)>,
    settings: Option<(Settings, Settings)>,
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
    Canvas {
        id: CanvasId,
        before: CanvasPropertyState,
    },
    AxisOverrides {
        canvas: usize,
        object: ObjectId,
        before: AxisOverrides,
    },
    Object(ObjectTargetSnapshot),
    Processing {
        dataset: DatasetId,
        before: DatasetProcessingState,
    },
    Settings {
        before: Settings,
        newly_selected: bool,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct CanvasPropertyState {
    pub(crate) layout: PageLayout,
    pub(crate) page_size: PageSizeState,
    pub(crate) auto_height: bool,
    pub(crate) caption: (String, bool),
    pub(crate) panel_label_style: PanelLabelStyle,
}

impl CanvasPropertyState {
    fn of(canvas: &crate::state::CanvasDocument) -> Self {
        Self {
            layout: canvas.layout,
            page_size: PageSizeState::of(canvas),
            auto_height: canvas.auto_height,
            caption: (canvas.caption.clone(), canvas.caption_visible),
            panel_label_style: canvas.panel_label_style,
        }
    }
}

struct CanvasPlan {
    id: CanvasId,
    before: CanvasPropertyState,
    after: CanvasPropertyState,
}

struct AxisOverridesPlan {
    canvas: usize,
    object: ObjectId,
    before: AxisOverrides,
    after: AxisOverrides,
}

#[derive(Clone)]
pub(crate) enum CanvasDirectEdit {
    ShowGrid { canvas: CanvasId, show: bool },
    AutoHeight { canvas: CanvasId, enabled: bool },
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
            TargetSnapshot::Canvas { id, before } => self
                .canvases
                .iter()
                .find(|plan| plan.id == *id)
                .is_some_and(|plan| plan.after != *before),
            TargetSnapshot::AxisOverrides {
                canvas,
                object,
                before,
            } => self
                .axis_overrides
                .iter()
                .find(|plan| plan.canvas == *canvas && plan.object == *object)
                .is_some_and(|plan| plan.after != *before),
            TargetSnapshot::Object(snapshot) => self.objects.target_changed(snapshot),
            TargetSnapshot::Processing { dataset, before } => self
                .processing
                .iter()
                .find(|(candidate, _, _)| candidate == dataset)
                .is_some_and(|(_, _, after)| after != before),
            TargetSnapshot::Settings { before, .. } => self
                .settings
                .as_ref()
                .is_some_and(|(_, after)| after != before),
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
                TargetSnapshot::Canvas { id, before } => {
                    if let Some(plan) = self.canvases.iter_mut().find(|plan| plan.id == *id) {
                        plan.after = before.clone();
                    }
                }
                TargetSnapshot::AxisOverrides {
                    canvas,
                    object,
                    before,
                } => {
                    if let Some(plan) = self
                        .axis_overrides
                        .iter_mut()
                        .find(|plan| plan.canvas == *canvas && plan.object == *object)
                    {
                        plan.after = before.clone();
                    }
                }
                TargetSnapshot::Object(snapshot) => self.objects.rollback(snapshot),
                TargetSnapshot::Processing { dataset, before } => {
                    if let Some((_, _, after)) = self
                        .processing
                        .iter_mut()
                        .find(|(candidate, _, _)| candidate == dataset)
                    {
                        *after = before.clone();
                    }
                }
                TargetSnapshot::Settings {
                    before,
                    newly_selected,
                } => {
                    if *newly_selected {
                        self.settings = None;
                    } else if let Some((_, after)) = self.settings.as_mut() {
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

    /// Select one canvas by stable identity. Its collection position is used
    /// only for this lookup and never leaves the function.
    pub(crate) fn canvas(
        &mut self,
        app: &PlotxApp,
        id: CanvasId,
    ) -> Result<&mut CanvasPropertyState, super::PropertyError> {
        let plan_index = if let Some(index) = self.canvases.iter().position(|plan| plan.id == id) {
            index
        } else {
            let index = app
                .doc
                .canvas_index(id)
                .ok_or_else(|| super::PropertyError::UnknownTarget(id.to_string()))?;
            let state = CanvasPropertyState::of(&app.doc.canvases[index]);
            self.canvases.push(CanvasPlan {
                id,
                before: state.clone(),
                after: state,
            });
            self.canvases.len() - 1
        };
        if !self.target_before.iter().any(
            |snapshot| matches!(snapshot, TargetSnapshot::Canvas { id: candidate, .. } if *candidate == id),
        ) {
            self.target_before.push(TargetSnapshot::Canvas {
                id,
                before: self.canvases[plan_index].after.clone(),
            });
        }
        Ok(&mut self.canvases[plan_index].after)
    }

    /// Stage the spacing mode through the same typed action constructor used
    /// by `PlotxApp::set_spacing_mode`.
    pub(crate) fn set_canvas_spacing_mode(
        &mut self,
        app: &PlotxApp,
        id: CanvasId,
        mode: crate::layout::SpacingMode,
    ) -> Result<(), super::PropertyError> {
        self.canvas(app, id)?.layout.spacing_mode = mode;
        Ok(())
    }

    /// Stage grid visibility for the direct, non-undoable
    /// `PlotxApp::set_show_grid` commit path.
    pub(crate) fn set_canvas_show_grid(
        &mut self,
        app: &PlotxApp,
        id: CanvasId,
        show: bool,
    ) -> Result<(), super::PropertyError> {
        self.canvas(app, id)?.layout.show_grid = show;
        Ok(())
    }

    /// Select one plot's complete axis override value. The existing
    /// `SetAxisOverrides` action is the undo and persistence boundary.
    pub(crate) fn axis_overrides(
        &mut self,
        app: &PlotxApp,
        canvas: usize,
        object: ObjectId,
    ) -> Result<&mut AxisOverrides, super::PropertyError> {
        let plan_index = if let Some(index) = self
            .axis_overrides
            .iter()
            .position(|plan| plan.canvas == canvas && plan.object == object)
        {
            index
        } else {
            let current = app
                .doc
                .canvases
                .get(canvas)
                .and_then(|canvas| canvas.object(object))
                .and_then(|object| object.plot())
                .map(|plot| plot.axis_overrides.clone())
                .ok_or_else(|| super::PropertyError::UnknownTarget(object.to_string()))?;
            self.axis_overrides.push(AxisOverridesPlan {
                canvas,
                object,
                before: current.clone(),
                after: current,
            });
            self.axis_overrides.len() - 1
        };
        if !self.target_before.iter().any(|snapshot| {
            matches!(
                snapshot,
                TargetSnapshot::AxisOverrides {
                    canvas: candidate_canvas,
                    object: candidate_object,
                    ..
                } if *candidate_canvas == canvas && *candidate_object == object
            )
        }) {
            self.target_before.push(TargetSnapshot::AxisOverrides {
                canvas,
                object,
                before: self.axis_overrides[plan_index].after.clone(),
            });
        }
        Ok(&mut self.axis_overrides[plan_index].after)
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

    /// Select the live application preferences for mutation. The provider
    /// chooses this storage exactly as another provider chooses typography or
    /// processing state; the planner does not infer it from scope.
    pub(crate) fn app_preferences(&mut self, app: &PlotxApp) -> &mut Settings {
        let newly_selected = self.settings.is_none();
        let settings = &mut self
            .settings
            .get_or_insert_with(|| (app.settings.clone(), app.settings.clone()))
            .1;
        if !self
            .target_before
            .iter()
            .any(|snapshot| matches!(snapshot, TargetSnapshot::Settings { .. }))
        {
            self.target_before.push(TargetSnapshot::Settings {
                before: settings.clone(),
                newly_selected,
            });
        }
        settings
    }

    /// The storage boundaries selected by providers in this transaction.
    pub(crate) fn storage_classes(&self) -> Vec<StorageClass> {
        let mut classes = Vec::with_capacity(2);
        if !self.bindings.entries.is_empty()
            || !self.canvases.is_empty()
            || !self.axis_overrides.is_empty()
            || !self.objects.is_empty()
            || self.typography.is_some()
            || !self.processing.is_empty()
        {
            classes.push(StorageClass::Document);
        }
        if self.settings.is_some() {
            classes.push(StorageClass::AppPreferences);
        }
        classes
    }

    /// Refuse a cross-storage request before either half becomes executable.
    pub(crate) fn ensure_single_storage(&self) -> Result<(), super::PropertyError> {
        let classes = self.storage_classes();
        if classes.len() <= 1 {
            return Ok(());
        }
        Err(super::PropertyError::MixedStorage {
            storages: classes
                .iter()
                .map(|class| class.label())
                .collect::<Vec<_>>()
                .join(" and "),
        })
    }

    pub(crate) fn into_commit(
        self,
        app: &PlotxApp,
        applied: Vec<super::PropertyAddress>,
        skipped: Vec<super::PropertySkip>,
    ) -> super::PropertyCommit {
        let mut actions = self.bindings.into_actions();
        actions.extend(
            self.axis_overrides
                .into_iter()
                .filter(|plan| plan.before != plan.after)
                .map(|plan| {
                    Action::set_axis_overrides(plan.canvas, plan.object, plan.before, plan.after)
                }),
        );
        actions.extend(self.objects.into_actions());
        let mut canvas_direct = Vec::new();
        for plan in self.canvases {
            let Some(canvas) = app.doc.canvas_index(plan.id) else {
                continue;
            };
            let mut layout_after = plan.after.layout;
            layout_after.show_grid = plan.before.layout.show_grid;
            if plan.before.layout != layout_after {
                let mut without_spacing = layout_after;
                without_spacing.spacing_mode = plan.before.layout.spacing_mode;
                if without_spacing == plan.before.layout {
                    actions.push(Action::set_spacing_mode(
                        canvas,
                        plan.before.layout,
                        layout_after.spacing_mode,
                    ));
                } else {
                    actions.push(Action::set_page_layout(
                        canvas,
                        plan.before.layout,
                        layout_after,
                    ));
                }
            }
            if plan.before.page_size != plan.after.page_size {
                actions.push(Action::set_canvas_size(
                    canvas,
                    plan.before.page_size,
                    plan.after.page_size,
                ));
            }
            if plan.before.caption != plan.after.caption {
                actions.push(Action::set_canvas_caption(
                    canvas,
                    plan.before.caption,
                    plan.after.caption,
                ));
            }
            if plan.before.panel_label_style != plan.after.panel_label_style {
                actions.push(Action::SetPanelLabelStyle {
                    canvas,
                    before: plan.before.panel_label_style,
                    after: plan.after.panel_label_style,
                });
            }
            if plan.before.layout.show_grid != plan.after.layout.show_grid {
                canvas_direct.push(CanvasDirectEdit::ShowGrid {
                    canvas: plan.id,
                    show: plan.after.layout.show_grid,
                });
            }
            if plan.before.auto_height != plan.after.auto_height {
                canvas_direct.push(CanvasDirectEdit::AutoHeight {
                    canvas: plan.id,
                    enabled: plan.after.auto_height,
                });
            }
        }
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
        let document_action = (!actions.is_empty()).then_some(Action::Composite(actions));
        let app_preferences = self
            .settings
            .and_then(|(before, after)| (before != after).then_some(after));
        super::PropertyCommit {
            document_action,
            canvas_direct,
            app_preferences,
            applied,
            skipped,
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mixed_storage_selection_is_refused_before_commit_creation() {
        let app = PlotxApp::new_with_settings(crate::settings::Settings::default());
        let mut transaction = PropertyTransaction::default();
        transaction.begin_target();
        transaction.figure_typography(&app);
        transaction.app_preferences(&app);

        assert_eq!(
            transaction.storage_classes(),
            [StorageClass::Document, StorageClass::AppPreferences]
        );
        let error = transaction
            .ensure_single_storage()
            .expect_err("a commit cannot span two persistence boundaries");
        let message = error.to_string();
        assert!(message.contains("document"), "{message}");
        assert!(message.contains("app preferences"), "{message}");
    }

    #[test]
    fn rolled_back_settings_target_leaves_no_storage_selected() {
        let app = PlotxApp::new_with_settings(crate::settings::Settings::default());
        let mut transaction = PropertyTransaction::default();
        transaction.begin_target();
        transaction.app_preferences(&app).export.dpi = 450;
        assert!(transaction.target_changed());

        transaction.rollback_target();

        assert!(!transaction.target_changed());
        assert!(transaction.storage_classes().is_empty());
    }
}
