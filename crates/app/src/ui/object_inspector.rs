//! The Object inspector: geometry + per-kind style editing for the current
//! page-space selection, at the top of the Secondary Side Bar.

mod axes;
mod chart_gallery;
mod data;
mod edits;
mod geometry;
mod panel_inspector;

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
    if let Some(path) = app.session.ui.hierarchical_selection.lead()
        && let Some(panel) = path.panel
        && path.content.is_none()
    {
        panel_inspector::render(app, ci, panel, ui);
        return;
    }
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
    if ids.len() == 1 {
        raster_image_section(app, ci, ids[0], ui);
    }
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

fn raster_image_section(app: &mut PlotxApp, ci: usize, id: ObjectId, ui: &mut Ui) {
    let Some(before) = app.doc.canvases[ci]
        .object(id)
        .and_then(|item| match &item.kind {
            plotx_core::state::CanvasObjectKind::RasterImage(image) => Some(image.clone()),
            _ => None,
        })
    else {
        return;
    };
    let mut after = before.clone();
    let mut continuous = Vec::new();
    ui.separator();
    ui.label(crate::typography::headline("Image"));
    if let Some(asset) = app.doc.assets.get(&before.asset) {
        let source_pixels = if before.page_index == 0 {
            asset.pixel_size
        } else {
            plotx_io::image::tiff_page_dimensions(&asset.bytes, before.page_index)
                .unwrap_or(asset.pixel_size)
        };
        ui.label(format!(
            "Original: {} × {} px · {} · page {}",
            source_pixels[0],
            source_pixels[1],
            asset.format,
            before.page_index + 1
        ));
        if let Some(dpi) = plotx_io::image::metadata_dpi(&asset.bytes) {
            ui.label(format!("Metadata DPI: {:.0} × {:.0}", dpi[0], dpi[1]));
        } else {
            ui.weak("Metadata DPI: not present or not reported by this format");
        }
        if let Some(frame) = app.doc.canvases[ci].content_page_frame(id) {
            let crop_width = before.crop[2] - before.crop[0];
            let crop_height = before.crop[3] - before.crop[1];
            let mut pixels = [
                source_pixels[0] as f32 * crop_width,
                source_pixels[1] as f32 * crop_height,
            ];
            if matches!(
                before.rotation,
                plotx_core::state::QuarterTurn::Clockwise90
                    | plotx_core::state::QuarterTurn::Clockwise270
            ) {
                pixels.swap(0, 1);
            }
            let ppi = [
                pixels[0] / (frame.width / 72.0),
                pixels[1] / (frame.height / 72.0),
            ];
            ui.label(format!("Effective PPI: {:.0} × {:.0}", ppi[0], ppi[1]));
            if ppi[0].min(ppi[1]) < 300.0 {
                ui.colored_label(
                    ui.visuals().warn_fg_color,
                    "Below 300 PPI at the current frame size; reduce the frame or use a larger source.",
                );
            }
        }
    } else {
        ui.colored_label(
            ui.visuals().error_fg_color,
            "Embedded image asset is missing. Replace the image before continuing.",
        );
    }
    egui::ComboBox::from_label("Fit")
        .selected_text(format!("{:?}", after.fit))
        .show_ui(ui, |ui| {
            ui.selectable_value(
                &mut after.fit,
                plotx_core::state::ImageFit::Contain,
                "Contain",
            );
            ui.selectable_value(&mut after.fit, plotx_core::state::ImageFit::Cover, "Cover");
            ui.selectable_value(
                &mut after.fit,
                plotx_core::state::ImageFit::Stretch,
                "Stretch",
            );
        });
    egui::ComboBox::from_label("Interpolation")
        .selected_text(format!("{:?}", after.interpolation))
        .show_ui(ui, |ui| {
            ui.selectable_value(
                &mut after.interpolation,
                plotx_core::state::ImageInterpolation::Auto,
                "Auto",
            );
            ui.selectable_value(
                &mut after.interpolation,
                plotx_core::state::ImageInterpolation::Nearest,
                "Nearest",
            );
            ui.selectable_value(
                &mut after.interpolation,
                plotx_core::state::ImageInterpolation::Linear,
                "Linear",
            );
        });
    continuous.push(ui.add(egui::Slider::new(&mut after.opacity, 0.0..=1.0).text("Opacity")));
    ui.checkbox(&mut after.preserve_aspect, "Preserve aspect ratio");
    ui.horizontal(|ui| {
        if ui.button("Rotate 90°").clicked() {
            after.rotation = match after.rotation {
                plotx_core::state::QuarterTurn::Zero => plotx_core::state::QuarterTurn::Clockwise90,
                plotx_core::state::QuarterTurn::Clockwise90 => {
                    plotx_core::state::QuarterTurn::Clockwise180
                }
                plotx_core::state::QuarterTurn::Clockwise180 => {
                    plotx_core::state::QuarterTurn::Clockwise270
                }
                plotx_core::state::QuarterTurn::Clockwise270 => {
                    plotx_core::state::QuarterTurn::Zero
                }
            };
        }
        if ui.button("Reset crop").clicked() {
            after.crop = [0.0, 0.0, 1.0, 1.0];
        }
        let replace =
            crate::ui::commands::describe(app, crate::ui::commands::CommandId::ReplaceImage);
        if ui
            .add_enabled(replace.enabled, egui::Button::new(replace.label))
            .on_disabled_hover_text(
                replace
                    .disabled_reason
                    .unwrap_or("Select one image to replace it."),
            )
            .clicked()
        {
            crate::ui::commands::execute_without_clipboard(
                crate::ui::commands::CommandId::ReplaceImage,
                app,
                ui.ctx(),
            );
        }
    });
    ui.collapsing("Crop mode", |ui| {
        let [left, top, right, bottom] = after.crop;
        continuous
            .push(ui.add(egui::Slider::new(&mut after.crop[0], 0.0..=right - 0.001).text("Left")));
        continuous
            .push(ui.add(egui::Slider::new(&mut after.crop[1], 0.0..=bottom - 0.001).text("Top")));
        continuous
            .push(ui.add(egui::Slider::new(&mut after.crop[2], left + 0.001..=1.0).text("Right")));
        continuous
            .push(ui.add(egui::Slider::new(&mut after.crop[3], top + 0.001..=1.0).text("Bottom")));
    });
    if after != before && after.validate().is_ok() {
        let gesture_id = egui::Id::new(("raster-inspector-gesture", ci, id));
        let started = continuous.iter().any(egui::Response::drag_started);
        let dragging = continuous.iter().any(egui::Response::dragged);
        let stopped = continuous.iter().any(egui::Response::drag_stopped);
        if started {
            ui.ctx()
                .data_mut(|data| data.insert_temp(gesture_id, before.clone()));
        }
        if dragging || stopped {
            if let Some(item) = app.doc.canvases[ci].object_mut(id)
                && let plotx_core::state::CanvasObjectKind::RasterImage(image) = &mut item.kind
            {
                *image = after.clone();
            }
            if stopped {
                let gesture_before = ui
                    .ctx()
                    .data_mut(|data| {
                        let value = data.get_temp(gesture_id);
                        data.remove::<plotx_core::state::RasterImageContent>(gesture_id);
                        value
                    })
                    .unwrap_or_else(|| before.clone());
                if let Some(item) = app.doc.canvases[ci].object_mut(id)
                    && let plotx_core::state::CanvasObjectKind::RasterImage(image) = &mut item.kind
                {
                    *image = gesture_before.clone();
                }
                app.execute_action(Action::SetRasterImage {
                    canvas: ci,
                    object: id,
                    before: gesture_before,
                    after,
                });
            }
            return;
        }
        app.execute_action(Action::SetRasterImage {
            canvas: ci,
            object: id,
            before,
            after,
        });
    }
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
