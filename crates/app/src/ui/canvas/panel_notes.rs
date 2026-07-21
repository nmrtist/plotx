use super::*;

pub(crate) fn handle_panel_label_interactions(
    app: &mut PlotxApp,
    ci: usize,
    rect: egui::Rect,
    ui: &mut Ui,
) -> bool {
    if !app.session.tool.is_layout_tool() {
        return false;
    }
    let Some(object_id) = app.doc.canvases[ci].selected_object else {
        return false;
    };
    let Some(label_rect) =
        panel_label_screen_rect(app.session.board, &app.doc.canvases[ci], object_id, rect)
    else {
        return false;
    };

    let (hover, primary_down, primary_pressed, primary_released) = ui.input(|i| {
        (
            i.pointer.hover_pos(),
            i.pointer.primary_down(),
            i.pointer.primary_pressed(),
            i.pointer.primary_released(),
        )
    });
    let label_hovered = hover
        .map(|pointer| label_rect.expand(2.0).contains(pointer))
        .unwrap_or(false);
    let id = ui.id().with(("panel_label", ci, object_id));
    let resp = ui
        .interact(label_rect, id, Sense::click_and_drag())
        .on_hover_text("Double-click to edit this panel note");
    let mut consumed =
        label_hovered || resp.hovered() || matches!(app.interaction(), Interaction::PanelLabel(_));

    if primary_pressed && label_hovered {
        app.select_panel_label(ci, object_id);
        if matches!(app.interaction(), Interaction::Object(_)) {
            app.reset_interaction();
        }
        app.session.status = "Panel letter selected. Double-click to edit its note.".to_owned();
        if let (Some(pointer), Some(panel)) = (
            hover,
            app.doc.canvases[ci]
                .object(object_id)
                .and_then(|object| object.plot())
                .map(|plot| plot.panel.clone()),
        ) {
            freeze_board_for_gesture(app);
            app.begin_interaction(Interaction::PanelLabel(PanelLabelDrag {
                canvas: ci,
                object: object_id,
                before: panel,
                start_pointer: [pointer.x, pointer.y],
            }));
        }
        consumed = true;
    }

    if resp.double_clicked() {
        open_panel_note_editor(app, ci, object_id);
        consumed = true;
    }

    resp.context_menu(|ui| {
        app.select_panel_label(ci, object_id);
        if ui.button("Edit panel note").clicked() {
            open_panel_note_editor(app, ci, object_id);
            ui.close();
        }
        if ui.button("Hide panel letter").clicked() {
            hide_panel_label(app, ci, object_id);
            ui.close();
        }
    });

    let label_drag = match &app.session.ui.interaction {
        Interaction::PanelLabel(d) if d.canvas == ci && d.object == object_id => Some(d.clone()),
        _ => None,
    };
    if let Some(drag) = label_drag {
        if primary_down {
            let zoom = app.session.board.zoom.max(0.01);
            let max_x = panel_label_max_x(app, ci, object_id);
            let max_y = panel_label_max_y(app, ci, object_id);
            if let Some(panel) = app.doc.canvases[ci]
                .object_mut(object_id)
                .and_then(|object| object.plot_mut())
                .map(|plot| &mut plot.panel)
                && let Some(pointer) = hover
            {
                let delta =
                    (pointer - Pos2::new(drag.start_pointer[0], drag.start_pointer[1])) / zoom;
                panel.position = [
                    (drag.before.position[0] + delta.x).clamp(0.0, max_x),
                    (drag.before.position[1] + delta.y).clamp(0.0, max_y),
                ];
                app.doc.dirty = true;
            }
        }
        if primary_released || !primary_down {
            finish_panel_label_drag(app, ci, object_id);
        }
        consumed = true;
    }

    consumed
}

pub(crate) fn panel_label_max_x(app: &PlotxApp, ci: usize, object_id: ObjectId) -> f32 {
    app.doc.canvases[ci]
        .object(object_id)
        .map(|object| object.frame.width)
        .unwrap_or(1.0)
}

pub(crate) fn panel_label_max_y(app: &PlotxApp, ci: usize, object_id: ObjectId) -> f32 {
    app.doc.canvases[ci]
        .object(object_id)
        .map(|object| object.frame.height)
        .unwrap_or(1.0)
}

pub(crate) fn open_panel_note_editor(app: &mut PlotxApp, ci: usize, object_id: ObjectId) {
    let Some(panel) = app.doc.canvases[ci]
        .object(object_id)
        .and_then(|object| object.plot())
        .map(|plot| plot.panel.clone())
    else {
        return;
    };
    app.select_panel_label(ci, object_id);
    app.session.ui.panel_note_inline_edit = None;
    app.session.ui.panel_note_edit = Some(PanelNoteEditState {
        canvas: ci,
        object: object_id,
        buffer: panel.user_note,
        focus: true,
    });
}

pub(crate) fn hide_panel_label(app: &mut PlotxApp, ci: usize, object_id: ObjectId) {
    let Some(before) = app.doc.canvases[ci]
        .object(object_id)
        .and_then(|object| object.plot())
        .map(|plot| plot.panel.clone())
    else {
        return;
    };
    let mut after = before.clone();
    after.visible = false;
    app.execute_action(Action::set_panel_meta(ci, object_id, before, after));
    app.select_object(ci, object_id);
    app.session.ui.panel_note_edit = None;
    app.session.status = "Panel letter hidden.".to_owned();
}

pub(crate) fn finish_panel_label_drag(app: &mut PlotxApp, ci: usize, object_id: ObjectId) {
    if !matches!(app.interaction(), Interaction::PanelLabel(_)) {
        return;
    }
    let Interaction::PanelLabel(drag) = app.take_interaction() else {
        return;
    };
    if drag.canvas != ci || drag.object != object_id {
        return;
    }
    let Some(after) = app.doc.canvases[ci]
        .object(object_id)
        .and_then(|object| object.plot())
        .map(|plot| plot.panel.clone())
    else {
        return;
    };
    app.execute_action(Action::set_panel_meta(ci, object_id, drag.before, after));
}

pub(crate) fn panel_label_screen_rect(
    board: BoardViewport,
    canvas: &CanvasDocument,
    object_id: ObjectId,
    screen: egui::Rect,
) -> Option<egui::Rect> {
    let frame = object_screen_rect(board, canvas, object_id, screen)?;
    let object = canvas.object(object_id)?;
    let panel = &object.plot()?.panel;
    if !panel.visible {
        return None;
    }
    let letter = canvas.panel_letter(object_id)?;
    let zoom = board.zoom;
    let font_size = (panel.font_size * zoom).max(6.0);
    let max_chars = letter.chars().count().max(1) as f32;
    let width = (max_chars * font_size * 0.62).max(14.0) + PANEL_LABEL_HIT_PAD_PX * 2.0;
    let height = font_size * 1.25 + PANEL_LABEL_HIT_PAD_PX * 2.0;
    let top_left = Pos2::new(
        frame.left + panel.position[0] * zoom - PANEL_LABEL_HIT_PAD_PX,
        frame.top + panel.position[1] * zoom - PANEL_LABEL_HIT_PAD_PX,
    );
    Some(egui::Rect::from_min_size(
        top_left,
        Vec2::new(width, height),
    ))
}
