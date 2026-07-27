use super::*;

fn pointer_frame(
    ctx: &egui::Context,
    app: &mut PlotxApp,
    screen: egui::Rect,
    pointer: Pos2,
    time: f64,
    pressed: bool,
) -> bool {
    let input = egui::RawInput {
        screen_rect: Some(screen),
        time: Some(time),
        events: vec![
            egui::Event::PointerMoved(pointer),
            egui::Event::PointerButton {
                pos: pointer,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: egui::Modifiers::default(),
            },
        ],
        ..Default::default()
    };
    let mut consumed = false;
    let _ = ctx.run_ui(input, |ui| {
        consumed = handle_navigation(app, 0, screen, ui);
    });
    consumed
}

#[test]
fn a_double_click_release_beats_the_zero_distance_box_zoom_and_resets_the_plot() {
    let (mut app, ids) = crate::ui::properties::fixture::contour_page(1);
    let object = ids[0];
    app.set_tool(Tool::BrowseZoom);
    app.session.board = BoardViewport {
        zoom: 1.0,
        pan: [0.0, 0.0],
        auto_fit: false,
    };
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1000.0, 800.0));
    let plot = plot_inner_rect(&app, 0, object, screen).expect("plot is on the board");
    let pointer = Pos2::new(
        (plot.left + plot.right()) * 0.5,
        (plot.top + plot.bottom()) * 0.5,
    );

    let plot_object = app.doc.canvases[0]
        .object_mut(object)
        .and_then(|object| object.plot_mut())
        .expect("fixture plot");
    let full_x = plot_object.viewport.full_x;
    let full_y = plot_object.viewport.full_y;
    plot_object.viewport.view_x = AxisRange::new(
        full_x.min + full_x.span() * 0.25,
        full_x.max - full_x.span() * 0.25,
    );
    plot_object.viewport.view_y = AxisRange::new(
        full_y.min + full_y.span() * 0.25,
        full_y.max - full_y.span() * 0.25,
    );
    plot_object.apply_viewport();

    let ctx = egui::Context::default();
    pointer_frame(&ctx, &mut app, screen, pointer, 0.00, true);
    pointer_frame(&ctx, &mut app, screen, pointer, 0.05, false);
    pointer_frame(&ctx, &mut app, screen, pointer, 0.10, true);
    app.begin_interaction(Interaction::Zoom(ZoomDrag {
        canvas: 0,
        object,
        start: [pointer.x, pointer.y],
        current: [pointer.x, pointer.y],
        axis: ZoomAxis::Box,
    }));
    assert!(
        pointer_frame(&ctx, &mut app, screen, pointer, 0.15, false),
        "the double-click is consumed as navigation"
    );

    let plot_object = app.doc.canvases[0]
        .object(object)
        .and_then(|object| object.plot())
        .expect("fixture plot remains");
    assert_eq!(plot_object.viewport.view_x, full_x);
    assert_eq!(plot_object.viewport.view_y, full_y);
    assert!(matches!(app.interaction(), Interaction::Idle));
}

/// The reset preempts every in-flight gesture, but a pan has already moved the
/// viewport. Cancelling it would drop that move's undo record and leave the
/// reset's own record starting from the panned view, so one undo would land on
/// the pan the user never asked to keep.
#[test]
fn a_double_click_during_a_pan_commits_the_pan_before_it_resets() {
    let (mut app, ids) = crate::ui::properties::fixture::contour_page(1);
    let object = ids[0];
    app.session.board = BoardViewport {
        zoom: 1.0,
        pan: [0.0, 0.0],
        auto_fit: false,
    };
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1000.0, 800.0));
    let plot = plot_inner_rect(&app, 0, object, screen).expect("plot is on the board");
    let pointer = Pos2::new(
        (plot.left + plot.right()) * 0.5,
        (plot.top + plot.bottom()) * 0.5,
    );

    let plot_object = app.doc.canvases[0]
        .object(object)
        .and_then(|object| object.plot())
        .expect("fixture plot");
    let full_x = plot_object.viewport.full_x;
    let full_y = plot_object.viewport.full_y;
    let before = plot_object.viewport.clone();

    let ctx = egui::Context::default();
    pointer_frame(&ctx, &mut app, screen, pointer, 0.00, true);
    pointer_frame(&ctx, &mut app, screen, pointer, 0.05, false);
    pointer_frame(&ctx, &mut app, screen, pointer, 0.10, true);

    // A pan in flight, with the viewport already dragged away from the fit.
    app.begin_interaction(Interaction::Pan(PanDrag {
        canvas: 0,
        object,
        before,
    }));
    let plot_object = app.doc.canvases[0]
        .object_mut(object)
        .and_then(|object| object.plot_mut())
        .expect("fixture plot");
    let panned_x = AxisRange::new(
        full_x.min + full_x.span() * 0.25,
        full_x.max - full_x.span() * 0.25,
    );
    plot_object.viewport.view_x = panned_x;
    plot_object.apply_viewport();
    let undo_before = app.session.undo_stack.len();

    assert!(
        pointer_frame(&ctx, &mut app, screen, pointer, 0.15, false),
        "the double-click is consumed as navigation"
    );

    let viewport = |app: &PlotxApp| {
        app.doc.canvases[0]
            .object(object)
            .and_then(|object| object.plot())
            .expect("fixture plot remains")
            .viewport
            .clone()
    };
    assert_eq!(viewport(&app).view_x, full_x);
    assert_eq!(viewport(&app).view_y, full_y);
    assert!(matches!(app.interaction(), Interaction::Idle));
    assert_eq!(
        app.session.undo_stack.len(),
        undo_before + 2,
        "the pan and the reset are two edits"
    );

    app.undo();
    assert_eq!(viewport(&app).view_x, panned_x, "one undo returns the pan");
    app.undo();
    assert_eq!(
        viewport(&app).view_x,
        full_x,
        "the second undo returns the pre-pan view"
    );
}

fn assert_axis_double_click_resets_only(axis: ZoomAxis) {
    let (mut app, ids) = crate::ui::properties::fixture::contour_page(1);
    let object = ids[0];
    app.session.board = BoardViewport {
        zoom: 1.0,
        pan: [0.0, 0.0],
        auto_fit: false,
    };
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1000.0, 800.0));
    let outer = object_screen_rect(app.session.board, &app.doc.canvases[0], object, screen)
        .expect("plot is on the board");
    let plot = plot_inner_rect(&app, 0, object, screen).expect("plot has an inner rectangle");
    let pointer = match axis {
        ZoomAxis::X => Pos2::new(
            (plot.left + plot.right()) * 0.5,
            (plot.bottom() + outer.bottom()) * 0.5,
        ),
        ZoomAxis::Y => Pos2::new(
            (outer.left + plot.left) * 0.5,
            (plot.top + plot.bottom()) * 0.5,
        ),
        ZoomAxis::Box => panic!("axis-strip test needs one axis"),
    };
    assert!(
        matches!(
            hit_zone(pointer, plot_rect(outer), plot),
            HitZone::XAxis if axis == ZoomAxis::X
        ) || matches!(
            hit_zone(pointer, plot_rect(outer), plot),
            HitZone::YAxis if axis == ZoomAxis::Y
        )
    );

    let plot_object = app.doc.canvases[0]
        .object_mut(object)
        .and_then(|object| object.plot_mut())
        .expect("fixture plot");
    let full_x = plot_object.viewport.full_x;
    let full_y = plot_object.viewport.full_y;
    let narrow_x = AxisRange::new(
        full_x.min + full_x.span() * 0.25,
        full_x.max - full_x.span() * 0.25,
    );
    let narrow_y = AxisRange::new(
        full_y.min + full_y.span() * 0.25,
        full_y.max - full_y.span() * 0.25,
    );
    plot_object.viewport.view_x = narrow_x;
    plot_object.viewport.view_y = narrow_y;
    plot_object.viewport.auto_y = false;
    plot_object.apply_viewport();

    let ctx = egui::Context::default();
    pointer_frame(&ctx, &mut app, screen, pointer, 0.00, true);
    pointer_frame(&ctx, &mut app, screen, pointer, 0.05, false);
    pointer_frame(&ctx, &mut app, screen, pointer, 0.10, true);
    assert!(
        pointer_frame(&ctx, &mut app, screen, pointer, 0.15, false),
        "the double-click is consumed as navigation"
    );

    let viewport = &app.doc.canvases[0]
        .object(object)
        .and_then(|object| object.plot())
        .expect("fixture plot remains")
        .viewport;
    match axis {
        ZoomAxis::X => {
            assert_eq!(viewport.view_x, full_x);
            assert_eq!(viewport.view_y, narrow_y);
        }
        ZoomAxis::Y => {
            assert_eq!(viewport.view_x, narrow_x);
            assert_eq!(viewport.view_y, full_y);
        }
        ZoomAxis::Box => unreachable!(),
    }
    assert!(matches!(app.interaction(), Interaction::Idle));
}

#[test]
fn a_double_click_on_the_x_axis_resets_only_x() {
    assert_axis_double_click_resets_only(ZoomAxis::X);
}

#[test]
fn a_double_click_on_the_y_axis_resets_only_y() {
    assert_axis_double_click_resets_only(ZoomAxis::Y);
}
