use super::*;
use std::cell::Cell;

#[test]
fn sidebar_hides_only_when_released_near_its_window_edge() {
    for edge in [SidebarEdge::Left, SidebarEdge::Right] {
        // Include the old hide threshold, the exact new boundary, outside the
        // window, and returning from the hide zone before releasing.
        for (distance, cancel, expected) in [
            (180.0, false, false),
            (100.0, false, false),
            (25.0, false, false),
            (24.0, false, true),
            (0.0, false, true),
            (-100.0, false, true),
            (10.0, true, false),
        ] {
            let ctx = egui::Context::default();
            let hidden = Cell::new(false);
            let window = Rect::from_min_max(Pos2::new(50.0, 30.0), Pos2::new(850.0, 530.0));
            let position = |distance: f32| {
                Pos2::new(
                    match edge {
                        SidebarEdge::Right => window.left() + distance,
                        SidebarEdge::Left => window.right() - distance,
                    },
                    250.0,
                )
            };
            let panel_id = Id::new("hide_test");
            let render = |events| {
                let _ = ctx.run_ui(
                    egui::RawInput {
                        screen_rect: Some(window),
                        events,
                        ..Default::default()
                    },
                    |ui| {
                        ui.interact(
                            Rect::from_center_size(position(180.0), egui::vec2(4.0, 200.0)),
                            panel_id.with("__resize"),
                            egui::Sense::drag(),
                        );
                        hidden.set(sidebars::resize_released_at_window_edge(
                            &ctx, panel_id, edge,
                        ));
                    },
                );
            };
            let button = |pos, pressed| egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: egui::Modifiers::NONE,
            };
            render(vec![]);
            render(vec![egui::Event::PointerMoved(position(180.0))]);
            render(vec![button(position(180.0), true)]);
            render(vec![egui::Event::PointerMoved(position(160.0))]);
            render(vec![egui::Event::PointerMoved(position(distance))]);
            assert!(!hidden.get(), "must not hide while dragging");
            let release = position(if cancel { 100.0 } else { distance });
            if cancel {
                render(vec![egui::Event::PointerMoved(release)]);
                assert!(!hidden.get());
            }
            render(vec![button(release, false)]);
            assert_eq!(
                hidden.get(),
                expected,
                "distance={distance}, cancel={cancel}"
            );
            render(vec![]);
            assert!(!hidden.get(), "release is a one-shot action");
        }
    }
}

#[test]
fn sidebar_drag_past_fixed_edge_stays_at_minimum_and_recovers() {
    for edge in [SidebarEdge::Left, SidebarEdge::Right] {
        let ctx = egui::Context::default();
        let rect = Cell::new(Rect::NOTHING);
        let render = |events| {
            let _ = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(600.0, 300.0))),
                    events,
                    ..Default::default()
                },
                |ui| {
                    let panel = match edge {
                        SidebarEdge::Left => egui::Panel::right("drag_sidebar"),
                        SidebarEdge::Right => egui::Panel::left("drag_sidebar"),
                    }
                    .frame(egui::Frame::NONE)
                    .default_size(150.0)
                    .size_range(50.0..=250.0);
                    let response = show_resizable_sidebar(
                        panel,
                        ui,
                        Id::new("drag_sidebar"),
                        edge,
                        50.0..=250.0,
                        |ui| ui.set_min_size(ui.available_size()),
                    );
                    rect.set(response.response.rect);
                    paint_sidebar_resize_edge(ui, Id::new("drag_sidebar"), rect.get(), edge, true);
                },
            );
        };
        render(vec![]);
        let (fixed, direction) = match edge {
            SidebarEdge::Left => (rect.get().right(), -1.0),
            SidebarEdge::Right => (rect.get().left(), 1.0),
        };
        let position = |width: f32| Pos2::new(fixed + direction * width, 150.0);
        render(vec![egui::Event::PointerMoved(position(150.0))]);
        render(vec![egui::Event::PointerButton {
            pos: position(150.0),
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers: egui::Modifiers::NONE,
        }]);
        for (requested, expected) in [
            (100.0, 100.0),
            (20.0, 50.0),
            (-100.0, 50.0),
            (-400.0, 50.0),
            (120.0, 120.0),
            (400.0, 250.0),
        ] {
            render(vec![egui::Event::PointerMoved(position(requested))]);
            assert!(
                (rect.get().width() - expected).abs() < 0.1,
                "requested {requested}, expected {expected}, got {}",
                rect.get().width()
            );
        }
        render(vec![egui::Event::PointerButton {
            pos: position(400.0),
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: egui::Modifiers::NONE,
        }]);
        render(vec![]);
        assert!((rect.get().width() - 250.0).abs() < 0.1);
    }
}

#[test]
fn highlight_falls_off_symmetrically_to_zero() {
    assert_eq!(sidebar_highlight_alpha(0.0), 1.0);
    assert_eq!(sidebar_highlight_alpha(SIDEBAR_HIGHLIGHT_RADIUS), 0.0);
    assert_eq!(
        sidebar_highlight_alpha(SIDEBAR_HIGHLIGHT_RADIUS + 20.0),
        0.0
    );

    let samples = [10.0, 30.0, 60.0, 80.0];
    for distance in samples {
        assert_eq!(
            sidebar_highlight_alpha(distance),
            sidebar_highlight_alpha(-distance)
        );
    }
    assert!(
        samples
            .windows(2)
            .all(|pair| { sidebar_highlight_alpha(pair[0]) > sidebar_highlight_alpha(pair[1]) })
    );
}

#[test]
fn central_margin_preserves_sidebar_gaps_only_when_present() {
    let both = central_workspace_margin(true, true);
    assert_eq!((both.left, both.right), (8, 8));

    let primary_only = central_workspace_margin(true, false);
    assert_eq!((primary_only.left, primary_only.right), (8, 4));

    let neither = central_workspace_margin(false, false);
    assert_eq!((neither.left, neither.right), (4, 4));
}

#[test]
fn resize_cursor_uses_two_point_radius_on_both_sidebar_edges() {
    for edge in [SidebarEdge::Left, SidebarEdge::Right] {
        assert_eq!(
            cursor_at_edge_offset(edge, 1.5),
            egui::CursorIcon::ResizeHorizontal
        );
        assert_eq!(cursor_at_edge_offset(edge, 2.5), egui::CursorIcon::Default);
    }
}

fn cursor_at_edge_offset(edge: SidebarEdge, offset: f32) -> egui::CursorIcon {
    let ctx = egui::Context::default();
    let screen_rect = Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(400.0, 300.0)));
    let boundary = Cell::new(0.0);
    let render = |events| {
        ctx.run_ui(
            egui::RawInput {
                screen_rect,
                events,
                ..Default::default()
            },
            |ui| {
                let panel = match edge {
                    SidebarEdge::Left => egui::Panel::right("test_sidebar"),
                    SidebarEdge::Right => egui::Panel::left("test_sidebar"),
                }
                .frame(egui::Frame::NONE)
                .default_size(100.0)
                .size_range(50.0..=200.0);
                let response = show_resizable_sidebar(
                    panel,
                    ui,
                    Id::new("test_sidebar"),
                    edge,
                    50.0..=200.0,
                    |ui| {
                        ui.set_min_size(ui.available_size());
                    },
                );
                boundary.set(match edge {
                    SidebarEdge::Left => response.response.rect.left(),
                    SidebarEdge::Right => response.response.rect.right(),
                });
                paint_sidebar_resize_edge(
                    ui,
                    Id::new("test_sidebar"),
                    response.response.rect,
                    edge,
                    true,
                );
                egui::CentralPanel::default().show_inside(ui, |_| {});
            },
        )
    };

    let _ = render(Vec::new());
    let pointer_x = match edge {
        SidebarEdge::Left => boundary.get() - offset,
        SidebarEdge::Right => boundary.get() + offset,
    };
    render(vec![egui::Event::PointerMoved(Pos2::new(pointer_x, 150.0))])
        .platform_output
        .cursor_icon
}
