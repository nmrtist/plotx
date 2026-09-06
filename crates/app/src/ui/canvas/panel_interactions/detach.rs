use super::*;

const DWELL_SECONDS: f64 = 0.5;

fn fully_detached(app: &PlotxApp, drag: &PanelDrag, rect: EguiRect, pointer: Option<Pos2>) -> bool {
    if !drag.active
        || drag.kind != ObjectDragKind::Move
        || !drag.others.is_empty()
        || !pointer.is_some_and(|p| rect.contains(p))
    {
        return false;
    }
    let Some(page) = app.doc.canvases.get(drag.canvas) else {
        return false;
    };
    let Some(panel) = page.panel(drag.panel) else {
        return false;
    };
    let frame = panel.frame;
    let bounds = EguiRect::from_min_size(
        Pos2::new(page.board_pos[0] + frame.x, page.board_pos[1] + frame.y),
        Vec2::new(frame.width, frame.height),
    );
    bounds.is_finite()
        && app.doc.canvases.iter().all(|page| {
            let size = page.size_pt();
            !bounds.intersects(EguiRect::from_min_size(
                Pos2::new(page.board_pos[0], page.board_pos[1]),
                Vec2::new(size[0], size[1]),
            ))
        })
}

pub(super) fn update(
    app: &mut PlotxApp,
    rect: EguiRect,
    pointer: Option<Pos2>,
    ctx: &egui::Context,
) {
    let Interaction::Panel(drag) = &app.session.ui.interaction else {
        return;
    };
    let detached = fully_detached(app, drag, rect, pointer);
    let now = ctx.input(|input| input.time);
    if let Interaction::Panel(drag) = &mut app.session.ui.interaction {
        if detached {
            drag.detached_since.get_or_insert(now);
            if !ready(drag, now) {
                ctx.request_repaint_after(std::time::Duration::from_millis(16));
            }
        } else {
            drag.detached_since = None;
        }
    }
}

pub(super) fn ready(drag: &PanelDrag, now: f64) -> bool {
    drag.detached_since
        .is_some_and(|since| now - since >= DWELL_SECONDS)
}

pub(super) fn commit(app: &mut PlotxApp, drag: &PanelDrag) {
    let Some(page) = app.doc.canvases.get_mut(drag.canvas) else {
        return;
    };
    let canvas = page.resource_id;
    let origin = page.board_pos;
    let Some(panel) = page.panel_mut(drag.panel) else {
        return;
    };
    let board_pos = [origin[0] + panel.frame.x, origin[1] + panel.frame.y];
    panel.frame = drag.before;
    match app.detach_panel_action(canvas, drag.panel, board_pos) {
        Ok(action) => {
            app.execute_action(action);
            if let Some(ci) = app.session.active_canvas {
                app.select_panel(ci, drag.panel);
            }
        }
        Err(error) => app.session.status = format!("Could not create canvas for Panel: {error}"),
    }
    app.reset_interaction();
}

pub(crate) fn paint_panel_detach(
    app: &PlotxApp,
    rect: EguiRect,
    painter: &egui::Painter,
    chrome: ChromeStyle,
) {
    let Interaction::Panel(drag) = &app.session.ui.interaction else {
        return;
    };
    let Some(since) = drag.detached_since else {
        return;
    };
    let now = painter.ctx().input(|input| input.time);
    let is_ready = ready(drag, now);
    let Some(pointer) = painter.ctx().input(|input| input.pointer.hover_pos()) else {
        return;
    };
    let page = &app.doc.canvases[drag.canvas];
    let Some(panel) = page.panel(drag.panel) else {
        return;
    };
    let bt = BoardTransform::from_board(app.session.board, rect);
    let frame = panel.frame;
    let r = EguiRect::from_min_size(
        bt.page_screen_rect(page).min + Vec2::new(frame.x, frame.y) * bt.zoom,
        Vec2::new(frame.width, frame.height) * bt.zoom,
    );
    let color = if is_ready {
        Color32::from_rgb(36, 180, 110)
    } else {
        chrome.tile_target_stroke
    };
    painter.rect_stroke(
        r,
        0.0,
        Stroke::new(if is_ready { 3.0_f32 } else { 1.5_f32 }, color),
        StrokeKind::Inside,
    );
    let size = Vec2::new(196.0, 34.0);
    let anchor = if r.bottom() + size.y + 8.0 <= rect.bottom() {
        r.left_bottom() + Vec2::new(0.0, 8.0)
    } else if r.top() - size.y - 8.0 >= rect.top() {
        r.left_top() - Vec2::new(0.0, size.y + 8.0)
    } else {
        pointer + Vec2::new(16.0, 20.0)
    };
    let min = anchor.min(rect.max - size).max(rect.min);
    let badge = EguiRect::from_min_size(min, size);
    let visuals = painter.ctx().global_style().visuals.clone();
    painter.rect_filled(badge, 4.0, visuals.extreme_bg_color);
    painter.rect_stroke(badge, 4.0, Stroke::new(1.0_f32, color), StrokeKind::Inside);
    painter.text(
        badge.center() - Vec2::new(0.0, 2.0),
        egui::Align2::CENTER_CENTER,
        if is_ready {
            "Release to create canvas"
        } else {
            "New canvas"
        },
        egui::FontId::proportional(13.0),
        visuals.text_color(),
    );
    let progress = ((now - since) / DWELL_SECONDS).clamp(0.0, 1.0) as f32;
    painter.rect_filled(
        EguiRect::from_min_size(
            badge.left_bottom() + Vec2::new(4.0, -5.0),
            Vec2::new((size.x - 8.0) * progress, 2.0),
        ),
        0.0,
        color,
    );
}

#[cfg(test)]
mod tests;

pub(crate) fn paint_panel_detach_background(
    app: &PlotxApp,
    ci: usize,
    rect: EguiRect,
    painter: &egui::Painter,
) {
    let Interaction::Panel(drag) = &app.session.ui.interaction else {
        return;
    };
    if drag.canvas != ci || drag.detached_since.is_none() {
        return;
    }
    let page = &app.doc.canvases[ci];
    let Some(panel) = page.panel(drag.panel) else {
        return;
    };
    let bt = BoardTransform::from_board(app.session.board, rect);
    let frame = panel.frame;
    let r = EguiRect::from_min_size(
        bt.page_screen_rect(page).min + Vec2::new(frame.x, frame.y) * bt.zoom,
        Vec2::new(frame.width, frame.height) * bt.zoom,
    );
    let color = page.background;
    painter.rect_filled(r, 0.0, Color32::from_rgb(color.r, color.g, color.b));
}
