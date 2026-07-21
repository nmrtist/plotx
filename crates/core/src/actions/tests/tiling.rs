use crate::actions::Action;
use crate::actions::tests::{push_canvas, sample_app};
use crate::layout::compute_tiling_plan;
use crate::state::ObjectFrame;

/// A drop of canvas 0's plot onto canvas 1 (which already has one plot) transfers
/// ownership and reframes both into a two-way split, undoably.
#[test]
fn tile_drop_transfers_reframes_and_round_trips() {
    let mut app = sample_app();
    push_canvas(&mut app, 0, "target", [120.0, 80.0]);
    app.session.active_canvas = Some(0);
    let newcomer = app.doc.canvases[0].objects[0].id;
    let existing = app.doc.canvases[1].objects[0].id;
    let src_before = app.doc.canvases[0].objects.len();
    let dst_before = app.doc.canvases[1].objects.len();

    let page = app.doc.canvases[1].size_pt();
    let layout = app.doc.canvases[1].layout;
    let ids = app.doc.canvases[1].plot_object_ids();
    // Pointer in the right region of the target page.
    let plan = compute_tiling_plan(page, &layout, &ids, [page[0] * 0.9, page[1] * 0.5]);
    let existing_after_frame = plan.existing[0].1;
    let action = Action::tile_drop(&app, 0, newcomer, 1, plan.newcomer, plan.existing).unwrap();
    app.execute_action(action);

    assert_eq!(app.doc.canvases[0].objects.len(), src_before - 1);
    assert_eq!(app.doc.canvases[1].objects.len(), dst_before + 1);
    assert_eq!(app.session.active_canvas, Some(1));
    let moved_id = app.doc.canvases[1].objects.last().unwrap().id;
    assert_eq!(app.session.ui.selection.object(), Some(moved_id));
    let ex = app.doc.canvases[1].object(existing).unwrap().frame;
    let nc = app.doc.canvases[1].objects.last().unwrap().frame;
    assert_eq!(ex, existing_after_frame);
    assert!(nc.x > ex.x);
    assert!(nc.x + nc.width <= page[0] + 0.5);

    app.undo();
    assert_eq!(app.doc.canvases[0].objects.len(), src_before);
    assert_eq!(app.doc.canvases[1].objects.len(), dst_before);
    assert_eq!(app.doc.canvases[0].objects[0].id, newcomer);
    assert_eq!(app.session.active_canvas, Some(0));
    assert_eq!(
        app.doc.canvases[1].object(existing).unwrap().frame,
        ObjectFrame::new(0.0, 0.0, page[0], page[1])
    );

    app.redo();
    assert_eq!(app.doc.canvases[0].objects.len(), src_before - 1);
    assert_eq!(app.doc.canvases[1].objects.len(), dst_before + 1);
    assert_eq!(app.session.active_canvas, Some(1));
    assert_eq!(app.doc.canvases[1].object(existing).unwrap().frame, ex);
}
