use super::*;

#[test]
fn automated_exit_bypasses_dirty_project_prompt() {
    let mut app = PlotxApp::new_with_settings(plotx_core::settings::Settings::default());
    app.mark_document_dirty();
    let ctx = egui::Context::default();

    let output = ctx.run_ui(egui::RawInput::default(), |ui| {
        request_exit(&mut app, ui.ctx());
    });

    assert!(app.session.allow_close);
    let root = output
        .viewport_output
        .get(&egui::ViewportId::ROOT)
        .expect("root viewport output");
    assert!(root.commands.contains(&egui::ViewportCommand::Close));
}
