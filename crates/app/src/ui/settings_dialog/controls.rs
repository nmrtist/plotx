//! Settings category controls and compound setting rows.

use super::{CONTROL_COL, ROW_GAP};
use egui::{Align, Layout, RichText, Ui, vec2};
use egui_phosphor::regular as icon;
use plotx_core::settings::Settings;
use plotx_core::state::MonitorScaleStatus;
use plotx_core::update::{UpdateService, UpdateStatus};

pub(super) fn render_recent(ui: &mut Ui, draft: &mut Settings) {
    if draft.recent.files.is_empty() {
        empty_state(
            ui,
            "No recent files yet. Open data or a project to fill this list.",
        );
        return;
    }
    ui.label(
        RichText::new("Reopen entries from the File menu (Open Recent) or the welcome screen.")
            .small()
            .color(ui.visuals().weak_text_color()),
    );
    ui.add_space(ROW_GAP);
    let weak = ui.visuals().weak_text_color();
    let strong = ui.visuals().strong_text_color();
    for path in &draft.recent.files {
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_else(|| path.to_str().unwrap_or("<path>"));
        ui.horizontal(|ui| {
            ui.label(RichText::new(icon::FILE).color(weak));
            ui.label(RichText::new(name).color(strong))
                .on_hover_text(path.display().to_string());
        });
        ui.add_space(4.0);
    }
    ui.add_space(ROW_GAP);
    if ui.button("Clear recent files").clicked() {
        draft.recent.files.clear();
    }
}

pub(super) fn setting_row(
    ui: &mut Ui,
    label: &str,
    desc: Option<&str>,
    control: impl FnOnce(&mut Ui),
) {
    let spacing = ui.spacing().item_spacing.x;
    let full = ui.available_width();
    let control_w = CONTROL_COL.min(full * 0.45);
    let label_w = (full - control_w - spacing).max(1.0);
    let strong = ui.visuals().strong_text_color();
    let weak = ui.visuals().weak_text_color();

    ui.horizontal_top(|ui| {
        ui.allocate_ui_with_layout(vec2(label_w, 0.0), Layout::top_down(Align::Min), |ui| {
            ui.set_width(label_w);
            ui.label(RichText::new(label).strong().color(strong));
            if let Some(desc) = desc {
                ui.label(RichText::new(desc).small().color(weak));
            }
        });
        ui.allocate_ui_with_layout(
            vec2(control_w, 0.0),
            Layout::right_to_left(Align::Center),
            |ui| {
                ui.set_width(control_w);
                control(ui);
            },
        );
    });
    ui.add_space(ROW_GAP);
}

fn empty_state(ui: &mut Ui, text: &str) {
    let weak = ui.visuals().weak_text_color();
    ui.vertical_centered(|ui| {
        ui.add_space(48.0);
        ui.label(RichText::new(text).color(weak));
    });
}

pub(super) fn update_status_row(ui: &mut Ui, updates: &mut UpdateService) {
    setting_row(
        ui,
        "Check for updates",
        Some(&format!("Installed version {}.", env!("CARGO_PKG_VERSION"))),
        |ui| {
            if ui
                .add_enabled(!updates.is_busy(), egui::Button::new("Check now"))
                .clicked()
            {
                updates.check_now();
            }
        },
    );
    let status = updates.status().clone();
    let label = status.label();
    if !label.is_empty() {
        let color = match status {
            UpdateStatus::Failed { .. } => ui.visuals().error_fg_color,
            UpdateStatus::Ready { .. } | UpdateStatus::Installed { .. } => {
                ui.visuals().strong_text_color()
            }
            _ => ui.visuals().weak_text_color(),
        };
        ui.horizontal(|ui| {
            ui.label(RichText::new(label).small().color(color));
            if let UpdateStatus::Installed { .. } = status
                && ui.button("Restart now").clicked()
            {
                crate::request_relaunch();
                ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
            }
        });
        ui.add_space(ROW_GAP);
    }
}

/// Manual percentages offered beside Automatic; Ctrl+= / Ctrl+- reach the 5%
/// steps in between.
const UI_SCALE_CHOICES: [f32; 8] = [1.0, 1.1, 1.25, 1.5, 1.75, 2.0, 2.5, 3.0];

pub(super) fn ui_scale_row(
    ui: &mut Ui,
    draft: &mut Settings,
    monitor: Option<&MonitorScaleStatus>,
) {
    let Some(monitor) = monitor else {
        setting_row(
            ui,
            "UI scale",
            Some("Size of all interface text and controls."),
            |ui| {
                ui.label(
                    RichText::new("Waiting for the display probe…")
                        .small()
                        .color(ui.visuals().weak_text_color()),
                );
            },
        );
        return;
    };
    let detail = match monitor.ppi {
        Some(ppi) => format!(
            "This display reports {ppi:.0} pixels per inch; automatic picks a physically \
             legible size ({:.0}%). Applies to this display only.",
            monitor.auto * 100.0
        ),
        None => format!(
            "This display did not report its physical size, so automatic keeps the system \
             scale ({:.0}%). Applies to this display only.",
            monitor.auto * 100.0
        ),
    };
    setting_row(ui, "UI scale", Some(&detail), |ui| {
        let entry = draft
            .appearance
            .ui_scale
            .monitors
            .entry(monitor.key.clone())
            .or_insert(plotx_core::settings::MonitorScale {
                auto: monitor.auto,
                user: None,
            });
        let selected = match entry.user {
            Some(user) => format!("{:.0}%", user * 100.0),
            None => format!("Automatic ({:.0}%)", entry.auto * 100.0),
        };
        egui::ComboBox::from_id_salt("settings_ui_scale")
            .selected_text(selected)
            .width(150.0)
            .show_ui(ui, |ui| {
                ui.selectable_value(
                    &mut entry.user,
                    None,
                    format!("Automatic ({:.0}%)", entry.auto * 100.0),
                );
                for choice in UI_SCALE_CHOICES {
                    ui.selectable_value(
                        &mut entry.user,
                        Some(choice),
                        format!("{:.0}%", choice * 100.0),
                    );
                }
            });
    });
}
