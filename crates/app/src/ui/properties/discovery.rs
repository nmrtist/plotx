//! The remaining discovery channels of §8.5, all derived from one registration.
//!
//! Search (channel 1) indexes properties individually. The Ribbon (2) and the
//! context menu (4) address *groups*: the Ribbon is an entry map, not a second
//! control surface, so a group gets one button that jumps to the section where
//! its members already live. The canvas gesture (3) addresses the one property
//! that declared itself steppable.
//!
//! None of these channels carries editing logic. Navigation opens the home the
//! presentation already names, and the gesture calls
//! [`PlotxApp::plan_property_step`], the same planner, validation and typed
//! action the panel control uses. There is exactly one source of state for a
//! property and exactly one path that writes it; what varies is only where the
//! user reaches for it.

use super::{PRESENTATIONS, PropertyGroup, PropertyPresentation};
use egui::Ui;
use plotx_core::automation::{ResourceRef, TargetRef};
use plotx_core::properties::{
    ComponentKind, PropertyId, PropertyStep, ScopeKind, Tier, definition, object,
};
use plotx_core::state::{ObjectId, PlotxApp};

/// The declared groups, in Ribbon order.
pub(crate) fn groups() -> &'static [PropertyGroup] {
    super::GROUPS
}

pub(crate) fn group(section: &str) -> Option<&'static PropertyGroup> {
    groups().iter().find(|group| group.section == section)
}

/// Every presentation whose home is this section. Membership is read off the
/// home route rather than listed again on the group, so registering a property
/// with an existing home is all it takes to appear in that group's Ribbon
/// button and context-menu entry.
pub(crate) fn members_of<'a>(
    section: &str,
    presentations: &'a [PropertyPresentation],
) -> Vec<&'a PropertyPresentation> {
    presentations
        .iter()
        .filter(|entry| entry.home_route.section == section)
        .collect()
}

/// Where a jump to a group lands: its first Essential member, or its first
/// member when the group is entirely advanced. Landing on the row the user is
/// most likely to have come for is the difference between navigation and
/// merely opening a panel.
pub(crate) fn entry_property(
    section: &str,
    presentations: &[PropertyPresentation],
) -> Option<PropertyId> {
    let members = members_of(section, presentations);
    members
        .iter()
        .find(|entry| entry.tier() == Some(Tier::Essential))
        .or_else(|| members.first())
        .map(|entry| entry.id)
}

/// The property the canvas `+` / `-` gesture drives, if any.
///
/// The opt-in lives on the presentation entry, so it is part of the property's
/// single registration rather than a second table a new property would have to
/// be added to. The gesture is not a channel every property can carry — a
/// colour has no direction — which is exactly why the entry declares it.
pub(crate) fn steppable_in(
    presentations: &[PropertyPresentation],
) -> Option<&PropertyPresentation> {
    presentations.iter().find(|entry| entry.canvas_step)
}

/// What to say when the only plots a setting applies to are locked. It names
/// the state and the way out, per the crate's hide-vs-disable rule.
pub(crate) const LOCKED_REASON: &str =
    "Unlock this plot to change its settings; it can still be read while locked.";

/// The plot objects the discovery channels *refer* to: the current selection, or
/// the page's active plot when nothing is selected.
///
/// Every channel shares this, so the Ribbon button, the context menu, the
/// gesture and the on-canvas readout can never disagree about what they refer
/// to. A lock is deliberately not applied here. Locking a plot means "do not
/// change this", not "stop telling me about this": the corner readout, the
/// palette's applicability answer and the context menu all describe a plot
/// rather than edit it, and filtering here made them vanish with no explanation
/// on a plot that was plainly selected.
pub(crate) fn selection_objects(app: &PlotxApp) -> Vec<ObjectId> {
    let Some(canvas) = app
        .session
        .active_canvas
        .and_then(|index| app.doc.canvases.get(index))
    else {
        return Vec::new();
    };
    let selected: Vec<_> = app
        .session
        .ui
        .selection
        .objects()
        .iter()
        .copied()
        .filter(|&id| {
            canvas
                .object(id)
                .is_some_and(|object| object.plot().is_some())
        })
        .collect();
    if !selected.is_empty() {
        return selected;
    }
    canvas.active_plot_object_id().into_iter().collect()
}

/// The subset of [`selection_objects`] a write may land on. This is the one
/// place the lock is applied, so no editing path can forget it and no reading
/// path can accidentally inherit it.
pub(crate) fn editable_objects(app: &PlotxApp) -> Vec<ObjectId> {
    let Some(canvas) = app
        .session
        .active_canvas
        .and_then(|index| app.doc.canvases.get(index))
    else {
        return Vec::new();
    };
    selection_objects(app)
        .into_iter()
        .filter(|&id| canvas.object(id).is_some_and(|object| !object.locked))
        .collect()
}

/// Every series target of [`selection_objects`], in binding order.
pub(crate) fn selection_targets(app: &PlotxApp) -> Vec<TargetRef> {
    targets_of(app, selection_objects(app))
}

/// Every series target a write may land on.
pub(crate) fn editable_targets(app: &PlotxApp) -> Vec<TargetRef> {
    targets_of(app, editable_objects(app))
}

/// The subset of [`targets_for_property`] a write may land on. Only object-owned
/// properties can be locked out; a document or dataset setting has no plot to
/// lock, so it passes through unchanged.
pub(crate) fn editable_targets_for_property(
    app: &PlotxApp,
    property: PropertyId,
) -> Vec<TargetRef> {
    match definition(property) {
        Some(definition)
            if definition.scope_kind == ScopeKind::Object
                && definition.applicability.component == ComponentKind::None
                && property != object::LOCKED =>
        {
            selected_object_targets(app, true)
        }
        Some(definition) if definition.applicability.component == ComponentKind::Series => {
            editable_targets(app)
        }
        _ => targets_for_property(app, property),
    }
}

fn selected_object_targets(app: &PlotxApp, unlocked_only: bool) -> Vec<TargetRef> {
    let Some(canvas) = app.session.active_canvas else {
        return Vec::new();
    };
    app.session
        .ui
        .selection
        .objects()
        .iter()
        .copied()
        .filter(|&object| {
            app.doc.canvases[canvas]
                .object(object)
                .is_some_and(|candidate| !unlocked_only || !candidate.locked)
        })
        .filter_map(|object| app.object_target(canvas, object))
        .collect()
}

fn targets_of(app: &PlotxApp, objects: Vec<ObjectId>) -> Vec<TargetRef> {
    let Some(canvas) = app.session.active_canvas else {
        return Vec::new();
    };
    objects
        .into_iter()
        .flat_map(|object| app.series_targets(canvas, object))
        .collect()
}

/// Derive the targets for one property from its catalog shape. This is target
/// discovery only: providers still decide how a target maps to typed storage.
/// Keeping this here lets document, object and processing-step presentations
/// share the same search and Ribbon applicability checks without inventing a
/// separate registry for each scope.
pub(crate) fn targets_for_property(app: &PlotxApp, property: PropertyId) -> Vec<TargetRef> {
    let Some(definition) = definition(property) else {
        return Vec::new();
    };
    match definition.applicability.component {
        ComponentKind::None if definition.scope_kind == ScopeKind::App => {
            vec![app.app_target()]
        }
        ComponentKind::None if definition.scope_kind == ScopeKind::Document => {
            vec![app.document_target()]
        }
        ComponentKind::None if definition.scope_kind == ScopeKind::Canvas => app
            .session
            .active_canvas
            .and_then(|index| app.doc.canvases.get(index))
            .map(|canvas| vec![app.canvas_target(canvas.resource_id)])
            .unwrap_or_default(),
        ComponentKind::None if definition.scope_kind == ScopeKind::Object => {
            selected_object_targets(app, false)
        }
        ComponentKind::None => Vec::new(),
        ComponentKind::Series => selection_targets(app),
        ComponentKind::ProcessingStep => {
            let Some(dataset) = app
                .active_dataset()
                .and_then(|index| app.doc.datasets.get(index))
            else {
                return Vec::new();
            };
            let resource = ResourceRef::from(dataset.resource_id());
            app.resource_property_targets(&resource, definition)
        }
    }
}

/// Whether any member of a group currently applies to the selection. This is
/// the group's own applicability, derived from its members' definitions —
/// capability and encoding gates included — not a second rule written here.
pub(crate) fn group_applies(app: &PlotxApp, section: &str) -> bool {
    members_of(section, PRESENTATIONS).into_iter().any(|entry| {
        let targets = targets_for_property(app, entry.id);
        !app.resolve_property_set(entry.id, &targets)
            .applicable_targets
            .is_empty()
    })
}

/// The property and targets the `+` / `-` gesture would act on right now.
pub(crate) fn step_target(app: &PlotxApp) -> Option<(PropertyId, Vec<TargetRef>)> {
    let property = steppable_in(PRESENTATIONS)?.id;
    let targets = selection_targets(app);
    let applicable = app
        .resolve_property_set(property, &targets)
        .applicable_targets;
    (!applicable.is_empty()).then_some((property, targets))
}

/// Channel 3: take one step on the selection, through the planner.
///
/// The gesture computes nothing itself. It names a property and a direction;
/// the catalog decides what a step is, validates it and compiles it into the
/// same atomic action the panel produces, so the two entry points cannot drift.
pub(crate) fn step_selection(app: &mut PlotxApp, step: PropertyStep) {
    let Some((property, _)) = step_target(app) else {
        return;
    };
    // The readout above the gesture describes every selected plot; the gesture
    // itself may only move the ones that are not locked, and says so when that
    // leaves it nothing to move.
    let targets = editable_targets_for_property(app, property);
    if app
        .resolve_property_set(property, &targets)
        .applicable_targets
        .is_empty()
    {
        app.session.status = LOCKED_REASON.to_owned();
        return;
    }
    match app.plan_property_step(property, &targets, step) {
        Ok(commit) => {
            let skipped = commit.skipped.len();
            let applied = app.commit_property(commit);
            let label = super::presentation(property)
                .map(|entry| entry.localized_label.get())
                .unwrap_or("setting");
            app.session.status = match skipped {
                0 => format!("Stepped {label} on {applied} series."),
                skipped => format!("Stepped {label} on {applied} series; skipped {skipped}."),
            };
        }
        // A step that lands outside the property's own range is refused by the
        // same validation a typed value meets, and the reason reaches the user
        // rather than the gesture silently doing nothing.
        Err(error) => app.session.status = format!("Could not step this setting: {error}"),
    }
}

/// Channel 4: the context-menu entries for whatever the selection draws.
///
/// Navigation only — every entry reveals a group at its canonical home.
pub(crate) fn context_menu(app: &mut PlotxApp, ui: &mut Ui) {
    let now = ui.input(|input| input.time);
    let reachable: Vec<(&'static str, PropertyId)> = groups()
        .iter()
        .filter(|group| group_applies(app, group.section))
        .filter_map(|group| {
            entry_property(group.section, PRESENTATIONS)
                .map(|property| (group.label.get(), property))
        })
        .collect();
    if reachable.is_empty() {
        return;
    }
    ui.separator();
    for (label, property) in reachable {
        if ui.button(format!("{label} settings…")).clicked() {
            super::super::command_palette::reveal_property(app, property, now);
            ui.close();
        }
    }
}

#[cfg(test)]
#[path = "discovery_tests.rs"]
mod tests;
