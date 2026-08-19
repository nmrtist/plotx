use super::*;
pub(super) fn paint_document(app: &PlotxApp, ci: usize, rect: egui::Rect, painter: &egui::Painter) {
    let canvas = &app.doc.canvases[ci];
    let [width, height] = canvas.size_pt();
    let mut items = plotx_core::state::document_items(canvas);
    for (object_index, object) in canvas.objects.iter().enumerate() {
        let plotx_core::state::CanvasObjectKind::RasterImage(image) = &object.kind else {
            continue;
        };
        let frame = canvas
            .content_page_frame(object.id)
            .unwrap_or(object.frame)
            .rect();
        let parent_panel = canvas
            .parent_panel(object.id)
            .and_then(|id| canvas.panel(id));
        let panel_visible = parent_panel.is_none_or(|panel| panel.visible);
        let clip = parent_panel
            .filter(|panel| panel.clip_children)
            .map(|panel| panel.frame.rect());
        let Some(asset) = app.doc.assets.get(&image.asset) else {
            items[object_index] = missing_image_placeholder(frame, panel_visible && object.visible);
            continue;
        };
        let Some(preview) =
            app.session.ui.raster_proxies.iter().find(|preview| {
                preview.hash == asset.sha256 && preview.page_index == image.page_index
            })
        else {
            items[object_index] = loading_image_placeholder(frame, panel_visible && object.visible);
            continue;
        };
        items[object_index] = plotx_render::DocumentItem::Raster(plotx_render::DocumentRaster {
            source_hash: raster_cache_hash(asset.sha256, image.page_index),
            frame,
            pixels: preview.rgba8.clone(),
            pixel_size: preview.pixel_size,
            source_pixel_size: if image.page_index == 0 {
                asset.pixel_size
            } else {
                preview.pixel_size
            },
            crop: image.crop,
            fit: match image.fit {
                plotx_core::state::ImageFit::Contain => plotx_render::RasterFit::Contain,
                plotx_core::state::ImageFit::Cover => plotx_render::RasterFit::Cover,
                plotx_core::state::ImageFit::Stretch => plotx_render::RasterFit::Stretch,
            },
            quarter_turns: match image.rotation {
                plotx_core::state::QuarterTurn::Zero => 0,
                plotx_core::state::QuarterTurn::Clockwise90 => 1,
                plotx_core::state::QuarterTurn::Clockwise180 => 2,
                plotx_core::state::QuarterTurn::Clockwise270 => 3,
            },
            opacity: image.opacity,
            nearest: matches!(
                image.interpolation,
                plotx_core::state::ImageInterpolation::Nearest
            ),
            clip,
            visible: panel_visible && object.visible,
        });
    }
    let document = plotx_render::Document {
        width,
        height,
        background: canvas.background,
        items,
    };
    let zoom = app.session.board.zoom;
    let bp = canvas.board_pos;
    plotx_render::screen::paint_document_for_editor(
        painter,
        PlotRect::new(rect.left(), rect.top(), rect.width(), rect.height()),
        &document,
        plotx_render::DocumentViewport {
            zoom,
            pan: [
                rect.width() * 0.5 + (bp[0] - app.session.board.world_center[0]) * zoom,
                rect.height() * 0.5 + (bp[1] - app.session.board.world_center[1]) * zoom,
            ],
        },
    );
}

fn loading_image_placeholder(
    frame: plotx_render::Rect,
    visible: bool,
) -> plotx_render::DocumentItem<'static> {
    plotx_render::DocumentItem::Overlay(plotx_render::DocumentOverlay {
        frame,
        visible,
        kind: plotx_render::OverlayKind::Text(plotx_render::OverlayText {
            text: "Preparing image preview…",
            font_size: 9.0,
            color: plotx_figure::Color::rgb(90, 90, 90),
            align: plotx_render::OverlayAlign::Center,
            bold: false,
        }),
    })
}

fn missing_image_placeholder(
    frame: plotx_render::Rect,
    visible: bool,
) -> plotx_render::DocumentItem<'static> {
    plotx_render::DocumentItem::Overlay(plotx_render::DocumentOverlay {
        frame,
        visible,
        kind: plotx_render::OverlayKind::Text(plotx_render::OverlayText {
            text: "Missing image — replace it in the inspector",
            font_size: 9.0,
            color: plotx_figure::Color::rgb(180, 45, 45),
            align: plotx_render::OverlayAlign::Center,
            bold: true,
        }),
    })
}

fn raster_cache_hash(mut hash: [u8; 32], page_index: u32) -> [u8; 32] {
    for (slot, value) in hash[..4].iter_mut().zip(page_index.to_le_bytes()) {
        *slot ^= value;
    }
    hash
}
