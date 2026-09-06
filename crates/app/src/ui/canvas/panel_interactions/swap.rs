use super::*;

pub(super) fn target(
    app: &PlotxApp,
    drag: &PanelDrag,
    rect: EguiRect,
    pointer: Option<Pos2>,
) -> Option<PanelId> {
    if !drag.active || drag.kind != ObjectDragKind::Move || !drag.others.is_empty() {
        return None;
    }
    let pointer = pointer?;
    if !rect.contains(pointer) || frame_at(app, rect, pointer) != Some(FrameRef::Page(drag.canvas))
    {
        return None;
    }
    let page = app.doc.canvases.get(drag.canvas)?;
    let point = BoardTransform::from_board(app.session.board, rect).screen_to_page(page, pointer);
    page.panels
        .iter()
        .rev()
        .find(|panel| {
            panel.id != drag.panel
                && panel.visible
                && !panel.locked
                && EguiRect::from_min_size(
                    Pos2::new(panel.frame.x, panel.frame.y),
                    Vec2::new(panel.frame.width, panel.frame.height),
                )
                .contains(point)
        })
        .map(|panel| panel.id)
}

pub(super) fn commit(app: &mut PlotxApp, drag: &PanelDrag, target: PanelId) {
    let Some(page) = app.doc.canvases.get_mut(drag.canvas) else {
        return;
    };
    let canvas = page.resource_id;
    // The history baseline must precede the entire live move.
    if let Some(panel) = page.panel_mut(drag.panel) {
        panel.frame = drag.before;
    }
    app.session.ui.tile_drop = None;
    match app.swap_panels_action(canvas, drag.panel, target) {
        Ok(action) => app.execute_action(action),
        Err(error) => app.session.status = format!("Could not swap Panels: {error}"),
    }
}

pub(super) fn restore_preview(app: &mut PlotxApp, rect: EguiRect, pointer: Option<Pos2>) {
    let Interaction::Panel(drag) = &app.session.ui.interaction else {
        return;
    };
    if target(app, drag, rect, pointer).is_none() {
        return;
    }
    let (canvas, panel, before) = (drag.canvas, drag.panel, drag.before);
    // Both highlighted slots remain readable while the pending exchange is shown.
    if let Some(panel) = app
        .doc
        .canvases
        .get_mut(canvas)
        .and_then(|page| page.panel_mut(panel))
    {
        panel.frame = before;
    }
}

pub(crate) fn paint_panel_swap(
    app: &PlotxApp,
    rect: EguiRect,
    painter: &egui::Painter,
    chrome: ChromeStyle,
) {
    let Interaction::Panel(drag) = &app.session.ui.interaction else {
        return;
    };
    let pointer = painter.ctx().input(|input| input.pointer.hover_pos());
    let Some(target) = target(app, drag, rect, pointer) else {
        return;
    };
    let page = &app.doc.canvases[drag.canvas];
    let Some(panel) = page.panel(target) else {
        return;
    };
    let bt = BoardTransform::from_board(app.session.board, rect);
    let page_rect = bt.page_screen_rect(page);
    for frame in [drag.before, panel.frame] {
        let r = EguiRect::from_min_size(
            page_rect.min + Vec2::new(frame.x, frame.y) * bt.zoom,
            Vec2::new(frame.width, frame.height) * bt.zoom,
        );
        painter.rect_filled(r, 0.0, chrome.tile_target_fill);
        painter.rect_stroke(r, 0.0, chrome.tile_target_stroke(), StrokeKind::Inside);
    }
}

#[cfg(test)]
mod tests;
