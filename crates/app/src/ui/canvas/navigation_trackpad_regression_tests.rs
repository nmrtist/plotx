use super::*;

#[test]
fn point_start_activates_trackpad_state() {
    let ctx = egui::Context::default();
    assert!(
        macos_trackpad_gesture(
            &ctx,
            &[mouse_wheel_with_phase(
                egui::MouseWheelUnit::Point,
                Vec2::ZERO,
                egui::TouchPhase::Start,
                egui::Modifiers::NONE,
            )],
            true,
        )
        .active
    );
}

#[test]
fn line_and_page_wheels_do_not_activate_trackpad_state() {
    let ctx = egui::Context::default();
    assert!(
        !macos_trackpad_gesture(
            &ctx,
            &[
                mouse_wheel(egui::MouseWheelUnit::Line),
                mouse_wheel(egui::MouseWheelUnit::Page),
            ],
            true,
        )
        .active
    );
}

#[test]
fn point_wheel_without_touch_start_retains_wheel_zoom() {
    let (mut app, _, _) = zoomed_plot_fixture();
    app.session.viewport_mode = ViewportMode::Manual;
    let before = plot_viewport(&app);

    let (consumed, _) = run_navigation_events(
        &mut app,
        Pos2::new(100.0, 60.0),
        egui::Modifiers::NONE,
        vec![mouse_wheel_with(
            egui::MouseWheelUnit::Point,
            Vec2::new(0.0, 24.0),
            egui::Modifiers::NONE,
        )],
    );

    let after = plot_viewport(&app);
    assert!(consumed);
    assert!(after.view_x.span() < before.view_x.span());
    assert_eq!(after.view_y, before.view_y);
    assert_eq!(app.session.board.world_center, [200.0, 150.0]);
}

#[test]
fn alt_point_wheel_without_touch_start_adjusts_display_intensity() {
    let (mut app, _, _) = zoomed_plot_fixture();
    app.session.viewport_mode = ViewportMode::Manual;
    let before = plot_viewport(&app);
    let screen = egui::Rect::from_min_size(Pos2::ZERO, Vec2::new(400.0, 300.0));
    let (_, outer, plot) = plot_under_cursor(&app, 0, screen, Pos2::new(100.0, 60.0)).unwrap();
    let pointer = Pos2::new(plot.left + plot.width * 0.5, plot.top + plot.height * 0.5);
    assert!(matches!(hit_zone(pointer, outer, plot), HitZone::Plot));
    let alt = egui::Modifiers {
        alt: true,
        ..Default::default()
    };

    let (consumed, _) = run_navigation_events(
        &mut app,
        pointer,
        alt,
        vec![mouse_wheel_with(
            egui::MouseWheelUnit::Point,
            Vec2::new(0.0, 24.0),
            alt,
        )],
    );

    let after = plot_viewport(&app);
    assert!(consumed);
    assert!(after.view_y.span() < before.view_y.span());
    assert!(app.session.status.starts_with("Adjusted plot intensity"));
    assert_eq!(app.session.board.world_center, [200.0, 150.0]);
}

#[test]
fn point_wheel_without_touch_start_over_y_axis_zooms_only_y() {
    let (mut app, _, _) = zoomed_plot_fixture();
    app.session.viewport_mode = ViewportMode::Manual;
    let screen = egui::Rect::from_min_size(Pos2::ZERO, Vec2::new(400.0, 300.0));
    let (_, outer, plot) = plot_under_cursor(&app, 0, screen, Pos2::new(100.0, 60.0)).unwrap();
    let pointer = Pos2::new(
        (outer.left() + plot.left) * 0.5,
        plot.top + plot.height * 0.5,
    );
    assert!(matches!(hit_zone(pointer, outer, plot), HitZone::YAxis));
    let before = plot_viewport(&app);

    let (consumed, _) = run_navigation_events(
        &mut app,
        pointer,
        egui::Modifiers::NONE,
        vec![mouse_wheel_with(
            egui::MouseWheelUnit::Point,
            Vec2::new(0.0, 24.0),
            egui::Modifiers::NONE,
        )],
    );

    let after = plot_viewport(&app);
    assert!(consumed);
    assert_eq!(after.view_x, before.view_x);
    assert!(after.view_y.span() < before.view_y.span());
}

#[test]
fn trackpad_target_stays_on_plot_when_control_changes_mid_gesture() {
    let (mut app, _, _) = zoomed_plot_fixture();
    app.session.viewport_mode = ViewportMode::Manual;
    let ctx = egui::Context::default();
    let pointer = Pos2::new(100.0, 60.0);
    let before = plot_viewport(&app);

    let (start_consumed, _) = run_navigation_events_on(
        &ctx,
        &mut app,
        pointer,
        egui::Modifiers::NONE,
        vec![
            mouse_wheel_with_phase(
                egui::MouseWheelUnit::Point,
                Vec2::ZERO,
                egui::TouchPhase::Start,
                egui::Modifiers::NONE,
            ),
            mouse_wheel_with(
                egui::MouseWheelUnit::Point,
                Vec2::new(4.0, -3.0),
                egui::Modifiers::NONE,
            ),
        ],
    );
    let after_start = plot_viewport(&app);
    assert!(start_consumed);
    assert_ne!(after_start, before);
    assert_eq!(app.session.board.world_center, [200.0, 150.0]);

    let control = egui::Modifiers {
        ctrl: true,
        ..Default::default()
    };
    let (control_consumed, _) = run_navigation_events_on(
        &ctx,
        &mut app,
        pointer,
        control,
        vec![mouse_wheel_with(
            egui::MouseWheelUnit::Point,
            Vec2::new(2.0, -2.0),
            control,
        )],
    );
    let after_control = plot_viewport(&app);
    assert!(control_consumed);
    assert_ne!(after_control, after_start);
    assert_eq!(app.session.board.world_center, [200.0, 150.0]);

    let (release_consumed, _) = run_navigation_events_on(
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

    assert!(release_consumed);
    assert_ne!(plot_viewport(&app), after_control);
    assert_eq!(app.session.board.world_center, [200.0, 150.0]);
}

#[test]
fn trackpad_target_stays_on_board_when_plot_moves_under_pointer() {
    let (mut app, _, _) = zoomed_plot_fixture();
    app.session.viewport_mode = ViewportMode::Manual;
    let ctx = egui::Context::default();
    let pointer = Pos2::new(300.0, 60.0);
    let before_viewport = plot_viewport(&app);

    let (start_consumed, _) = run_navigation_events_on(
        &ctx,
        &mut app,
        pointer,
        egui::Modifiers::NONE,
        vec![
            mouse_wheel_with_phase(
                egui::MouseWheelUnit::Point,
                Vec2::ZERO,
                egui::TouchPhase::Start,
                egui::Modifiers::NONE,
            ),
            mouse_wheel_with(
                egui::MouseWheelUnit::Point,
                Vec2::new(120.0, 0.0),
                egui::Modifiers::NONE,
            ),
        ],
    );
    assert!(start_consumed);
    assert_eq!(app.session.board.world_center, [80.0, 150.0]);
    assert_eq!(plot_viewport(&app), before_viewport);

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
    assert_eq!(app.session.board.world_center, [78.0, 152.0]);
    assert_eq!(plot_viewport(&app), before_viewport);
}

#[test]
fn trackpad_end_outside_canvas_clears_the_active_sequence() {
    let ctx = egui::Context::default();
    let start = mouse_wheel_with_phase(
        egui::MouseWheelUnit::Point,
        Vec2::ZERO,
        egui::TouchPhase::Start,
        egui::Modifiers::NONE,
    );
    assert!(macos_trackpad_gesture(&ctx, &[start], true).active);

    let end = mouse_wheel_with_phase(
        egui::MouseWheelUnit::Point,
        Vec2::ZERO,
        egui::TouchPhase::End,
        egui::Modifiers::NONE,
    );
    sync_macos_trackpad_gesture(&ctx, &[end], false);

    let point_wheel = mouse_wheel_with(
        egui::MouseWheelUnit::Point,
        Vec2::new(0.0, 2.0),
        egui::Modifiers::NONE,
    );
    assert!(!macos_trackpad_gesture(&ctx, &[point_wheel], true).active);
}

#[test]
fn trackpad_sequence_started_outside_canvas_stays_suppressed() {
    let (mut app, _, _) = zoomed_plot_fixture();
    app.session.viewport_mode = ViewportMode::Manual;
    let ctx = egui::Context::default();
    let before = plot_viewport(&app);
    let start = mouse_wheel_with_phase(
        egui::MouseWheelUnit::Point,
        Vec2::ZERO,
        egui::TouchPhase::Start,
        egui::Modifiers::NONE,
    );
    sync_macos_trackpad_gesture(&ctx, &[start], false);

    let (consumed, _) = run_navigation_events_on(
        &ctx,
        &mut app,
        Pos2::new(100.0, 60.0),
        egui::Modifiers::NONE,
        vec![mouse_wheel_with(
            egui::MouseWheelUnit::Point,
            Vec2::new(0.0, 24.0),
            egui::Modifiers::NONE,
        )],
    );

    assert!(!consumed);
    assert_eq!(plot_viewport(&app), before);
    assert_eq!(app.session.board.world_center, [200.0, 150.0]);

    let (end_consumed, _) = run_navigation_events_on(
        &ctx,
        &mut app,
        Pos2::new(100.0, 60.0),
        egui::Modifiers::NONE,
        vec![mouse_wheel_with_phase(
            egui::MouseWheelUnit::Point,
            Vec2::ZERO,
            egui::TouchPhase::End,
            egui::Modifiers::NONE,
        )],
    );
    assert!(!end_consumed);

    let (wheel_consumed, _) = run_navigation_events_on(
        &ctx,
        &mut app,
        Pos2::new(100.0, 60.0),
        egui::Modifiers::NONE,
        vec![mouse_wheel_with(
            egui::MouseWheelUnit::Point,
            Vec2::new(0.0, 24.0),
            egui::Modifiers::NONE,
        )],
    );
    assert!(wheel_consumed);
    assert_ne!(plot_viewport(&app), before);
}
