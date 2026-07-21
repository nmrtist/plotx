use super::*;

pub(super) fn diagnostic_history_window(app: &mut PlotxApp, ctx: &egui::Context) {
    if !app.session.ui.diagnostics_open {
        return;
    }

    let mut open = true;
    let mut clear = false;
    let copied_text = app.session.sanitized_diagnostics_text();
    let window = egui::Window::new("Operation history")
        .default_width(620.0)
        .default_height(420.0)
        .open(&mut open);
    // Save failures link here from a foreground modal. Keep the history above
    // that modal while it is open so its details remain visible and interactive.
    let window = if app.session.ui.save_project_options || app.session.ui.quit_confirm {
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
        egui::ScrollArea::vertical().show(ui, |ui| {
            let mut any = false;
            for operation in app.session.operation_history.operations().rev() {
                any = true;
                ui.group(|ui| {
                    ui.horizontal_wrapped(|ui| {
                        ui.strong(format!("#{} {}", operation.id, operation.kind.as_str()));
                        ui.label(operation.outcome.as_str());
                    });
                    ui.label(&operation.summary);
                    for diagnostic in &operation.diagnostics {
                        ui.horizontal_wrapped(|ui| {
                            ui.strong(format!(
                                "{} {}",
                                diagnostic.severity.as_str(),
                                diagnostic.code.as_str()
                            ));
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

    if clear {
        app.session.clear_operation_history();
    }
    if !open {
        app.session.ui.diagnostics_open = false;
    }
}
