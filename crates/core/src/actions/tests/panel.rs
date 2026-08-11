use crate::state::{
    CanvasDocument, ContentId, ContentItem, ContentKind, GroupMember, LayoutGroup, ObjectFrame,
    PanelLabelMode, PlotxApp, TextBox,
};

fn app() -> (PlotxApp, [ContentId; 3]) {
    let mut app = PlotxApp::default();
    let mut page = CanvasDocument::new("Figure".to_owned(), [100.0, 100.0]);
    let ids = [
        page.allocate_object_id(),
        page.allocate_object_id(),
        page.allocate_object_id(),
    ];
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
    app.doc.canvases.push(page);
    (app, ids)
}

fn signature(app: &PlotxApp) -> String {
    let page = &app.doc.canvases[0];
    format!(
        "{:?}|{:?}|{:?}",
        page.panels
            .iter()
            .map(|p| (p.id, p.frame, p.item_order.clone(), p.label.clone()))
            .collect::<Vec<_>>(),
        page.objects
            .iter()
            .map(|i| (i.id, i.frame))
            .collect::<Vec<_>>(),
        page.groups
    )
}

fn cycle(app: &mut PlotxApp, action: crate::actions::Action) {
    let before = signature(app);
    app.try_execute_action(action).unwrap();
    let after = signature(app);
    assert_ne!(before, after);
    app.undo();
    assert_eq!(signature(app), before);
    app.redo();
    assert_eq!(signature(app), after);
}

#[test]
fn create_compose_dissolve_delete_round_trip() {
    let (mut app, ids) = app();
    let (_, action) = app
        .create_panel_action(
            0,
            "empty".to_owned(),
            ObjectFrame::new(0.0, 0.0, 20.0, 20.0),
        )
        .unwrap();
    cycle(&mut app, action);
    let (_, action) = app
        .compose_panel_action(0, "a".to_owned(), &ids[..2], 2.0)
        .unwrap();
    cycle(&mut app, action);
    let panel = app.doc.canvases[0].parent_panel(ids[0]).unwrap();
    let action = app.dissolve_panel_action(0, panel).unwrap();
    cycle(&mut app, action);
    let (_, action) = app
        .compose_panel_action(0, "b".to_owned(), &ids[..2], 0.0)
        .unwrap();
    app.try_execute_action(action).unwrap();
    let panel = app.doc.canvases[0].parent_panel(ids[0]).unwrap();
    let action = app.delete_panel_action(0, panel).unwrap();
    cycle(&mut app, action);
}

#[test]
fn move_split_merge_and_duplicate_round_trip() {
    let (mut app, ids) = app();
    let (a, action) = app
        .compose_panel_action(0, "a".to_owned(), &ids[..2], 0.0)
        .unwrap();
    app.try_execute_action(action).unwrap();
    let (b, action) = app
        .create_panel_action(0, "b".to_owned(), ObjectFrame::new(60.0, 0.0, 30.0, 30.0))
        .unwrap();
    app.try_execute_action(action).unwrap();
    let action = app
        .move_content_to_panel_action(0, ids[1], Some(b), 0)
        .unwrap();
    cycle(&mut app, action);
    let (split, action) = app
        .split_panel_action(0, a, &[ids[0]], "split".to_owned())
        .unwrap();
    cycle(&mut app, action);
    let action = app.merge_panels_action(0, a, &[split]).unwrap();
    cycle(&mut app, action);
    let (_, action) = app.duplicate_panel_action(0, a, [5.0, 5.0]).unwrap();
    cycle(&mut app, action);
    let action = app.reorder_panel_labels_action(0).unwrap();
    cycle(&mut app, action);
}

#[test]
fn label_edit_round_trips_without_plot_owned_state() {
    let (mut app, ids) = app();
    let (panel, action) = app
        .compose_panel_action(0, "a".to_owned(), &ids[..1], 0.0)
        .unwrap();
    app.try_execute_action(action).unwrap();
    let before = app.doc.canvases[0].panel_meta_for_content(ids[0]).unwrap();
    let mut after = before.clone();
    after.user_note = "edited".to_owned();
    after.position = [4.0, 5.0];
    after.visible = false;
    app.execute_action(crate::actions::Action::set_panel_meta(
        0,
        ids[0],
        before.clone(),
        after.clone(),
    ));
    assert_eq!(
        app.doc.canvases[0].panel_meta_for_content(ids[0]).unwrap(),
        after
    );
    app.undo();
    assert_eq!(
        app.doc.canvases[0].panel_meta_for_content(ids[0]).unwrap(),
        before
    );
    app.redo();
    assert_eq!(
        app.doc.canvases[0].panel_meta_for_content(ids[0]).unwrap(),
        after
    );
    let _ = panel;
}

#[test]
fn renumber_reserves_positions_for_locked_and_manual_labels() {
    let (mut app, _) = app();
    let mut panel_ids = Vec::new();
    for x in [0.0, 20.0, 40.0] {
        let (id, action) = app
            .create_panel_action(0, "p".to_owned(), ObjectFrame::new(x, 0.0, 10.0, 10.0))
            .unwrap();
        app.try_execute_action(action).unwrap();
        panel_ids.push(id);
    }
    app.doc.canvases[0]
        .panel_mut(panel_ids[0])
        .unwrap()
        .label
        .mode = PanelLabelMode::LockedAuto {
        value: "a".to_owned(),
    };
    app.doc.canvases[0]
        .panel_mut(panel_ids[1])
        .unwrap()
        .label
        .mode = PanelLabelMode::Manual {
        value: "custom".to_owned(),
    };
    let action = app.reorder_panel_labels_action(0).unwrap();
    app.try_execute_action(action).unwrap();
    assert!(matches!(
        app.doc.canvases[0].panel(panel_ids[2]).unwrap().label.mode,
        PanelLabelMode::Auto { slot: 2 }
    ));
}

#[test]
fn merging_grouped_panels_rewrites_and_dissolves_groups() {
    let (mut app, _) = app();
    let mut ids = Vec::new();
    for x in [0.0, 20.0, 40.0] {
        let (id, action) = app
            .create_panel_action(0, "p".to_owned(), ObjectFrame::new(x, 0.0, 10.0, 10.0))
            .unwrap();
        app.try_execute_action(action).unwrap();
        ids.push(id);
    }
    app.doc.canvases[0].groups.push(LayoutGroup {
        id: 1,
        members: vec![GroupMember::Panel(ids[1]), GroupMember::Panel(ids[2])],
    });
    let action = app.merge_panels_action(0, ids[0], &[ids[1]]).unwrap();
    cycle(&mut app, action);
    let members = &app.doc.canvases[0].groups[0].members;
    assert_eq!(members.len(), 2);
    assert!(members.contains(&GroupMember::Panel(ids[0])));
    assert!(members.contains(&GroupMember::Panel(ids[2])));
}

#[test]
fn raster_edit_round_trips_through_undo_and_redo() {
    let (mut app, ids) = app();
    let asset = crate::state::AssetId::new();
    let bytes = b"raster-edit-fixture".to_vec();
    app.doc.assets.insert(
        asset,
        crate::state::AssetRecord {
            id: asset,
            sha256: plotx_io::image::sha256(&bytes),
            format: "png".to_owned(),
            pixel_size: [2, 2],
            bytes,
        },
    );
    let before = crate::state::RasterImageContent::new(asset);
    app.doc.canvases[0].object_mut(ids[0]).unwrap().kind = ContentKind::RasterImage(before.clone());
    let mut after = before.clone();
    after.crop = [0.1, 0.2, 0.9, 0.8];
    after.rotation = crate::state::QuarterTurn::Clockwise90;
    after.opacity = 0.4;
    after.interpolation = crate::state::ImageInterpolation::Nearest;
    app.try_execute_action(crate::actions::Action::SetRasterImage {
        canvas: 0,
        object: ids[0],
        before: before.clone(),
        after: after.clone(),
    })
    .unwrap();
    let image = |app: &PlotxApp| match &app.doc.canvases[0].object(ids[0]).unwrap().kind {
        ContentKind::RasterImage(image) => image.clone(),
        _ => panic!("expected raster image"),
    };
    assert_eq!(image(&app), after);
    app.undo();
    assert_eq!(image(&app), before);
    app.redo();
    assert_eq!(image(&app), after);
}

#[test]
fn raster_edit_rejects_invalid_values_and_wrong_content_kind() {
    let (mut app, ids) = app();
    let asset = crate::state::AssetId::new();
    let bytes = b"raster-validation-fixture".to_vec();
    app.doc.assets.insert(
        asset,
        crate::state::AssetRecord {
            id: asset,
            sha256: plotx_io::image::sha256(&bytes),
            format: "png".to_owned(),
            pixel_size: [2, 2],
            bytes,
        },
    );
    let before = crate::state::RasterImageContent::new(asset);
    let mut invalid = before.clone();
    invalid.opacity = f32::NAN;
    assert!(
        app.try_execute_action(crate::actions::Action::SetRasterImage {
            canvas: 0,
            object: ids[0],
            before: before.clone(),
            after: invalid,
        })
        .is_err()
    );

    app.doc.canvases[0].object_mut(ids[0]).unwrap().kind = ContentKind::RasterImage(before.clone());
    let mut invalid_crop = before.clone();
    invalid_crop.crop = [0.8, 0.0, 0.2, 1.0];
    assert!(
        app.try_execute_action(crate::actions::Action::SetRasterImage {
            canvas: 0,
            object: ids[0],
            before,
            after: invalid_crop,
        })
        .is_err()
    );
}

#[test]
fn deterministic_panel_layout_round_trips_frames_and_parameters() {
    let (mut app, ids) = app();
    let (panel, action) = app
        .compose_panel_action(0, "stack".to_owned(), &ids[..2], 2.0)
        .unwrap();
    app.try_execute_action(action).unwrap();
    let action = app
        .set_panel_layout_action(
            0,
            panel,
            crate::state::PanelLayout::VerticalStack,
            3.0,
            2.0,
            crate::state::PanelAlignment::Stretch,
        )
        .unwrap();
    cycle(&mut app, action);
    let page = &app.doc.canvases[0];
    let panel = page.panel(panel).unwrap();
    assert_eq!(panel.layout_gap, 3.0);
    assert_eq!(panel.layout_padding, 2.0);
    let first = page.object(ids[0]).unwrap().frame;
    let second = page.object(ids[1]).unwrap().frame;
    assert_eq!(first.x, 2.0);
    assert_eq!(second.x, 2.0);
    assert!(second.y > first.y + first.height);
}

#[test]
fn moving_one_content_into_a_stacked_panel_reflows_and_round_trips() {
    let (mut app, ids) = app();
    let (panel, action) = app
        .compose_panel_action(0, "stack".to_owned(), &ids[..2], 2.0)
        .unwrap();
    app.try_execute_action(action).unwrap();
    let action = app
        .set_panel_layout_action(
            0,
            panel,
            crate::state::PanelLayout::VerticalStack,
            3.0,
            2.0,
            crate::state::PanelAlignment::Stretch,
        )
        .unwrap();
    app.try_execute_action(action).unwrap();

    let before = signature(&app);
    let action = app
        .move_content_to_panel_action(0, ids[2], Some(panel), usize::MAX)
        .unwrap();
    app.try_execute_action(action).unwrap();

    let page = &app.doc.canvases[0];
    let panel_state = page.panel(panel).unwrap();
    assert_eq!(panel_state.item_order, ids);
    let frames: Vec<_> = ids
        .iter()
        .map(|id| page.object(*id).unwrap().frame)
        .collect();
    assert!(
        frames
            .windows(2)
            .all(|pair| pair[1].y > pair[0].y + pair[0].height)
    );

    app.undo();
    assert_eq!(signature(&app), before);
    app.redo();
    assert_eq!(app.doc.canvases[0].panel(panel).unwrap().item_order, ids);
}

#[test]
fn panel_z_order_moves_its_content_block_and_round_trips() {
    let (mut app, ids) = app();
    app.session.active_canvas = Some(0);
    let (first, action) = app
        .compose_panel_action(0, "first".to_owned(), &ids[..1], 2.0)
        .unwrap();
    app.try_execute_action(action).unwrap();
    let (_second, action) = app
        .compose_panel_action(0, "second".to_owned(), &ids[1..2], 2.0)
        .unwrap();
    app.try_execute_action(action).unwrap();
    let before = signature(&app);

    app.select_panel(0, first);
    app.z_order_selected(crate::actions::ZOrder::Front);
    let object_order: Vec<_> = app.doc.canvases[0]
        .objects
        .iter()
        .map(|object| object.id)
        .collect();
    assert!(
        object_order.iter().position(|id| *id == ids[0]).unwrap()
            > object_order.iter().position(|id| *id == ids[1]).unwrap()
    );
    assert_eq!(app.doc.canvases[0].panels.last().unwrap().id, first);

    app.undo();
    assert_eq!(signature(&app), before);
    app.redo();
    assert!(
        app.doc.canvases[0]
            .objects
            .iter()
            .position(|object| object.id == ids[0])
            .unwrap()
            > app.doc.canvases[0]
                .objects
                .iter()
                .position(|object| object.id == ids[1])
                .unwrap()
    );
}
