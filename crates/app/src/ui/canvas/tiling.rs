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
    let region = plotx_core::layout::tiling_drop_region(
        page_pt,
        existing_ids.len(),
        [pointer_page.x, pointer_page.y],
    );
    let pointer_cell = plotx_core::layout::tiling_drop_cell(
        page_pt,
        &layout,
        existing_ids.len() + 1,
        [pointer_page.x, pointer_page.y],
    );
    let cache_key = tile_cache_key(
        drag,
        target,
        page_pt,
        layout,
        &existing_ids,
        region,
        pointer_cell,
    );
    if app
        .session
        .ui
        .tile_drop
        .as_ref()
        .is_some_and(|preview| preview.cache_key == cache_key)
    {
        if let Some(preview) = app.session.ui.tile_drop.as_mut() {
            preview.pointer_screen = [p.x, p.y];
        }
        if let Some(object) = app.doc.canvases[drag.canvas].object_mut(drag.object) {
            object.frame = drag.before;
        }
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
        cache_key,
        target,
        newcomer: plan.newcomer,
        existing: plan.existing,
        pointer_screen: [p.x, p.y],
        anchor: [
            ((drag.start_pointer[0] - drag.before.x) / drag.before.width.max(f32::EPSILON))
                .clamp(0.0, 1.0),
            ((drag.start_pointer[1] - drag.before.y) / drag.before.height.max(f32::EPSILON))
                .clamp(0.0, 1.0),
        ],
    });
    app.session.status = if app.keep_empty_source_canvas {
        "Hold Alt to remove the empty source canvas.".into()
    } else {
        "Hold Alt to keep the empty source canvas.".into()
    };
    if let Some(object) = app.doc.canvases[drag.canvas].object_mut(drag.object) {
        object.frame = drag.before;
    }
    true
}

fn tile_cache_key(
    drag: &ObjectDrag,
    target_canvas: usize,
    target_page_pt: [f32; 2],
    target_layout: plotx_core::layout::PageLayout,
    target_existing_ids: &[ObjectId],
    region: plotx_core::layout::TilingDropRegion,
    pointer_cell: Option<usize>,
) -> TileDropCacheKey {
    TileDropCacheKey {
        source_canvas: drag.canvas,
        source_object: drag.object,
        target_canvas,
        target_page_pt,
        target_layout,
        target_existing_ids: target_existing_ids.to_vec(),
        region,
        pointer_cell,
    }
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

/// Falls back to a plain move if the atomic action cannot be built.
pub(crate) fn commit_tile_drop(
    app: &mut PlotxApp,
    drag: ObjectDrag,
    preview: TileDropPreview,
    alt: bool,
) {
    let remove_empty_source = app.keep_empty_source_canvas == alt;
    let source_becomes_empty =
        app.doc.canvases.get(drag.canvas).is_some_and(|canvas| {
            canvas.objects.len() == 1 && canvas.object(drag.object).is_some()
        });
    let Some(action) = Action::tile_drop(
        app,
        drag.canvas,
        drag.object,
        preview.target,
        preview.newcomer,
        preview.existing,
        remove_empty_source,
    ) else {
        if drag.active {
            finish_object_drag(app, drag.canvas, drag);
        }
        return;
    };
    let target = app.doc.canvases[preview.target].name.clone();
    app.execute_action(action);
    app.session.status = if remove_empty_source && source_becomes_empty {
        format!("Tiled plot into “{target}” and removed the empty source canvas.")
    } else {
        format!("Tiled plot into “{target}”; kept the source canvas.")
    };
}

pub(crate) fn paint_tile_ghost(app: &PlotxApp, painter: &egui::Painter, chrome: ChromeStyle) {
    let (Some(preview), Interaction::Object(drag)) =
        (&app.session.ui.tile_drop, &app.session.ui.interaction)
    else {
        return;
    };
    if preview.cache_key.source_canvas != drag.canvas
        || preview.cache_key.source_object != drag.object
    {
        return;
    }
    let Some(plot) = app
        .doc
        .canvases
        .get(drag.canvas)
        .and_then(|canvas| canvas.object(drag.object))
        .and_then(|object| object.plot())
    else {
        return;
    };
    let ghost = preview.ghost_frame(drag.before, app.session.board.zoom);
    if ![ghost.x, ghost.y, ghost.width, ghost.height]
        .iter()
        .all(|v| v.is_finite())
    {
        return;
    }
    let screen = PlotRect::new(ghost.x, ghost.y, ghost.width, ghost.height);
    plotx_render::screen::paint(painter, screen, &plot.figure, app.session.board.zoom);
    let r = EguiRect::from_min_size(
        Pos2::new(ghost.x, ghost.y),
        Vec2::new(ghost.width, ghost.height),
    );
    painter.rect_filled(r, 0.0, Color32::from_white_alpha(36));
    painter.rect_stroke(
        r,
        0.0,
        Stroke::new(2.0_f32, chrome.tile_target_stroke),
        StrokeKind::Inside,
    );
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

#[cfg(test)]
mod tests {
    use super::*;

    fn drag(canvas: usize, object: ObjectId) -> ObjectDrag {
        ObjectDrag {
            canvas,
            object,
            kind: ObjectDragKind::Move,
            before: ObjectFrame::new(0.0, 0.0, 10.0, 10.0),
            start_pointer: [0.0; 2],
            start_pointer_screen: [0.0; 2],
            others: Vec::new(),
            active: true,
        }
    }

    #[test]
    fn tile_cache_identity_tracks_source_region_target_and_existing_order() {
        let layout = plotx_core::layout::PageLayout::default();
        let page = [400.0, 300.0];
        let base = tile_cache_key(
            &drag(0, 10),
            2,
            page,
            layout,
            &[20, 21],
            plotx_core::layout::TilingDropRegion::Left,
            None,
        );
        assert_eq!(
            base,
            tile_cache_key(
                &drag(0, 10),
                2,
                page,
                layout,
                &[20, 21],
                plotx_core::layout::TilingDropRegion::Left,
                None,
            )
        );
        assert_ne!(
            base,
            tile_cache_key(
                &drag(1, 11),
                2,
                page,
                layout,
                &[20, 21],
                plotx_core::layout::TilingDropRegion::Left,
                None,
            )
        );
        assert_ne!(
            base,
            tile_cache_key(
                &drag(0, 10),
                3,
                page,
                layout,
                &[20, 21],
                plotx_core::layout::TilingDropRegion::Left,
                None,
            )
        );
        assert_ne!(
            base,
            tile_cache_key(
                &drag(0, 10),
                2,
                [401.0, 300.0],
                layout,
                &[20, 21],
                plotx_core::layout::TilingDropRegion::Left,
                None,
            )
        );
        assert_ne!(
            base,
            tile_cache_key(
                &drag(0, 10),
                2,
                page,
                plotx_core::layout::PageLayout { cols: 2, ..layout },
                &[20, 21],
                plotx_core::layout::TilingDropRegion::Left,
                None,
            )
        );
        assert_ne!(
            base,
            tile_cache_key(
                &drag(0, 10),
                2,
                page,
                layout,
                &[20, 21],
                plotx_core::layout::TilingDropRegion::Right,
                None,
            )
        );
        assert_ne!(
            base,
            tile_cache_key(
                &drag(0, 10),
                2,
                page,
                layout,
                &[21, 20],
                plotx_core::layout::TilingDropRegion::Left,
                None,
            )
        );
        let retile_top_left = tile_cache_key(
            &drag(0, 10),
            2,
            page,
            layout,
            &[20, 21],
            plotx_core::layout::TilingDropRegion::Retile,
            Some(0),
        );
        let retile_bottom_right = tile_cache_key(
            &drag(0, 10),
            2,
            page,
            layout,
            &[20, 21],
            plotx_core::layout::TilingDropRegion::Retile,
            Some(3),
        );
        assert_ne!(retile_top_left, retile_bottom_right);
    }
}
