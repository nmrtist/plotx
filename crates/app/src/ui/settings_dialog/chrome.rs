//! Settings dialog chrome, navigation rail, theme, and footer.

use egui::{
    Align, Align2, CornerRadius, FontId, Layout, Response, RichText, Sense, Ui, pos2, vec2,
};
use egui_phosphor::regular as icon;
use plotx_core::settings::ThemeMode;
use plotx_core::state::{SettingsCategory, SettingsDialog};

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
    ctx.data_mut(|data| {
        data.insert_temp(egui::Id::new("plotx_applied_chrome_theme"), mode);
    });
}

pub(crate) fn sync_chrome_theme(ctx: &egui::Context, mode: ThemeMode) {
    let applied =
        ctx.data(|data| data.get_temp::<ThemeMode>(egui::Id::new("plotx_applied_chrome_theme")));
    if applied != Some(mode) {
        apply_chrome_theme(ctx, mode);
    }
}

pub(super) fn rail_row(ui: &mut Ui, cat: SettingsCategory, selected: bool) -> Response {
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

pub(super) fn footer(ui: &mut Ui, dialog: &SettingsDialog) -> (bool, bool) {
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
