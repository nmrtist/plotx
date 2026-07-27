//! Panel note editing window.

use super::*;
use plotx_core::state::ObjectId;

pub(in crate::ui) fn panel_note_edit_window(app: &mut PlotxApp, ctx: &egui::Context) {
    let Some(edit) = app.session.ui.panel_note_edit.as_ref() else {
        return;
    };
    let ci = edit.canvas;
    let object_id = edit.object;
    if ci >= app.doc.canvases.len()
        || app.doc.canvases[ci]
            .object(object_id)
            .and_then(|object| object.plot())
            .is_none()
    {
        app.session.ui.panel_note_edit = None;
        app.session.ui.selection = Selection::None;
        return;
    }

    let mut open = true;
    let mut save = false;
    let mut delete = false;
    let mut cancel = false;
    egui::Window::new("Edit panel note")
        .collapsible(false)
        .resizable(false)
        .open(&mut open)
        .show(ctx, |ui| {
            let Some(edit) = app.session.ui.panel_note_edit.as_mut() else {
                return;
            };
            let resp = ui.add(
                egui::TextEdit::multiline(&mut edit.buffer)
                    .desired_width(320.0)
                    .desired_rows(3),
            );
            if edit.focus {
                resp.request_focus();
                edit.focus = false;
            }
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ui.button("Save").clicked() {
                    save = true;
                }
                if ui.button("Clear").clicked() {
                    delete = true;
                }
                if ui.button("Cancel").clicked() {
                    cancel = true;
                }
            });
        });

    if save {
        let buffer = app
            .session
            .ui
            .panel_note_edit
            .as_ref()
            .map(|edit| edit.buffer.trim().to_owned())
            .unwrap_or_default();
        if write_panel_note(app, ci, object_id, buffer) {
            app.select_panel_label(ci, object_id);
            app.session.status = "Panel note updated.".to_owned();
        }
        app.session.ui.panel_note_edit = None;
    } else if delete {
        if write_panel_note(app, ci, object_id, String::new()) {
            app.session.status = "Panel note cleared.".to_owned();
        }
        app.session.ui.panel_note_edit = None;
        app.select_object(ci, object_id);
    } else if cancel || !open {
        app.session.ui.panel_note_edit = None;
    }
}

fn write_panel_note(app: &mut PlotxApp, canvas: usize, object: ObjectId, note: String) -> bool {
    let Some(target) = app.object_target(canvas, object) else {
        return false;
    };
    let Ok(commit) = app.plan_property_write(
        plotx_core::properties::object::PANEL_USER_NOTE,
        std::slice::from_ref(&target),
        &plotx_core::properties::PropertyValue::Text(note),
    ) else {
        return false;
    };
    app.commit_property(commit) == 1
}
