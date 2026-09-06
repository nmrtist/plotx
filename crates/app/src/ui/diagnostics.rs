use super::*;

pub(super) fn diagnostic_history_window(app: &mut PlotxApp, ctx: &egui::Context) {
    if !app.session.ui.diagnostics_open {
        return;
    }
    super::activity::observe(app);

    let mut open = true;
    let mut clear = false;
    let tab_id = super::activity::messages_tab_id();
    let mut messages_tab = ctx.data(|data| data.get_temp::<bool>(tab_id).unwrap_or(false));
    let copied_text = app.session.sanitized_diagnostics_text();
    let window = egui::Window::new("Operation history")
        .default_width(620.0)
        .default_height(420.0)
        .open(&mut open);
    // Save failures link here from a foreground modal. Keep the history above
    // that modal while it is open so its details remain visible and interactive.
    let window =
        if app.session.ui.save_project_options || app.session.ui.project_transition.is_some() {
            window.order(egui::Order::Foreground)
        } else {
            window
        };
    window.show(ctx, |ui| {
        ui.horizontal(|ui| {
            ui.label(format!(
                "{} operations, {} diagnostics",
                app.session.operation_history.operation_count(),
                app.session.operation_history.diagnostic_count()
            ));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("Clear").clicked() {
                    clear = true;
                }
                if ui
                    .add_enabled(!copied_text.is_empty(), egui::Button::new("Copy sanitized"))
                    .on_hover_text("Copies diagnostics with local paths redacted")
                    .clicked()
                {
                    ui.ctx().copy_text(copied_text.clone());
                }
            });
        });
        ui.separator();
        ui.add(egui::Label::new(crate::typography::callout(&app.session.status)).wrap());
        if let Some(di) = app.active_dataset() {
            ui.weak(app.doc.datasets[di].summary());
        }
        ui.horizontal(|ui| {
            ui.selectable_value(&mut messages_tab, true, "Messages");
            ui.selectable_value(&mut messages_tab, false, "Diagnostics");
        });
        ui.separator();
        egui::ScrollArea::vertical()
            .id_salt(("history", messages_tab))
            .show(ui, |ui| {
                if messages_tab {
                    super::activity::messages(app, ui);
                    return;
                }
                let mut any = false;
                for operation in app.session.operation_history.operations().rev() {
                    any = true;
                    ui.group(|ui| {
                        ui.horizontal_wrapped(|ui| {
                            ui.label(crate::typography::headline(format!(
                                "#{} {}",
                                operation.id,
                                operation.kind.as_str()
                            )));
                            ui.label(operation.outcome.as_str());
                        });
                        ui.label(&operation.summary);
                        for diagnostic in &operation.diagnostics {
                            ui.horizontal_wrapped(|ui| {
                                ui.label(crate::typography::headline(format!(
                                    "{} {}",
                                    diagnostic.severity.as_str(),
                                    diagnostic.code.as_str()
                                )));
                                ui.label(&diagnostic.message);
                            });
                            if let Some(source) = &diagnostic.source {
                                ui.weak(format!("source: {source}"));
                            }
                            for (key, value) in &diagnostic.context {
                                ui.weak(format!("{key}: {value}"));
                            }
                        }
                    });
                    ui.add_space(6.0);
                }
                if !any {
                    ui.weak("No structured operations have been recorded yet.");
                }
            });
    });
    ctx.data_mut(|data| data.insert_temp(tab_id, messages_tab));

    if clear {
        app.session.clear_operation_history();
    }
    if !open {
        app.session.ui.diagnostics_open = false;
    }
}
