use super::*;

pub(super) fn paint(
    app: &PlotxApp,
    ci: usize,
    rect: egui::Rect,
    painter: &egui::Painter,
    chrome: ChromeStyle,
) {
    let canvas = &app.doc.canvases[ci];
    let bt = BoardTransform::from_board(app.session.board, rect);
    let page = bt.page_screen_rect(canvas);
    if let Some(target) = app.session.ui.panel_drop_target
        && let Some(panel) = canvas.panel(target)
        && panel.visible
    {
        let target_rect = screen_rect(page, bt.zoom, panel.frame);
        painter.rect_filled(target_rect, 0.0, chrome.selection_fill.gamma_multiply(0.35));
        painter.rect_stroke(
            target_rect,
            0.0,
            Stroke::new(2.0_f32, chrome.selection_active),
            StrokeKind::Inside,
        );
    }
    // Editing a child switches the selection affordance to local coordinates,
    // but the Panel remains the spatial container. Keep its boundary visible as
    // a quiet orientation cue without presenting it as the active selection.
    if let Some((canvas_id, panel_id)) = app.session.ui.hierarchical_selection.editing_panel()
        && canvas_id == canvas.resource_id
        && let Some(panel) = canvas.panel(panel_id).filter(|panel| panel.visible)
    {
        let panel_rect = screen_rect(page, bt.zoom, panel.frame);
        painter.rect_stroke(
            panel_rect,
            0.0,
            Stroke::new(1.0_f32, chrome.selection_stroke.gamma_multiply(0.38)),
            StrokeKind::Inside,
        );
    }
    for panel in canvas
        .panels
        .iter()
        .filter(|panel| panel.visible && panel.item_order.is_empty())
    {
        let panel_rect = screen_rect(page, bt.zoom, panel.frame);
        painter.rect_stroke(
            panel_rect,
            0.0,
            Stroke::new(1.0_f32, chrome.selection_stroke.gamma_multiply(0.45)),
            StrokeKind::Inside,
        );
        painter.text(
            panel_rect.center(),
            egui::Align2::CENTER_CENTER,
            "Empty panel",
            egui::FontId::proportional(11.0),
            chrome.selection_stroke.gamma_multiply(0.65),
        );
    }
    let selected_panels: Vec<_> = app
        .session
        .ui
        .hierarchical_selection
        .paths()
        .iter()
        .filter_map(|path| {
            (path.canvas == canvas.resource_id && path.content.is_none())
                .then_some(path.panel)
                .flatten()
        })
        .filter_map(|id| canvas.panel(id))
        .collect();
    let primary = app
        .session
        .ui
        .hierarchical_selection
        .lead()
        .and_then(|path| (path.content.is_none()).then_some(path.panel).flatten());
    for panel in selected_panels {
        let panel_rect = screen_rect(page, bt.zoom, panel.frame);
        painter.rect_stroke(
            panel_rect,
            0.0,
            Stroke::new(1.5_f32, chrome.selection_stroke),
            StrokeKind::Inside,
        );
        if primary == Some(panel.id) && app.session.tool.is_layout_tool() && !panel.locked {
            for point in [
                panel_rect.left_top(),
                panel_rect.right_top(),
                panel_rect.left_bottom(),
                panel_rect.right_bottom(),
            ] {
                painter.rect_filled(
                    egui::Rect::from_center_size(point, egui::vec2(HANDLE_SIZE_PX, HANDLE_SIZE_PX)),
                    0.0,
                    chrome.selection_stroke,
                );
            }
        }
    }
}

fn screen_rect(page: EguiRect, zoom: f32, frame: ObjectFrame) -> EguiRect {
    EguiRect::from_min_size(
        Pos2::new(page.left() + frame.x * zoom, page.top() + frame.y * zoom),
        egui::vec2(frame.width * zoom, frame.height * zoom),
    )
}
