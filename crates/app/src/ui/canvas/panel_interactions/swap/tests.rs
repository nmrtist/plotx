use super::*;
use plotx_core::state::{CanvasObject, CanvasObjectKind, TextBox};

fn fixture() -> (PlotxApp, PanelDrag, PanelId, EguiRect, Pos2) {
    let mut app = PlotxApp::default();
    let mut page = CanvasDocument::new("Swap".into(), [200.0, 100.0]);
    let a = page.create_panel("A".into(), ObjectFrame::new(10.0, 10.0, 100.0, 80.0));
    let b = page.create_panel("B".into(), ObjectFrame::new(200.0, 10.0, 200.0, 160.0));
    for (panel, width) in [(a, 100.0), (b, 200.0)] {
        for x in [0.0, 20.0] {
            let id = page.allocate_object_id();
            page.objects.push(CanvasObject {
                id,
                name: "Text".into(),
                frame: ObjectFrame::new(x, 5.0, width / 2.0, 20.0),
                locked: false,
                visible: true,
                kind: CanvasObjectKind::Text(TextBox::label("Example".into())),
            });
            page.panel_mut(panel).unwrap().item_order.push(id);
        }
    }
    app.doc.canvases.push(page);
    app.session.active_canvas = Some(0);
    app.session.board.zoom = 1.0;
    let rect = EguiRect::from_min_size(Pos2::ZERO, Vec2::new(1000.0, 800.0));
    let page_rect =
        BoardTransform::from_board(app.session.board, rect).page_screen_rect(&app.doc.canvases[0]);
    let pointer = page_rect.min + Vec2::new(250.0, 50.0);
    app.session.tool = Tool::Select;
    begin_panel_drag(
        &mut app,
        0,
        a,
        ObjectDragKind::Move,
        Some(Pos2::new(50.0, 50.0)),
        page_rect.min + Vec2::new(50.0, 50.0),
        false,
    );
    let Interaction::Panel(mut drag) = app.take_interaction() else {
        panic!("panel gesture expected");
    };
    drag.active = true;
    (app, drag, b, rect, pointer)
}

#[test]
fn panel_swap_handles_first_movement_and_release_in_one_frame() {
    assert_swap_through_drag_handler(true);
}

#[test]
fn panel_swap_handles_movement_before_release() {
    assert_swap_through_drag_handler(false);
}

fn assert_swap_through_drag_handler(release_on_first_movement: bool) {
    let (mut app, drag, b, rect, pointer) = fixture();
    let source = drag.panel;
    let before = PanelState::of(&app.doc.canvases[0]);
    let ctx = egui::Context::default();
    let start = Pos2::new(drag.start_pointer_screen[0], drag.start_pointer_screen[1]);
    let button = |pos, pressed| egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    };
    let mut frame = |events| {
        let _ = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(rect),
                events,
                ..Default::default()
            },
            |ui| {
                let response = ui.interact(rect, egui::Id::new("swap"), egui::Sense::drag());
                handle_object_interactions(&mut app, 0, rect, ui, &response);
            },
        );
    };
    frame(vec![egui::Event::PointerMoved(start), button(start, true)]);
    if !release_on_first_movement {
        frame(vec![egui::Event::PointerMoved(pointer)]);
    }
    frame(vec![
        egui::Event::PointerMoved(pointer),
        button(pointer, false),
    ]);
    assert_eq!(
        app.doc.canvases[0].panel(source).unwrap().frame,
        before.panels[1].frame
    );
    assert_eq!(
        app.doc.canvases[0].panel(b).unwrap().frame,
        before.panels[0].frame
    );
    app.undo();
    assert_eq!(app.doc.canvases[0].panels, before.panels);
    assert!(!app.can_undo());
    app.redo();
    assert_eq!(
        app.doc.canvases[0].panel(source).unwrap().frame,
        before.panels[1].frame
    );
}

#[test]
fn panel_swap_release_scales_multiple_children_and_undo_restores_pre_drag() {
    let (mut app, drag, b, rect, pointer) = fixture();
    let before = PanelState::of(&app.doc.canvases[0]);
    app.doc.canvases[0].panel_mut(drag.panel).unwrap().frame.x = 210.0;
    assert_eq!(target(&app, &drag, rect, Some(pointer)), Some(b));
    commit(&mut app, &drag, b);
    let page = &app.doc.canvases[0];
    assert_eq!(
        page.panel(drag.panel).unwrap().frame,
        before.panels[1].frame
    );
    assert_eq!(page.panel(b).unwrap().frame, drag.before);
    assert_eq!(page.objects[0].frame.width, 100.0);
    assert_eq!(page.objects[2].frame.width, 50.0);
    page.validate_structure().unwrap();
    let after = PanelState::of(page);
    app.undo();
    assert_eq!(app.doc.canvases[0].panels, before.panels);
    assert_eq!(
        app.doc.canvases[0].objects[0].frame,
        before.objects[0].frame
    );
    assert!(!app.can_undo());
    app.redo();
    assert_eq!(app.doc.canvases[0].panels, after.panels);
    assert_eq!(app.doc.canvases.len(), 1);
}

#[test]
fn panel_swap_excludes_locked_hidden_self_inactive_and_multiple_selection() {
    let (mut app, mut drag, b, rect, pointer) = fixture();
    app.doc.canvases[0].panel_mut(b).unwrap().locked = true;
    assert_eq!(target(&app, &drag, rect, Some(pointer)), None);
    assert!(
        app.swap_panels_action(app.doc.canvases[0].resource_id, drag.panel, b)
            .is_err()
    );
    app.doc.canvases[0].panel_mut(b).unwrap().locked = false;
    app.doc.canvases[0].panel_mut(b).unwrap().visible = false;
    assert_eq!(target(&app, &drag, rect, Some(pointer)), None);
    app.doc.canvases[0].panel_mut(b).unwrap().visible = true;
    assert_eq!(target(&app, &drag, rect, None), None);
    let own = pointer - Vec2::new(200.0, 0.0);
    assert_eq!(target(&app, &drag, rect, Some(own)), None);
    drag.others
        .push((b, app.doc.canvases[0].panel(b).unwrap().frame));
    assert_eq!(target(&app, &drag, rect, Some(pointer)), None);
    drag.others.clear();
    drag.active = false;
    assert_eq!(target(&app, &drag, rect, Some(pointer)), None);
}
