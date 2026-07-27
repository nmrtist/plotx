//! The Object inspector: geometry + per-kind style editing for the current
//! page-space selection, at the top of the Secondary Side Bar.

mod axes;
mod chart_gallery;
mod data;
mod edits;
mod geometry;

use axes::{axes_section, commit_if_target_changed};
use chart_gallery::chart_gallery;
use data::data_section;
use edits::{flush_inspector_edit, format_once_section, kind_targets, selection_label};
use egui::{DragValue, Ui};
use egui_phosphor::regular as icon;
use geometry::geometry_section;
use plotx_core::actions::{Action, PendingInspectorEdit};
use plotx_core::state::{
    CanvasObject, DataBinding, Dataset, MM_TO_PT, OVERLAY_PALETTE, ObjectFrame, ObjectId, PlotxApp,
    SeriesBinding, StackSpec,
};
use plotx_figure::Color;

pub(crate) fn render(app: &mut PlotxApp, ui: &mut Ui) {
    let ids: Vec<ObjectId> = app.session.ui.selection.objects().to_vec();
    let axis_target = app.session.active_canvas.and_then(|ci| {
        (ids.len() == 1
            && app
                .doc
                .canvases
                .get(ci)?
                .object(ids[0])
                .is_some_and(|object| object.plot().is_some()))
        .then(|| (ci, ids[0]))
    });
    commit_if_target_changed(app, axis_target);
    let Some(ci) = app.session.active_canvas else {
        crate::ui::properties::panel::typography_section(app, ui);
        return;
    };
    if ci >= app.doc.canvases.len() {
        crate::ui::properties::panel::typography_section(app, ui);
        return;
    }
    if ids.is_empty() {
        property_sections(app, ci, true, ui);
        return;
    }

    ui.add_space(4.0);
    ui.horizontal(|ui| {
        ui.strong("Object");
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.weak(selection_label(app, ci, &ids));
        });
    });
    ui.add_space(4.0);

    geometry_section(app, ci, &ids, ui);
    let property_objects: Vec<_> = ids
        .iter()
        .copied()
        .filter(|&id| {
            app.doc.canvases[ci]
                .object(id)
                .is_some_and(|object| !object.locked)
        })
        .collect();
    crate::ui::properties::panel::axis_section(app, ci, &property_objects, ui);

    let mut axes_focused = false;
    if ids.len() == 1
        && app.doc.canvases[ci]
            .object(ids[0])
            .map(|o| o.plot().is_some())
            .unwrap_or(false)
    {
        ui.separator();
        axes_focused = axes_section(app, ci, ids[0], ui);
        crate::ui::properties::panel::panel_section(app, ci, &property_objects, ui);
        data_section(app, ci, ids[0], ui);
    }
    property_sections(app, ci, false, ui);

    let text_ids = kind_targets(app, ci, &ids, |o| o.text().is_some());
    let shape_ids = kind_targets(app, ci, &ids, |o| o.shape().is_some());

    if !text_ids.is_empty() {
        crate::ui::properties::panel::text_section(app, ci, &text_ids, ui);
    }
    if !shape_ids.is_empty() {
        crate::ui::properties::panel::shape_section(app, ci, &shape_ids, ui);
    }

    let primary = ids[0];
    if app.doc.canvases[ci]
        .object(primary)
        .and_then(|o| o.style())
        .is_some()
    {
        ui.separator();
        format_once_section(app, ci, primary, ui);
    }

    flush_inspector_edit(app, ui, axes_focused);
    ui.separator();
    ui.add_space(2.0);
}

/// Catalog-driven rows for whatever the resolved plot selection draws.
///
/// The objects come from the same resolution the Ribbon button, the context
/// menu and the canvas gesture use — every selected plot, or the page's active
/// plot when nothing is selected — rather than from this panel's own
/// single-selection guard. Reading the selection differently is what let those
/// channels enable a jump to a section that then drew nothing, and it put the
/// cross-target `Mixed` aggregate out of reach of the interface entirely. The
/// section renders nothing at all when no resolved series has an applicable
/// encoding.
/// The typography section is deliberately unconditional: it is a *document*
/// property, so it applies whenever a document is open, whatever is selected.
/// Gating it on the selection would hide an always-applicable control for a
/// transient reason, which the crate's hide-vs-disable rule forbids.
fn property_sections(app: &mut PlotxApp, ci: usize, include_axes: bool, ui: &mut Ui) {
    // The write side of the shared selection: these sections carry controls, so
    // they take the editable subset. The lock lives in one place rather than
    // being re-derived here.
    let objects = crate::ui::properties::discovery::editable_objects(app);
    if include_axes {
        crate::ui::properties::panel::axis_section(app, ci, &objects, ui);
    }
    crate::ui::properties::panel::contour_section(app, ci, &objects, ui);
    crate::ui::properties::panel::line_section(app, ci, &objects, ui);
    crate::ui::properties::panel::typography_section(app, ui);
}

#[cfg(test)]
#[path = "object_inspector_tests.rs"]
mod tests;
