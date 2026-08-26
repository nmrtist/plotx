//! Shared geometry for canvas task cards. They start in the same canvas corner,
//! so their initial position and sizing rules live in one place.

use egui::{
    Align, Area, CursorIcon, Id, Layout, Order, Pos2, RichText, Sense, Ui, UiBuilder, Vec2,
};
use egui_phosphor::regular as icon;
use plotx_core::{
    settings::TaskCardSize,
    state::{PlotxApp, TaskDockTab, Tool},
};

use super::task_card_layout::{CardLayout, HorizontalAnchor, VerticalAnchor, fit_layout};

const COLLAPSED_WIDTH: f32 = 310.0;
// Sidebar/Ribbon spacing is already applied by the central workspace panel.
// Task cards use that same boundary and must not add a second local gap.
const TOP_OFFSET: f32 = 0.0;
/// Room the card header, frame and footprint need outside the resizable body.
const CHROME: f32 = 64.0;
/// Preferred interaction minimum. A smaller viewport may temporarily force a
/// smaller body; viewport fitting always wins over this preference.
const FLOOR: f32 = 120.0;
const MIN_CANVAS_WIDTH: f32 = 320.0;
const STANDARD_MIN_WIDTH: f32 = 300.0;
const CRAFT_MIN_WIDTH: f32 = 380.0;
const STANDARD_MAX_WIDTH: f32 = 520.0;
const CRAFT_MAX_WIDTH: f32 = 760.0;

pub(super) struct TaskCardGeometry {
    pub pos: Pos2,
    pub width: f32,
    pub body_height: f32,
}

/// Anchors a card to the host's top-right corner and sizes its body to the
/// height the canvas actually has.
///
/// `preferred_min_body` is honoured only while it fits: `egui::Resize` applies
/// `at_least(min).at_most(max)`, so a min taller than the host would win over
/// the fitted max and force the card past the canvas. `Area` then constrains it
/// to the screen and slides it up over the Ribbon, hiding the very buttons that
/// opened it. Clamping the min keeps a short window shrinking instead.
pub(super) fn geometry(
    app: &PlotxApp,
    host: &Ui,
    tab: TaskDockTab,
    preferred_min_body: f32,
    collapsed: bool,
) -> TaskCardGeometry {
    let host_rect = bounds(host);
    let id = area_id(tab);
    let preferred = preferred_size(app, tab);
    let requested_width = if collapsed {
        COLLAPSED_WIDTH.min(host_rect.width().max(1.0))
    } else {
        card_width(host_rect, tab, preferred.width)
    };
    let stored = host
        .ctx()
        .data(|data| data.get_temp::<CardLayout>(id.with("layout")));
    let chrome = stored.map_or(CHROME, |layout| layout.chrome_height);
    let extra_width = stored.map_or(0.0, |layout| layout.extra_width);
    let preferred_body = preferred.body_height.max(preferred_min_body);
    let desired_size = Vec2::new(
        requested_width + extra_width,
        if collapsed {
            chrome
        } else {
            chrome + preferred_body
        },
    );
    let initial = CardLayout {
        rect: egui::Rect::from_min_size(
            host_rect.right_top() + egui::vec2(-desired_size.x, TOP_OFFSET),
            desired_size,
        ),
        bounds: host_rect,
        horizontal: HorizontalAnchor::Right,
        vertical: VerticalAnchor::Top,
        chrome_height: chrome,
        extra_width,
        collapsed,
    };
    let layout = fit_layout(
        stored.unwrap_or(initial),
        host_rect,
        desired_size,
        collapsed,
    );
    host.ctx()
        .data_mut(|data| data.insert_temp(id.with("layout"), layout));
    TaskCardGeometry {
        pos: layout.rect.min,
        width: (layout.rect.width() - layout.extra_width).max(1.0),
        body_height: if collapsed {
            0.0
        } else {
            (layout.rect.height() - layout.chrome_height).max(0.0)
        },
    }
}

fn card_width(host: egui::Rect, tab: TaskDockTab, preferred_width: f32) -> f32 {
    let (minimum, absolute_maximum) = width_limits(tab);
    let physical_maximum = host.width().max(1.0);
    let canvas_preserving_maximum =
        (host.width() - MIN_CANVAS_WIDTH).max(minimum.min(physical_maximum));
    preferred_width
        .clamp(
            minimum.min(physical_maximum),
            absolute_maximum.min(physical_maximum),
        )
        .min(canvas_preserving_maximum)
}

fn area_id(tab: TaskDockTab) -> Id {
    match tab {
        TaskDockTab::Processing => Id::new("processing_task_card"),
        TaskDockTab::Regions => Id::new("region_task_card"),
        TaskDockTab::CurveFit => Id::new("curve_fit_task_card"),
        TaskDockTab::Statistics => Id::new("statistics_task_card"),
        TaskDockTab::Craft => Id::new("craft_task_card"),
    }
}

fn bounds(host: &Ui) -> egui::Rect {
    crate::ui::workspace_geometry::task_card_bounds(host.ctx())
        .unwrap_or_else(|| host.ctx().content_rect())
}

fn width_limits(tab: TaskDockTab) -> (f32, f32) {
    if tab == TaskDockTab::Craft {
        (CRAFT_MIN_WIDTH, CRAFT_MAX_WIDTH)
    } else {
        (STANDARD_MIN_WIDTH, STANDARD_MAX_WIDTH)
    }
}

fn preferred_size(app: &PlotxApp, tab: TaskDockTab) -> TaskCardSize {
    let cards = &app.settings.window.task_cards;
    match tab {
        TaskDockTab::Processing => cards.processing,
        TaskDockTab::Regions => cards.regions,
        TaskDockTab::CurveFit => cards.curve_fit,
        TaskDockTab::Statistics => cards.statistics,
        TaskDockTab::Craft => cards.craft,
    }
}

fn preferred_size_mut(app: &mut PlotxApp, tab: TaskDockTab) -> &mut TaskCardSize {
    let cards = &mut app.settings.window.task_cards;
    match tab {
        TaskDockTab::Processing => &mut cards.processing,
        TaskDockTab::Regions => &mut cards.regions,
        TaskDockTab::CurveFit => &mut cards.curve_fit,
        TaskDockTab::Statistics => &mut cards.statistics,
        TaskDockTab::Craft => &mut cards.craft,
    }
}

pub(crate) fn visible_area_id(app: &PlotxApp) -> Option<Id> {
    let active = app.active_dataset();
    match app.session.ui.task_dock_active? {
        TaskDockTab::Processing => app
            .session
            .ui
            .processing_task_dataset
            .and_then(|id| app.doc.dataset_index(id))
            .filter(|dataset| Some(*dataset) == active)
            .map(|_| Id::new("processing_task_card")),
        TaskDockTab::Regions => app
            .session
            .ui
            .region_task_dataset
            .and_then(|id| app.doc.dataset_index(id))
            .filter(|dataset| Some(*dataset) == active)
            .map(|_| Id::new("region_task_card")),
        TaskDockTab::CurveFit => app
            .session
            .ui
            .curve_fit_task_dataset
            .filter(|dataset| Some(*dataset) == active)
            .map(|_| Id::new("curve_fit_task_card")),
        TaskDockTab::Statistics => app
            .session
            .ui
            .stat_task_dataset
            .filter(|dataset| Some(*dataset) == active)
            .map(|_| Id::new("statistics_task_card")),
        TaskDockTab::Craft => app
            .session
            .ui
            .craft_task_dataset
            .and_then(|id| app.doc.dataset_index(id))
            .filter(|dataset| Some(*dataset) == active)
            .map(|_| Id::new("craft_task_card")),
    }
}

/// Whether the pointer is over the visible card or its resize grips. Canvas
/// gestures are dispatched before the card is rendered in the current frame,
/// so they must consult the previous authoritative area rectangle explicitly
/// instead of relying only on `layer_id_at`.
pub(crate) fn pointer_allows_canvas(app: &PlotxApp, ui: &Ui, pos: Pos2) -> bool {
    let card_hit = visible_area_id(app)
        .and_then(|id| ui.ctx().memory(|memory| memory.area_rect(id)))
        .is_some_and(|rect| rect.expand(6.0).contains(pos));
    !card_hit
        && ui
            .ctx()
            .layer_id_at(pos)
            .is_none_or(|layer| layer == ui.layer_id())
}

#[derive(Clone, Copy)]
struct DragOrigin {
    rect: egui::Rect,
}

pub(super) fn area(_host: &Ui, id: Id, pos: Pos2) -> Area {
    // Position and size are constrained together by the task-card gesture
    // model. Area's own constraint pass uses its previous-frame size, which
    // creates a visible one-frame correction whenever both change at once.
    Area::new(id)
        .order(Order::Middle)
        .movable(false)
        .fixed_pos(pos)
}

/// Renders the task-card title as one move surface with a dedicated action
/// slot. Text responses participate in the same gesture while buttons remain
/// ordinary controls, so the visual and interactive title bars are identical.
pub(super) fn header<R>(
    ui: &mut Ui,
    area_id: Id,
    title: &str,
    detail: Option<impl Into<RichText>>,
    add_actions: impl FnOnce(&mut Ui) -> R,
) -> R {
    let (drag_rect, drag) = ui.allocate_exact_size(
        Vec2::new(ui.available_width(), ui.spacing().interact_size.y),
        Sense::drag(),
    );
    let drag = drag.on_hover_cursor(CursorIcon::Grab);
    update_drag_position(ui, area_id, &drag);
    let mut actions = ui.new_child(
        UiBuilder::new()
            .id_salt(area_id.with("title_actions"))
            .max_rect(drag_rect)
            .layout(Layout::right_to_left(Align::Center)),
    );
    actions.set_clip_rect(drag_rect);
    let result = add_actions(&mut actions);
    let text_right = (actions.min_rect().left() - ui.spacing().item_spacing.x)
        .clamp(drag_rect.left(), drag_rect.right());
    let text_rect =
        egui::Rect::from_min_max(drag_rect.min, egui::pos2(text_right, drag_rect.max.y));
    let mut text = ui.new_child(
        UiBuilder::new()
            .id_salt(area_id.with("title_text"))
            .max_rect(text_rect)
            .layout(Layout::left_to_right(Align::Center)),
    );
    text.set_clip_rect(text_rect);
    let title_drag = text
        .label(crate::typography::headline(title))
        .interact(Sense::drag())
        .on_hover_cursor(CursorIcon::Grab);
    update_drag_position(&text, area_id, &title_drag);
    if let Some(detail) = detail {
        let detail_drag = text
            .label(detail.into().weak())
            .interact(Sense::drag())
            .on_hover_cursor(CursorIcon::Grab);
        update_drag_position(&text, area_id, &detail_drag);
    }
    result
}

fn update_drag_position(ui: &Ui, area_id: Id, drag: &egui::Response) {
    let origin_id = area_id.with("drag_origin");
    let current_rect = ui
        .ctx()
        .data(|data| data.get_temp::<CardLayout>(area_id.with("layout")))
        .map(|layout| layout.rect)
        .or_else(|| ui.ctx().memory(|memory| memory.area_rect(area_id)));
    if drag.drag_started()
        && let Some(rect) = current_rect
    {
        ui.ctx()
            .data_mut(|data| data.insert_temp(origin_id, DragOrigin { rect }));
    }
    if drag.dragged()
        && let Some(origin) = ui.ctx().data(|data| data.get_temp::<DragOrigin>(origin_id))
        && let Some(delta) = drag.total_drag_delta()
    {
        let bounds = bounds(ui);
        let desired_min = origin.rect.min + delta;
        let pos = egui::pos2(
            desired_min.x.clamp(
                bounds.left(),
                (bounds.right() - origin.rect.width()).max(bounds.left()),
            ),
            desired_min.y.clamp(
                bounds.top(),
                (bounds.bottom() - origin.rect.height()).max(bounds.top()),
            ),
        );
        let rect = egui::Rect::from_min_size(pos, origin.rect.size().min(bounds.size()));
        ui.ctx().data_mut(|data| {
            let mut layout = data
                .get_temp::<CardLayout>(area_id.with("layout"))
                .unwrap_or(CardLayout {
                    rect: origin.rect,
                    bounds,
                    horizontal: HorizontalAnchor::Right,
                    vertical: VerticalAnchor::Top,
                    chrome_height: CHROME,
                    extra_width: 0.0,
                    collapsed: false,
                });
            layout.rect = rect;
            layout.bounds = bounds;
            layout.horizontal = if rect.left() - bounds.left() <= bounds.right() - rect.right() {
                HorizontalAnchor::Left
            } else {
                HorizontalAnchor::Right
            };
            layout.vertical = if rect.top() - bounds.top() <= bounds.bottom() - rect.bottom() {
                VerticalAnchor::Top
            } else {
                VerticalAnchor::Bottom
            };
            data.insert_temp(area_id.with("layout"), layout);
        });
        ui.ctx().request_repaint();
    }
    if drag.drag_stopped() {
        ui.ctx()
            .data_mut(|data| data.remove::<DragOrigin>(origin_id));
    }
}

/// Gives the card body the height resolved by the shared whole-card geometry.
pub(super) fn sized_body<R>(
    ui: &mut Ui,
    height: f32,
    add_contents: impl FnOnce(&mut Ui) -> R,
) -> R {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), height), Sense::hover());
    let mut body = ui.new_child(
        UiBuilder::new()
            .id_salt("task_card_body")
            .max_rect(rect)
            .layout(*ui.layout()),
    );
    add_contents(&mut body)
}

#[derive(Clone, Copy)]
struct ResizeOrigin {
    rect: egui::Rect,
    chrome_height: f32,
}

#[derive(Clone, Copy)]
pub(super) struct ResizeEdges {
    pub(super) left: bool,
    pub(super) right: bool,
    pub(super) top: bool,
    pub(super) bottom: bool,
}

/// Adds resize handles on every edge and corner of the whole card. The result
/// is constrained before it is stored, so pulling beyond an edge cannot move
/// the opposite edge or create a growing sidebar gap.
pub(super) fn resize_handles(
    app: &mut PlotxApp,
    ui: &mut Ui,
    area_id: Id,
    tab: TaskDockTab,
    requested_width: f32,
    body_height: f32,
) {
    // At this point the card has been laid out in the current pass. Reading
    // Area memory here returns the previous pass's rectangle, which is exactly
    // the wrong origin when a viewport change and a resize happen together.
    let rect = ui.min_rect();
    let chrome_height = (rect.height() - body_height).max(0.0);
    let extra_width = (rect.width() - requested_width).max(0.0);
    ui.ctx().data_mut(|data| {
        if let Some(mut layout) = data.get_temp::<CardLayout>(area_id.with("layout")) {
            // Measurement refines content-to-frame metrics only. Gesture and
            // workspace fitting are the sole writers of the authoritative
            // rectangle; copying the just-rendered rectangle here would
            // overwrite a title drag performed earlier in the same pass.
            layout.chrome_height = chrome_height;
            layout.extra_width = extra_width;
            data.insert_temp(area_id.with("layout"), layout);
        }
    });
    let grip = 5.0;
    let corner = 12.0;
    let handles = [
        (
            egui::Rect::from_min_max(
                egui::pos2(rect.left() - grip, rect.top() - grip),
                egui::pos2(rect.left() + corner, rect.top() + corner),
            ),
            "resize_north_west",
            CursorIcon::ResizeNorthWest,
            ResizeEdges {
                left: true,
                right: false,
                top: true,
                bottom: false,
            },
        ),
        (
            egui::Rect::from_min_max(
                egui::pos2(rect.right() - corner, rect.top() - grip),
                egui::pos2(rect.right() + grip, rect.top() + corner),
            ),
            "resize_north_east",
            CursorIcon::ResizeNorthEast,
            ResizeEdges {
                left: false,
                right: true,
                top: true,
                bottom: false,
            },
        ),
        (
            egui::Rect::from_min_max(
                egui::pos2(rect.left() - grip, rect.bottom() - corner),
                egui::pos2(rect.left() + corner, rect.bottom() + grip),
            ),
            "resize_south_west",
            CursorIcon::ResizeSouthWest,
            ResizeEdges {
                left: true,
                right: false,
                top: false,
                bottom: true,
            },
        ),
        (
            egui::Rect::from_min_max(
                egui::pos2(rect.right() - corner, rect.bottom() - corner),
                egui::pos2(rect.right() + grip, rect.bottom() + grip),
            ),
            "resize_south_east",
            CursorIcon::ResizeSouthEast,
            ResizeEdges {
                left: false,
                right: true,
                top: false,
                bottom: true,
            },
        ),
        (
            egui::Rect::from_min_max(
                egui::pos2(rect.left() - grip, rect.top() + corner),
                egui::pos2(rect.left() + grip, rect.bottom() - corner),
            ),
            "resize_west",
            CursorIcon::ResizeWest,
            ResizeEdges {
                left: true,
                right: false,
                top: false,
                bottom: false,
            },
        ),
        (
            egui::Rect::from_min_max(
                egui::pos2(rect.right() - grip, rect.top() + corner),
                egui::pos2(rect.right() + grip, rect.bottom() - corner),
            ),
            "resize_east",
            CursorIcon::ResizeEast,
            ResizeEdges {
                left: false,
                right: true,
                top: false,
                bottom: false,
            },
        ),
        (
            egui::Rect::from_min_max(
                egui::pos2(rect.left() + corner, rect.top() - grip),
                egui::pos2(rect.right() - corner, rect.top() + grip),
            ),
            "resize_north",
            CursorIcon::ResizeNorth,
            ResizeEdges {
                left: false,
                right: false,
                top: true,
                bottom: false,
            },
        ),
        (
            egui::Rect::from_min_max(
                egui::pos2(rect.left() + corner, rect.bottom() - grip),
                egui::pos2(rect.right() - corner, rect.bottom() + grip),
            ),
            "resize_south",
            CursorIcon::ResizeSouth,
            ResizeEdges {
                left: false,
                right: false,
                top: false,
                bottom: true,
            },
        ),
    ];
    let responses = handles.map(|(rect, salt, cursor, edges)| {
        (
            ui.interact(rect, area_id.with(salt), Sense::drag())
                .on_hover_cursor(cursor),
            edges,
            rect,
        )
    });
    for (response, edges, hit_rect) in &responses {
        super::task_card_resize::paint_feedback(ui, rect, *hit_rect, *edges, response);
    }
    let origin_id = area_id.with("resize_origin");
    let active = responses.iter().find(|(response, _, _)| {
        response.drag_started() || response.dragged() || response.drag_stopped()
    });
    let Some((response, edges, _)) = active else {
        return;
    };
    if response.drag_started() {
        ui.ctx().data_mut(|data| {
            data.insert_temp(
                origin_id,
                ResizeOrigin {
                    rect,
                    chrome_height,
                },
            )
        });
    }
    if response.dragged()
        && let Some(origin) = ui
            .ctx()
            .data(|data| data.get_temp::<ResizeOrigin>(origin_id))
        && let Some(delta) = response.total_drag_delta()
    {
        let (minimum, maximum) = width_limits(tab);
        let bounds = bounds(ui);
        // Geometry and interaction must use the same effective maximum. The
        // canvas-preserving cap can be lower than the product-level maximum;
        // storing the larger width made the next layout shrink without moving
        // the left edge, which appeared as a growing right-hand gap.
        let effective_maximum = card_width(bounds, tab, maximum);
        let resized = resized_rect(
            origin,
            *edges,
            delta,
            bounds,
            minimum + extra_width,
            effective_maximum + extra_width,
        );
        *preferred_size_mut(app, tab) = TaskCardSize::new(
            (resized.width() - extra_width).max(1.0),
            (resized.height() - origin.chrome_height).max(1.0),
        );
        ui.ctx().data_mut(|data| {
            if let Some(mut layout) = data.get_temp::<CardLayout>(area_id.with("layout")) {
                layout.rect = resized;
                layout.bounds = bounds;
                if edges.left {
                    layout.horizontal = HorizontalAnchor::Right;
                } else if edges.right {
                    layout.horizontal = HorizontalAnchor::Left;
                }
                if edges.top {
                    layout.vertical = VerticalAnchor::Bottom;
                } else if edges.bottom {
                    layout.vertical = VerticalAnchor::Top;
                }
                data.insert_temp(area_id.with("layout"), layout);
            }
        });
        ui.ctx().request_repaint();
    }
    if response.drag_stopped() {
        ui.ctx()
            .data_mut(|data| data.remove::<ResizeOrigin>(origin_id));
        app.persist_settings();
    }
}

fn resized_rect(
    origin: ResizeOrigin,
    edges: ResizeEdges,
    delta: Vec2,
    bounds: egui::Rect,
    minimum_width: f32,
    maximum_width: f32,
) -> egui::Rect {
    let minimum_width = minimum_width.min(bounds.width());
    let maximum_width = maximum_width.min(bounds.width());
    let minimum_height = (origin.chrome_height + FLOOR).min(bounds.height());
    let mut resized = origin.rect;
    if edges.left {
        resized.min.x = (origin.rect.left() + delta.x).clamp(
            bounds.left(),
            (origin.rect.right() - minimum_width).max(bounds.left()),
        );
    }
    if edges.right {
        resized.max.x = (origin.rect.right() + delta.x).clamp(
            (origin.rect.left() + minimum_width).min(bounds.right()),
            bounds.right(),
        );
    }
    if resized.width() > maximum_width {
        if edges.left {
            resized.min.x = resized.right() - maximum_width;
        } else {
            resized.max.x = resized.left() + maximum_width;
        }
    }
    if edges.top {
        resized.min.y = (origin.rect.top() + delta.y).clamp(
            bounds.top(),
            (origin.rect.bottom() - minimum_height).max(bounds.top()),
        );
    }
    if edges.bottom {
        resized.max.y = (origin.rect.bottom() + delta.y).clamp(
            (origin.rect.top() + minimum_height).min(bounds.bottom()),
            bounds.bottom(),
        );
    }
    resized
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
            TaskDockTab::Craft,
            icon::WAVEFORM,
            "CRAFT",
            app.session
                .ui
                .craft_task_dataset
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
    let mut action = None;
    ui.scope(|ui| {
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            for (tab, glyph, label, dataset) in &open {
                let selected = *tab == current;
                let (fill, stroke) = if selected {
                    (
                        ui.visuals().selection.bg_fill,
                        ui.visuals().selection.stroke,
                    )
                } else {
                    (
                        ui.visuals().widgets.inactive.weak_bg_fill,
                        ui.visuals().widgets.inactive.bg_stroke,
                    )
                };
                egui::Frame::NONE
                    .fill(fill)
                    .stroke(stroke)
                    .corner_radius(ui.visuals().widgets.inactive.corner_radius)
                    .inner_margin(egui::Margin::symmetric(6, 2))
                    .show(ui, |ui| {
                        ui.spacing_mut().item_spacing.x = 2.0;
                        ui.horizontal(|ui| {
                            if ui
                                .add(
                                    egui::Button::selectable(
                                        selected,
                                        RichText::new(format!("{glyph} {label}")).small(),
                                    )
                                    .frame(false),
                                )
                                .clicked()
                            {
                                action = Some((false, *tab, *dataset));
                            }
                            if ui
                                .add(egui::Button::new(RichText::new(icon::X).small()).frame(false))
                                .on_hover_text(format!("Close {label}"))
                                .clicked()
                            {
                                action = Some((true, *tab, *dataset));
                            }
                        });
                    });
            }
        });
    });
    if let Some((close, tab, dataset)) = action {
        if close {
            app.session.ui.close_task_tab(tab);
            return true;
        }
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
#[path = "task_card_tests.rs"]
mod tests;
