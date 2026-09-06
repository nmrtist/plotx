use super::*;
use crate::state::{
    CanvasObject, CanvasObjectKind, LayoutGroup, ObjectFrame, PanelLayout, TextBox,
};

#[test]
fn detach_panel_undo_restores_active_dataset_and_redo_retains_it() {
    let mut app = crate::actions::tests::sample_app();
    let object = app.doc.canvases[0].objects[0].id;
    let panel = app.doc.canvases[0].create_panel_for_plot(object).unwrap();
    let canvas = app.doc.canvases[0].resource_id;
    assert_eq!(app.active_dataset(), Some(0));
    app.try_execute_action(
        app.detach_panel_action(canvas, panel, [500.0, 100.0])
            .unwrap(),
    )
    .unwrap();
    for _ in 0..2 {
        assert_eq!(app.session.active_canvas, Some(1));
        assert_eq!(app.active_dataset(), Some(0));
        assert!(app.doc.canvases[1].object(object).is_some());
        app.undo();
        assert_eq!(app.session.active_canvas, Some(0));
        assert!(app.doc.canvases[0].panel(panel).is_some());
        assert!(app.doc.canvases[0].object(object).is_some());
        assert_eq!(app.active_dataset(), Some(0));
        assert!(!app.can_undo());
        app.redo();
    }
}

#[test]
fn detach_panel_preserves_explicit_origin_next_to_existing_canvas() {
    let mut app = PlotxApp::default();
    let mut page = CanvasDocument::new("Source".into(), [100.0, 100.0]);
    page.board_pos = [100.0, 0.0];
    let panel = page.create_panel("Panel".into(), ObjectFrame::new(0.0, 0.0, 90.0, 60.0));
    let canvas = page.resource_id;
    app.doc.canvases.push(page);
    app.session.active_canvas = Some(0);
    app.try_execute_action(app.detach_panel_action(canvas, panel, [0.0, 0.0]).unwrap())
        .unwrap();
    assert_eq!(app.doc.canvases[1].board_pos, [0.0, 0.0]);
    app.undo();
    app.redo();
    assert_eq!(app.doc.canvases[1].board_pos, [0.0, 0.0]);
}

#[test]
fn ordinary_canvas_insertion_still_auto_places_default_origin() {
    let mut app = PlotxApp::default();
    app.doc
        .canvases
        .push(CanvasDocument::new("Existing".into(), [100.0, 100.0]));
    let page = CanvasDocument::new("New".into(), [30.0, 20.0]);
    let expected = crate::state::next_board_frame_pos(&app, page.size_pt());
    assert_ne!(expected, [0.0, 0.0]);
    app.try_execute_action(Action::insert_canvas(1, page, Some(0)))
        .unwrap();
    assert_eq!(app.doc.canvases[1].board_pos, expected);
    app.undo();
    app.redo();
    assert_eq!(app.doc.canvases[1].board_pos, expected);
}

#[test]
fn detach_panel_preserves_contents_metadata_groups_and_round_trips() {
    let mut app = PlotxApp::default();
    let mut page = CanvasDocument::new("Source".into(), [100.0, 100.0]);
    let panel = page.create_panel(
        "Composite".into(),
        ObjectFrame::new(20.0, 30.0, 180.0, 120.0),
    );
    let ids: Vec<_> = (0..3).map(|_| page.allocate_object_id()).collect();
    for &id in &ids {
        page.objects.push(CanvasObject {
            id,
            name: "Text".into(),
            frame: ObjectFrame::new(5.0, 10.0, 30.0, 20.0),
            locked: false,
            visible: true,
            kind: CanvasObjectKind::Text(TextBox::label("Kept".into())),
        });
    }
    let moved = page.panel_mut(panel).unwrap();
    moved.item_order = ids[..2].to_vec();
    moved.note = "Panel note".into();
    moved.layout = PanelLayout::HorizontalStack;
    moved.clip_children = true;
    page.groups.push(LayoutGroup {
        id: 1,
        members: ids[..2]
            .iter()
            .map(|id| GroupMember::Content(*id))
            .collect(),
    });
    let before = page.clone();
    app.doc.canvases.push(page);
    app.session.active_canvas = Some(0);
    let action = app
        .detach_panel_action(before.resource_id, panel, [400.0, -200.0])
        .unwrap();
    app.try_execute_action(action).unwrap();
    assert_eq!(app.doc.canvases.len(), 2);
    assert_eq!(app.doc.canvases[0].objects.len(), 1);
    assert!(app.doc.canvases[0].panels.is_empty());
    assert!(app.doc.canvases[0].groups.is_empty());
    let target = &app.doc.canvases[1];
    let new_id = target.resource_id;
    assert_ne!(new_id, before.resource_id);
    assert_eq!(target.board_pos, [400.0, -200.0]);
    assert_eq!(target.objects.len(), 2);
    assert_eq!(target.objects[0].frame, before.objects[0].frame);
    assert_eq!(target.groups, before.groups);
    let mut expected = before.panel(panel).unwrap().clone();
    expected.frame.x = 0.0;
    expected.frame.y = 0.0;
    assert_eq!(target.panel(panel), Some(&expected));
    assert!((target.size_pt()[0] - 180.0).abs() < 0.001);
    for page in &app.doc.canvases {
        page.validate_structure().unwrap();
    }
    assert_eq!(app.session.active_canvas, Some(1));
    app.undo();
    assert_eq!(app.doc.canvases.len(), 1);
    assert_eq!(app.doc.canvases[0].panels, before.panels);
    assert_eq!(app.doc.canvases[0].groups, before.groups);
    assert_eq!(app.doc.canvases[0].objects.len(), 3);
    assert_eq!(app.session.active_canvas, Some(0));
    assert!(!app.can_undo());
    app.redo();
    assert_eq!(app.doc.canvases[1].resource_id, new_id);
    assert_eq!(app.doc.canvases[1].panel(panel), Some(&expected));
}

#[test]
fn detach_empty_panel_and_reject_invalid_or_locked_source() {
    let mut app = PlotxApp::default();
    let mut page = CanvasDocument::new("Source".into(), [100.0, 100.0]);
    let panel = page.create_panel("Empty".into(), ObjectFrame::new(0.0, 0.0, 90.0, 60.0));
    let id = page.resource_id;
    app.doc.canvases.push(page);
    assert!(app.detach_panel_action(id, panel, [f32::NAN, 0.0]).is_err());
    assert!(
        app.detach_panel_action(CanvasId::new(), panel, [1.0, 1.0])
            .is_err()
    );
    app.doc.canvases[0].panel_mut(panel).unwrap().locked = true;
    assert!(app.detach_panel_action(id, panel, [1.0, 1.0]).is_err());
    app.doc.canvases[0].panel_mut(panel).unwrap().locked = false;
    app.try_execute_action(app.detach_panel_action(id, panel, [1.0, 1.0]).unwrap())
        .unwrap();
    assert!(
        app.doc.canvases[1]
            .panel(panel)
            .unwrap()
            .item_order
            .is_empty()
    );
    app.undo();
    assert!(app.doc.canvases[0].panel(panel).is_some());
}
