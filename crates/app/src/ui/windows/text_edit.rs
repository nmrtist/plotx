//! Text object editing window.

use super::*;

pub(in crate::ui) fn text_edit_window(app: &mut PlotxApp, ctx: &egui::Context) {
    let Some(edit) = app.session.ui.text_edit.as_ref() else {
        return;
    };
    let ci = edit.canvas;
    let object_id = edit.object;
    if ci >= app.doc.canvases.len()
        || app.doc.canvases[ci]
            .object(object_id)
            .and_then(|object| object.text())
            .is_none()
    {
        app.session.ui.text_edit = None;
        return;
    }

    let mut open = true;
    let mut save = false;
    let mut delete = false;
    let mut cancel = false;
    egui::Window::new("Edit text")
        .collapsible(false)
        .resizable(false)
        .open(&mut open)
        .show(ctx, |ui| {
            let Some(edit) = app.session.ui.text_edit.as_mut() else {
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
                if ui.button("Delete").clicked() {
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
            .text_edit
            .as_ref()
            .map(|edit| edit.buffer.trim().to_owned())
            .unwrap_or_default();
        if let Some(before) = app.doc.canvases[ci]
            .object(object_id)
            .and_then(|object| object.text())
            .cloned()
        {
            let mut after = before.clone();
            if !buffer.is_empty() {
                after.text = buffer;
            }
            app.execute_action(Action::set_object_text(ci, object_id, before, after));
            app.select_object(ci, object_id);
            app.session.status = "Text updated.".to_owned();
        }
        app.session.ui.text_edit = None;
    } else if delete {
        if let Some(action) = Action::delete_object(app, ci, object_id) {
            app.execute_action(action);
        }
        app.session.ui.text_edit = None;
        app.session.status = "Object deleted.".to_owned();
    } else if cancel || !open {
        app.session.ui.text_edit = None;
    }
}
