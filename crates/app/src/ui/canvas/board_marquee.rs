use super::*;

pub(super) fn handle(app: &mut PlotxApp, rect: egui::Rect, ui: &Ui) -> bool {
    let (hover, pressed, down, released, shift, command) = ui.input(|input| {
        (
            input.pointer.hover_pos(),
            input.pointer.primary_pressed(),
            input.pointer.primary_down(),
            input.pointer.primary_released(),
            input.modifiers.shift,
            input.modifiers.command || input.modifiers.ctrl,
        )
    });
    if !matches!(app.session.ui.interaction, Interaction::BoardMarquee(_))
        && pressed
        && let Some(point) = hover
        && rect.contains(point)
        && object_at_screen(app, rect, point).is_none()
        && frame_at(app, rect, point).is_none()
        && frame_header_at(app, rect, point).is_none()
    {
        app.session.ui.selection_scope = plotx_core::state::SelectionScope::Board;
        if !shift && !command {
            app.session.ui.frame_selection.clear();
        }
        freeze_board_for_gesture(app);
        app.begin_interaction(Interaction::BoardMarquee(BoardMarqueeDrag {
            start: [point.x, point.y],
            current: [point.x, point.y],
            additive: shift,
            toggle: command,
        }));
    }
    let mut drag = match app.session.ui.interaction {
        Interaction::BoardMarquee(drag) => drag,
        _ => return false,
    };
    if down && let Some(point) = hover {
        drag.current = [point.x, point.y];
        app.session.ui.interaction = Interaction::BoardMarquee(drag);
    }
    let marquee = egui::Rect::from_two_pos(
        Pos2::new(drag.start[0], drag.start[1]),
        Pos2::new(drag.current[0], drag.current[1]),
    );
    ui.painter().rect_filled(
        marquee,
        0.0,
        ui.visuals().selection.bg_fill.gamma_multiply(0.15),
    );
    ui.painter().rect_stroke(
        marquee,
        0.0,
        ui.visuals().selection.stroke,
        StrokeKind::Inside,
    );
    if released || !down {
        let transform = BoardTransform::from_board(app.session.board, rect);
        let hits = board_frames(app)
            .into_iter()
            .filter(|frame| {
                frame_screen_rect(&transform, app, *frame)
                    .is_some_and(|frame_rect| marquee.intersects(frame_rect))
            })
            .filter_map(|frame| board_frame_id(app, frame))
            .collect::<Vec<_>>();
        if drag.toggle {
            for id in hits {
                if let Some(index) = app
                    .session
                    .ui
                    .frame_selection
                    .iter()
                    .position(|item| *item == id)
                {
                    app.session.ui.frame_selection.remove(index);
                } else {
                    app.session.ui.frame_selection.push(id);
                }
            }
        } else {
            if !drag.additive {
                app.session.ui.frame_selection.clear();
            }
            for id in hits {
                if !app.session.ui.frame_selection.contains(&id) {
                    app.session.ui.frame_selection.push(id);
                }
            }
        }
        plotx_core::state::sync_frame_selection_to_data(app);
        app.reset_interaction();
    }
    true
}
