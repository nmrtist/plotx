use super::*;

fn page(board_pos: [f32; 2]) -> CanvasDocument {
    let mut canvas = CanvasDocument::new("p".to_owned(), [100.0, 100.0]);
    canvas.board_pos = board_pos;
    canvas
}

fn unit_board() -> BoardViewport {
    BoardViewport {
        zoom: 1.0,
        world_center: [1000.0, 1000.0],
    }
}

fn screen() -> egui::Rect {
    egui::Rect::from_min_size(Pos2::ZERO, egui::vec2(2000.0, 2000.0))
}

fn workspace(board_rect: egui::Rect) -> crate::ui::workspace_geometry::WorkspaceGeometry {
    crate::ui::workspace_geometry::WorkspaceGeometry {
        board_rect,
        fit_occluders: Vec::new(),
        revision: 0,
    }
}

fn app_with_pages(positions: &[[f32; 2]]) -> PlotxApp {
    let mut app = PlotxApp::new();
    for &bp in positions {
        app.doc.canvases.push(page(bp));
    }
    app.session.board = unit_board();
    app
}

#[test]
fn frame_at_returns_hit_page() {
    let app = app_with_pages(&[[0.0, 0.0], [500.0, 0.0]]);
    let screen = screen();

    assert_eq!(
        frame_at(&app, screen, Pos2::new(10.0, 12.0)),
        Some(FrameRef::Page(0))
    );
    assert_eq!(
        frame_at(&app, screen, Pos2::new(510.0, 12.0)),
        Some(FrameRef::Page(1))
    );
    assert!(frame_at(&app, screen, Pos2::new(400.0, 12.0)).is_none());
}

#[test]
fn frame_at_prefers_active_frame_on_overlap() {
    let mut app = app_with_pages(&[[0.0, 0.0], [50.0, 0.0]]);
    let screen = screen();
    let p = Pos2::new(60.0, 12.0);

    app.session.active_canvas = Some(1);
    assert_eq!(frame_at(&app, screen, p), Some(FrameRef::Page(1)));
    app.session.active_canvas = Some(0);
    assert_eq!(frame_at(&app, screen, p), Some(FrameRef::Page(0)));
    app.session.active_canvas = None;
    assert_eq!(frame_at(&app, screen, p), Some(FrameRef::Page(1)));
}

#[test]
fn frame_header_at_hits_strip_above_page() {
    let app = app_with_pages(&[[0.0, 0.0]]);
    let screen = screen();
    assert_eq!(
        frame_header_at(&app, screen, Pos2::new(10.0, -5.0)),
        Some(FrameRef::Page(0))
    );
    assert_eq!(frame_header_at(&app, screen, Pos2::new(10.0, 10.0)), None);
}

#[test]
fn toggle_frame_selection_adds_and_removes() {
    let mut app = app_with_pages(&[[0.0, 0.0]]);
    plotx_core::state::toggle_frame_selection(&mut app, FrameRef::Page(0));
    assert_eq!(
        app.session.ui.frame_selection,
        vec![plotx_core::state::board_frame_id(&app, FrameRef::Page(0)).unwrap()]
    );
    plotx_core::state::toggle_frame_selection(&mut app, FrameRef::Page(0));
    assert!(app.session.ui.frame_selection.is_empty());
}

#[test]
fn zoom_to_selection_targets_selected_then_all_frames() {
    let mut app = app_with_pages(&[[0.0, 0.0], [1000.0, 0.0]]);
    let ctx = egui::Context::default();

    app.session.ui.frame_selection =
        vec![plotx_core::state::board_frame_id(&app, FrameRef::Page(1)).unwrap()];
    zoom_to_selection(&mut app, &ctx);
    let r = app.doc.canvases[1].board_rect_pt();
    match app.session.viewport_mode {
        ViewportMode::Fit(BoardFitTarget::Region(b)) => {
            assert!((b[0] - r.left).abs() < 1e-3 && (b[2] - r.right()).abs() < 1e-3);
        }
        other => panic!("expected a region fit, got {other:?}"),
    }

    app.session.ui.frame_selection.clear();
    zoom_to_selection(&mut app, &ctx);
    let all = all_frames_bbox(&app).unwrap();
    match app.session.viewport_mode {
        ViewportMode::Fit(BoardFitTarget::Region(b)) => {
            assert!((b[0] - all.0).abs() < 1e-3 && (b[2] - all.2).abs() < 1e-3);
        }
        other => panic!("expected a region fit, got {other:?}"),
    }
}

#[test]
fn request_board_fit_viewport_targets_exact_camera() {
    let mut app = app_with_pages(&[[0.0, 0.0]]);
    let ctx = egui::Context::default();
    request_board_fit_viewport(&mut app, &ctx, 2.5, [30.0, -40.0]);
    assert_eq!(
        app.session.viewport_mode,
        ViewportMode::Fit(BoardFitTarget::Viewport {
            zoom: 2.5,
            world_center: [30.0, -40.0]
        })
    );
}

#[test]
fn transient_reveal_is_consumed_into_the_fit_animation() {
    let mut app = app_with_pages(&[[0.0, 0.0]]);
    let ctx = egui::Context::default();
    let page_id = app.doc.canvases[0].resource_id;
    app.session.board_reveal = Some(plotx_core::state::BoardFrameId::Page(page_id));

    consume_board_reveal(&mut app, &ctx);

    assert_eq!(app.session.board_reveal, None);
    assert_eq!(
        app.session.viewport_mode,
        ViewportMode::Fit(BoardFitTarget::Frame(
            plotx_core::state::BoardFrameId::Page(page_id)
        ))
    );
}

#[test]
fn reveal_and_fit_keep_target_identity_when_page_indices_shift() {
    let mut app = app_with_pages(&[[0.0, 0.0], [500.0, 0.0]]);
    let ctx = egui::Context::default();
    let target_id = app.doc.canvases[1].resource_id;
    app.session.board_reveal = Some(plotx_core::state::BoardFrameId::Page(target_id));

    app.doc.canvases.swap(0, 1);
    consume_board_reveal(&mut app, &ctx);

    let target = plotx_core::state::BoardFrameId::Page(target_id);
    assert_eq!(
        app.session.viewport_mode,
        ViewportMode::Fit(BoardFitTarget::Frame(target))
    );
    assert_eq!(board_frame_ref(&app, target), Some(FrameRef::Page(0)));

    app.doc.canvases.swap(0, 1);
    assert_eq!(board_frame_ref(&app, target), Some(FrameRef::Page(1)));
}

#[test]
fn reveal_of_a_removed_page_is_discarded_without_retargeting() {
    let mut app = app_with_pages(&[[0.0, 0.0], [500.0, 0.0]]);
    let ctx = egui::Context::default();
    let removed_id = app.doc.canvases[0].resource_id;
    app.session.board_reveal = Some(plotx_core::state::BoardFrameId::Page(removed_id));

    app.doc.canvases.remove(0);
    consume_board_reveal(&mut app, &ctx);

    assert_eq!(app.session.board_reveal, None);
    assert_eq!(
        app.session.viewport_mode,
        ViewportMode::Fit(BoardFitTarget::AllFrames)
    );
}

#[test]
fn fit_animation_cancels_when_its_stable_target_is_removed() {
    let mut app = app_with_pages(&[[0.0, 0.0], [500.0, 0.0]]);
    let ctx = egui::Context::default();
    request_board_fit(&mut app, &ctx, FrameRef::Page(0));
    assert!(matches!(app.session.viewport_mode, ViewportMode::Fit(_)));

    app.doc.canvases.remove(0);
    let rect = screen();
    let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
        drive_board_fit(&mut app, ui, &workspace(rect));
    });

    assert_eq!(app.session.viewport_mode, ViewportMode::Manual);
}

#[test]
fn fit_intent_survives_settling_and_resolves_again_for_a_new_layout() {
    let mut app = app_with_pages(&[[0.0, 0.0]]);
    let ctx = egui::Context::default();
    request_board_fit(&mut app, &ctx, FrameRef::Page(0));
    let wide = egui::Rect::from_min_size(Pos2::ZERO, egui::vec2(1200.0, 800.0));
    for _ in 0..180 {
        let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
            drive_board_fit(&mut app, ui, &workspace(wide));
        });
    }
    let wide_camera = app.session.board;
    assert!(matches!(
        app.session.viewport_mode,
        ViewportMode::Fit(BoardFitTarget::Frame(_))
    ));

    let narrow = egui::Rect::from_min_size(Pos2::ZERO, egui::vec2(700.0, 800.0));
    let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
        drive_board_fit(&mut app, ui, &workspace(narrow));
    });

    assert!(app.session.board.zoom < wide_camera.zoom);
    assert!(matches!(
        app.session.viewport_mode,
        ViewportMode::Fit(BoardFitTarget::Frame(_))
    ));
}

#[test]
fn fit_uses_the_open_space_below_a_floating_task_card_when_it_is_better() {
    let ctx = egui::Context::default();
    let board = egui::Rect::from_min_size(Pos2::ZERO, egui::vec2(1000.0, 700.0));
    let geometry = crate::ui::workspace_geometry::WorkspaceGeometry {
        board_rect: board,
        fit_occluders: vec![egui::Rect::from_min_max(
            egui::pos2(680.0, 0.0),
            egui::pos2(1000.0, 420.0),
        )],
        revision: 1,
    };

    let _ = fit_bbox_around_occluders((0.0, 0.0, 800.0, 100.0), &geometry, &ctx);

    assert_eq!(
        ctx.data(|data| data.get_temp::<usize>(egui::Id::new(FIT_CANDIDATE_ID))),
        Some(1),
        "a wide target should use the full-width space below the card"
    );
}

#[test]
fn floating_task_cards_do_not_change_a_manual_camera() {
    let mut app = app_with_pages(&[[0.0, 0.0]]);
    app.session.viewport_mode = ViewportMode::Manual;
    app.session.board = BoardViewport {
        zoom: 1.75,
        world_center: [240.0, 180.0],
    };
    let camera = app.session.board;
    let board = screen();
    let geometry = crate::ui::workspace_geometry::WorkspaceGeometry {
        board_rect: board,
        fit_occluders: vec![egui::Rect::from_min_max(
            egui::pos2(1500.0, 0.0),
            egui::pos2(2000.0, 900.0),
        )],
        revision: 1,
    };
    let ctx = egui::Context::default();

    let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
        drive_board_fit(&mut app, ui, &geometry);
    });

    assert_eq!(app.session.board, camera);
}

#[test]
fn dragging_a_focused_frame_immediately_takes_ownership_from_fit() {
    let mut app = app_with_pages(&[[100.0, 100.0]]);
    let ctx = egui::Context::default();
    request_board_fit(&mut app, &ctx, FrameRef::Page(0));
    let rect = screen();
    let header = Pos2::new(120.0, 90.0);

    let frame = |app: &mut PlotxApp, events| {
        let _ = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(rect),
                events,
                ..Default::default()
            },
            |ui| {
                handle_frame_drag(app, rect, ui);
            },
        );
    };
    frame(
        &mut app,
        vec![
            egui::Event::PointerMoved(header),
            egui::Event::PointerButton {
                pos: header,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::default(),
            },
        ],
    );
    assert_eq!(app.session.viewport_mode, ViewportMode::Manual);

    frame(
        &mut app,
        vec![egui::Event::PointerMoved(header + egui::vec2(80.0, 50.0))],
    );
    assert_eq!(app.doc.canvases[0].board_pos, [180.0, 150.0]);

    let moved = app.doc.canvases[0].board_pos;
    let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
        drive_board_fit(&mut app, ui, &workspace(rect));
    });
    assert_eq!(app.doc.canvases[0].board_pos, moved);
}

/// Presses the primary button at `p` with a right side bar covering x >= 800.
/// The first pass registers the layers; only the second one carries the press.
fn press_with_side_bar(app: &mut PlotxApp, p: Pos2) {
    // Use a deterministic manual camera with the world origin at the visible
    // board's top-left corner, so each pointer targets a known frame.
    app.session.board.world_center = [400.0, 400.0];
    app.session.viewport_mode = ViewportMode::Manual;
    let ctx = egui::Context::default();
    let screen_rect = Some(egui::Rect::from_min_size(
        Pos2::ZERO,
        egui::vec2(1000.0, 800.0),
    ));
    let frame = |app: &mut PlotxApp, events: Vec<egui::Event>| {
        let input = egui::RawInput {
            screen_rect,
            events,
            ..Default::default()
        };
        let _ = ctx.run_ui(input, |ui| {
            egui::Panel::right("secondary_sidebar")
                .resizable(false)
                .default_size(200.0)
                .show_inside(ui, |ui| {
                    let _ = ui.button("a tool button");
                });
            egui::CentralPanel::default().show_inside(ui, |ui| render_central(app, ui));
        });
    };

    frame(app, vec![egui::Event::PointerMoved(p)]);
    frame(
        app,
        vec![
            egui::Event::PointerMoved(p),
            egui::Event::PointerButton {
                pos: p,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::default(),
            },
        ],
    );
}

#[test]
fn side_bar_press_does_not_activate_the_frame_under_it() {
    // Page 1 lies under the side bar: its board rect spans screen x 900..1000.
    let mut app = app_with_pages(&[[0.0, 0.0], [900.0, 0.0]]);
    app.session.active_canvas = Some(0);

    press_with_side_bar(&mut app, Pos2::new(940.0, 60.0));

    assert_eq!(app.session.active_canvas, Some(0));
}

#[test]
fn canvas_press_still_activates_the_frame_under_it() {
    let mut app = app_with_pages(&[[0.0, 0.0], [900.0, 0.0]]);
    app.session.active_canvas = Some(1);

    press_with_side_bar(&mut app, Pos2::new(50.0, 60.0));

    assert_eq!(app.session.active_canvas, Some(0));
}

#[test]
fn frame_at_and_header_hit_sheet_frames() {
    let mut app = app_with_pages(&[[0.0, 0.0]]);
    let mut sheet = plotx_core::state::materialized_float_series_table(
        ("x".into(), "".into(), vec![Some(0.0), Some(1.0)]),
        Vec::new(),
        "plotx.test.sheet.v1",
    )
    .unwrap();
    sheet.board_pos = [600.0, 0.0];
    app.doc.datasets.push(Dataset::Table(Box::new(sheet)));
    let screen = screen();

    let r = app.doc.datasets[0].as_table().unwrap().board_rect_pt();
    assert_eq!(
        frame_at(&app, screen, Pos2::new(r.left + 5.0, r.top + 5.0)),
        Some(FrameRef::Sheet(0))
    );
    assert_eq!(
        frame_header_at(&app, screen, Pos2::new(r.left + 5.0, r.top - 5.0)),
        Some(FrameRef::Sheet(0))
    );
}
