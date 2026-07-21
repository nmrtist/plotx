use super::*;

fn drag_is_tileable(app: &PlotxApp, drag: &ObjectDrag) -> bool {
    if drag.kind != ObjectDragKind::Move || !drag.others.is_empty() {
        return false;
    }
    if app.session.ui.selection.objects().len() > 1 {
        return false;
    }
    app.doc
        .canvases
        .get(drag.canvas)
        .and_then(|c| c.object(drag.object))
        .is_some_and(|o| o.group.is_none() && o.plot().is_some())
}

pub(crate) fn update_tile_drop(
    app: &mut PlotxApp,
    _ci: usize,
    rect: egui::Rect,
    drag: &ObjectDrag,
    pointer_screen: Option<Pos2>,
) -> bool {
    if !drag_is_tileable(app, drag) {
        app.session.ui.tile_drop = None;
        return false;
    }
    let Some(p) = pointer_screen else {
        app.session.ui.tile_drop = None;
        return false;
    };
    let Some(FrameRef::Page(target)) = frame_at(app, rect, p) else {
        app.session.ui.tile_drop = None;
        return false;
    };
    if target == drag.canvas {
        app.session.ui.tile_drop = None;
        return false;
    }
    let bt = BoardTransform::from_board(app.session.board, rect);
    let pointer_page = bt.screen_to_page(&app.doc.canvases[target], p);
    let page_pt = app.doc.canvases[target].size_pt();
    let layout = app.doc.canvases[target].layout;
    let existing_ids = app.doc.canvases[target].plot_object_ids();
    let plan = plotx_core::layout::compute_tiling_plan(
        page_pt,
        &layout,
        &existing_ids,
        [pointer_page.x, pointer_page.y],
    );
    app.session.ui.tile_drop = Some(TileDropPreview {
        target,
        newcomer: plan.newcomer,
        existing: plan.existing,
    });
    true
}

/// Falls back to a plain move if the atomic action cannot be built.
pub(crate) fn commit_tile_drop(
    app: &mut PlotxApp,
    ci: usize,
    drag: ObjectDrag,
    preview: TileDropPreview,
) {
    let Some(action) = Action::tile_drop(
        app,
        ci,
        drag.object,
        preview.target,
        preview.newcomer,
        preview.existing,
    ) else {
        if drag.active {
            finish_object_drag(app, ci, drag);
        }
        return;
    };
    let target = app.doc.canvases[preview.target].name.clone();
    app.execute_action(action);
    app.session.status = format!("Tiled plot into “{target}”.");
}

pub(crate) fn paint_tile_preview(app: &PlotxApp, rect: egui::Rect, painter: &egui::Painter) {
    let Some(preview) = &app.session.ui.tile_drop else {
        return;
    };
    let Some(canvas) = app.doc.canvases.get(preview.target) else {
        return;
    };
    let bt = BoardTransform::from_board(app.session.board, rect);
    let page = bt.page_screen_rect(canvas);
    let zoom = bt.zoom;
    let to_screen = |f: &ObjectFrame| {
        EguiRect::from_min_size(
            Pos2::new(page.left() + f.x * zoom, page.top() + f.y * zoom),
            Vec2::new(f.width * zoom, f.height * zoom),
        )
    };
    let existing_fill = Color32::from_rgba_premultiplied(0x5a, 0xa9, 0xc4, 40);
    for (_, f) in &preview.existing {
        let r = to_screen(f);
        painter.rect_filled(r, 0.0, existing_fill);
        painter.rect_stroke(r, 0.0, Stroke::new(1.0_f32, GRID_COLOR), StrokeKind::Inside);
    }
    let r = to_screen(&preview.newcomer);
    painter.rect_filled(r, 0.0, SELECT_FILL);
    painter.rect_stroke(
        r,
        0.0,
        Stroke::new(2.0_f32, SELECT_STROKE),
        StrokeKind::Inside,
    );
}
