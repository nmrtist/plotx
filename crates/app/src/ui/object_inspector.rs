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
use edits::{flush_inspector_edit, format_once_section, kind_targets, selection_context_label};
use egui::{DragValue, Ui};
use egui_phosphor::regular as icon;
use geometry::geometry_section;
use plotx_core::actions::{Action, PendingInspectorEdit};
use plotx_core::state::{
    CanvasObject, DataBinding, Dataset, MM_TO_PT, OVERLAY_PALETTE, ObjectFrame, ObjectId, PlotxApp,
    StackSpec,
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
        inspector_header(app, None, &ids, ui);
        ui.weak("Open a canvas to inspect its objects.");
        crate::ui::properties::panel::typography_section(app, ui);
        return;
    };
    if ci >= app.doc.canvases.len() {
        inspector_header(app, None, &ids, ui);
        ui.weak("Open a canvas to inspect its objects.");
        crate::ui::properties::panel::typography_section(app, ui);
        return;
    }
    inspector_header(app, Some(ci), &ids, ui);
    if ids.is_empty() {
        if app.session.ui.requested_inspector_section.as_deref() == Some("inspector.layout") {
            app.session.ui.requested_inspector_section = None;
        }
        ui.weak("Select an object on this canvas to inspect it.");
        section_navigation(app, ci, &ids, ui);
        crate::ui::properties::panel::typography_section(app, ui);
        return;
    }

    section_navigation(app, ci, &ids, ui);
    custom_section_heading(app, "inspector.layout", "Layout", ui);
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

    // Catalog sections stay in their PanelRoute order. Non-catalog Layout and
    // Data anchors are inserted at stable semantic boundaries without creating
    // parallel property registrations.
    crate::ui::properties::panel::contour_section(app, ci, &property_objects, ui);
    crate::ui::properties::panel::heatmap_section(app, ci, &property_objects, ui);
    crate::ui::properties::panel::line_section(app, ci, &property_objects, ui);
    crate::ui::properties::panel::axis_section(app, ci, &property_objects, ui);
    crate::ui::properties::panel::guide_section(app, ci, &property_objects, ui);

    let mut axes_focused = false;
    if ids.len() == 1
        && app.doc.canvases[ci]
            .object(ids[0])
            .map(|o| o.plot().is_some())
            .unwrap_or(false)
    {
        axes_focused = axes_section(app, ci, ids[0], ui);
        custom_section_heading(app, "inspector.data", "Data", ui);
        data_section(app, ci, ids[0], ui);
    }

    let text_ids = kind_targets(app, ci, &ids, |o| o.text().is_some());
    let shape_ids = kind_targets(app, ci, &ids, |o| o.shape().is_some());

    if !text_ids.is_empty() {
        crate::ui::properties::panel::text_section(app, ci, &text_ids, ui);
    }
    if !shape_ids.is_empty() {
        crate::ui::properties::panel::shape_section(app, ci, &shape_ids, ui);
    }
    crate::ui::properties::panel::panel_section(app, ci, &property_objects, ui);
    crate::ui::properties::panel::general_object_section(app, ci, &ids, ui);
    crate::ui::properties::panel::typography_section(app, ui);

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

pub(crate) fn finish_series_edit_if_inactive(app: &mut PlotxApp, inspector_visible: bool) {
    let target_changed = app
        .session
        .ui
        .series_presentation_edit
        .as_ref()
        .is_some_and(|edit| {
            app.session.active_canvas != Some(edit.canvas)
                || app
                    .doc
                    .canvases
                    .get(edit.canvas)
                    .and_then(|canvas| canvas.selected_object)
                    != Some(edit.object)
        });
    if !inspector_visible || target_changed {
        app.finish_series_presentation_edit();
    }
}

fn inspector_header(app: &PlotxApp, canvas: Option<usize>, ids: &[ObjectId], ui: &mut Ui) {
    let context = canvas
        .map(|canvas| selection_context_label(app, canvas, ids))
        .unwrap_or_else(|| "No canvas".to_owned());
    ui.add_space(4.0);
    egui::containers::Sides::new()
        .shrink_right()
        .truncate()
        .show(
            ui,
            |ui| {
                ui.label(crate::typography::headline("Inspector"));
            },
            |ui| {
                ui.add(egui::Label::new(&context).truncate())
                    .on_hover_text(context);
            },
        );
    ui.add_space(4.0);
}

fn section_navigation(app: &mut PlotxApp, ci: usize, ids: &[ObjectId], ui: &mut Ui) {
    let has_single_plot = ids.len() == 1
        && app.doc.canvases[ci]
            .object(ids[0])
            .is_some_and(|object| object.plot().is_some());
    ui.horizontal_wrapped(|ui| {
        if !ids.is_empty() {
            section_button(app, "inspector.layout", icon::FRAME_CORNERS, "Layout", ui);
        }
        for section in inspector_catalog_sections(app, !ids.is_empty()) {
            if let Some(group) = crate::ui::properties::discovery::group(section) {
                section_button(
                    app,
                    section,
                    group.icon,
                    short_section_label(group.label.get()),
                    ui,
                );
            }
            if section == crate::ui::properties::panel::AXIS_SECTION && has_single_plot {
                section_button(app, "inspector.data", icon::DATABASE, "Data", ui);
            }
        }
    });
    ui.separator();
}

fn inspector_catalog_sections(app: &PlotxApp, has_selection: bool) -> Vec<&'static str> {
    crate::ui::properties::PanelRoute::SecondarySidebar
        .sections()
        .iter()
        .copied()
        .filter(|section| {
            has_selection || *section == crate::ui::properties::panel::TYPOGRAPHY_SECTION
        })
        .filter(|section| crate::ui::properties::discovery::group_applies(app, section))
        .collect()
}

fn short_section_label(label: &'static str) -> &'static str {
    match label {
        "Figure typography" => "Type",
        value => value,
    }
}

fn section_button(
    app: &mut PlotxApp,
    section: &'static str,
    icon: &'static str,
    label: &'static str,
    ui: &mut Ui,
) {
    if ui
        .small_button(format!("{icon}  {label}"))
        .on_hover_text(format!("Jump to {label}"))
        .clicked()
    {
        app.session.ui.requested_inspector_section = Some(section.to_owned());
    }
}

fn custom_section_heading(app: &mut PlotxApp, section: &'static str, title: &str, ui: &mut Ui) {
    ui.separator();
    let response = ui.label(crate::typography::headline(title));
    if app.session.ui.requested_inspector_section.as_deref() == Some(section) {
        response.scroll_to_me(Some(egui::Align::Min));
        app.session.ui.requested_inspector_section = None;
    }
}

#[cfg(test)]
#[path = "object_inspector_tests.rs"]
mod tests;
