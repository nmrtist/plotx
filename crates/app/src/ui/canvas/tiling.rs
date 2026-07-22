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
    if app.session.ui.tile_drop.as_ref().is_some_and(|preview| {
        preview.target == target
            && preview
                .existing
                .iter()
                .map(|(id, _)| *id)
                .eq(existing_ids.iter().copied())
            && preview_cell_matches(preview, page_pt, existing_ids.len(), pointer_page)
    }) {
        return true;
    }
    let existing_items: Vec<_> = existing_ids
        .iter()
        .filter_map(|&id| layout_item(&app.doc.canvases[target], id))
        .collect();
    let Some(newcomer_item) = layout_item(&app.doc.canvases[drag.canvas], drag.object) else {
        app.session.ui.tile_drop = None;
        return false;
    };
    let plan = plotx_core::layout::compute_tiling_plan_for_items(
        page_pt,
        &layout,
        &existing_items,
        newcomer_item,
        [pointer_page.x, pointer_page.y],
    );
    app.session.ui.tile_drop = Some(TileDropPreview {
        target,
        newcomer: plan.newcomer,
        existing: plan.existing,
    });
    true
}

fn layout_item(canvas: &CanvasDocument, id: ObjectId) -> Option<plotx_core::layout::LayoutItem> {
    let object = canvas.object(id)?;
    let plot = object.plot()?;
    Some(plotx_core::layout::layout_item(
        id,
        &plot.figure,
        object.frame,
    ))
}

fn preview_cell_matches(
    preview: &TileDropPreview,
    page: [f32; 2],
    existing_count: usize,
    pointer: Pos2,
) -> bool {
    if existing_count != 1 {
        return true;
    }
    let Some((_, existing)) = preview.existing.first() else {
        return false;
    };
    let nx = if page[0] > 0.0 {
        pointer.x / page[0]
    } else {
        0.5
    };
    let ny = if page[1] > 0.0 {
        pointer.y / page[1]
    } else {
        0.5
    };
    let horizontal = (nx - 0.5).abs() >= (ny - 0.5).abs();
    let newcomer_last = if horizontal { nx >= 0.5 } else { ny >= 0.5 };
    let dx = preview.newcomer.x - existing.x;
    let dy = preview.newcomer.y - existing.y;
    let preview_horizontal = dx.abs() >= dy.abs();
    let preview_last = if preview_horizontal {
        dx >= 0.0
    } else {
        dy >= 0.0
    };
    horizontal == preview_horizontal && newcomer_last == preview_last
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

pub(crate) fn paint_tile_preview(
    app: &PlotxApp,
    rect: egui::Rect,
    painter: &egui::Painter,
    chrome: ChromeStyle,
) {
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
    for (_, f) in &preview.existing {
        let r = to_screen(f);
        painter.rect_filled(r, 0.0, chrome.tile_existing_fill);
        painter.rect_stroke(r, 0.0, chrome.tile_existing_stroke(), StrokeKind::Inside);
    }
    let r = to_screen(&preview.newcomer);
    painter.rect_filled(r, 0.0, chrome.tile_target_fill);
    let outline = [
        r.left_top(),
        r.right_top(),
        r.right_bottom(),
        r.left_bottom(),
        r.left_top(),
    ];
    for segment in egui::Shape::dashed_line(&outline, chrome.tile_target_stroke(), 6.0, 4.0) {
        painter.add(segment);
    }
}
