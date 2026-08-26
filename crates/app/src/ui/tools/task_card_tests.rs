use super::*;
use plotx_core::state::{CanvasDocument, Dataset, NmrDataset};
use plotx_io::{Domain, NmrData};
use std::cell::Cell;

fn app_with_task(tab: TaskDockTab, collapsed: bool) -> PlotxApp {
    let mut app = PlotxApp::new();
    let data = NmrData {
        points: vec![num_complex::Complex64::new(0.0, 0.0); 2],
        domain: Domain::Time,
        spectral_width_hz: 1.0,
        observe_freq_mhz: 1.0,
        carrier_ppm: 0.0,
        nucleus: "1H".into(),
        source: "test".into(),
        group_delay: 0.0,
    };
    app.doc
        .datasets
        .push(Dataset::Nmr(Box::new(NmrDataset::load(data))));
    app.doc
        .canvases
        .push(CanvasDocument::new("p".into(), [100.0, 80.0]));
    app.focus_single(0);
    app.session.ui.task_dock_active = Some(tab);
    let id = app.doc.datasets[0].resource_id();
    match tab {
        TaskDockTab::Processing => {
            app.session.ui.processing_task_dataset = Some(id);
            app.session.ui.processing_task_collapsed = collapsed;
        }
        TaskDockTab::Regions => {
            app.session.ui.region_task_dataset = Some(id);
            app.session.ui.region_task_collapsed = collapsed;
        }
        _ => unreachable!(),
    }
    app
}

#[test]
fn visible_task_uses_the_area_id_that_is_actually_rendered() {
    let processing = app_with_task(TaskDockTab::Processing, false);
    let regions = app_with_task(TaskDockTab::Regions, true);

    assert_eq!(
        visible_area_id(&processing),
        Some(Id::new("processing_task_card"))
    );
    assert_eq!(visible_area_id(&regions), Some(Id::new("region_task_card")));
}

#[test]
fn task_card_geometry_uses_the_central_board_boundary() {
    let app = PlotxApp::new();
    let ctx = egui::Context::default();
    let screen = egui::Rect::from_min_size(Pos2::ZERO, egui::vec2(1000.0, 700.0));
    let board = egui::Rect::from_min_max(egui::pos2(180.0, 80.0), egui::pos2(760.0, 680.0));
    let observed = Cell::new(None);

    let _ = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(screen),
            ..Default::default()
        },
        |ui| {
            crate::ui::workspace_geometry::resolve(&app, board, ui.ctx());
            observed.set(Some(geometry(
                &app,
                ui,
                TaskDockTab::Processing,
                200.0,
                false,
            )));
        },
    );

    let card = observed.take().expect("task-card geometry");
    assert!(card.pos.x >= board.left());
    assert_eq!(card.pos.x + card.width, board.right());
    assert!(card.pos.y >= board.top());
}

#[test]
fn narrow_boards_keep_a_valid_card_width() {
    let board = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(250.0, 140.0));

    let width = card_width(board, TaskDockTab::Processing, 340.0);
    assert!(width < 340.0);
    assert!(width > 0.0);
}

#[test]
fn title_drag_moves_the_shared_task_card() {
    let ctx = egui::Context::default();
    let mut fonts = egui::FontDefinitions::default();
    let emphasized = fonts
        .families
        .get(&egui::FontFamily::Proportional)
        .cloned()
        .expect("default proportional fonts");
    fonts.families.insert(
        egui::FontFamily::Name(crate::typography::EMPHASIZED_FAMILY_NAME.into()),
        emphasized,
    );
    ctx.set_fonts(fonts);
    crate::typography::apply(&ctx);
    let screen = egui::Rect::from_min_size(Pos2::ZERO, egui::vec2(640.0, 480.0));
    let area_id = Id::new("test_task_card");
    let start = Pos2::new(100.0, 80.0);
    let mut app = PlotxApp::new_with_settings(plotx_core::settings::Settings::default());

    let mut frame = |events| {
        let input = egui::RawInput {
            screen_rect: Some(screen),
            events,
            ..Default::default()
        };
        let _ = ctx.run_ui(input, |ui| {
            egui::CentralPanel::default().show_inside(ui, |ui| {
                let pos = ui
                    .ctx()
                    .data(|data| data.get_temp::<CardLayout>(area_id.with("layout")))
                    .map_or(start, |layout| layout.rect.min);
                area(ui, area_id, pos).show(ui.ctx(), |ui| {
                    ui.set_width(COLLAPSED_WIDTH);
                    header(ui, area_id, "Processing", None::<&str>, |ui| {
                        let _ = ui.button("Close");
                    });
                    sized_body(ui, 120.0, |ui| {
                        ui.label("Body");
                    });
                    resize_handles(
                        &mut app,
                        ui,
                        area_id,
                        TaskDockTab::Processing,
                        COLLAPSED_WIDTH,
                        120.0,
                    );
                });
            });
        });
    };

    frame(Vec::new());
    frame(Vec::new());
    let initial = ctx
        .memory(|memory| memory.area_rect(area_id))
        .expect("laid-out task card");
    // Start directly over the rendered title text, not merely empty chrome.
    let pointer_start = initial.min + egui::vec2(20.0, 12.0);
    let pointer_end = pointer_start + egui::vec2(120.0, 100.0);
    frame(vec![egui::Event::PointerMoved(pointer_start)]);
    let before_drag = ctx
        .memory(|memory| memory.area_rect(area_id))
        .expect("hovered task card");
    frame(vec![
        egui::Event::PointerMoved(pointer_start),
        egui::Event::PointerButton {
            pos: pointer_start,
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers: egui::Modifiers::default(),
        },
    ]);
    frame(vec![egui::Event::PointerMoved(pointer_end)]);
    frame(vec![egui::Event::PointerButton {
        pos: pointer_end,
        button: egui::PointerButton::Primary,
        pressed: false,
        modifiers: egui::Modifiers::default(),
    }]);

    let moved = ctx
        .memory(|memory| memory.area_rect(area_id))
        .expect("task card area");
    assert_eq!(moved.min, before_drag.min + (pointer_end - pointer_start));
}

#[test]
fn collapsed_cards_use_the_compact_width_without_overwriting_the_preference() {
    let app = PlotxApp::new_with_settings(plotx_core::settings::Settings::default());
    let board = egui::Rect::from_min_size(Pos2::ZERO, egui::vec2(1_000.0, 700.0));

    assert_eq!(COLLAPSED_WIDTH.min(board.width()), 310.0);
    assert_eq!(preferred_size(&app, TaskDockTab::Craft).width, 520.0);
}

#[test]
fn resize_clamps_every_edge_without_moving_the_opposite_edge() {
    let bounds = egui::Rect::from_min_max(egui::pos2(20.0, 30.0), egui::pos2(780.0, 690.0));
    let origin = ResizeOrigin {
        rect: egui::Rect::from_min_max(egui::pos2(400.0, 80.0), egui::pos2(760.0, 600.0)),
        chrome_height: 72.0,
    };
    let west = resized_rect(
        origin,
        ResizeEdges {
            left: true,
            right: false,
            top: false,
            bottom: false,
        },
        Vec2::new(-1_000.0, 0.0),
        bounds,
        300.0,
        520.0,
    );
    assert_eq!(west.left(), origin.rect.right() - 520.0);
    assert_eq!(west.right(), origin.rect.right());

    let south_east = resized_rect(
        origin,
        ResizeEdges {
            left: false,
            right: true,
            top: false,
            bottom: true,
        },
        Vec2::splat(1_000.0),
        bounds,
        300.0,
        520.0,
    );
    assert_eq!(south_east.right(), bounds.right());
    assert_eq!(south_east.bottom(), bounds.bottom());
    assert_eq!(south_east.min, origin.rect.min);
}

#[test]
fn maximum_vertical_resize_aligns_with_the_sidebar_bottom() {
    let bounds = egui::Rect::from_min_max(egui::pos2(20.0, 30.0), egui::pos2(780.0, 690.0));
    let origin = ResizeOrigin {
        rect: egui::Rect::from_min_max(egui::pos2(400.0, 80.0), egui::pos2(760.0, 500.0)),
        chrome_height: 72.0,
    };
    let resized = resized_rect(
        origin,
        ResizeEdges {
            left: false,
            right: false,
            top: false,
            bottom: true,
        },
        Vec2::new(0.0, 1_000.0),
        bounds,
        300.0,
        520.0,
    );

    assert_eq!(resized.bottom(), bounds.bottom());
    assert_eq!(resized.top(), origin.rect.top());

    let further = resized_rect(
        origin,
        ResizeEdges {
            left: false,
            right: false,
            top: false,
            bottom: true,
        },
        Vec2::new(0.0, 2_000.0),
        bounds,
        300.0,
        520.0,
    );
    assert_eq!(further, resized);
}

#[test]
fn shrinking_from_the_left_keeps_the_right_edge_fixed() {
    let bounds = egui::Rect::from_min_max(egui::pos2(20.0, 30.0), egui::pos2(780.0, 690.0));
    let origin = ResizeOrigin {
        rect: egui::Rect::from_min_max(egui::pos2(240.0, 80.0), egui::pos2(760.0, 600.0)),
        chrome_height: 72.0,
    };
    let resized = resized_rect(
        origin,
        ResizeEdges {
            left: true,
            right: false,
            top: false,
            bottom: false,
        },
        Vec2::new(140.0, 0.0),
        bounds,
        300.0,
        520.0,
    );

    assert_eq!(resized.left(), origin.rect.left() + 140.0);
    assert_eq!(resized.right(), origin.rect.right());
}

fn layout(rect: egui::Rect, bounds: egui::Rect) -> CardLayout {
    CardLayout {
        rect,
        bounds,
        horizontal: HorizontalAnchor::Right,
        vertical: VerticalAnchor::Top,
        chrome_height: 70.0,
        extra_width: 0.0,
        collapsed: false,
    }
}

#[test]
fn right_anchored_width_change_is_atomic() {
    let bounds = egui::Rect::from_min_max(Pos2::ZERO, egui::pos2(900.0, 700.0));
    let original = layout(
        egui::Rect::from_min_max(egui::pos2(380.0, 40.0), egui::pos2(880.0, 600.0)),
        bounds,
    );

    let narrower = fit_layout(original, bounds, Vec2::new(360.0, 560.0), false);

    assert_eq!(narrower.rect.right(), original.rect.right());
    assert_eq!(narrower.rect.left(), original.rect.right() - 360.0);
}

#[test]
fn bottom_right_resize_keeps_the_top_left_corner() {
    let bounds = egui::Rect::from_min_max(Pos2::ZERO, egui::pos2(900.0, 700.0));
    let mut original = layout(
        egui::Rect::from_min_max(egui::pos2(240.0, 40.0), egui::pos2(600.0, 500.0)),
        bounds,
    );
    original.horizontal = HorizontalAnchor::Left;

    let larger = fit_layout(original, bounds, Vec2::new(500.0, 620.0), false);

    assert_eq!(larger.rect.min, original.rect.min);
}

#[test]
fn shrinking_viewport_preserves_top_anchor_without_overflow() {
    let bounds = egui::Rect::from_min_max(Pos2::ZERO, egui::pos2(900.0, 700.0));
    let original = layout(
        egui::Rect::from_min_max(egui::pos2(380.0, 40.0), egui::pos2(880.0, 700.0)),
        bounds,
    );
    let shorter = egui::Rect::from_min_max(Pos2::ZERO, egui::pos2(900.0, 480.0));

    let fitted = fit_layout(original, shorter, Vec2::new(500.0, 660.0), false);

    assert_eq!(fitted.rect.top(), original.rect.top());
    assert_eq!(fitted.rect.bottom(), shorter.bottom());
}

#[test]
fn left_anchored_card_follows_primary_sidebar_boundary() {
    let bounds = egui::Rect::from_min_max(egui::pos2(200.0, 0.0), egui::pos2(900.0, 700.0));
    let mut original = layout(
        egui::Rect::from_min_max(egui::pos2(200.0, 40.0), egui::pos2(600.0, 600.0)),
        bounds,
    );
    original.horizontal = HorizontalAnchor::Left;
    let sidebar_wider = egui::Rect::from_min_max(egui::pos2(320.0, 0.0), egui::pos2(900.0, 700.0));

    let fitted = fit_layout(original, sidebar_wider, Vec2::new(400.0, 560.0), false);

    assert_eq!(fitted.rect.left(), sidebar_wider.left());
    assert!(fitted.rect.right() <= sidebar_wider.right());
}

#[test]
fn right_anchored_card_follows_secondary_sidebar_visibility() {
    let without_sidebar =
        egui::Rect::from_min_max(egui::pos2(200.0, 0.0), egui::pos2(1196.0, 700.0));
    let original = layout(
        egui::Rect::from_min_max(egui::pos2(696.0, 40.0), egui::pos2(1196.0, 600.0)),
        without_sidebar,
    );
    let with_sidebar = egui::Rect::from_min_max(egui::pos2(200.0, 0.0), egui::pos2(892.0, 700.0));

    let fitted = fit_layout(original, with_sidebar, Vec2::new(500.0, 560.0), false);

    assert_eq!(fitted.rect.right(), with_sidebar.right());
    assert!(with_sidebar.contains(fitted.rect.min));
    assert!(with_sidebar.contains(fitted.rect.max));
}

#[test]
fn rendered_card_and_title_actions_stay_inside_the_workspace() {
    let ctx = egui::Context::default();
    let mut fonts = egui::FontDefinitions::default();
    let emphasized = fonts.families[&egui::FontFamily::Proportional].clone();
    fonts.families.insert(
        egui::FontFamily::Name(crate::typography::EMPHASIZED_FAMILY_NAME.into()),
        emphasized,
    );
    ctx.set_fonts(fonts);
    crate::typography::apply(&ctx);
    let screen = egui::Rect::from_min_size(Pos2::ZERO, Vec2::new(1_000.0, 700.0));
    let mut app = PlotxApp::new_with_settings(plotx_core::settings::Settings::default());
    let action_rect = Cell::new(None);
    let mut frame = |board: egui::Rect| {
        let _ = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(screen),
                ..Default::default()
            },
            |ui| {
                crate::ui::workspace_geometry::resolve(&app, board, ui.ctx());
                let card = geometry(&app, ui, TaskDockTab::Processing, 120.0, false);
                let id = area_id(TaskDockTab::Processing);
                area(ui, id, card.pos).show(ui.ctx(), |ui| {
                    ui.set_width(card.width);
                    crate::ui::card_frame(false, egui::Margin::ZERO).show(ui, |ui| {
                        header(
                            ui,
                            id,
                            "Processing",
                            Some("A deliberately long status that must yield to actions"),
                            |ui| action_rect.set(Some(ui.small_button("Close").rect)),
                        );
                        sized_body(ui, card.body_height, |ui| {
                            ui.label("Body");
                        });
                    });
                    resize_handles(
                        &mut app,
                        ui,
                        id,
                        TaskDockTab::Processing,
                        card.width,
                        card.body_height,
                    );
                });
            },
        );
    };

    let wide = egui::Rect::from_min_max(egui::pos2(100.0, 60.0), egui::pos2(996.0, 680.0));
    frame(wide);
    frame(wide);
    let wide_card = ctx
        .memory(|memory| memory.area_rect(area_id(TaskDockTab::Processing)))
        .expect("right-anchored task card");
    assert_eq!(wide_card.right(), wide.right());
    let narrow = egui::Rect::from_min_max(egui::pos2(100.0, 60.0), egui::pos2(692.0, 680.0));
    frame(narrow);
    frame(narrow);

    let card = ctx
        .memory(|memory| memory.area_rect(area_id(TaskDockTab::Processing)))
        .expect("rendered task card");
    let action = action_rect.get().expect("title action");
    assert_eq!(card.right(), narrow.right());
    assert!(action.right() <= card.right());
}
