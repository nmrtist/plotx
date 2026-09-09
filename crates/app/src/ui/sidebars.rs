use super::*;

const MIN_WORKSPACE_WIDTH: f32 = 320.0;
/// Logical pixels from the window's outer edge, independent of sidebar width.
const HIDE_EDGE_DISTANCE: f32 = 24.0;

pub(super) fn render(app: &mut PlotxApp, ui: &mut Ui, dark: bool, workspace_width: f32) {
    let mut primary_rect = None;
    let mut secondary_rect = None;
    let compact = workspace_width < 1200.0;
    let inspector_visible = app.session.secondary_sidebar_visible;
    if !inspector_visible {
        app.finish_axis_overrides_edit();
    }
    object_inspector::finish_series_edit_if_inactive(app, inspector_visible);
    if app.session.primary_sidebar_visible {
        let min_width = if compact { 150.0 } else { 190.0 };
        let other_width = if app.session.secondary_sidebar_visible {
            app.session.secondary_sidebar_width
        } else {
            0.0
        };
        let max_width =
            (workspace_width - other_width - MIN_WORKSPACE_WIDTH).clamp(min_width, 420.0);
        let panel = egui::Panel::left("primary_sidebar")
            .frame(egui::Frame::NONE.inner_margin(egui::Margin {
                left: 8,
                right: 0,
                top: 4,
                bottom: 8,
            }))
            .show_separator_line(false)
            .resizable(true)
            .default_size(
                app.session
                    .primary_sidebar_width
                    .clamp(min_width, max_width),
            )
            .size_range(min_width..=max_width);
        let response = show_sidebar(panel, app, ui, dark, true, min_width..=max_width);
        paint_sidebar_resize_edge(
            ui,
            Id::new("primary_sidebar"),
            response.inner,
            SidebarEdge::Right,
            dark,
        );
        app.session.primary_sidebar_width = response.response.rect.width();
        primary_rect = Some(response.inner);
        if resize_released_at_window_edge(ui.ctx(), Id::new("primary_sidebar"), SidebarEdge::Right)
        {
            app.session.primary_sidebar_visible = false;
            sidebar_hidden_status(app, commands::CommandId::TogglePrimarySidebar, "Left");
        }
    } else {
        // A shown panel consumes one auto-id slot from this ui. Burn the same
        // slot while hidden so the later siblings' container ids (secondary
        // panel, central panel) do not shift when a sidebar toggles.
        ui.skip_ahead_auto_ids(1);
    }

    if app.session.secondary_sidebar_visible {
        let min_width = if compact { 180.0 } else { 230.0 };
        let other_width = if app.session.primary_sidebar_visible {
            app.session.primary_sidebar_width
        } else {
            0.0
        };
        let max_width =
            (workspace_width - other_width - MIN_WORKSPACE_WIDTH).clamp(min_width, 460.0);
        let panel = egui::Panel::right("secondary_sidebar")
            .frame(egui::Frame::NONE.inner_margin(egui::Margin {
                left: 0,
                right: 8,
                top: 4,
                bottom: 8,
            }))
            .show_separator_line(false)
            .resizable(true)
            .default_size(
                app.session
                    .secondary_sidebar_width
                    .clamp(min_width, max_width),
            )
            .size_range(min_width..=max_width);
        let response = show_sidebar(panel, app, ui, dark, false, min_width..=max_width);
        paint_sidebar_resize_edge(
            ui,
            Id::new("secondary_sidebar"),
            response.inner,
            SidebarEdge::Left,
            dark,
        );
        app.session.secondary_sidebar_width = response.response.rect.width();
        secondary_rect = Some(response.inner);
        if resize_released_at_window_edge(ui.ctx(), Id::new("secondary_sidebar"), SidebarEdge::Left)
        {
            app.session.secondary_sidebar_visible = false;
            sidebar_hidden_status(app, commands::CommandId::ToggleSecondarySidebar, "Right");
        }
    } else {
        // See the primary branch: keep the sibling auto-id sequence stable.
        ui.skip_ahead_auto_ids(1);
    }
    super::workspace_geometry::set_sidebar_rects(ui.ctx(), primary_rect, secondary_rect);
}

/// Preview and commit use the same current pointer position: dragging back
/// from the window edge cancels hiding without any latched state.
pub(super) fn resize_released_at_window_edge(
    ctx: &egui::Context,
    panel_id: Id,
    edge: SidebarEdge,
) -> bool {
    let Some(resize) = ctx.read_response(panel_id.with("__resize")) else {
        return false;
    };
    if !resize.dragged() && !resize.drag_stopped() {
        return false;
    }
    let Some(pointer) = resize
        .interact_pointer_pos()
        .or_else(|| ctx.pointer_interact_pos())
    else {
        return false;
    };
    let window = ctx.content_rect();
    let near_edge = match edge {
        SidebarEdge::Right => pointer.x <= window.left() + HIDE_EDGE_DISTANCE,
        SidebarEdge::Left => pointer.x >= window.right() - HIDE_EDGE_DISTANCE,
    };
    if near_edge && resize.dragged() {
        // Keep the hint visible even when the pointer is outside the window.
        egui::Tooltip::always_open(
            ctx.clone(),
            resize.layer_id,
            panel_id.with("hide_hint"),
            window.shrink(12.0).clamp(pointer),
        )
        .show(|ui| {
            ui.label("Release to hide sidebar");
        });
    }
    near_edge && resize.drag_stopped()
}

fn sidebar_hidden_status(app: &mut PlotxApp, id: commands::CommandId, side: &str) {
    let recovery = match shortcuts::shortcut_label(id) {
        Some(chord) => format!("the layout buttons in the title row or {chord}"),
        None => "the layout buttons in the title row".to_owned(),
    };
    app.session.status = format!("{side} sidebar hidden. Show it again with {recovery}.");
}

fn show_sidebar(
    panel: egui::Panel,
    app: &mut PlotxApp,
    ui: &mut Ui,
    dark: bool,
    primary: bool,
    width_range: std::ops::RangeInclusive<f32>,
) -> InnerResponse<Rect> {
    let (id, edge) = if primary {
        (Id::new("primary_sidebar"), SidebarEdge::Right)
    } else {
        (Id::new("secondary_sidebar"), SidebarEdge::Left)
    };
    show_resizable_sidebar(panel, ui, id, edge, width_range, |ui| {
        // Anchor the content ids globally: a Ui's per-pass unique id folds in
        // the parent's auto-id counter, so without this every widget in this
        // sidebar changes id whenever an earlier sibling panel toggles. That
        // dropped focus mid-edit and tripped egui's rect-changed-id debug
        // overlay (one-frame red boxes) on the unmoved sidebar.
        ui.scope_builder(
            UiBuilder::new()
                .id_salt(id.with("stable_scope"))
                .global_scope(true),
            |ui| {
                let size = ui.available_size();
                let frame = card_frame(dark, egui::Margin::ZERO);
                let inset = frame.total_margin().sum();
                frame
                    .show(ui, |ui| {
                        ui.set_min_size((size - inset).max(Vec2::ZERO));
                        if primary {
                            primary_sidebar::render(app, ui);
                        } else {
                            secondary_sidebar::render(app, ui);
                        }
                    })
                    .response
                    .rect
            },
        )
        .inner
    })
}
