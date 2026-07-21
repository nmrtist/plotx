use super::{sample_app, table_app};
use crate::actions::Action;
use crate::state::FrameRef;
use crate::state::page_frame_showing_dataset;

#[test]
fn new_table_dataset_adds_placed_starter_table() {
    let mut app = sample_app();
    let before = app.doc.datasets.len();
    app.new_table_dataset();

    assert_eq!(app.doc.datasets.len(), before + 1);
    let di = app.doc.datasets.len() - 1;
    let t = app.doc.datasets[di].as_table().expect("a table dataset");
    assert_eq!(t.typed_state.envelope.revision.snapshot.row_count, 3);
    assert_eq!(t.series_bindings.len(), 1);
    assert_eq!(app.active_dataset(), Some(di));
    assert_eq!(app.session.ui.sheet_open, Some(di));
    assert_eq!(app.session.ui.frame_selection, vec![FrameRef::Sheet(di)]);
    // Placed off the origin so it does not land on a page at [0, 0].
    assert_ne!(t.board_pos, [0.0, 0.0]);
}

#[test]
fn page_frame_showing_dataset_finds_charting_page() {
    // table_app charts dataset 0 (a table) as a plot object on canvas 0.
    let (app, _) = table_app();
    assert_eq!(page_frame_showing_dataset(&app, 0), Some(FrameRef::Page(0)));
    assert_eq!(page_frame_showing_dataset(&app, 5), None);
}

#[test]
fn move_canvas_on_board_applies_reverts_and_noops() {
    let mut app = sample_app();
    let before = app.doc.canvases[0].board_pos;
    let after = [720.0, 360.0];

    app.execute_action(Action::move_canvas_on_board(0, before, after));
    assert_eq!(app.doc.canvases[0].board_pos, after);
    app.undo();
    assert_eq!(app.doc.canvases[0].board_pos, before);
    app.redo();
    assert_eq!(app.doc.canvases[0].board_pos, after);
    // An unchanged position is a no-op: no new undo entry.
    app.execute_action(Action::move_canvas_on_board(0, after, after));
    assert_eq!(app.session.undo_stack.len(), 1);
}

#[test]
fn tidy_board_repacks_frames_and_is_undoable() {
    let mut app = sample_app();
    let mut extra = crate::state::CanvasDocument::new("far".to_owned(), [120.0, 80.0]);
    extra.board_pos = [2000.0, 1500.0];
    app.doc.canvases.push(extra);
    let before = [app.doc.canvases[0].board_pos, app.doc.canvases[1].board_pos];

    app.tidy_board();
    assert_eq!(app.doc.canvases[0].board_pos, [0.0, 0.0]);
    let r0 = app.doc.canvases[0].board_rect_pt();
    assert_eq!(app.doc.canvases[1].board_pos[1], 0.0);
    assert!(app.doc.canvases[1].board_pos[0] > r0.right());

    app.undo();
    assert_eq!(app.doc.canvases[0].board_pos, before[0]);
    assert_eq!(app.doc.canvases[1].board_pos, before[1]);
}

#[test]
fn move_sheet_on_board_applies_reverts_and_noops() {
    let (mut app, _) = table_app();
    let before = app.doc.datasets[0].as_table().unwrap().board_pos;
    let after = [3240.0, 720.0];

    app.execute_action(Action::move_sheet_on_board(0, before, after));
    assert_eq!(app.doc.datasets[0].as_table().unwrap().board_pos, after);
    app.undo();
    assert_eq!(app.doc.datasets[0].as_table().unwrap().board_pos, before);
    app.redo();
    assert_eq!(app.doc.datasets[0].as_table().unwrap().board_pos, after);
    // An unchanged position is a no-op: no new undo entry.
    let len = app.session.undo_stack.len();
    app.execute_action(Action::move_sheet_on_board(0, after, after));
    assert_eq!(app.session.undo_stack.len(), len);
}

#[test]
fn layer_flag_toggles_apply_revert_and_noop() {
    let (mut app, object) = super::table_app();
    let flags = |app: &crate::state::PlotxApp| {
        let object = app.doc.canvases[0].object(object).expect("charted object");
        (object.visible, object.locked)
    };
    assert_eq!(flags(&app), (true, false));

    app.execute_action(Action::set_object_flags(
        0,
        object,
        (true, false),
        (false, false),
    ));
    assert_eq!(flags(&app), (false, false));
    app.undo();
    assert_eq!(flags(&app), (true, false));
    app.redo();
    assert_eq!(flags(&app), (false, false));

    // Unchanged flags are a no-op: no new undo entry.
    let depth = app.session.undo_stack.len();
    app.execute_action(Action::set_object_flags(
        0,
        object,
        (false, false),
        (false, false),
    ));
    assert_eq!(app.session.undo_stack.len(), depth);
}

#[test]
fn board_view_bookmarks_are_undoable() {
    let mut app = super::sample_app();
    let view = crate::state::NamedView {
        name: "overview".to_owned(),
        zoom: 1.5,
        pan: [10.0, 20.0],
    };

    app.execute_action(Action::board_view_insert(0, view.clone()));
    assert_eq!(
        app.session.board_views.as_slice(),
        std::slice::from_ref(&view)
    );
    app.undo();
    assert!(app.session.board_views.is_empty());
    app.redo();
    assert_eq!(app.session.board_views.len(), 1);

    app.execute_action(Action::board_view_remove(0, view.clone()));
    assert!(app.session.board_views.is_empty());
    app.undo();
    assert_eq!(app.session.board_views.as_slice(), [view]);
}

/// Both directions of the board-view pair degrade the same way on a stale
/// list: removal falls back to matching by value, insertion clamps the index.
#[test]
fn board_view_replay_degrades_symmetrically_on_stale_lists() {
    let mut app = super::sample_app();
    let view = |name: &str| crate::state::NamedView {
        name: name.to_owned(),
        zoom: 1.0,
        pan: [0.0, 0.0],
    };
    app.execute_action(Action::board_view_insert(0, view("a")));
    app.execute_action(Action::board_view_insert(1, view("b")));

    // Undo the "a" insert after "a" moved to a different index than recorded:
    // the value fallback still removes "a", not "b".
    app.session.board_views.swap(0, 1);
    app.undo(); // reverts insert of "b" — index 1 is now "a", fallback finds "b"
    assert_eq!(app.session.board_views.len(), 1);
    assert_eq!(app.session.board_views[0].name, "a");
    app.redo();
    assert_eq!(app.session.board_views.len(), 2);

    // A remove recorded at a stale, out-of-range index clamps on revert and
    // matches by value on apply, so nothing is lost or duplicated.
    let target = app.session.board_views[0].clone();
    app.execute_action(Action::board_view_remove(usize::MAX, target.clone()));
    assert_eq!(app.session.board_views.len(), 1);
    app.undo();
    assert_eq!(app.session.board_views.len(), 2);
    assert!(app.session.board_views.contains(&target));
}
