//! The pinned Board views section: named board-viewport bookmarks, split
//! from the parent module to keep it under the repository size limit.

use egui::Ui;
use egui_phosphor::regular as icon;
use plotx_core::actions::Action;
use plotx_core::state::{NamedView, PlotxApp};

pub(super) fn board_views_section(app: &mut PlotxApp, ui: &mut Ui) {
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        ui.strong("Board views");
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let can_save = !app.session.ui.board_view_name.trim().is_empty();
            if ui
                .add_enabled(can_save, egui::Button::new("Save").small())
                .on_hover_text("Bookmark the current board zoom and pan")
                .clicked()
            {
                let view = NamedView {
                    name: app.session.ui.board_view_name.trim().to_owned(),
                    zoom: app.session.board.zoom,
                    pan: app.session.board.pan,
                };
                app.execute_action(Action::board_view_insert(
                    app.session.board_views.len(),
                    view,
                ));
                app.session.ui.board_view_name.clear();
            }
            ui.add(
                egui::TextEdit::singleline(&mut app.session.ui.board_view_name)
                    .hint_text("Name this view")
                    .desired_width(f32::INFINITY),
            );
        });
    });

    let mut jump: Option<usize> = None;
    let mut delete: Option<usize> = None;
    for (i, view) in app.session.board_views.iter().enumerate() {
        ui.horizontal(|ui| {
            if ui.selectable_label(false, &view.name).clicked() {
                jump = Some(i);
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .small_button(icon::X)
                    .on_hover_text("Delete view")
                    .clicked()
                {
                    delete = Some(i);
                }
            });
        });
    }
    ui.add_space(4.0);

    if let Some(i) = jump {
        let (zoom, pan) = (
            app.session.board_views[i].zoom,
            app.session.board_views[i].pan,
        );
        crate::ui::canvas::request_board_fit_viewport(app, ui.ctx(), zoom, pan);
        app.session.status = format!("Jumped to view “{}”.", app.session.board_views[i].name);
    }
    if let Some(i) = delete {
        let view = app.session.board_views[i].clone();
        app.execute_action(Action::board_view_remove(i, view));
    }
}
