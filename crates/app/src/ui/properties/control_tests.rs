use super::*;
use plotx_core::properties::{FloatBounds, FloatDisplay, axis};

#[test]
fn stepped_drag_candidates_snap_to_the_schema_lattice() {
    for candidate in 3..=15 {
        let snapped = snapped_stepped_int(candidate, 3, 15, 2);
        assert!((3..=15).contains(&snapped));
        assert_eq!((snapped - 3) % 2, 0, "candidate {candidate}");
    }
    assert_eq!(snapped_stepped_int(10, 3, 15, 2), 11);
}

#[test]
fn continuous_text_input_commits_one_undo_record() {
    let (mut app, ids) = crate::ui::properties::fixture::contour_page(1);
    let target = app.object_target(0, ids[0]).expect("plot target");
    let undo_before = app.session.undo_stack.len();
    let mut editing = true;
    let mut buffer = String::new();
    let mut submissions = 0;

    for character in ["A", "x", "i", "s"] {
        buffer.push_str(character);
        if should_submit_text_edit(&mut editing, true, false, false) {
            submissions += 1;
        }
    }
    if should_submit_text_edit(&mut editing, false, true, false) {
        submissions += 1;
        let commit = app
            .plan_property_write(
                axis::X_LABEL,
                std::slice::from_ref(&target),
                &PropertyValue::Text(buffer),
            )
            .expect("text edit plans");
        app.commit_property(commit);
    }

    assert_eq!(submissions, 1);
    assert_eq!(app.session.undo_stack.len(), undo_before + 1);
    assert_eq!(
        app.doc.canvases[0].objects[0]
            .plot()
            .expect("plot")
            .axis_overrides
            .x_label
            .as_deref(),
        Some("Axis")
    );
    app.undo();
    assert_eq!(
        app.doc.canvases[0].objects[0]
            .plot()
            .expect("plot")
            .axis_overrides
            .x_label,
        None
    );
}

#[test]
fn continuous_text_box_input_commits_one_undo_record() {
    use plotx_core::state::{CanvasDocument, CanvasObject, CanvasObjectKind, ObjectFrame, TextBox};
    let mut app = PlotxApp::new();
    let mut canvas = CanvasDocument::new("text".to_owned(), [120.0, 80.0]);
    let id = canvas.allocate_object_id();
    canvas.objects.push(CanvasObject {
        id,
        name: "Caption".to_owned(),
        frame: ObjectFrame::new(0.0, 0.0, 40.0, 20.0),
        locked: false,
        visible: true,
        group: None,
        kind: CanvasObjectKind::Text(TextBox::label(String::new())),
    });
    app.doc.canvases.push(canvas);
    let target = app.object_target(0, id).unwrap();
    let undo_before = app.session.undo_stack.len();
    let mut editing = true;
    let mut buffer = String::new();
    let mut submissions = 0;
    for character in ["P", "l", "o", "t", "X"] {
        buffer.push_str(character);
        if should_submit_text_edit(&mut editing, true, false, false) {
            submissions += 1;
        }
    }
    if should_submit_text_edit(&mut editing, false, true, false) {
        submissions += 1;
        let commit = app
            .plan_property_write(
                plotx_core::properties::object::TEXT,
                std::slice::from_ref(&target),
                &PropertyValue::Text(buffer),
            )
            .unwrap();
        app.commit_property(commit);
    }
    assert_eq!(submissions, 1);
    assert_eq!(app.session.undo_stack.len(), undo_before + 1);
    assert_eq!(
        app.doc.canvases[0].object(id).unwrap().text().unwrap().text,
        "PlotX"
    );
    app.undo();
    let text = app.doc.canvases[0].object(id).unwrap().text().unwrap();
    assert!(text.text.is_empty());
}

#[test]
fn a_drag_across_zero_never_emits_a_kernel_rejected_divisor() {
    let bounds = FloatBounds::excluding_magnitude(-f64::MAX, f64::MAX, f64::MIN_POSITIVE);
    let next = admitted_float_from_control(bounds, 1.0, 0.0, FloatDisplay::Linear(""), 0.1);
    assert!(next < 0.0, "a downward drag crosses to the negative side");
    assert!(bounds.admits(next));
    assert!(next.abs() > f64::MIN_POSITIVE);
}

#[test]
fn canvas_length_projection_changes_value_caption_and_write_space_together() {
    let projection = FloatControlProjection::new(
        true,
        FloatBounds::inclusive(0.0, 100.0),
        FloatDisplay::Linear("mm"),
        25.4,
        CanvasSizeUnit::Inch,
        Some(1.0),
    );
    assert!((projection.displayed - 1.0).abs() < 1.0e-6);
    assert_eq!(projection.caption, "in");
    assert_eq!(projection.decimals, Some(3));
    assert_eq!(projection.speed, CanvasSizeUnit::Inch.drag_speed());
    assert!(
        (projection.to_domain(2.0) - 50.8).abs() < 1.0e-5,
        "the catalog always receives millimetres"
    );
}

#[test]
fn line_width_presets_include_the_default_and_two_emphasis_levels() {
    assert_eq!(
        LINE_WIDTH_PRESETS,
        [("Fine", 0.5), ("Medium", 0.75), ("Bold", 1.25)]
    );
}
