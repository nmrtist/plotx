use super::*;

fn page(board_pos: [f32; 2]) -> CanvasDocument {
    let mut canvas = CanvasDocument::new("p".to_owned(), [100.0, 100.0]);
    canvas.board_pos = board_pos;
    canvas
}

fn unit_board() -> BoardViewport {
    BoardViewport {
        zoom: 1.0,
        pan: [0.0, 0.0],
        auto_fit: false,
    }
}

fn screen() -> egui::Rect {
    egui::Rect::from_min_size(Pos2::ZERO, egui::vec2(2000.0, 2000.0))
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
    assert_eq!(app.session.ui.frame_selection, vec![FrameRef::Page(0)]);
    plotx_core::state::toggle_frame_selection(&mut app, FrameRef::Page(0));
    assert!(app.session.ui.frame_selection.is_empty());
}

#[test]
fn zoom_to_selection_targets_selected_then_all_frames() {
    let mut app = app_with_pages(&[[0.0, 0.0], [1000.0, 0.0]]);
    let ctx = egui::Context::default();

    app.session.ui.frame_selection = vec![FrameRef::Page(1)];
    zoom_to_selection(&mut app, &ctx);
    let r = app.doc.canvases[1].board_rect_pt();
    match app.session.board_fit {
        Some(BoardFitTarget::Region(b)) => {
            assert!((b[0] - r.left).abs() < 1e-3 && (b[2] - r.right()).abs() < 1e-3);
        }
        other => panic!("expected a region fit, got {other:?}"),
    }

    app.session.ui.frame_selection.clear();
    zoom_to_selection(&mut app, &ctx);
    let all = all_frames_bbox(&app).unwrap();
    match app.session.board_fit {
        Some(BoardFitTarget::Region(b)) => {
            assert!((b[0] - all.0).abs() < 1e-3 && (b[2] - all.2).abs() < 1e-3);
        }
        other => panic!("expected a region fit, got {other:?}"),
    }
}

#[test]
fn request_board_fit_viewport_targets_exact_zoom_and_pan() {
    let mut app = app_with_pages(&[[0.0, 0.0]]);
    let ctx = egui::Context::default();
    request_board_fit_viewport(&mut app, &ctx, 2.5, [30.0, -40.0]);
    assert_eq!(
        app.session.board_fit,
        Some(BoardFitTarget::Viewport {
            zoom: 2.5,
            pan: [30.0, -40.0]
        })
    );
}

/// Presses the primary button at `p` with a right side bar covering x >= 800.
/// The first pass registers the layers; only the second one carries the press.
fn press_with_side_bar(app: &mut PlotxApp, p: Pos2) {
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
