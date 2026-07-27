//! Project saving and quit-confirmation windows.

use super::*;

pub(in crate::ui) fn save_project_window(app: &mut PlotxApp, ctx: &egui::Context) {
    if !app.session.ui.save_project_options {
        return;
    }

    let mut save = false;
    let mut save_as = false;
    let mut cancel = false;
    let modal = super::modal(ctx, "save_project_modal", ModalKind::Dialog).show(ctx, |ui| {
        ui.set_width(390.0);
        ui.heading("Save project");
        ui.separator();
        if app.session.status.starts_with("Save failed:") {
            ui.colored_label(ui.visuals().error_fg_color, &app.session.status);
            if ui.link("Open diagnostic details").clicked() {
                app.session.ui.diagnostics_open = true;
            }
            ui.add_space(8.0);
        }
        ui.checkbox(
            &mut app.doc.save_include_view_snapshots,
            "Include rendered canvas snapshots",
        )
        .on_hover_text(
            "Stores materialized view data for faster and more stable reopening. \
                 This can make .plotx files much larger.",
        );
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            if ui.button("Save").clicked() {
                save = true;
            }
            if ui.button("Save As…").clicked() {
                save_as = true;
            }
            if ui.button("Cancel").clicked() {
                cancel = true;
            }
        });
    });

    if save {
        if let Some(path) = app.doc.project_path.clone() {
            app.session.ui.save_project_options =
                !app.save_project_to(&path, app.doc.save_include_view_snapshots);
        } else {
            crate::ui::file_dialogs::save_project_as(app, app.doc.save_include_view_snapshots);
            app.session.ui.save_project_options = app.doc.dirty;
        }
    } else if save_as {
        crate::ui::file_dialogs::save_project_as(app, app.doc.save_include_view_snapshots);
        app.session.ui.save_project_options = app.doc.dirty;
    } else if cancel || modal.should_close() {
        app.session.ui.save_project_options = false;
    }
}

/// Intercept a window-close request when the project has unsaved changes: veto the
/// close and raise the confirm dialog. Once the user confirms (Save or Discard),
/// `allow_close` lets the re-issued request through.
pub(in crate::ui) fn handle_close_request(app: &mut PlotxApp, ctx: &egui::Context) {
    if !ctx.input(|i| i.viewport().close_requested()) {
        return;
    }
    if app.doc.dirty && !app.session.allow_close {
        ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
        app.session.ui.quit_confirm = true;
    }
}

/// Save / Discard / Cancel dialog shown when a close was intercepted on a dirty
/// project. Save routes through the normal save flow (opening Save As… if the
/// project has no path yet) and only closes once the save actually succeeds.
pub(in crate::ui) fn quit_confirm_window(app: &mut PlotxApp, ctx: &egui::Context) {
    if !app.session.ui.quit_confirm {
        return;
    }
    let mut save = false;
    let mut discard = false;
    let mut cancel = false;
    let modal = super::modal(ctx, "quit_confirm_modal", ModalKind::Dialog).show(ctx, |ui| {
        ui.set_width(420.0);
        ui.heading("Unsaved changes");
        ui.separator();
        ui.label("This project has unsaved changes. Save before closing?");
        if app.session.status.starts_with("Save failed:") {
            ui.add_space(8.0);
            egui::Frame::new()
                .fill(ui.visuals().error_fg_color.linear_multiply(0.12))
                .corner_radius(6)
                .inner_margin(8)
                .show(ui, |ui| {
                    ui.colored_label(ui.visuals().error_fg_color, &app.session.status);
                    if ui.link("Open diagnostic details").clicked() {
                        app.session.ui.diagnostics_open = true;
                    }
                });
        }
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            if ui.button("Save").clicked() {
                save = true;
            }
            if ui.button("Discard").clicked() {
                discard = true;
            }
            if ui.button("Cancel").clicked() {
                cancel = true;
            }
        });
    });

    if save {
        let saved = if let Some(path) = app.doc.project_path.clone() {
            app.save_project_to(&path, app.doc.save_include_view_snapshots)
        } else {
            crate::ui::file_dialogs::save_project_as(app, app.doc.save_include_view_snapshots);
            !app.doc.dirty
        };
        if saved {
            app.session.ui.quit_confirm = false;
            app.session.allow_close = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        } else {
            crate::cancel_relaunch();
        }
    } else if discard {
        app.session.ui.quit_confirm = false;
        app.session.allow_close = true;
        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
    } else if cancel || modal.should_close() {
        app.session.ui.quit_confirm = false;
        crate::cancel_relaunch();
    }
}
