use crate::actions::Action;
use crate::state::{CanvasDocument, PlotxApp};

#[test]
fn composite_is_one_history_entry_and_reverts_in_reverse_order() {
    let mut app = PlotxApp::new();
    app.doc
        .canvases
        .push(CanvasDocument::new("A".to_owned(), [210.0, 297.0]));
    app.execute_action(Action::Composite(vec![
        Action::rename_canvas(0, "A".to_owned(), "B".to_owned()),
        Action::rename_canvas(0, "B".to_owned(), "C".to_owned()),
    ]));
    assert_eq!(app.doc.canvases[0].name, "C");
    assert_eq!(app.session.undo_stack.len(), 1);
    app.undo();
    assert_eq!(app.doc.canvases[0].name, "A");
    app.redo();
    assert_eq!(app.doc.canvases[0].name, "C");
}

#[test]
fn composite_is_noop_only_when_every_child_is_noop() {
    let mut app = PlotxApp::new();
    app.execute_action(Action::Composite(vec![Action::rename_canvas(
        0,
        "same".to_owned(),
        "same".to_owned(),
    )]));
    assert!(app.session.undo_stack.is_empty());
}
