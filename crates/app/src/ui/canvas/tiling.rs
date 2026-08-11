use super::*;

/// The document-level source of an automatic canvas tiling gesture. A one-item
/// Panel and a loose object use the same source so changing page-scope selection
/// can never change the resulting transfer semantics.
#[derive(Clone, Copy, Debug)]
pub(crate) struct TileTransferSource {
    pub(crate) canvas: usize,
    pub(crate) object: ObjectId,
    pub(crate) frame: ObjectFrame,
    pub(crate) start_pointer: [f32; 2],
    pub(crate) panel: Option<PanelId>,
}

pub(crate) fn tile_source_for_object(
    app: &PlotxApp,
    drag: &ObjectDrag,
) -> Option<TileTransferSource> {
    (drag.kind == ObjectDragKind::Move && drag.others.is_empty()).then_some(())?;
    tile_source(
        app,
        drag.canvas,
        drag.object,
        drag.before,
        drag.start_pointer,
        None,
    )
}

pub(crate) fn tile_source_for_panel(
    app: &PlotxApp,
    drag: &PanelDrag,
) -> Option<TileTransferSource> {
    (drag.kind == ObjectDragKind::Move && drag.others.is_empty()).then_some(())?;
    let canvas = app.doc.canvases.get(drag.canvas)?;
    let panel = canvas.panel(drag.panel)?;
    (panel.item_order.len() == 1).then_some(())?;
    tile_source(
        app,
        drag.canvas,
        panel.item_order[0],
        drag.before,
        drag.start_pointer,
        Some(drag.panel),
    )
}

fn tile_source(
    app: &PlotxApp,
    canvas_index: usize,
    object: ObjectId,
    frame: ObjectFrame,
    start_pointer: [f32; 2],
    panel: Option<PanelId>,
) -> Option<TileTransferSource> {
    let canvas = app.doc.canvases.get(canvas_index)?;
    let item = canvas.object(object)?;
    (canvas.content_group(object).is_none()
        && matches!(
            item.kind,
            CanvasObjectKind::Plot(_) | CanvasObjectKind::RasterImage(_)
        ))
    .then_some(TileTransferSource {
        canvas: canvas_index,
        object,
        frame,
        start_pointer,
        panel,
    })
}

pub(crate) fn restore_tile_source(app: &mut PlotxApp, source: TileTransferSource) {
    let Some(canvas) = app.doc.canvases.get_mut(source.canvas) else {
        return;
    };
    if let Some(panel) = source.panel {
        if let Some(panel) = canvas.panel_mut(panel) {
            panel.frame = source.frame;
        }
    } else {
        canvas.set_layout_frame(source.object, source.frame);
    }
}

pub(crate) fn update_tile_drop(
    app: &mut PlotxApp,
    _ci: usize,
    rect: egui::Rect,
    source: TileTransferSource,
    pointer_screen: Option<Pos2>,
) -> bool {
    let Some(p) = pointer_screen else {
        app.session.ui.tile_drop = None;
        return false;
    };
    let Some(FrameRef::Page(target)) = frame_at(app, rect, p) else {
        app.session.ui.tile_drop = None;
        return false;
    };
    if target == source.canvas {
        app.session.ui.tile_drop = None;
        return false;
    }
    let bt = BoardTransform::from_board(app.session.board, rect);
    let pointer_page = bt.screen_to_page(&app.doc.canvases[target], p);
    let page_pt = app.doc.canvases[target].size_pt();
    let layout = app.doc.canvases[target].layout;
    let existing_ids = tileable_object_ids(&app.doc.canvases[target]);
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
        source,
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
        restore_tile_source(app, source);
        return true;
    }
    let existing_items: Vec<_> = existing_ids
        .iter()
        .filter_map(|&id| layout_item(&app.doc.canvases[target], id))
        .collect();
    let Some(newcomer_item) = layout_item(&app.doc.canvases[source.canvas], source.object) else {
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
        source_frame: source.frame,
        pointer_screen: [p.x, p.y],
        anchor: [
            ((source.start_pointer[0] - source.frame.x) / source.frame.width.max(f32::EPSILON))
                .clamp(0.0, 1.0),
            ((source.start_pointer[1] - source.frame.y) / source.frame.height.max(f32::EPSILON))
                .clamp(0.0, 1.0),
        ],
    });
    app.session.status = if app.settings.general.keep_empty_source_canvas {
        "Hold Alt to remove the empty source canvas.".into()
    } else {
        "Hold Alt to keep the empty source canvas.".into()
    };
    restore_tile_source(app, source);
    true
}

fn tile_cache_key(
    source: TileTransferSource,
    target_canvas: usize,
    target_page_pt: [f32; 2],
    target_layout: plotx_core::layout::PageLayout,
    target_existing_ids: &[ObjectId],
    region: plotx_core::layout::TilingDropRegion,
    pointer_cell: Option<usize>,
) -> TileDropCacheKey {
    TileDropCacheKey {
        source_canvas: source.canvas,
        source_object: source.object,
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
    let frame = canvas.layout_frame(id)?;
    match &object.kind {
        CanvasObjectKind::Plot(plot) => {
            Some(plotx_core::layout::layout_item(id, plot.figure(), frame))
        }
        CanvasObjectKind::RasterImage(_) => Some(plotx_core::layout::LayoutItem {
            id,
            insets: [0.0; 4],
        }),
        CanvasObjectKind::Text(_) | CanvasObjectKind::Shape(_) => None,
    }
}

fn tileable_object_ids(canvas: &CanvasDocument) -> Vec<ObjectId> {
    canvas
        .objects
        .iter()
        .filter(|object| {
            matches!(
                object.kind,
                CanvasObjectKind::Plot(_) | CanvasObjectKind::RasterImage(_)
            )
        })
        .map(|object| object.id)
        .collect()
}

/// Falls back to a plain move if the atomic action cannot be built.
pub(crate) fn commit_tile_drop(
    app: &mut PlotxApp,
    source: TileTransferSource,
    preview: TileDropPreview,
    alt: bool,
) {
    let remove_empty_source = app.settings.general.keep_empty_source_canvas == alt;
    let source_becomes_empty =
        app.doc.canvases.get(source.canvas).is_some_and(|canvas| {
            canvas.objects.len() == 1 && canvas.object(source.object).is_some()
        });
    let Some(action) = Action::tile_drop(
        app,
        source.canvas,
        source.object,
        preview.target,
        preview.newcomer,
        preview.existing,
        remove_empty_source,
    ) else {
        restore_tile_source(app, source);
        app.session.status = "Could not tile this content into the destination canvas.".to_owned();
        return;
    };
    let target = app.doc.canvases[preview.target].name.clone();
    app.execute_action(action);
    app.session.status = if remove_empty_source && source_becomes_empty {
        format!("Tiled content into “{target}” and removed the empty source canvas.")
    } else {
        format!("Tiled content into “{target}”; kept the source canvas.")
    };
}

pub(crate) fn paint_tile_ghost(app: &PlotxApp, painter: &egui::Painter, chrome: ChromeStyle) {
    let Some(preview) = &app.session.ui.tile_drop else {
        return;
    };
    let object = app
        .doc
        .canvases
        .get(preview.cache_key.source_canvas)
        .and_then(|canvas| canvas.object(preview.cache_key.source_object));
    let Some(object) = object else {
        return;
    };
    let ghost = preview.ghost_frame(app.session.board.zoom);
    if ![ghost.x, ghost.y, ghost.width, ghost.height]
        .iter()
        .all(|v| v.is_finite())
    {
        return;
    }
    let screen = PlotRect::new(ghost.x, ghost.y, ghost.width, ghost.height);
    if let Some(plot) = object.plot() {
        plotx_render::screen::paint(painter, screen, plot.figure(), app.session.board.zoom);
    }
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

    fn tile_source_for_test(canvas: usize, object: u64) -> TileTransferSource {
        TileTransferSource {
            canvas,
            object: ObjectId::new(object),
            frame: ObjectFrame::new(0.0, 0.0, 10.0, 10.0),
            start_pointer: [0.0; 2],
            panel: None,
        }
    }

    #[test]
    fn tile_cache_identity_tracks_source_region_target_and_existing_order() {
        let layout = plotx_core::layout::PageLayout::default();
        let page = [400.0, 300.0];
        let ids = [ObjectId::new(20), ObjectId::new(21)];
        let reversed_ids = [ObjectId::new(21), ObjectId::new(20)];
        let base = tile_cache_key(
            tile_source_for_test(0, 10),
            2,
            page,
            layout,
            &ids,
            plotx_core::layout::TilingDropRegion::Left,
            None,
        );
        assert_eq!(
            base,
            tile_cache_key(
                tile_source_for_test(0, 10),
                2,
                page,
                layout,
                &ids,
                plotx_core::layout::TilingDropRegion::Left,
                None,
            )
        );
        assert_ne!(
            base,
            tile_cache_key(
                tile_source_for_test(1, 11),
                2,
                page,
                layout,
                &ids,
                plotx_core::layout::TilingDropRegion::Left,
                None,
            )
        );
        assert_ne!(
            base,
            tile_cache_key(
                tile_source_for_test(0, 10),
                3,
                page,
                layout,
                &ids,
                plotx_core::layout::TilingDropRegion::Left,
                None,
            )
        );
        assert_ne!(
            base,
            tile_cache_key(
                tile_source_for_test(0, 10),
                2,
                [401.0, 300.0],
                layout,
                &ids,
                plotx_core::layout::TilingDropRegion::Left,
                None,
            )
        );
        assert_ne!(
            base,
            tile_cache_key(
                tile_source_for_test(0, 10),
                2,
                page,
                plotx_core::layout::PageLayout { cols: 2, ..layout },
                &ids,
                plotx_core::layout::TilingDropRegion::Left,
                None,
            )
        );
        assert_ne!(
            base,
            tile_cache_key(
                tile_source_for_test(0, 10),
                2,
                page,
                layout,
                &ids,
                plotx_core::layout::TilingDropRegion::Right,
                None,
            )
        );
        assert_ne!(
            base,
            tile_cache_key(
                tile_source_for_test(0, 10),
                2,
                page,
                layout,
                &reversed_ids,
                plotx_core::layout::TilingDropRegion::Left,
                None,
            )
        );
        let retile_top_left = tile_cache_key(
            tile_source_for_test(0, 10),
            2,
            page,
            layout,
            &ids,
            plotx_core::layout::TilingDropRegion::Retile,
            Some(0),
        );
        let retile_bottom_right = tile_cache_key(
            tile_source_for_test(0, 10),
            2,
            page,
            layout,
            &ids,
            plotx_core::layout::TilingDropRegion::Retile,
            Some(3),
        );
        assert_ne!(retile_top_left, retile_bottom_right);
    }
}
