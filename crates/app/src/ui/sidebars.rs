use super::*;

const MIN_WORKSPACE_WIDTH: f32 = 320.0;

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
        let response = show_sidebar(panel, app, ui, dark, true);
        paint_sidebar_resize_edge(
            ui,
            Id::new("primary_sidebar"),
            response.inner,
            SidebarEdge::Right,
            dark,
        );
        app.session.primary_sidebar_width = response.response.rect.width();
        primary_rect = Some(response.inner);
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
        let response = show_sidebar(panel, app, ui, dark, false);
        paint_sidebar_resize_edge(
            ui,
            Id::new("secondary_sidebar"),
            response.inner,
            SidebarEdge::Left,
            dark,
        );
        app.session.secondary_sidebar_width = response.response.rect.width();
        secondary_rect = Some(response.inner);
    }
    super::workspace_geometry::set_sidebar_rects(ui.ctx(), primary_rect, secondary_rect);
}

fn show_sidebar(
    panel: egui::Panel,
    app: &mut PlotxApp,
    ui: &mut Ui,
    dark: bool,
    primary: bool,
) -> InnerResponse<Rect> {
    let (id, edge) = if primary {
        (Id::new("primary_sidebar"), SidebarEdge::Right)
    } else {
        (Id::new("secondary_sidebar"), SidebarEdge::Left)
    };
    show_resizable_sidebar(panel, ui, id, edge, |ui| {
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
    })
}
