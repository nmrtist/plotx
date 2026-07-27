//! Settings dialog rendering tests.

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
