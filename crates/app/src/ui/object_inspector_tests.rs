//! Object inspector rendering and target-filter tests.

use super::*;

#[test]
fn renders_safely_during_active_canvas_transition() {
    let mut app = PlotxApp::new();
    app.session.active_canvas = Some(0);
    assert!(app.doc.canvases.is_empty());
    assert!(app.session.ui.selection.objects().is_empty());

    let ctx = egui::Context::default();
    let _ = ctx.run_ui(egui::RawInput::default(), |ui| render(&mut app, ui));
}

#[test]
fn property_target_filter_excludes_locked_plot_objects() {
    let (mut app, ids) = crate::ui::properties::fixture::contour_page(1);
    let object = ids[0];
    app.doc.canvases[0]
        .object_mut(object)
        .expect("the fixture plot exists")
        .locked = true;
    let targets = kind_targets(&app, 0, &ids, |candidate| candidate.plot().is_some());
    assert!(
        targets.is_empty(),
        "a locked plot must be excluded before catalog targets are built"
    );
}

#[test]
fn frame_drag_with_mid_gesture_catalog_style_write_keeps_two_independent_undo_records() {
    let mut app = PlotxApp::new();
    let mut canvas = plotx_core::state::CanvasDocument::new("objects".to_owned(), [120.0, 80.0]);
    let id = canvas.allocate_object_id();
    canvas.objects.push(plotx_core::state::CanvasObject {
        id,
        name: "Shape".to_owned(),
        frame: ObjectFrame::new(1.0, 2.0, 30.0, 20.0),
        locked: false,
        visible: true,
        group: None,
        kind: plotx_core::state::CanvasObjectKind::Shape(plotx_core::state::ShapeObject::new(
            plotx_core::state::ShapeKind::Rect,
        )),
    });
    app.doc.canvases.push(canvas);
    app.session.active_canvas = Some(0);
    let frame_before = app.doc.canvases[0].object(id).unwrap().frame;
    let width_before = app.doc.canvases[0]
        .object(id)
        .unwrap()
        .shape()
        .unwrap()
        .stroke_width;

    edits::note_inspector_edit(&mut app, 0, &[id]);
    let frame_after = ObjectFrame::new(8.0, 9.0, 40.0, 25.0);
    app.set_object_frame(0, id, frame_after);

    let target = app.object_target(0, id).unwrap();
    let commit = app
        .plan_property_write(
            plotx_core::properties::object::SHAPE_STROKE_WIDTH,
            std::slice::from_ref(&target),
            &plotx_core::properties::PropertyValue::Float(6.0),
        )
        .unwrap();
    app.commit_property(commit);
    assert_eq!(app.session.undo_stack.len(), 1);

    let ctx = egui::Context::default();
    let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
        flush_inspector_edit(&mut app, ui, false);
    });
    assert_eq!(app.session.undo_stack.len(), 2);

    app.undo();
    assert_eq!(app.doc.canvases[0].object(id).unwrap().frame, frame_before);
    assert_eq!(
        app.doc.canvases[0]
            .object(id)
            .unwrap()
            .shape()
            .unwrap()
            .stroke_width,
        6.0,
        "undoing the frame record must not touch the independent style record"
    );
    app.undo();
    assert_eq!(
        app.doc.canvases[0]
            .object(id)
            .unwrap()
            .shape()
            .unwrap()
            .stroke_width,
        width_before
    );
}
