use super::*;
use crate::state::{CanvasDocument, ContentItem, ContentKind, TextBox};

fn page() -> (CanvasDocument, [ContentId; 2]) {
    let mut page = CanvasDocument::new("Figure".to_owned(), [100.0, 100.0]);
    let ids = [page.allocate_object_id(), page.allocate_object_id()];
    for (index, id) in ids.into_iter().enumerate() {
        page.objects.push(ContentItem {
            id,
            name: format!("item {index}"),
            frame: ObjectFrame::new(index as f32 * 20.0, 10.0, 10.0, 10.0),
            locked: false,
            visible: true,
            kind: ContentKind::Text(TextBox::label("x".to_owned())),
        });
    }
    (page, ids)
}

#[test]
fn rejects_multi_parent_duplicate_order_and_invalid_geometry() {
    let (mut page, ids) = page();
    let first = page.create_panel("a".to_owned(), ObjectFrame::new(0.0, 0.0, 50.0, 50.0));
    page.panel_mut(first).unwrap().item_order = vec![ids[0], ids[0]];
    assert!(page.validate_structure().unwrap_err().contains("repeats"));
    page.panel_mut(first).unwrap().item_order = vec![ids[0]];
    let second = page.create_panel("b".to_owned(), ObjectFrame::new(0.0, 0.0, 50.0, 50.0));
    page.panel_mut(second).unwrap().item_order = vec![ids[0]];
    assert!(
        page.validate_structure()
            .unwrap_err()
            .contains("both panel")
    );
    page.panel_mut(second).unwrap().item_order.clear();
    page.panels[0].frame.width = f32::NAN;
    assert!(page.validate_structure().unwrap_err().contains("finite"));
}

#[test]
fn manual_unicode_duplicates_warn_but_validate() {
    let (mut page, _) = page();
    for _ in 0..2 {
        let id = page.create_panel(
            "supplement".to_owned(),
            ObjectFrame::new(0.0, 0.0, 10.0, 10.0),
        );
        page.panel_mut(id).unwrap().label.mode = PanelLabelMode::Manual {
            value: "图 α".to_owned(),
        };
    }
    page.validate_structure().unwrap();
    assert_eq!(page.structure_warnings().len(), 1);
}

#[test]
fn groups_reject_cross_layer_mixing() {
    let (mut page, ids) = page();
    let panel = page.create_panel("a".to_owned(), ObjectFrame::new(0.0, 0.0, 10.0, 10.0));
    page.groups.push(LayoutGroup {
        id: 1,
        members: vec![GroupMember::Panel(panel), GroupMember::Content(ids[1])],
    });
    assert!(page.validate_structure().unwrap_err().contains("mixes"));
}

#[test]
fn groups_reject_multiple_membership_and_cross_scope_content() {
    let (mut page, ids) = page();
    page.groups = vec![
        LayoutGroup {
            id: 1,
            members: vec![GroupMember::Content(ids[0]), GroupMember::Content(ids[1])],
        },
        LayoutGroup {
            id: 2,
            members: vec![GroupMember::Content(ids[0]), GroupMember::Content(ids[1])],
        },
    ];
    assert!(
        page.validate_structure()
            .unwrap_err()
            .contains("both group")
    );

    page.groups.truncate(1);
    let panel = page.create_panel("a".to_owned(), ObjectFrame::new(0.0, 0.0, 20.0, 20.0));
    page.panel_mut(panel).unwrap().item_order.push(ids[0]);
    assert!(
        page.validate_structure()
            .unwrap_err()
            .contains("different scopes")
    );
}

#[test]
fn panel_visibility_and_labels_are_independent_of_content_kind() {
    let (mut page, ids) = page();
    let panel = page.create_panel("a".to_owned(), ObjectFrame::new(20.0, 30.0, 40.0, 40.0));
    page.panel_mut(panel).unwrap().item_order.push(ids[0]);
    page.panel_mut(panel).unwrap().visible = false;
    let items = crate::state::document_items(&page);
    assert!(matches!(&items[0], plotx_render::DocumentItem::Overlay(overlay) if !overlay.visible));
    assert!(
        matches!(&items[2], plotx_render::DocumentItem::PanelLabel { frame, visible: false, .. } if frame.left == 20.0 && frame.top == 30.0)
    );

    page.panel_mut(panel).unwrap().visible = true;
    page.panel_mut(panel).unwrap().item_order.clear();
    let items = crate::state::document_items(&page);
    assert!(matches!(
        items.last(),
        Some(plotx_render::DocumentItem::PanelLabel { visible: false, .. })
    ));

    let second = page.create_panel("b".to_owned(), ObjectFrame::new(70.0, 30.0, 40.0, 40.0));
    page.panel_mut(second).unwrap().item_order.push(ids[1]);
    let items = crate::state::document_items(&page);
    assert!(items.iter().rev().take(2).all(|item| matches!(
        item,
        plotx_render::DocumentItem::PanelLabel { visible: true, .. }
    )));
}

#[test]
fn layout_frame_keeps_panel_page_geometry_separate_from_local_content() {
    let (mut page, ids) = page();
    let panel = page.create_panel("a".to_owned(), ObjectFrame::new(50.0, 20.0, 40.0, 30.0));
    page.panel_mut(panel).unwrap().item_order.push(ids[0]);
    page.object_mut(ids[0]).unwrap().frame = ObjectFrame::new(0.0, 0.0, 40.0, 30.0);

    assert_eq!(page.layout_frame(ids[0]).unwrap().x, 50.0);
    page.set_layout_frame(ids[0], ObjectFrame::new(80.0, 60.0, 20.0, 15.0));

    assert_eq!(
        page.panel(panel).unwrap().frame,
        ObjectFrame::new(80.0, 60.0, 20.0, 15.0)
    );
    assert_eq!(
        page.object(ids[0]).unwrap().frame,
        ObjectFrame::new(0.0, 0.0, 20.0, 15.0)
    );
    assert_eq!(
        page.content_page_frame(ids[0]).unwrap(),
        ObjectFrame::new(80.0, 60.0, 20.0, 15.0)
    );
}

#[test]
fn panel_resize_scales_children_to_the_requested_frame() {
    let (mut page, ids) = page();
    let panel = page.create_panel("a".to_owned(), ObjectFrame::new(10.0, 20.0, 40.0, 20.0));
    page.panel_mut(panel).unwrap().item_order.push(ids[0]);
    page.object_mut(ids[0]).unwrap().frame = ObjectFrame::new(0.0, 0.0, 40.0, 20.0);
    page.set_layout_frame(ids[0], ObjectFrame::new(0.0, 10.0, 50.0, 30.0));
    assert_eq!(
        page.panel(panel).unwrap().frame,
        ObjectFrame::new(0.0, 10.0, 50.0, 30.0)
    );
    assert_eq!(
        page.object(ids[0]).unwrap().frame,
        ObjectFrame::new(0.0, 0.0, 50.0, 30.0)
    );
}
