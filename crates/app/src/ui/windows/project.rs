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
            if ui
                .add_enabled(
                    !app.session.ui.project_save_in_progress,
                    egui::Button::new("Save"),
                )
                .clicked()
            {
                save = true;
            }
            if ui
                .add_enabled(
                    !app.session.ui.project_save_in_progress,
                    egui::Button::new("Save As…"),
                )
                .clicked()
            {
                save_as = true;
            }
            if ui.button("Cancel").clicked() {
                cancel = true;
            }
        });
    });

    if save {
        let path = app
            .doc
            .project_path
            .clone()
            .or_else(crate::ui::file_dialogs::choose_project_save_path);
        if let Some(path) = path {
            app.queue_project_save(path, app.doc.save_include_view_snapshots, false);
            app.session.ui.save_project_options = false;
        }
    } else if save_as {
        if let Some(path) = crate::ui::file_dialogs::choose_project_save_path() {
            app.queue_project_save(path, app.doc.save_include_view_snapshots, false);
            app.session.ui.save_project_options = false;
        }
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
        if app.session.ui.project_transition.is_none() {
            app.request_project_transition(plotx_core::state::ProjectTransition::Quit);
        }
    }
}

/// Shared Save / Discard / Cancel gate for every operation that replaces the
/// current project.
pub(in crate::ui) fn quit_confirm_window(app: &mut PlotxApp, ctx: &egui::Context) {
    use plotx_core::state::{ProjectTransition, ProjectTransitionPhase};

    let Some(transition) = app.session.ui.project_transition.clone() else {
        return;
    };
    if transition.phase == ProjectTransitionPhase::Ready {
        return;
    }
    let saving = transition.phase == ProjectTransitionPhase::Saving;
    let action = match &transition.target {
        ProjectTransition::New => "starting a new project",
        ProjectTransition::Close => "closing this project",
        ProjectTransition::Open(path) => {
            let name = path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_else(|| path.display().to_string());
            return project_transition_dialog(app, ctx, saving, &format!("opening {name}"));
        }
        ProjectTransition::Quit => "quitting PlotX",
    };
    project_transition_dialog(app, ctx, saving, action);
}

fn project_transition_dialog(app: &mut PlotxApp, ctx: &egui::Context, saving: bool, action: &str) {
    use plotx_core::state::ProjectTransitionPhase;

    let mut save = false;
    let mut discard = false;
    let mut cancel = false;
    let modal =
        super::modal(ctx, "project_transition_confirm_modal", ModalKind::Dialog).show(ctx, |ui| {
            ui.set_width(420.0);
            ui.heading("Unsaved changes");
            ui.separator();
            ui.label(format!(
                "This project has unsaved changes. Save before {action}?"
            ));
            if saving {
                ui.add_space(8.0);
                ui.spinner();
                ui.label(&app.session.status);
            }
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
                if ui.add_enabled(!saving, egui::Button::new("Save")).clicked() {
                    save = true;
                }
                if ui
                    .add_enabled(!saving, egui::Button::new("Discard"))
                    .clicked()
                {
                    discard = true;
                }
                if ui
                    .add_enabled(!saving, egui::Button::new("Cancel"))
                    .clicked()
                {
                    cancel = true;
                }
            });
        });

    if save {
        let path = app
            .doc
            .project_path
            .clone()
            .or_else(crate::ui::file_dialogs::choose_project_save_path);
        if let Some(path) = path {
            app.queue_project_save(path, app.doc.save_include_view_snapshots, true);
        }
    } else if discard {
        if let Some(transition) = app.session.ui.project_transition.as_mut() {
            transition.phase = ProjectTransitionPhase::Ready;
        }
    } else if cancel || modal.should_close() {
        app.session.ui.project_transition = None;
        crate::cancel_relaunch();
    }
}
