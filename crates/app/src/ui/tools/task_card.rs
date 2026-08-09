//! Shared geometry for canvas task cards. They start in the same canvas corner,
//! so their initial position and sizing rules live in one place.

use egui::{
    Align, Area, CursorIcon, Id, Layout, Order, Pos2, RichText, Sense, Ui, UiBuilder, Vec2,
};
use egui_phosphor::regular as icon;
use plotx_core::state::{PlotxApp, TaskDockTab, Tool};
use std::hash::Hash;

/// Width shared by every task card, and the gap it keeps from the canvas edges.
const WIDTH: f32 = 310.0;
const MARGIN: f32 = 12.0;
const TOP_OFFSET: f32 = 8.0;
/// Room the card header, frame and footprint need outside the resizable body.
const CHROME: f32 = 64.0;
/// Below this the body is useless anyway; the card is allowed to overhang.
const FLOOR: f32 = 120.0;
const COLLAPSED_HEIGHT: f32 = 96.0;
const MIN_SAFE_EDGE: f32 = 72.0;
const MIN_CARD_WIDTH: f32 = 120.0;

pub(super) struct TaskCardGeometry {
    pub pos: Pos2,
    pub width: f32,
    pub min_body_height: f32,
    pub max_body_height: f32,
}

/// Anchors a card to the host's top-right corner and sizes its body to the
/// height the canvas actually has.
///
/// `preferred_min_body` is honoured only while it fits: `egui::Resize` applies
/// `at_least(min).at_most(max)`, so a min taller than the host would win over
/// the fitted max and force the card past the canvas. `Area` then constrains it
/// to the screen and slides it up over the Ribbon, hiding the very buttons that
/// opened it. Clamping the min keeps a short window shrinking instead.
pub(super) fn geometry(host: &Ui, preferred_min_body: f32) -> TaskCardGeometry {
    let host_rect = host.max_rect();
    let width = card_width(host_rect);
    let pos = host_rect.right_top() + egui::vec2(-width - MARGIN, TOP_OFFSET);
    let max_body_height = (host_rect.bottom() - pos.y - CHROME).max(FLOOR);
    TaskCardGeometry {
        pos,
        width,
        min_body_height: preferred_min_body.min(max_body_height),
        max_body_height,
    }
}

fn card_width(host: egui::Rect) -> f32 {
    WIDTH.min(
        (host.width() - MIN_SAFE_EDGE - MARGIN * 3.0)
            .max(MIN_CARD_WIDTH)
            .min((host.width() - MARGIN * 2.0).max(1.0)),
    )
}

fn visible_card_collapsed(app: &PlotxApp) -> Option<bool> {
    let active = app.active_dataset();
    match app.session.ui.task_dock_active? {
        TaskDockTab::Processing => app
            .session
            .ui
            .processing_task_dataset
            .and_then(|id| app.doc.dataset_index(id))
            .filter(|dataset| Some(*dataset) == active)
            .map(|_| app.session.ui.processing_task_collapsed),
        TaskDockTab::Regions => app
            .session
            .ui
            .region_task_dataset
            .and_then(|id| app.doc.dataset_index(id))
            .filter(|dataset| Some(*dataset) == active)
            .map(|_| app.session.ui.region_task_collapsed),
        TaskDockTab::CurveFit => app
            .session
            .ui
            .curve_fit_task_dataset
            .filter(|dataset| Some(*dataset) == active)
            .map(|_| app.session.ui.curve_fit_task_collapsed),
        TaskDockTab::Statistics => app
            .session
            .ui
            .stat_task_dataset
            .filter(|dataset| Some(*dataset) == active)
            .map(|_| app.session.ui.stat_task_collapsed),
    }
}

/// Largest practical board-fit rectangle not covered by the visible task card.
/// Persistent sidebars are already excluded from `host`; expanded cards reserve
/// the right dock strip, while a collapsed one leaves the full-width band below
/// its header available.
pub(crate) fn safe_fit_rect(app: &PlotxApp, host: egui::Rect) -> egui::Rect {
    let Some(collapsed) = visible_card_collapsed(app) else {
        return host;
    };
    if collapsed {
        if host.height() <= MIN_SAFE_EDGE {
            return host;
        }
        let top = (host.top() + TOP_OFFSET + COLLAPSED_HEIGHT + MARGIN)
            .min(host.bottom() - MIN_SAFE_EDGE);
        return egui::Rect::from_min_max(egui::pos2(host.left(), top), host.max);
    }
    if host.width() <= MIN_SAFE_EDGE {
        return host;
    }
    let right = (host.right() - card_width(host) - MARGIN * 2.0)
        .clamp(host.left() + MIN_SAFE_EDGE, host.right());
    egui::Rect::from_min_max(host.min, egui::pos2(right, host.bottom()))
}

/// A foreground task card that starts at `pos` and follows the shared title-bar
/// drag position maintained by [`header`].
pub(super) fn area(host: &Ui, id: Id, pos: Pos2) -> Area {
    let stored = host
        .ctx()
        .data(|data| data.get_temp::<Pos2>(id.with("position")));
    let area = Area::new(id)
        .order(Order::Foreground)
        .movable(false)
        .constrain_to(host.max_rect());
    if let Some(stored) = stored {
        area.current_pos(stored)
    } else {
        // `default_pos` only applies when egui first creates the Area. The
        // central workspace can move when sidebars or workflow chrome change,
        // so an untouched task card must be re-anchored to the current host on
        // every frame. A user drag stores an explicit position above.
        area.current_pos(pos)
    }
}

/// Renders a draggable title row. The drag zone is registered before its child
/// buttons so close, collapse and menu controls retain priority.
pub(super) fn header<R>(ui: &mut Ui, area_id: Id, add_contents: impl FnOnce(&mut Ui) -> R) -> R {
    let (drag_rect, drag) = ui.allocate_exact_size(
        Vec2::new(ui.available_width(), ui.spacing().interact_size.y),
        Sense::drag(),
    );
    let drag = drag.on_hover_cursor(CursorIcon::Grab);
    update_drag_position(ui, area_id, &drag);
    let mut header = ui.new_child(
        UiBuilder::new()
            .id_salt(area_id.with("title_contents"))
            .max_rect(drag_rect)
            .layout(Layout::left_to_right(Align::Center)),
    );
    add_contents(&mut header)
}

fn update_drag_position(ui: &Ui, area_id: Id, drag: &egui::Response) {
    let origin_id = area_id.with("drag_origin");
    let position_id = area_id.with("position");
    if drag.drag_started()
        && let Some(origin) = ui
            .ctx()
            .memory(|memory| memory.area_rect(area_id).map(|rect| rect.min))
    {
        ui.ctx()
            .data_mut(|data| data.insert_temp(origin_id, origin));
    }
    if drag.dragged()
        && let Some(origin) = ui.ctx().data(|data| data.get_temp::<Pos2>(origin_id))
        && let Some(delta) = drag.total_drag_delta()
    {
        ui.ctx()
            .data_mut(|data| data.insert_temp(position_id, origin + delta));
        ui.ctx().request_repaint();
    }
    if drag.drag_stopped() {
        ui.ctx().data_mut(|data| data.remove::<Pos2>(origin_id));
    }
}

/// Renders the vertically resizable body shared by every task card.
///
/// `egui::Resize` normally collapses a non-resizable axis to its content size.
/// Keeping the content UI at the card width aligns its handle with the card.
pub(super) fn resizable_body<R>(
    ui: &mut Ui,
    id_salt: impl Hash,
    default_height: f32,
    min_height: f32,
    max_height: f32,
    add_contents: impl FnOnce(&mut Ui) -> R,
) -> R {
    let width = ui.available_width();
    egui::Resize::default()
        .id_salt(id_salt)
        .default_size([width, default_height])
        .min_size([width, min_height])
        .max_size([width, max_height])
        .resizable([false, true])
        .with_stroke(false)
        .show(ui, |ui| {
            ui.set_min_width(width);
            add_contents(ui)
        })
}

pub(super) fn is_active(app: &PlotxApp, tab: TaskDockTab) -> bool {
    app.session.ui.task_dock_active == Some(tab)
}

/// Labelled switcher shared by every page in the one top-right task dock.
/// Hidden task pages keep their state; selecting a tab also restores the
/// dataset that owns it so controls never edit a dataset other than the canvas.
pub(super) fn tab_bar(app: &mut PlotxApp, current: TaskDockTab, ui: &mut Ui) -> bool {
    let tabs = [
        (
            TaskDockTab::Processing,
            icon::FLOW_ARROW,
            "Process",
            app.session
                .ui
                .processing_task_dataset
                .and_then(|id| app.doc.dataset_index(id)),
        ),
        (
            TaskDockTab::Regions,
            icon::SELECTION,
            "Regions",
            app.session
                .ui
                .region_task_dataset
                .and_then(|id| app.doc.dataset_index(id)),
        ),
        (
            TaskDockTab::CurveFit,
            icon::CHART_LINE_UP,
            "Fit",
            app.session.ui.curve_fit_task_dataset,
        ),
        (
            TaskDockTab::Statistics,
            icon::FUNCTION,
            "Stats",
            app.session.ui.stat_task_dataset,
        ),
    ];
    let open = tabs
        .into_iter()
        .filter(|(_, _, _, dataset)| dataset.is_some())
        .collect::<Vec<_>>();
    if open.len() < 2 {
        return false;
    }
    let mut selected = None;
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 3.0;
        for (tab, glyph, label, dataset) in &open {
            if ui
                .selectable_label(
                    *tab == current,
                    RichText::new(format!("{glyph} {label}")).small(),
                )
                .clicked()
            {
                selected = Some((*tab, *dataset));
            }
        }
    });
    if let Some((tab, dataset)) = selected {
        if app.session.ui.task_dock_active == Some(TaskDockTab::Regions)
            && tab != TaskDockTab::Regions
            && app.session.tool == Tool::Regions
        {
            app.set_tool(Tool::BrowseZoom);
        }
        app.session.ui.open_task_tab(tab);
        if let Some(dataset) = dataset {
            app.set_active_dataset(Some(dataset));
        }
    }
    true
}

#[cfg(test)]
mod tests {
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
    fn expanded_card_reserves_the_right_dock_strip() {
        let app = app_with_task(TaskDockTab::Processing, false);
        let host = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1000.0, 700.0));
        let safe = safe_fit_rect(&app, host);
        assert_eq!(safe.left(), host.left());
        assert!(safe.right() < host.right() - WIDTH);
        assert_eq!(safe.height(), host.height());
    }

    #[test]
    fn collapsed_card_uses_the_full_width_below_its_header() {
        let app = app_with_task(TaskDockTab::Regions, true);
        let host = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1000.0, 700.0));
        let safe = safe_fit_rect(&app, host);
        assert_eq!(safe.width(), host.width());
        assert!(safe.top() > host.top());
    }

    #[test]
    fn narrow_hosts_keep_valid_nonempty_fit_geometry() {
        let app = app_with_task(TaskDockTab::Processing, false);
        let host = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(250.0, 140.0));
        let safe = safe_fit_rect(&app, host);
        assert!(safe.is_finite());
        assert!(card_width(host) < WIDTH);
        assert!(safe.width() >= MIN_SAFE_EDGE && safe.height() > 0.0);
    }

    #[test]
    fn sub_minimum_width_expanded_card_uses_the_recoverable_host_rect() {
        let app = app_with_task(TaskDockTab::Processing, false);
        let host = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(48.0, 140.0));

        let safe = safe_fit_rect(&app, host);

        assert_eq!(safe, host);
        assert!(safe.is_finite());
    }

    #[test]
    fn sub_minimum_height_collapsed_card_uses_the_recoverable_host_rect() {
        let app = app_with_task(TaskDockTab::Regions, true);
        let host = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(250.0, 48.0));

        let safe = safe_fit_rect(&app, host);

        assert_eq!(safe, host);
        assert!(safe.is_finite());
    }

    #[test]
    fn title_drag_moves_the_shared_task_card() {
        let ctx = egui::Context::default();
        let screen = egui::Rect::from_min_size(Pos2::ZERO, egui::vec2(640.0, 480.0));
        let area_id = Id::new("test_task_card");
        let start = Pos2::new(100.0, 80.0);

        let frame = |events| {
            let input = egui::RawInput {
                screen_rect: Some(screen),
                events,
                ..Default::default()
            };
            let _ = ctx.run_ui(input, |ui| {
                egui::CentralPanel::default().show_inside(ui, |ui| {
                    area(ui, area_id, start).show(ui.ctx(), |ui| {
                        ui.set_width(WIDTH);
                        header(ui, area_id, |ui| {
                            ui.label("Processing");
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                let _ = ui.button("Close");
                            });
                        });
                        ui.label("Body");
                    });
                });
            });
        };

        frame(Vec::new());
        frame(Vec::new());
        let initial = ctx
            .memory(|memory| memory.area_rect(area_id))
            .expect("laid-out task card");
        let pointer_start = initial.min + egui::vec2(80.0, 12.0);
        let pointer_end = pointer_start + egui::vec2(120.0, 100.0);
        frame(vec![egui::Event::PointerMoved(pointer_start)]);
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
        assert_eq!(moved.min, initial.min + (pointer_end - pointer_start));
    }

    #[test]
    fn resize_handle_stays_at_the_task_card_right_edge() {
        let ctx = egui::Context::default();
        let screen = egui::Rect::from_min_size(Pos2::ZERO, egui::vec2(640.0, 480.0));
        let expected_right = Cell::new(0.0);
        let actual_right = Cell::new(None);

        let frame = || {
            let _ = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(screen),
                    ..Default::default()
                },
                |ui| {
                    egui::CentralPanel::default().show_inside(ui, |ui| {
                        Area::new(Id::new("resize_test_card"))
                            .fixed_pos(Pos2::new(100.0, 80.0))
                            .show(ui.ctx(), |ui| {
                                ui.set_width(WIDTH);
                                let body_id = Id::new("resize_test_body");
                                let corner_id = ui
                                    .make_persistent_id(Id::new(body_id))
                                    .with("__resize_corner");
                                expected_right
                                    .set(ui.next_widget_position().x + ui.available_width());
                                resizable_body(ui, body_id, 200.0, 120.0, 300.0, |ui| {
                                    ui.label("Short content");
                                });
                                actual_right.set(
                                    ui.ctx()
                                        .read_response(corner_id)
                                        .map(|response| response.rect.right()),
                                );
                            });
                    });
                },
            );
        };

        frame();
        frame();
        assert_eq!(actual_right.get(), Some(expected_right.get()));
    }
}
