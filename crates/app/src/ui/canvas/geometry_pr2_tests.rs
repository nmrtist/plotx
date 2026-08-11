use super::*;
use plotx_core::state::TextBox;

#[test]
fn aspect_resize_preserves_ratio_and_opposite_corner() {
    let before = ObjectFrame::new(10.0, 20.0, 40.0, 20.0);
    let candidate = ObjectFrame::new(0.0, 5.0, 50.0, 35.0);
    let after = preserve_aspect_frame(
        before,
        candidate,
        ObjectDragKind::Resize(ResizeHandle::TopLeft),
    );
    assert_eq!(after, ObjectFrame::new(-20.0, 5.0, 70.0, 35.0));
}

#[test]
fn panel_hit_includes_empty_panels_and_resize_handles() {
    let mut canvas = CanvasDocument::new("page".to_owned(), [100.0, 80.0]);
    let panel = canvas.create_panel("Panel".to_owned(), ObjectFrame::new(10.0, 12.0, 40.0, 30.0));

    assert_eq!(
        hit_panel(&canvas, Pos2::new(20.0, 20.0), 1.0)
            .unwrap()
            .panel,
        panel
    );
    assert!(matches!(
        hit_panel(&canvas, Pos2::new(10.0, 12.0), 1.0).unwrap().kind,
        ObjectDragKind::Resize(ResizeHandle::TopLeft)
    ));
}

#[test]
fn content_hit_uses_page_position_inside_an_editing_panel() {
    let mut canvas = CanvasDocument::new("page".to_owned(), [100.0, 80.0]);
    let content = ObjectId::new(1);
    canvas.objects.push(CanvasObject {
        id: content,
        name: "content".to_owned(),
        frame: ObjectFrame::new(4.0, 5.0, 16.0, 12.0),
        locked: false,
        visible: true,
        kind: CanvasObjectKind::Text(TextBox::label("text".to_owned())),
    });
    let panel = canvas.create_panel("Panel".to_owned(), ObjectFrame::new(30.0, 20.0, 40.0, 30.0));
    canvas.panel_mut(panel).unwrap().item_order.push(content);

    assert_eq!(
        hit_content_object(&canvas, Pos2::new(36.0, 26.0), 1.0, Some(panel))
            .unwrap()
            .object,
        content
    );
    assert!(hit_content_object(&canvas, Pos2::new(6.0, 11.0), 1.0, Some(panel)).is_none());
}
