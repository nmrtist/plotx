use egui::Ui;
use egui_phosphor::regular as icon;
use plotx_core::actions::Action;
use plotx_core::state::{ObjectId, PlotxApp};

pub(super) fn panel_note_section(
    app: &mut PlotxApp,
    ci: usize,
    object: ObjectId,
    ui: &mut Ui,
) -> bool {
    let Some((letter, panel)) = app.doc.canvases[ci]
        .object(object)
        .and_then(|o| o.plot())
        .map(|p| {
            (
                app.doc.canvases[ci]
                    .panel_letter(object)
                    .unwrap_or_else(|| "?".to_owned()),
                p.panel.clone(),
            )
        })
    else {
        return false;
    };

    ui.horizontal(|ui| {
        ui.strong("Panel note");
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.weak(letter);
        });
    });

    let mut buffer = panel.user_note.clone();
    let resp = ui.add(
        egui::TextEdit::multiline(&mut buffer)
            .desired_width(f32::INFINITY)
            .desired_rows(3)
            .hint_text("Shown in the notes below the page"),
    );
    if resp.gained_focus() {
        commit_panel_note_edit(app);
        app.session.ui.note_edit_before = Some((ci, object, panel.clone()));
    }
    if resp.changed()
        && let Some(plot) = app.doc.canvases[ci]
            .object_mut(object)
            .and_then(|o| o.plot_mut())
    {
        plot.panel.user_note = buffer;
        app.doc.dirty = true;
    }
    if resp.lost_focus() {
        commit_panel_note_edit(app);
    }

    ui.horizontal(|ui| {
        if ui
            .small_button(icon::PENCIL_SIMPLE)
            .on_hover_text("Edit in dialog")
            .clicked()
        {
            crate::ui::canvas::open_panel_note_editor(app, ci, object);
        }
        if ui
            .add_enabled(
                !panel.user_note.trim().is_empty(),
                egui::Button::new(icon::X).small(),
            )
            .on_hover_text("Clear note")
            .clicked()
        {
            let mut after = panel.clone();
            after.user_note.clear();
            app.execute_action(Action::set_panel_meta(ci, object, panel, after));
            app.session.status = "Panel note cleared.".to_owned();
        }
    });

    resp.has_focus()
}

pub(super) fn commit_panel_note_edit(app: &mut PlotxApp) {
    let Some((ci, id, before)) = app.session.ui.note_edit_before.take() else {
        return;
    };
    let Some(after) = app
        .doc
        .canvases
        .get(ci)
        .and_then(|c| c.object(id))
        .and_then(|o| o.plot())
        .map(|p| p.panel.clone())
    else {
        return;
    };
    app.execute_action(Action::set_panel_meta(ci, id, before, after));
}
