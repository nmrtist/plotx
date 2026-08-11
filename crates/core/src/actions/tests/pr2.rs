use super::*;

#[test]
fn page_scope_aligns_panel_and_loose_content_as_siblings_with_undo_redo() {
    let mut app = sample_app();
    let loose = app.doc.canvases[0].objects[0].id;
    app.doc.canvases[0].objects[0].frame = ObjectFrame::new(35.0, 12.0, 20.0, 10.0);
    let panel = app.doc.canvases[0].create_panel(
        "Panel a".to_owned(),
        ObjectFrame::new(5.0, 30.0, 40.0, 25.0),
    );
    let canvas = app.doc.canvases[0].resource_id;
    app.set_hierarchical_paths(
        0,
        &[
            crate::state::SelectionPath::panel(canvas, panel),
            crate::state::SelectionPath::content(canvas, None, loose),
        ],
        false,
    )
    .unwrap();
    let original = (
        app.doc.canvases[0].panel(panel).unwrap().frame,
        app.doc.canvases[0].object(loose).unwrap().frame,
    );
    app.align_selected(crate::layout::Align::Left);
    assert_eq!(app.doc.canvases[0].panel(panel).unwrap().frame.x, 5.0);
    assert_eq!(app.doc.canvases[0].object(loose).unwrap().frame.x, 5.0);
    let aligned = (
        app.doc.canvases[0].panel(panel).unwrap().frame,
        app.doc.canvases[0].object(loose).unwrap().frame,
    );
    app.undo();
    assert_eq!(app.doc.canvases[0].panel(panel).unwrap().frame, original.0);
    assert_eq!(app.doc.canvases[0].object(loose).unwrap().frame, original.1);
    app.redo();
    assert_eq!(app.doc.canvases[0].panel(panel).unwrap().frame, aligned.0);
    assert_eq!(app.doc.canvases[0].object(loose).unwrap().frame, aligned.1);
}

#[test]
fn imported_asset_action_applies_undoes_and_redoes() {
    let mut app = sample_app();
    let id = crate::state::AssetId::new();
    let bytes = b"embedded image bytes".to_vec();
    let asset = crate::state::AssetRecord {
        id,
        sha256: plotx_io::image::sha256(&bytes),
        format: "png".to_owned(),
        pixel_size: [2, 3],
        bytes,
    };
    app.execute_action(Action::SetAsset {
        id,
        before: None,
        after: Some(asset),
    });
    assert!(app.doc.assets.contains_key(&id));
    app.undo();
    assert!(!app.doc.assets.contains_key(&id));
    app.redo();
    assert!(app.doc.assets.contains_key(&id));
}
