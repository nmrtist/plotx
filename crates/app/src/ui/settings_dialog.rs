use super::*;
use egui::{Align, Align2, CornerRadius, FontId, Layout, RichText, pos2, vec2};
use egui_phosphor::regular as icon;
use plotx_core::settings::{self, GraphicsPowerPreference, Settings, ThemeMode};
use plotx_core::state::{MonitorScaleStatus, SettingsCategory, SettingsDialog};
use plotx_core::update::{UpdateChannelSetting, UpdateService, UpdateStatus};

const RAIL_WIDTH: f32 = 172.0;
const CONTROL_COL: f32 = 200.0;
const ROW_GAP: f32 = 12.0;
const WINDOW_W: f32 = 664.0;
const WINDOW_H: f32 = 430.0;
const MIN_W: f32 = 468.0;
const MIN_H: f32 = 300.0;
const FLUSH_DELAY: f64 = 0.6;

pub(crate) fn apply_chrome_theme(ctx: &egui::Context, mode: ThemeMode) {
    let pref = match mode {
        ThemeMode::System => egui::ThemePreference::System,
        ThemeMode::Light => egui::ThemePreference::Light,
        ThemeMode::Dark => egui::ThemePreference::Dark,
    };
    ctx.set_theme(pref);
    for theme in [egui::Theme::Light, egui::Theme::Dark] {
        ctx.style_mut_of(theme, |style| {
            // Disabled widgets keep the normal button fill and fade only via
            // `disabled_alpha`. Stock egui swaps in the near-panel
            // `noninteractive` fill, which makes light-theme buttons *brighten*
            // when a modal disables the chrome behind it.
            style.visuals.widgets.noninteractive.weak_bg_fill =
                style.visuals.widgets.inactive.weak_bg_fill;
        });
    }
}

pub(super) fn settings_window(app: &mut PlotxApp, ctx: &egui::Context) {
    if app.session.ui.settings_dialog.is_none() {
        return;
    }
    let now = ctx.input(|i| i.time);
    let mut done = false;
    let mut changed = false;

    let available = ctx.content_rect().size() - vec2(48.0, 48.0);
    let size = vec2(WINDOW_W, WINDOW_H)
        .min(available)
        .max(vec2(MIN_W.min(available.x), MIN_H.min(available.y)));
    let monitor = app.session.monitor.clone();
    let modal = super::modal(ctx, "preferences_modal", ModalKind::Dialog).show(ctx, |ui| {
        ui.set_min_size(size);
        ui.heading("Preferences");
        ui.separator();
        let session = &mut app.session;
        let dialog = session.ui.settings_dialog.as_mut().unwrap();
        let before = dialog.draft.clone();
        let (d, reset) = window_body(ui, dialog, &mut session.updates, monitor.as_ref());
        done = d;
        if reset {
            // Reset restores preferences, not history: the recent-files list
            // is user data and survives.
            let recent = dialog.draft.recent.clone();
            dialog.draft = Settings::default();
            dialog.draft.recent = recent;
            // The probed automatic scale of the current display is a fact, not
            // a preference; reseed it so reset only drops the manual override.
            if let Some(monitor) = &monitor {
                dialog.draft.appearance.ui_scale.monitors.insert(
                    monitor.key.clone(),
                    plotx_core::settings::MonitorScale {
                        auto: monitor.auto,
                        user: None,
                    },
                );
            }
            dialog.last_error = None;
        }
        if dialog.draft != before {
            changed = true;
        }
    });

    if changed {
        let draft = app
            .session
            .ui
            .settings_dialog
            .as_ref()
            .unwrap()
            .draft
            .clone();
        app.apply_settings(&draft);
        apply_chrome_theme(ctx, draft.appearance.theme);
        // `apply_settings` has synced the current monitor's record from the
        // draft; the egui zoom is an app-shell concern, applied here.
        if let Some(monitor) = &app.session.monitor {
            ctx.set_zoom_factor(monitor.effective());
        }
        if let Some(dialog) = app.session.ui.settings_dialog.as_mut() {
            dialog.flush_at = Some(now + FLUSH_DELAY);
        }
    }

    let close = done || modal.should_close();
    if let Some(dialog) = app.session.ui.settings_dialog.as_mut() {
        let due = dialog.flush_at.is_some_and(|t| now >= t);
        if close || due {
            dialog.last_error = settings::save(&dialog.draft).err().map(|e| {
                format!("Couldn't save preferences — changes apply this session only ({e})")
            });
            dialog.flush_at = None;
        }
        if let Some(t) = dialog.flush_at {
            ctx.request_repaint_after(std::time::Duration::from_secs_f64((t - now).max(0.0)));
        }
    }

    if close
        && app
            .session
            .ui
            .settings_dialog
            .as_ref()
            .is_some_and(|dialog| dialog.last_error.is_none())
    {
        app.session.ui.settings_dialog = None;
    }
}

fn window_body(
    ui: &mut Ui,
    dialog: &mut SettingsDialog,
    updates: &mut UpdateService,
    monitor: Option<&MonitorScaleStatus>,
) -> (bool, bool) {
    let mut done = false;
    let mut reset = false;

    egui::Panel::bottom("settings_footer")
        .frame(egui::Frame::side_top_panel(ui.style()).inner_margin(egui::Margin::symmetric(8, 8)))
        .show_inside(ui, |ui| {
            let (d, r) = footer(ui, dialog);
            done = d;
            reset = r;
        });

    egui::Panel::left("settings_rail")
        .resizable(false)
        .exact_size(RAIL_WIDTH)
        .show_inside(ui, |ui| {
            ui.add_space(6.0);
            for cat in SettingsCategory::ALL {
                if rail_row(ui, cat, dialog.category == cat).clicked() {
                    dialog.category = cat;
                }
            }
        });

    egui::CentralPanel::default()
        .frame(egui::Frame::central_panel(ui.style()).inner_margin(egui::Margin::symmetric(18, 12)))
        .show_inside(ui, |ui| {
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    render_category(ui, dialog.category, &mut dialog.draft, updates, monitor);
                });
        });

    (done, reset)
}

fn rail_row(ui: &mut Ui, cat: SettingsCategory, selected: bool) -> Response {
    let width = ui.available_width();
    let (rect, resp) = ui.allocate_exact_size(vec2(width, 30.0), Sense::click());
    let visuals = ui.visuals();
    let color = if selected || resp.hovered() {
        visuals.strong_text_color()
    } else {
        visuals.text_color()
    };
    if selected {
        ui.painter()
            .rect_filled(rect, CornerRadius::same(6), visuals.selection.bg_fill);
    } else if resp.hovered() {
        ui.painter()
            .rect_filled(rect, CornerRadius::same(6), visuals.widgets.hovered.bg_fill);
    }
    let cy = rect.center().y;
    let painter = ui.painter();
    painter.text(
        pos2(rect.left() + 14.0, cy),
        Align2::LEFT_CENTER,
        rail_icon(cat),
        FontId::proportional(15.0),
        color,
    );
    painter.text(
        pos2(rect.left() + 38.0, cy),
        Align2::LEFT_CENTER,
        cat.label(),
        FontId::proportional(14.0),
        color,
    );
    resp
}

fn rail_icon(cat: SettingsCategory) -> &'static str {
    match cat {
        SettingsCategory::General => icon::GEAR_SIX,
        SettingsCategory::Appearance => icon::PALETTE,
        SettingsCategory::Processing => icon::WAVEFORM,
        SettingsCategory::Export => icon::EXPORT,
        SettingsCategory::Recent => icon::CLOCK_COUNTER_CLOCKWISE,
    }
}

fn render_category(
    ui: &mut Ui,
    cat: SettingsCategory,
    draft: &mut Settings,
    updates: &mut UpdateService,
    monitor: Option<&MonitorScaleStatus>,
) {
    ui.add_space(2.0);
    match cat {
        SettingsCategory::General => {
            setting_row(
                ui,
                "Object snapping",
                Some("Snap plots and shapes to guides while dragging."),
                |ui| {
                    toggle(ui, &mut draft.general.snap_enabled);
                },
            );
            setting_row(
                ui,
                "Project backup copies",
                Some(
                    "Keep this many complete previous saves as hidden files beside the project. \
                     Each copy can be as large as the project; choose Off to disable them.",
                ),
                |ui| backup_count_combo(ui, &mut draft.general.project_backup_generations),
            );
            setting_row(
                ui,
                "Automatic updates",
                Some("Check for new versions in the background."),
                |ui| {
                    toggle(ui, &mut draft.updates.auto_check);
                },
            );
            setting_row(
                ui,
                "Update channel",
                Some("Which release train to follow. Each channel only offers its own builds."),
                |ui| channel_combo(ui, &mut draft.updates.channel),
            );
            update_status_row(ui, updates);
        }
        SettingsCategory::Appearance => {
            setting_row(
                ui,
                "Chrome theme",
                Some("Light, dark, or follow the system appearance."),
                |ui| theme_combo(ui, &mut draft.appearance.theme),
            );
            ui_scale_row(ui, draft, monitor);
            setting_row(
                ui,
                "Graphics processor",
                Some("Choose the GPU class PlotX requests at startup. Restart required."),
                |ui| graphics_power_combo(ui, &mut draft.appearance.graphics_power),
            );
        }
        SettingsCategory::Processing => {
            empty_state(ui, "Nothing to configure here yet.");
        }
        SettingsCategory::Export => {
            setting_row(
                ui,
                "Embed view snapshots",
                Some("Save each plot's on-screen view into the .plotx file."),
                |ui| {
                    toggle(ui, &mut draft.export.include_view_snapshots);
                },
            );
            setting_row(
                ui,
                "Raster resolution",
                Some("Pixel density for bitmap (PNG) exports."),
                |ui| {
                    ui.add(
                        egui::DragValue::new(&mut draft.export.dpi)
                            .range(72..=1200)
                            .suffix(" dpi"),
                    );
                },
            );
        }
        SettingsCategory::Recent => render_recent(ui, draft),
    }
}

fn graphics_power_combo(ui: &mut Ui, value: &mut GraphicsPowerPreference) {
    egui::ComboBox::from_id_salt("settings_graphics_power")
        .selected_text(value.label())
        .show_ui(ui, |ui| {
            for choice in GraphicsPowerPreference::ALL {
                ui.selectable_value(value, choice, choice.label());
            }
        });
}

fn render_recent(ui: &mut Ui, draft: &mut Settings) {
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

fn setting_row(ui: &mut Ui, label: &str, desc: Option<&str>, control: impl FnOnce(&mut Ui)) {
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

fn channel_combo(ui: &mut Ui, channel: &mut UpdateChannelSetting) {
    egui::ComboBox::from_id_salt("settings_update_channel")
        .selected_text(channel.label())
        .width(150.0)
        .show_ui(ui, |ui| {
            for candidate in UpdateChannelSetting::ALL {
                ui.selectable_value(channel, candidate, candidate.label());
            }
        });
}

fn update_status_row(ui: &mut Ui, updates: &mut UpdateService) {
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

fn ui_scale_row(ui: &mut Ui, draft: &mut Settings, monitor: Option<&MonitorScaleStatus>) {
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

fn theme_combo(ui: &mut Ui, mode: &mut ThemeMode) {
    egui::ComboBox::from_id_salt("settings_theme")
        .selected_text(mode.label())
        .width(150.0)
        .show_ui(ui, |ui| {
            for candidate in ThemeMode::ALL {
                ui.selectable_value(mode, candidate, candidate.label());
            }
        });
}

fn footer(ui: &mut Ui, dialog: &SettingsDialog) -> (bool, bool) {
    let mut done = false;
    let mut reset = false;
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        if ui.button("Reset to Defaults").clicked() {
            reset = true;
        }
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if ui.button("Done").clicked() {
                done = true;
            }
            if let Some(err) = &dialog.last_error {
                ui.add_space(10.0);
                let color = ui.visuals().error_fg_color;
                ui.label(RichText::new(err).small().color(color));
            }
        });
    });
    (done, reset)
}

fn toggle(ui: &mut Ui, on: &mut bool) -> Response {
    let (rect, mut resp) = ui.allocate_exact_size(vec2(38.0, 20.0), Sense::click());
    if resp.clicked() {
        *on = !*on;
        resp.mark_changed();
    }
    let enabled = ui.is_enabled();
    resp.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Checkbox, enabled, *on, ""));
    if ui.is_rect_visible(rect) {
        let how = ui.ctx().animate_bool(resp.id, *on);
        let visuals = ui.style().interact_selectable(&resp, *on);
        let rect = rect.expand(visuals.expansion);
        let radius = 0.5 * rect.height();
        ui.painter().rect(
            rect,
            radius,
            visuals.bg_fill,
            visuals.bg_stroke,
            egui::StrokeKind::Inside,
        );
        let cx = egui::lerp((rect.left() + radius)..=(rect.right() - radius), how);
        ui.painter().circle(
            pos2(cx, rect.center().y),
            0.75 * radius,
            visuals.bg_fill,
            visuals.fg_stroke,
        );
    }
    resp
}

fn backup_count_combo(ui: &mut Ui, count: &mut u8) {
    let selected = match *count {
        0 => "Off".to_owned(),
        1 => "1 copy".to_owned(),
        value => format!("{value} copies"),
    };
    egui::ComboBox::from_id_salt("project_backup_generations")
        .selected_text(selected)
        .width(120.0)
        .show_ui(ui, |ui| {
            ui.selectable_value(count, 0, "Off");
            for value in 1..=plotx_core::settings::MAX_PROJECT_BACKUP_GENERATIONS {
                let label = if value == 1 {
                    "1 copy".to_owned()
                } else {
                    format!("{value} copies")
                };
                ui.selectable_value(count, value, label);
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::{Pos2, RawInput, Rect, vec2};

    fn run_all_categories(app: &mut PlotxApp, size: egui::Vec2) {
        let ctx = egui::Context::default();
        for cat in SettingsCategory::ALL {
            app.session.ui.settings_dialog.as_mut().unwrap().category = cat;
            let input = RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, size)),
                ..Default::default()
            };
            let _ = ctx.run_ui(input, |ui| settings_window(app, ui.ctx()));
        }
    }

    #[test]
    fn renders_every_category_at_any_size_without_panic() {
        let mut app = PlotxApp::new();
        app.open_settings();
        for _ in 0..3 {
            run_all_categories(&mut app, vec2(480.0, 360.0));
            run_all_categories(&mut app, vec2(1600.0, 1000.0));
        }
        assert!(app.session.ui.settings_dialog.is_some());
    }
}
