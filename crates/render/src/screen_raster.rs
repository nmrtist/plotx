use crate::{DocumentItem, DocumentRaster, DocumentText, DocumentViewport, RasterFit, Rect};
use plotx_figure::Color;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct TextureKey {
    hash: [u8; 32],
    pixel_size: [u32; 2],
    nearest: bool,
}

#[derive(Clone)]
struct CachedTexture {
    key: TextureKey,
    texture: egui::TextureHandle,
    bytes: usize,
    last_used: u64,
}

#[derive(Clone, Default)]
struct RasterTextureCache {
    entries: Vec<CachedTexture>,
    tick: u64,
    bytes: usize,
}

const TEXTURE_BUDGET: usize = 256 * 1024 * 1024;

pub(crate) fn contrasting_label_color(
    items: &[DocumentItem<'_>],
    frame: &Rect,
    text: &DocumentText,
    background: Color,
) -> Color {
    let point = [frame.left + text.position[0], frame.top + text.position[1]];
    let Some(raster) = items.iter().rev().find_map(|item| match item {
        DocumentItem::Raster(raster) if raster.visible && point_in_rect(point, raster.frame) => {
            Some(raster)
        }
        _ => None,
    }) else {
        return Color::BLACK;
    };
    let page_frame = egui::Rect::from_min_size(
        egui::pos2(raster.frame.left, raster.frame.top),
        egui::vec2(raster.frame.width, raster.frame.height),
    );
    let (display, crop) = fitted_geometry(
        page_frame,
        raster.source_pixel_size,
        raster.crop,
        raster.quarter_turns,
        raster.fit,
    );
    let point = egui::pos2(point[0], point[1]);
    if !display.contains(point) {
        return Color::BLACK;
    }
    let x = (point.x - display.left()) / display.width().max(f32::EPSILON);
    let y = (point.y - display.top()) / display.height().max(f32::EPSILON);
    let [u, v] = rotated_uv(x, y, crop, raster.quarter_turns);
    let px = (u * raster.pixel_size[0].saturating_sub(1) as f32).round() as u32;
    let py = (v * raster.pixel_size[1].saturating_sub(1) as f32).round() as u32;
    let offset = (py as usize * raster.pixel_size[0] as usize + px as usize) * 4;
    let Some(pixel) = raster.pixels.get(offset..offset + 4) else {
        return Color::BLACK;
    };
    let alpha = f32::from(pixel[3]) / 255.0;
    let blend = |channel: u8, bg: u8| f32::from(channel) * alpha + f32::from(bg) * (1.0 - alpha);
    let luminance = 0.2126 * blend(pixel[0], background.r)
        + 0.7152 * blend(pixel[1], background.g)
        + 0.0722 * blend(pixel[2], background.b);
    if luminance < 140.0 {
        Color::rgb(255, 255, 255)
    } else {
        Color::BLACK
    }
}

fn point_in_rect(point: [f32; 2], rect: Rect) -> bool {
    point[0] >= rect.left
        && point[0] <= rect.left + rect.width
        && point[1] >= rect.top
        && point[1] <= rect.top + rect.height
}

fn rotated_uv(x: f32, y: f32, crop: [f32; 4], turns: u8) -> [f32; 2] {
    let [left, top, right, bottom] = crop;
    let (u, v) = match turns % 4 {
        1 => (y, 1.0 - x),
        2 => (1.0 - x, 1.0 - y),
        3 => (1.0 - y, x),
        _ => (x, y),
    };
    [left + u * (right - left), top + v * (bottom - top)]
}

pub(crate) fn paint_document_raster(
    painter: &egui::Painter,
    page: Rect,
    raster: &DocumentRaster<'_>,
    viewport: DocumentViewport,
) {
    if !raster.visible || raster.pixel_size.contains(&0) {
        return;
    }
    let Ok(width) = usize::try_from(raster.pixel_size[0]) else {
        return;
    };
    let Ok(height) = usize::try_from(raster.pixel_size[1]) else {
        return;
    };
    let Some(expected) = width
        .checked_mul(height)
        .and_then(|value| value.checked_mul(4))
    else {
        return;
    };
    if raster.pixels.len() != expected {
        return;
    }
    let frame = egui::Rect::from_min_size(
        egui::pos2(
            page.left + raster.frame.left * viewport.zoom,
            page.top + raster.frame.top * viewport.zoom,
        ),
        egui::vec2(
            raster.frame.width * viewport.zoom,
            raster.frame.height * viewport.zoom,
        ),
    );
    let (rect, crop) = fitted_geometry(
        frame,
        raster.source_pixel_size,
        raster.crop,
        raster.quarter_turns,
        raster.fit,
    );
    let key = TextureKey {
        hash: raster.source_hash,
        pixel_size: raster.pixel_size,
        nearest: raster.nearest,
    };
    let options = if raster.nearest {
        egui::TextureOptions::NEAREST
    } else {
        egui::TextureOptions::LINEAR
    };
    let texture = cached_texture(painter.ctx(), key, [width, height], raster.pixels, options);
    let [left, top, right, bottom] = crop;
    let uv = egui::Rect::from_min_max(egui::pos2(left, top), egui::pos2(right, bottom));
    let tint =
        egui::Color32::from_white_alpha((raster.opacity.clamp(0.0, 1.0) * 255.0).round() as u8);
    let corners = [
        rect.left_top(),
        rect.right_top(),
        rect.right_bottom(),
        rect.left_bottom(),
    ];
    let uv_corners = [
        uv.left_top(),
        uv.right_top(),
        uv.right_bottom(),
        uv.left_bottom(),
    ];
    let turns = usize::from(raster.quarter_turns % 4);
    let mut mesh = egui::Mesh::with_texture(texture.id());
    for index in 0..4 {
        mesh.vertices.push(egui::epaint::Vertex {
            pos: corners[index],
            uv: uv_corners[(index + 4 - turns) % 4],
            color: tint,
        });
    }
    mesh.indices.extend_from_slice(&[0, 1, 2, 0, 2, 3]);
    let mut clip = painter.clip_rect().intersect(frame);
    if let Some(bounds) = raster.clip {
        let bounds = egui::Rect::from_min_size(
            egui::pos2(
                page.left + bounds.left * viewport.zoom,
                page.top + bounds.top * viewport.zoom,
            ),
            egui::vec2(bounds.width * viewport.zoom, bounds.height * viewport.zoom),
        );
        clip = clip.intersect(bounds);
    }
    painter.with_clip_rect(clip).add(egui::Shape::mesh(mesh));
}

fn cached_texture(
    ctx: &egui::Context,
    key: TextureKey,
    size: [usize; 2],
    pixels: &[u8],
    options: egui::TextureOptions,
) -> egui::TextureHandle {
    let cache_id = egui::Id::new("plotx-raster-texture-cache-v1");
    if let Some(hit) = ctx.data_mut(|data| {
        let mut cache = data
            .get_temp::<RasterTextureCache>(cache_id)
            .unwrap_or_default();
        cache.tick = cache.tick.wrapping_add(1);
        let tick = cache.tick;
        let hit = cache
            .entries
            .iter_mut()
            .find(|entry| entry.key == key)
            .map(|entry| {
                entry.last_used = tick;
                entry.texture.clone()
            });
        data.insert_temp(cache_id, cache);
        hit
    }) {
        return hit;
    }
    let image = egui::ColorImage::from_rgba_unmultiplied(size, pixels);
    let texture = ctx.load_texture(
        format!("plotx-raster-{:02x?}", &key.hash[..4]),
        image,
        options,
    );
    ctx.data_mut(|data| {
        let mut cache = data
            .get_temp::<RasterTextureCache>(cache_id)
            .unwrap_or_default();
        cache.tick = cache.tick.wrapping_add(1);
        let bytes = pixels.len();
        cache.bytes = cache.bytes.saturating_add(bytes);
        cache.entries.push(CachedTexture {
            key,
            texture: texture.clone(),
            bytes,
            last_used: cache.tick,
        });
        while cache.bytes > TEXTURE_BUDGET && cache.entries.len() > 1 {
            let index = cache
                .entries
                .iter()
                .enumerate()
                .min_by_key(|(_, entry)| entry.last_used)
                .map(|(index, _)| index)
                .unwrap_or(0);
            let removed = cache.entries.swap_remove(index);
            cache.bytes = cache.bytes.saturating_sub(removed.bytes);
        }
        data.insert_temp(cache_id, cache);
    });
    texture
}

fn fitted_geometry(
    frame: egui::Rect,
    pixel_size: [u32; 2],
    crop: [f32; 4],
    turns: u8,
    fit: RasterFit,
) -> (egui::Rect, [f32; 4]) {
    if fit == RasterFit::Stretch {
        return (frame, crop);
    }
    let mut source = [
        pixel_size[0] as f32 * (crop[2] - crop[0]),
        pixel_size[1] as f32 * (crop[3] - crop[1]),
    ];
    if turns % 2 == 1 {
        source.swap(0, 1);
    }
    let source_aspect = source[0] / source[1];
    let frame_aspect = frame.width() / frame.height();
    if fit == RasterFit::Contain {
        let size = if source_aspect > frame_aspect {
            egui::vec2(frame.width(), frame.width() / source_aspect)
        } else {
            egui::vec2(frame.height() * source_aspect, frame.height())
        };
        return (egui::Rect::from_center_size(frame.center(), size), crop);
    }
    let mut covered = crop;
    if source_aspect > frame_aspect {
        let fraction = frame_aspect / source_aspect;
        shrink_crop_axis(&mut covered, turns.is_multiple_of(2), fraction);
    } else {
        let fraction = source_aspect / frame_aspect;
        shrink_crop_axis(&mut covered, turns % 2 == 1, fraction);
    }
    (frame, covered)
}

fn shrink_crop_axis(crop: &mut [f32; 4], horizontal: bool, fraction: f32) {
    let (start, end) = if horizontal { (0, 2) } else { (1, 3) };
    let center = (crop[start] + crop[end]) * 0.5;
    let half = (crop[end] - crop[start]) * fraction * 0.5;
    crop[start] = center - half;
    crop[end] = center + half;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn raster(pixels: &[u8]) -> DocumentRaster<'_> {
        DocumentRaster {
            source_hash: [0; 32],
            frame: Rect::new(0.0, 0.0, 100.0, 100.0),
            pixels,
            pixel_size: [1, 1],
            source_pixel_size: [1, 1],
            crop: [0.0, 0.0, 1.0, 1.0],
            fit: RasterFit::Stretch,
            quarter_turns: 0,
            opacity: 1.0,
            nearest: false,
            clip: None,
            visible: true,
        }
    }

    #[test]
    fn panel_label_contrast_uses_white_on_dark_and_black_on_light_rasters() {
        let text = DocumentText {
            text: "a".into(),
            position: [5.0, 5.0],
            font_size: 8.0,
        };
        let frame = Rect::new(0.0, 0.0, 100.0, 100.0);
        let dark = [DocumentItem::Raster(raster(&[0, 0, 0, 255]))];
        assert_eq!(
            contrasting_label_color(&dark, &frame, &text, Color::rgb(255, 255, 255)),
            Color::rgb(255, 255, 255)
        );
        let light = [DocumentItem::Raster(raster(&[255, 255, 255, 255]))];
        assert_eq!(
            contrasting_label_color(&light, &frame, &text, Color::BLACK),
            Color::BLACK
        );
    }

    #[test]
    fn contain_letterboxes_without_changing_crop() {
        let frame = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(100.0, 100.0));
        let (rect, crop) = fitted_geometry(
            frame,
            [200, 100],
            [0.0, 0.0, 1.0, 1.0],
            0,
            RasterFit::Contain,
        );
        assert_eq!(rect.size(), egui::vec2(100.0, 50.0));
        assert_eq!(crop, [0.0, 0.0, 1.0, 1.0]);
    }

    #[test]
    fn cover_crops_the_displayed_horizontal_axis_after_rotation() {
        let frame = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(100.0, 100.0));
        let (_, crop) =
            fitted_geometry(frame, [100, 200], [0.0, 0.0, 1.0, 1.0], 1, RasterFit::Cover);
        assert_eq!(crop, [0.0, 0.25, 1.0, 0.75]);
    }
}
