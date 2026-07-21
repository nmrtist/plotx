use super::*;

pub(crate) fn handle_frame_caption_interactions(
    app: &mut PlotxApp,
    screen: egui::Rect,
    ui: &mut Ui,
) -> bool {
    let bt = BoardTransform::from_board(app.session.board, screen);
    let color = ui.visuals().text_color();
    let mut consumed = false;
    for ci in 0..app.doc.canvases.len() {
        let canvas = &app.doc.canvases[ci];
        if !canvas.caption_visible {
            continue;
        }
        let page = bt.page_screen_rect(canvas);
        let font = egui::FontId::proportional((11.0 * bt.zoom).clamp(7.0, 28.0));
        let mut y = page.bottom() + CAPTION_GAP_PX;
        if !canvas.caption.trim().is_empty() {
            let galley = ui.painter().layout(
                canvas.caption.clone(),
                font.clone(),
                color,
                page.width().max(1.0),
            );
            y += galley.size().y;
        }

        let entries = canvas.panel_note_entries();
        for (object_id, letter, note) in entries {
            let text = format!("{letter} - {note}");
            let galley = ui
                .painter()
                .layout(text, font.clone(), color, page.width().max(1.0));
            let row = egui::Rect::from_min_size(Pos2::new(page.left(), y), galley.size());
            y += galley.size().y;
            if !screen.intersects(row) {
                continue;
            }
            let resp = ui
                .interact(
                    row.expand(3.0),
                    ui.id().with(("panel_note_row", ci, object_id)),
                    Sense::click(),
                )
                .on_hover_text("Click to select. Double-click to edit this panel note.");
            if resp.hovered() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                consumed = true;
            }
            if resp.double_clicked() {
                app.session.active_canvas = Some(ci);
                app.select_object(ci, object_id);
                open_inline_panel_note_editor(app, ci, object_id);
                consumed = true;
            } else if resp.clicked() {
                app.session.active_canvas = Some(ci);
                app.select_object(ci, object_id);
                app.session.status =
                    "Panel selected. Double-click its note to edit in place.".to_owned();
                consumed = true;
            }
            resp.context_menu(|ui| {
                app.session.active_canvas = Some(ci);
                app.select_object(ci, object_id);
                if ui.button("Edit note in place").clicked() {
                    open_inline_panel_note_editor(app, ci, object_id);
                    ui.close();
                }
                if ui.button("Edit note in dialog").clicked() {
                    app.session.ui.panel_note_inline_edit = None;
                    open_panel_note_editor(app, ci, object_id);
                    ui.close();
                }
            });
        }
    }
    consumed
}

pub(crate) fn open_inline_panel_note_editor(app: &mut PlotxApp, ci: usize, object_id: ObjectId) {
    let Some(panel) = app
        .doc
        .canvases
        .get(ci)
        .and_then(|canvas| canvas.object(object_id))
        .and_then(|object| object.plot())
        .map(|plot| plot.panel.clone())
    else {
        return;
    };
    app.session.ui.panel_note_edit = None;
    app.session.ui.note_edit_before = Some((ci, object_id, panel.clone()));
    app.session.ui.panel_note_inline_edit = Some(PanelNoteEditState {
        canvas: ci,
        object: object_id,
        buffer: panel.user_note,
        focus: true,
    });
}

pub(crate) fn render_inline_panel_note_editor(app: &mut PlotxApp, screen: egui::Rect, ui: &mut Ui) {
    let Some(edit) = app.session.ui.panel_note_inline_edit.as_ref() else {
        return;
    };
    let ci = edit.canvas;
    let object_id = edit.object;
    let Some(row) = panel_note_row_rect(app, screen, ui, ci, object_id) else {
        cancel_inline_panel_note_edit(app);
        return;
    };

    let mut buffer = edit.buffer.clone();
    let focus = edit.focus;
    let desired_rows = buffer.lines().count().clamp(1, 4);
    let row = row.expand2(egui::vec2(4.0, 3.0));
    let resp = ui.put(
        row,
        egui::TextEdit::multiline(&mut buffer)
            .desired_width(row.width())
            .desired_rows(desired_rows),
    );
    if focus {
        resp.request_focus();
        if let Some(edit) = app.session.ui.panel_note_inline_edit.as_mut() {
            edit.focus = false;
        }
    }

    if resp.changed() {
        if let Some(edit) = app.session.ui.panel_note_inline_edit.as_mut() {
            edit.buffer.clone_from(&buffer);
        }
        if let Some(plot) = app.doc.canvases[ci]
            .object_mut(object_id)
            .and_then(|object| object.plot_mut())
        {
            plot.panel.user_note = buffer.clone();
            app.doc.dirty = true;
        }
    }

    let (escape, commit_key) = ui.input(|i| {
        (
            i.key_pressed(egui::Key::Escape),
            i.key_pressed(egui::Key::Enter) && !i.modifiers.shift,
        )
    });
    if resp.has_focus() && escape {
        cancel_inline_panel_note_edit(app);
    } else if (resp.has_focus() && commit_key) || resp.lost_focus() {
        commit_inline_panel_note_edit(app);
    }
}

fn panel_note_row_rect(
    app: &PlotxApp,
    screen: egui::Rect,
    ui: &Ui,
    ci: usize,
    object_id: ObjectId,
) -> Option<egui::Rect> {
    let canvas = app.doc.canvases.get(ci)?;
    if !canvas.caption_visible {
        return None;
    }
    let bt = BoardTransform::from_board(app.session.board, screen);
    let page = bt.page_screen_rect(canvas);
    let color = ui.visuals().text_color();
    let font = egui::FontId::proportional((11.0 * bt.zoom).clamp(7.0, 28.0));
    let mut y = page.bottom() + CAPTION_GAP_PX;
    if !canvas.caption.trim().is_empty() {
        let galley = ui.painter().layout(
            canvas.caption.clone(),
            font.clone(),
            color,
            page.width().max(1.0),
        );
        y += galley.size().y;
    }

    for (id, letter, note) in canvas.panel_note_entries() {
        let text = format!("{letter} - {note}");
        let galley = ui
            .painter()
            .layout(text, font.clone(), color, page.width().max(1.0));
        let row = egui::Rect::from_min_size(Pos2::new(page.left(), y), galley.size());
        if id == object_id {
            return Some(row);
        }
        y += galley.size().y;
    }
    None
}

fn commit_inline_panel_note_edit(app: &mut PlotxApp) {
    app.session.ui.panel_note_inline_edit = None;
    let Some((ci, id, before)) = app.session.ui.note_edit_before.take() else {
        return;
    };
    let Some(after) = app
        .doc
        .canvases
        .get(ci)
        .and_then(|canvas| canvas.object(id))
        .and_then(|object| object.plot())
        .map(|plot| plot.panel.clone())
    else {
        return;
    };
    app.execute_action(Action::set_panel_meta(ci, id, before, after));
    app.session.status = "Panel note updated.".to_owned();
}

fn cancel_inline_panel_note_edit(app: &mut PlotxApp) {
    app.session.ui.panel_note_inline_edit = None;
    let Some((ci, id, before)) = app.session.ui.note_edit_before.take() else {
        return;
    };
    if let Some(plot) = app
        .doc
        .canvases
        .get_mut(ci)
        .and_then(|canvas| canvas.object_mut(id))
        .and_then(|object| object.plot_mut())
    {
        plot.panel = before;
    }
    app.session.status = "Panel note edit cancelled.".to_owned();
}
