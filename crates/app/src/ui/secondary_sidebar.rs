//! The right dock (Secondary Side Bar): the Object inspector on top, contextual
//! per-dataset processing tools below. Reserves its own layout space, so it
//! never occludes the canvas.

use crate::ui::{object_inspector, tools};
use egui::Ui;
use plotx_core::state::PlotxApp;

pub fn render(app: &mut PlotxApp, ui: &mut Ui) {
    object_inspector::render(app, ui);

    ui.add_space(6.0);
    ui.horizontal(|ui| {
        ui.strong("Dataset tools");
        if let Some(di) = app.active_dataset() {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.weak(app.doc.datasets[di].kind_label());
            });
        }
    });
    ui.add_space(6.0);
    ui.separator();

    let Some(di) = app.active_dataset() else {
        ui.add_space(10.0);
        ui.weak("Select a dataset in the Primary Side Bar to see its tools.");
        return;
    };
    if di >= app.doc.datasets.len() {
        app.clear_selection();
        return;
    }

    let groups = app.doc.datasets[di].tool_groups();
    let mut dirty = false;
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            for &group in groups {
                ui.add_space(2.0);
                let id = ui.make_persistent_id(("secondary_tool_group", group.title()));
                if app.session.ui.requested_tool_group == Some(group) {
                    let mut state =
                        egui::collapsing_header::CollapsingState::load_with_default_open(
                            ui.ctx(),
                            id,
                            true,
                        );
                    state.set_open(true);
                    state.store(ui.ctx());
                    app.session.ui.requested_tool_group = None;
                }
                egui::CollapsingHeader::new(group.title())
                    .id_salt(("secondary_tool_group", group.title()))
                    .default_open(group == groups[0])
                    .show(ui, |ui| {
                        dirty |= tools::render_group(app, di, group, ui);
                    });
            }
        });

    if dirty {
        app.apply_dataset_edit(di);
    }
}
