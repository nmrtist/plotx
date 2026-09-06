use egui::Ui;
use egui_phosphor::regular as icon;
use plotx_core::state::PlotxApp;

use super::clipboard_table::ClipboardTablePaste;
use super::commands::{self, CommandId};

pub(super) fn messages_tab_id() -> egui::Id {
    egui::Id::new("operation_history_messages_tab")
}

pub(super) fn observe(app: &mut PlotxApp) {
    app.session.status_history.observe(&app.session.status);
}

pub(crate) fn open_diagnostics(app: &mut PlotxApp, ctx: &egui::Context) {
    app.session.ui.diagnostics_open = true;
    ctx.data_mut(|data| data.insert_temp(messages_tab_id(), false));
}

pub(super) fn history_button(app: &mut PlotxApp, clipboard: &mut ClipboardTablePaste, ui: &mut Ui) {
    let response = ui
        .add_sized(
            [
                super::ribbon_chrome::SIDEBAR_TOGGLE_WIDTH,
                ui.spacing().interact_size.y,
            ],
            egui::Button::new(icon::CLOCK_COUNTER_CLOCKWISE)
                .frame_when_inactive(false)
                .selected(app.session.ui.diagnostics_open),
        )
        .on_hover_text(format!("Operation history\n{}", app.session.status));
    if super::pending_feedback(app).is_some() {
        ui.painter().circle_filled(
            response.rect.right_top() + egui::vec2(-5.0, 5.0),
            2.5,
            ui.visuals().warn_fg_color,
        );
    }
    if response.clicked() {
        commands::execute(CommandId::OperationHistory, app, clipboard, ui.ctx());
        ui.ctx()
            .data_mut(|data| data.insert_temp(messages_tab_id(), true));
    }
}

pub(super) fn messages(app: &PlotxApp, ui: &mut Ui) {
    let count = app.session.status_history.messages().len();
    ui.vertical(|ui| {
        if count == 0 {
            ui.weak("No messages yet. Open data to begin.");
        }
        for message in app.session.status_history.messages().rev() {
            let elapsed = message.recorded_at.elapsed().unwrap_or_default().as_secs();
            ui.horizontal_top(|ui| {
                ui.add_sized(
                    [56.0, ui.spacing().interact_size.y],
                    egui::Label::new(
                        crate::typography::caption(format!(
                            "{}:{:02} ago",
                            elapsed / 60,
                            elapsed % 60
                        ))
                        .color(ui.visuals().weak_text_color()),
                    ),
                );
                ui.add(egui::Label::new(&message.text).wrap().selectable(true));
            });
            ui.separator();
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diagnostic_links_reveal_details_after_browsing_messages() {
        let ctx = egui::Context::default();
        let mut app = PlotxApp::new_with_settings(plotx_core::settings::Settings::default());
        ctx.data_mut(|data| data.insert_temp(messages_tab_id(), true));
        open_diagnostics(&mut app, &ctx);
        assert!(app.session.ui.diagnostics_open);
        assert_eq!(
            ctx.data(|data| data.get_temp::<bool>(messages_tab_id())),
            Some(false)
        );
    }
}
