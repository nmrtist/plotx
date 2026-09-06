use super::*;

fn fixture() -> (PlotxApp, egui::Context, EguiRect, Pos2) {
    let mut app = PlotxApp::default();
    let mut page = CanvasDocument::new("Source".into(), [100.0, 100.0]);
    let before = ObjectFrame::new(10.0, 10.0, 80.0, 60.0);
    let panel = page.create_panel("Panel".into(), before);
    app.doc.canvases.push(page);
    app.session.active_canvas = Some(0);
    app.session.board.world_center = [300.0, 0.0];
    app.session.board.zoom = 1.0;
    let rect = EguiRect::from_min_size(Pos2::ZERO, Vec2::new(1000.0, 800.0));
    let origin = BoardTransform::from_board(app.session.board, rect)
        .page_screen_rect(&app.doc.canvases[0])
        .min;
    app.session.ui.interaction = Interaction::Panel(PanelDrag {
        canvas: 0,
        panel,
        kind: ObjectDragKind::Move,
        before,
        others: vec![],
        children: vec![],
        start_pointer: [20.0, 20.0],
        start_pointer_screen: [origin.x + 20.0, origin.y + 20.0],
        active: false,
        detached_since: None,
    });
    (
        app,
        egui::Context::default(),
        rect,
        origin + Vec2::new(360.0, 20.0),
    )
}

fn frame(
    app: &mut PlotxApp,
    ctx: &egui::Context,
    rect: EguiRect,
    pointer: Option<Pos2>,
    now: f64,
    released: bool,
) {
    let _ = ctx.run_ui(
        egui::RawInput {
            time: Some(now),
            screen_rect: Some(rect),
            ..Default::default()
        },
        |ui| {
            handle_panel_drag(app, 0, rect, pointer, !released, released, ui.ctx());
        },
    );
}

#[test]
fn panel_detach_requires_full_half_second_and_release_and_undo_restores_origin() {
    let (mut app, ctx, rect, pointer) = fixture();
    let before = app.doc.canvases[0].panels.clone();
    frame(&mut app, &ctx, rect, Some(pointer), 1.0, false);
    frame(&mut app, &ctx, rect, Some(pointer), 1.499, false);
    assert!(matches!(&app.session.ui.interaction, Interaction::Panel(d) if !ready(d, 1.499)));
    frame(&mut app, &ctx, rect, Some(pointer), 1.5, false);
    assert_eq!(app.doc.canvases.len(), 1);
    assert!(matches!(&app.session.ui.interaction, Interaction::Panel(d) if ready(d, 1.5)));
    frame(&mut app, &ctx, rect, Some(pointer), 1.51, true);
    assert_eq!(app.doc.canvases.len(), 2);
    assert_eq!(app.doc.canvases[1].board_pos, [350.0, 10.0]);
    assert_eq!(app.doc.canvases[1].panels[0].id, before[0].id);
    app.undo();
    assert_eq!(app.doc.canvases.len(), 1);
    assert_eq!(app.doc.canvases[0].panels, before);
    assert!(!app.can_undo());
}

#[test]
fn panel_detach_resets_on_partial_overlap_or_pointer_loss_and_quick_release_moves_only() {
    let (mut app, ctx, rect, pointer) = fixture();
    frame(&mut app, &ctx, rect, Some(pointer), 1.0, false);
    frame(&mut app, &ctx, rect, Some(pointer), 1.5, false);
    let overlap = pointer - Vec2::new(100.0, 0.0);
    frame(&mut app, &ctx, rect, Some(overlap), 1.6, false);
    assert!(
        matches!(&app.session.ui.interaction, Interaction::Panel(d) if d.detached_since.is_none())
    );
    frame(&mut app, &ctx, rect, Some(pointer), 1.7, false);
    frame(&mut app, &ctx, rect, None, 2.3, false);
    assert!(
        matches!(&app.session.ui.interaction, Interaction::Panel(d) if d.detached_since.is_none())
    );
    frame(&mut app, &ctx, rect, Some(pointer), 2.4, false);
    frame(&mut app, &ctx, rect, Some(pointer), 2.899, true);
    assert_eq!(app.doc.canvases.len(), 1);
    assert_eq!(app.doc.canvases[0].panels[0].frame.x, 350.0);
}

#[test]
fn panel_detach_checks_all_canvas_bounds_not_only_pointer_and_escape_cancels() {
    let (mut app, ctx, rect, pointer) = fixture();
    let mut other = CanvasDocument::new("Other".into(), [20.0, 20.0]);
    other.board_pos = [420.0, 10.0];
    app.doc.canvases.push(other);
    frame(&mut app, &ctx, rect, Some(pointer), 1.0, false);
    assert!(
        matches!(&app.session.ui.interaction, Interaction::Panel(d) if d.detached_since.is_none())
    );
    app.doc.canvases.pop();
    frame(&mut app, &ctx, rect, Some(pointer), 1.1, false);
    frame(&mut app, &ctx, rect, Some(pointer), 1.7, false);
    app.cancel_interaction();
    assert!(matches!(app.session.ui.interaction, Interaction::Idle));
    assert_eq!(app.doc.canvases[0].panels[0].frame.x, 10.0);
    assert!(!app.can_undo());
}

#[test]
fn panel_detach_excludes_multi_selection_and_release_back_over_page() {
    let (mut app, ctx, rect, pointer) = fixture();
    frame(&mut app, &ctx, rect, Some(pointer), 1.0, false);
    if let Interaction::Panel(drag) = &mut app.session.ui.interaction {
        drag.others.push((PanelId::new(), drag.before));
    }
    frame(&mut app, &ctx, rect, Some(pointer), 1.6, false);
    assert!(
        matches!(&app.session.ui.interaction, Interaction::Panel(d) if d.detached_since.is_none())
    );
    if let Interaction::Panel(drag) = &mut app.session.ui.interaction {
        drag.others.clear();
    }
    frame(&mut app, &ctx, rect, Some(pointer), 1.7, false);
    frame(&mut app, &ctx, rect, Some(pointer), 2.3, false);
    frame(
        &mut app,
        &ctx,
        rect,
        Some(pointer - Vec2::new(340.0, 0.0)),
        2.4,
        true,
    );
    assert_eq!(app.doc.canvases.len(), 1);
    assert_eq!(app.doc.canvases[0].panels[0].frame.x, 10.0);
}
