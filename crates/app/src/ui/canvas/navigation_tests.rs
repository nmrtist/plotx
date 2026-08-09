use super::*;
use plotx_core::state::{CanvasViewport, PanelMeta, PlotObject};
use plotx_figure::{Axis, Figure};

const PLOT_ID: ObjectId = ObjectId::new(1);

fn zoomed_plot_fixture() -> (PlotxApp, ObjectId, PlotRect) {
    let mut app = PlotxApp::new();
    let mut canvas = CanvasDocument::new("page".to_owned(), [200.0, 120.0]);
    let mut figure = Figure::new("plot", Axis::new("x", 0.0, 10.0), Axis::new("y", 0.0, 10.0));
    let viewport = CanvasViewport {
        full_x: AxisRange::new(0.0, 10.0),
        full_y: AxisRange::new(0.0, 10.0),
        view_x: AxisRange::new(2.0, 8.0),
        view_y: AxisRange::new(2.0, 8.0),
        auto_y: false,
    };
    viewport.apply_to(&mut figure);
    canvas.objects.push(CanvasObject {
        id: PLOT_ID,
        name: "Plot".to_owned(),
        frame: ObjectFrame::new(10.0, 10.0, 180.0, 100.0),
        locked: false,
        visible: true,
        kind: CanvasObjectKind::Plot(Box::new(PlotObject::new(
            None,
            plotx_core::state::SeriesId::new(1),
            plotx_core::state::DataBinding { series: Vec::new() },
            plotx_core::state::ChartSpec::default(),
            plotx_core::state::StackSpec::default(),
            plotx_core::state::AxisProjections::default(),
            plotx_core::state::AxisOverrides::default(),
            figure,
            viewport,
            PanelMeta::new("title".to_owned(), 50.0),
        ))),
    });
    app.doc.canvases.push(canvas);
    (
        app,
        PLOT_ID,
        PlotRect {
            left: 0.0,
            top: 0.0,
            width: 200.0,
            height: 100.0,
        },
    )
}

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

#[cfg(target_os = "macos")]
fn mouse_wheel_with(
    unit: egui::MouseWheelUnit,
    delta: Vec2,
    modifiers: egui::Modifiers,
) -> egui::Event {
    mouse_wheel_with_phase(unit, delta, egui::TouchPhase::Move, modifiers)
}

#[cfg(target_os = "macos")]
fn mouse_wheel_with_phase(
    unit: egui::MouseWheelUnit,
    delta: Vec2,
    phase: egui::TouchPhase,
    modifiers: egui::Modifiers,
) -> egui::Event {
    egui::Event::MouseWheel {
        unit,
        delta,
        phase,
        modifiers,
    }
}

#[cfg(target_os = "macos")]
fn mouse_wheel(unit: egui::MouseWheelUnit) -> egui::Event {
    mouse_wheel_with(unit, Vec2::ZERO, egui::Modifiers::NONE)
}

#[cfg(target_os = "macos")]
fn plot_viewport(app: &PlotxApp) -> CanvasViewport {
    app.doc.canvases[0]
        .object(PLOT_ID)
        .and_then(|object| object.plot())
        .unwrap()
        .viewport
        .clone()
}

#[cfg(target_os = "macos")]
fn run_navigation_frame(
    app: &mut PlotxApp,
    pointer: Pos2,
    scroll_delta: Vec2,
    modifiers: egui::Modifiers,
) -> (bool, std::time::Duration) {
    run_navigation_events(
        app,
        pointer,
        modifiers,
        vec![
            mouse_wheel_with_phase(
                egui::MouseWheelUnit::Point,
                Vec2::ZERO,
                egui::TouchPhase::Start,
                modifiers,
            ),
            mouse_wheel_with(egui::MouseWheelUnit::Point, scroll_delta, modifiers),
        ],
    )
}

#[cfg(target_os = "macos")]
fn run_navigation_events(
    app: &mut PlotxApp,
    pointer: Pos2,
    modifiers: egui::Modifiers,
    events: Vec<egui::Event>,
) -> (bool, std::time::Duration) {
    let ctx = egui::Context::default();
    let rect = egui::Rect::from_min_size(Pos2::ZERO, Vec2::new(400.0, 300.0));
    let blank_input = egui::RawInput {
        screen_rect: Some(rect),
        ..Default::default()
    };
    for _ in 0..2 {
        let _ = ctx.run_ui(blank_input.clone(), |_| {});
    }
    let (consumed, mut repaint_delay) =
        run_navigation_events_on(&ctx, app, pointer, modifiers, events);
    for _ in 0..2 {
        if repaint_delay > std::time::Duration::ZERO && repaint_delay < std::time::Duration::MAX {
            break;
        }
        let output = ctx.run_ui(blank_input.clone(), |ui| {
            let _ = handle_navigation(app, 0, rect, ui);
        });
        repaint_delay = output.viewport_output[&egui::ViewportId::ROOT].repaint_delay;
    }
    (consumed, repaint_delay)
}

#[cfg(target_os = "macos")]
fn run_navigation_events_on(
    ctx: &egui::Context,
    app: &mut PlotxApp,
    pointer: Pos2,
    modifiers: egui::Modifiers,
    mut events: Vec<egui::Event>,
) -> (bool, std::time::Duration) {
    let rect = egui::Rect::from_min_size(Pos2::ZERO, Vec2::new(400.0, 300.0));
    events.insert(0, egui::Event::PointerMoved(pointer));
    let input = egui::RawInput {
        screen_rect: Some(rect),
        modifiers,
        events,
        ..Default::default()
    };
    let mut consumed = false;
    let output = ctx.run_ui(input, |ui| {
        consumed = handle_navigation(app, 0, rect, ui);
    });
    (
        consumed,
        output.viewport_output[&egui::ViewportId::ROOT].repaint_delay,
    )
}

#[cfg(target_os = "macos")]
#[path = "navigation_trackpad_regression_tests.rs"]
mod trackpad_regressions;

#[test]
fn precise_scroll_with_two_axes_is_trackpad_pan() {
    assert_eq!(
        classify_navigation_input(1.0, Vec2::new(7.0, -5.0), true),
        NavigationInput::TrackpadPan(Vec2::new(7.0, -5.0)),
    );
}

#[test]
fn vertical_only_precise_scroll_is_trackpad_pan() {
    assert_eq!(
        classify_navigation_input(1.0, Vec2::new(0.0, -5.0), true),
        NavigationInput::TrackpadPan(Vec2::new(0.0, -5.0)),
    );
}

#[test]
fn horizontal_only_precise_scroll_is_trackpad_pan() {
    assert_eq!(
        classify_navigation_input(1.0, Vec2::new(7.0, 0.0), true),
        NavigationInput::TrackpadPan(Vec2::new(7.0, 0.0)),
    );
}

#[test]
fn vertical_non_precise_scroll_is_wheel_zoom() {
    assert_eq!(
        classify_navigation_input(1.0, Vec2::new(0.0, 24.0), false),
        NavigationInput::WheelZoom(24.0),
    );
}

#[test]
fn pinch_takes_priority_over_simultaneous_precise_scroll() {
    assert_eq!(
        classify_navigation_input(1.25, Vec2::new(7.0, -5.0), true),
        NavigationInput::Pinch(1.25),
    );
}

#[test]
fn horizontal_non_precise_scroll_is_ignored() {
    assert_eq!(
        classify_navigation_input(1.0, Vec2::new(24.0, 0.0), false),
        NavigationInput::None,
    );
}

#[cfg(target_os = "macos")]
#[test]
fn point_scroll_over_plot_pans_viewport_without_moving_board() {
    let (mut app, _, _) = zoomed_plot_fixture();
    app.session.board.auto_fit = false;
    let before_viewport = plot_viewport(&app);
    let before_board = app.session.board;

    let (consumed, repaint_delay) = run_navigation_frame(
        &mut app,
        Pos2::new(100.0, 60.0),
        Vec2::new(4.0, -3.0),
        egui::Modifiers::NONE,
    );

    let after_viewport = plot_viewport(&app);
    assert!(consumed);
    assert_ne!(after_viewport.view_x, before_viewport.view_x);
    assert_ne!(after_viewport.view_y, before_viewport.view_y);
    assert!((after_viewport.view_x.span() - before_viewport.view_x.span()).abs() < 1e-12);
    assert!((after_viewport.view_y.span() - before_viewport.view_y.span()).abs() < 1e-12);
    assert_eq!(app.session.board, before_board);
    assert!(repaint_delay > std::time::Duration::ZERO);
    assert!(repaint_delay <= std::time::Duration::from_millis(200));
}

#[cfg(target_os = "macos")]
#[test]
fn command_point_scroll_over_plot_pans_board_without_changing_viewport() {
    let (mut app, _, _) = zoomed_plot_fixture();
    app.session.board.auto_fit = false;
    let before_viewport = plot_viewport(&app);
    let modifiers = egui::Modifiers {
        command: true,
        ..Default::default()
    };

    let (consumed, _) = run_navigation_frame(
        &mut app,
        Pos2::new(100.0, 60.0),
        Vec2::new(4.0, -3.0),
        modifiers,
    );

    assert!(consumed);
    assert_eq!(plot_viewport(&app), before_viewport);
    assert_eq!(app.session.board.pan, [4.0, -3.0]);
}

#[cfg(target_os = "macos")]
fn assert_modified_point_scroll_keeps_board_target_after_modifier_release(
    modifiers: egui::Modifiers,
) {
    let (mut app, _, _) = zoomed_plot_fixture();
    app.session.board.auto_fit = false;
    let before_viewport = plot_viewport(&app);
    let ctx = egui::Context::default();
    let pointer = Pos2::new(100.0, 60.0);
    let (start_consumed, _) = run_navigation_events_on(
        &ctx,
        &mut app,
        pointer,
        modifiers,
        vec![
            mouse_wheel_with_phase(
                egui::MouseWheelUnit::Point,
                Vec2::ZERO,
                egui::TouchPhase::Start,
                modifiers,
            ),
            mouse_wheel_with(egui::MouseWheelUnit::Point, Vec2::new(4.0, -3.0), modifiers),
        ],
    );

    assert!(start_consumed);
    assert_eq!(plot_viewport(&app), before_viewport);
    assert_eq!(app.session.board.pan, [4.0, -3.0]);

    let (move_consumed, _) = run_navigation_events_on(
        &ctx,
        &mut app,
        pointer,
        egui::Modifiers::NONE,
        vec![mouse_wheel_with(
            egui::MouseWheelUnit::Point,
            Vec2::new(2.0, -2.0),
            egui::Modifiers::NONE,
        )],
    );

    assert!(move_consumed);
    assert_eq!(plot_viewport(&app), before_viewport);
    assert_eq!(app.session.board.pan, [6.0, -5.0]);
}

#[cfg(target_os = "macos")]
#[test]
fn command_point_scroll_keeps_board_target_after_modifier_release() {
    assert_modified_point_scroll_keeps_board_target_after_modifier_release(egui::Modifiers {
        command: true,
        ..Default::default()
    });
}

#[cfg(target_os = "macos")]
#[test]
fn control_point_scroll_keeps_board_target_after_modifier_release() {
    assert_modified_point_scroll_keeps_board_target_after_modifier_release(egui::Modifiers {
        ctrl: true,
        ..Default::default()
    });
}

#[cfg(target_os = "macos")]
fn assert_board_gesture_finish_releases_target(finish_phase: egui::TouchPhase) {
    let (mut app, _, _) = zoomed_plot_fixture();
    app.session.board.auto_fit = false;
    let ctx = egui::Context::default();
    let pointer = Pos2::new(100.0, 60.0);
    let command = egui::Modifiers {
        command: true,
        ..Default::default()
    };

    let _ = run_navigation_events_on(
        &ctx,
        &mut app,
        pointer,
        command,
        vec![
            mouse_wheel_with_phase(
                egui::MouseWheelUnit::Point,
                Vec2::ZERO,
                egui::TouchPhase::Start,
                command,
            ),
            mouse_wheel_with(egui::MouseWheelUnit::Point, Vec2::new(4.0, -3.0), command),
        ],
    );
    let before_viewport = plot_viewport(&app);
    let _ = run_navigation_events_on(
        &ctx,
        &mut app,
        pointer,
        egui::Modifiers::NONE,
        vec![mouse_wheel_with_phase(
            egui::MouseWheelUnit::Point,
            Vec2::ZERO,
            finish_phase,
            egui::Modifiers::NONE,
        )],
    );

    let (consumed, _) = run_navigation_events_on(
        &ctx,
        &mut app,
        pointer,
        egui::Modifiers::NONE,
        vec![mouse_wheel_with(
            egui::MouseWheelUnit::Point,
            Vec2::new(2.0, -2.0),
            egui::Modifiers::NONE,
        )],
    );

    assert!(consumed);
    assert_ne!(plot_viewport(&app), before_viewport);
    assert_eq!(app.session.board.pan, [4.0, -3.0]);
}

#[cfg(target_os = "macos")]
#[test]
fn command_point_scroll_releases_board_target_on_end() {
    assert_board_gesture_finish_releases_target(egui::TouchPhase::End);
}

#[cfg(target_os = "macos")]
#[test]
fn command_point_scroll_releases_board_target_on_cancel() {
    assert_board_gesture_finish_releases_target(egui::TouchPhase::Cancel);
}

#[cfg(target_os = "macos")]
#[test]
fn point_scroll_over_blank_board_pans_both_axes() {
    let (mut app, _, _) = zoomed_plot_fixture();
    app.session.board.auto_fit = false;
    let before_viewport = plot_viewport(&app);

    let (consumed, _) = run_navigation_frame(
        &mut app,
        Pos2::new(300.0, 200.0),
        Vec2::new(4.0, -3.0),
        egui::Modifiers::NONE,
    );

    assert!(consumed);
    assert_eq!(plot_viewport(&app), before_viewport);
    assert_eq!(app.session.board.pan, [4.0, -3.0]);
}

#[cfg(target_os = "macos")]
#[test]
fn native_pinch_has_priority_over_command_point_scroll() {
    let (mut app, _, _) = zoomed_plot_fixture();
    app.session.board.auto_fit = false;
    let modifiers = egui::Modifiers {
        command: true,
        ..Default::default()
    };

    let (consumed, _) = run_navigation_events(
        &mut app,
        Pos2::new(100.0, 60.0),
        modifiers,
        vec![
            mouse_wheel_with(egui::MouseWheelUnit::Point, Vec2::new(4.0, -3.0), modifiers),
            egui::Event::Zoom(1.25),
        ],
    );

    assert!(consumed);
    assert!((app.session.board.zoom - 1.25).abs() < f32::EPSILON);
    assert_eq!(app.session.board.pan, [-25.0, -15.0]);
}

#[cfg(target_os = "macos")]
#[test]
fn native_pinch_does_not_inherit_board_target_from_point_gesture() {
    let (mut app, _, _) = zoomed_plot_fixture();
    app.session.board.auto_fit = false;
    let ctx = egui::Context::default();
    let pointer = Pos2::new(100.0, 60.0);
    let command = egui::Modifiers {
        command: true,
        ..Default::default()
    };
    let _ = run_navigation_events_on(
        &ctx,
        &mut app,
        pointer,
        command,
        vec![
            mouse_wheel_with_phase(
                egui::MouseWheelUnit::Point,
                Vec2::ZERO,
                egui::TouchPhase::Start,
                command,
            ),
            mouse_wheel_with(egui::MouseWheelUnit::Point, Vec2::new(4.0, -3.0), command),
        ],
    );
    let before_viewport = plot_viewport(&app);

    let (consumed, _) = run_navigation_events_on(
        &ctx,
        &mut app,
        pointer,
        egui::Modifiers::NONE,
        vec![egui::Event::Zoom(1.25)],
    );

    let after_viewport = plot_viewport(&app);
    assert!(consumed);
    assert!(after_viewport.view_x.span() < before_viewport.view_x.span());
    assert!(after_viewport.view_y.span() < before_viewport.view_y.span());
    assert_eq!(app.session.board.zoom, 1.0);
    assert_eq!(app.session.board.pan, [4.0, -3.0]);
}

#[test]
fn board_pan_clears_fit_and_moves_both_axes() {
    let mut app = PlotxApp::new();
    app.session.board.pan = [10.0, -4.0];
    app.session.board.auto_fit = true;
    app.session.board_fit = Some(BoardFitTarget::Region([0.0, 0.0, 1.0, 1.0]));

    pan_board_view(&mut app, Vec2::new(7.0, -3.0));

    assert_eq!(app.session.board.pan, [17.0, -7.0]);
    assert!(!app.session.board.auto_fit);
    assert!(app.session.board_fit.is_none());
}

#[test]
fn consecutive_plot_pans_coalesce_into_one_undo_action() {
    let (mut app, object_id, plot) = zoomed_plot_fixture();
    let before = app.doc.canvases[0]
        .object(object_id)
        .and_then(|object| object.plot())
        .unwrap()
        .viewport
        .clone();

    pan_plot_viewport(&mut app, 0, object_id, plot, Vec2::new(10.0, -5.0), 1.0);
    pan_plot_viewport(&mut app, 0, object_id, plot, Vec2::new(4.0, 3.0), 2.0);

    let pending = app.session.ui.wheel_zoom.as_ref().unwrap();
    assert_eq!(pending.canvas, 0);
    assert_eq!(pending.object, object_id);
    assert_eq!(pending.before, before);
    assert_eq!(pending.last_input_time, 2.0);
    assert!(app.session.undo_stack.is_empty());

    app.finish_pending_wheel_zoom(f64::INFINITY, true);

    assert_eq!(app.session.undo_stack.len(), 1);
    assert!(app.session.ui.wheel_zoom.is_none());
}
