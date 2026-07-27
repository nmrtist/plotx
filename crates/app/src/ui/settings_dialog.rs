//! Settings dialog layout and category rendering.

mod chrome;
mod controls;

use super::*;
use egui::vec2;
use plotx_core::properties::{PropertyAccess, ScopeKind, catalog};
use plotx_core::settings::Settings;
use plotx_core::state::{MonitorScaleStatus, SettingsCategory};

const RAIL_WIDTH: f32 = 172.0;
const CONTROL_COL: f32 = 200.0;
const ROW_GAP: f32 = 12.0;
const WINDOW_W: f32 = 664.0;
const WINDOW_H: f32 = 430.0;
const MIN_W: f32 = 468.0;
const MIN_H: f32 = 300.0;
const FLUSH_DELAY: f64 = 0.6;

pub(crate) use chrome::{apply_chrome_theme, sync_chrome_theme};
use chrome::{footer, rail_row};
use controls::{render_recent, ui_scale_row, update_status_row};

pub(super) fn settings_window(app: &mut PlotxApp, ctx: &egui::Context) {
    if app.session.ui.settings_dialog.is_none() {
        return;
    }
    let now = ctx.input(|i| i.time);
    let mut done = false;
    let mut reset = false;
    let settings_before = app.settings.clone();

    let available = ctx.content_rect().size() - vec2(48.0, 48.0);
    let size = vec2(WINDOW_W, WINDOW_H)
        .min(available)
        .max(vec2(MIN_W.min(available.x), MIN_H.min(available.y)));
    let monitor = app.session.monitor.clone();
    let modal = super::modal(ctx, "preferences_modal", ModalKind::Dialog).show(ctx, |ui| {
        ui.set_min_size(size);
        ui.heading("Preferences");
        ui.separator();
        let (d, r) = window_body(ui, app, monitor.as_ref());
        done = d;
        reset = r;
    });

    if reset {
        reset_preferences(app, monitor.as_ref());
    }

    let draft_changed = app
        .session
        .ui
        .settings_dialog
        .as_ref()
        .is_some_and(|dialog| dialog.draft != app.settings);
    if draft_changed {
        let draft = app
            .session
            .ui
            .settings_dialog
            .as_ref()
            .unwrap()
            .draft
            .clone();
        app.apply_settings(draft.clone());
        if let Some(dialog) = app.session.ui.settings_dialog.as_mut() {
            dialog.flush_at = Some(now + FLUSH_DELAY);
        }
    }
    if app.settings.appearance.theme != settings_before.appearance.theme {
        apply_chrome_theme(ctx, app.settings.appearance.theme);
    }
    if app.settings.appearance.ui_scale != settings_before.appearance.ui_scale
        && let Some(monitor) = &app.session.monitor
    {
        // The current monitor's resolved zoom belongs to the app shell.
        ctx.set_zoom_factor(monitor.effective());
    }

    let close = done || modal.should_close();
    let flush = app
        .session
        .ui
        .settings_dialog
        .as_ref()
        .is_some_and(|dialog| close || dialog.flush_at.is_some_and(|t| now >= t));
    if flush {
        let saved = app.persist_settings();
        let error = (!saved).then(|| app.session.status.clone());
        if let Some(dialog) = app.session.ui.settings_dialog.as_mut() {
            dialog.last_error = error;
            dialog.flush_at = None;
        }
    }
    if let Some(dialog) = app.session.ui.settings_dialog.as_ref()
        && let Some(t) = dialog.flush_at
    {
        ctx.request_repaint_after(std::time::Duration::from_secs_f64((t - now).max(0.0)));
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
    app: &mut PlotxApp,
    monitor: Option<&MonitorScaleStatus>,
) -> (bool, bool) {
    let mut done = false;
    let mut reset = false;

    egui::Panel::bottom("settings_footer")
        .frame(egui::Frame::side_top_panel(ui.style()).inner_margin(egui::Margin::symmetric(8, 8)))
        .show_inside(ui, |ui| {
            let dialog = app.session.ui.settings_dialog.as_ref().unwrap();
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
                let dialog = app.session.ui.settings_dialog.as_mut().unwrap();
                if rail_row(ui, cat, dialog.category == cat).clicked() {
                    dialog.category = cat;
                }
            }
        });

    let category = app.session.ui.settings_dialog.as_ref().unwrap().category;
    egui::CentralPanel::default()
        .frame(egui::Frame::central_panel(ui.style()).inner_margin(egui::Margin::symmetric(18, 12)))
        .show_inside(ui, |ui| {
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    render_category(ui, category, app, monitor);
                });
        });

    (done, reset)
}

fn render_category(
    ui: &mut Ui,
    cat: SettingsCategory,
    app: &mut PlotxApp,
    monitor: Option<&MonitorScaleStatus>,
) {
    ui.add_space(2.0);
    match cat {
        SettingsCategory::General => {
            properties::panel::preferences_section(app, SettingsCategory::General.section_id(), ui);
            properties::panel::preferences_section(
                app,
                properties::panel::PREFERENCES_UPDATES_SECTION,
                ui,
            );
            update_status_row(ui, &mut app.session.updates);
        }
        SettingsCategory::Appearance => {
            properties::panel::preferences_section(
                app,
                SettingsCategory::Appearance.section_id(),
                ui,
            );
            let draft = &mut app.session.ui.settings_dialog.as_mut().unwrap().draft;
            ui_scale_row(ui, draft, monitor);
        }
        SettingsCategory::Processing => {
            properties::panel::preferences_section(
                app,
                SettingsCategory::Processing.section_id(),
                ui,
            );
        }
        SettingsCategory::Export => {
            properties::panel::preferences_section(app, SettingsCategory::Export.section_id(), ui);
        }
        SettingsCategory::Recent => {
            let draft = &mut app.session.ui.settings_dialog.as_mut().unwrap().draft;
            render_recent(ui, draft);
        }
    }
}

fn reset_preferences(app: &mut PlotxApp, monitor: Option<&MonitorScaleStatus>) {
    let target = app.app_target();
    let properties = catalog()
        .iter()
        .filter(|definition| {
            definition.scope_kind == ScopeKind::App
                && definition.access == PropertyAccess::ReadWrite
        })
        .map(|definition| definition.id)
        .collect::<Vec<_>>();
    match app.plan_property_resets(&properties, std::slice::from_ref(&target)) {
        Ok(commit) => {
            app.commit_property(commit);
        }
        Err(error) => {
            app.session.status = format!("Could not reset preferences: {error}");
            return;
        }
    }

    let defaults = Settings::default();
    let mut next = app.settings.clone();
    next.schema_version = defaults.schema_version;
    next.app_version = defaults.app_version;
    next.appearance.ui_scale = defaults.appearance.ui_scale;
    next.canvas_size.recent_presets = defaults.canvas_size.recent_presets;
    next.canvas_size.custom_presets = defaults.canvas_size.custom_presets;
    next.window = defaults.window;
    if let Some(monitor) = monitor {
        next.appearance.ui_scale.monitors.insert(
            monitor.key.clone(),
            plotx_core::settings::MonitorScale {
                auto: monitor.auto,
                user: None,
            },
        );
    }
    app.apply_settings(next);
    app.persist_settings();
    if let Some(dialog) = app.session.ui.settings_dialog.as_mut() {
        dialog.last_error = None;
    }
}

#[cfg(test)]
#[path = "settings_dialog_tests.rs"]
mod tests;
