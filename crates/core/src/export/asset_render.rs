use super::ExportError;
use crate::state::{
    AssetId, AssetRecord, CanvasDocument, CanvasObjectKind, ImageFit, ImageInterpolation,
    QuarterTurn, document_items,
};
use plotx_figure::Color;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::sync::Arc;

const MAX_EXPORT_SAMPLE_EDGE: u32 = 4096;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MissingImagePolicy {
    #[default]
    Block,
    Placeholder,
}

pub fn prepare_render_document<'a>(
    canvas: &'a CanvasDocument,
    assets: &BTreeMap<AssetId, AssetRecord>,
    missing_policy: MissingImagePolicy,
) -> Result<plotx_render::Document<'a>, ExportError> {
    let [width, height] = canvas.size_pt();
    let mut items = document_items(canvas);
    let mut decoded_assets: BTreeMap<(AssetId, u32), DecodedAsset> = BTreeMap::new();
    for (index, object) in canvas.objects.iter().enumerate() {
        let CanvasObjectKind::RasterImage(image) = &object.kind else {
            continue;
        };
        let frame = canvas
            .content_page_frame(object.id)
            .unwrap_or(object.frame)
            .rect();
        let parent = canvas
            .parent_panel(object.id)
            .and_then(|panel| canvas.panel(panel));
        let visible = object.visible && parent.is_none_or(|panel| panel.visible);
        let clip = parent
            .filter(|panel| panel.clip_children)
            .map(|panel| panel.frame.rect());
        let key = (image.asset, image.page_index);
        let decoded = if let Some(decoded) = decoded_assets.get(&key) {
            Ok(decoded.clone())
        } else {
            assets
                .get(&image.asset)
                .ok_or(ExportError::MissingImageAsset { asset: image.asset })
                .and_then(|asset| decode_asset(asset, image.page_index))
                .inspect(|decoded| {
                    decoded_assets.insert(key, (*decoded).clone());
                })
        };
        let decoded = match decoded {
            Ok(decoded) => decoded,
            Err(_error) if missing_policy == MissingImagePolicy::Placeholder => {
                items[index] = missing_image_placeholder(frame, visible);
                continue;
            }
            Err(error) => return Err(error),
        };
        items[index] = plotx_render::DocumentItem::Raster(plotx_render::DocumentRaster {
            source_hash: raster_cache_hash(decoded.sha256, image.page_index),
            frame,
            pixels: decoded.pixels,
            pixel_size: decoded.pixel_size,
            source_pixel_size: decoded.source_pixel_size,
            crop: image.crop,
            fit: match image.fit {
                ImageFit::Contain => plotx_render::RasterFit::Contain,
                ImageFit::Cover => plotx_render::RasterFit::Cover,
                ImageFit::Stretch => plotx_render::RasterFit::Stretch,
            },
            quarter_turns: match image.rotation {
                QuarterTurn::Zero => 0,
                QuarterTurn::Clockwise90 => 1,
                QuarterTurn::Clockwise180 => 2,
                QuarterTurn::Clockwise270 => 3,
            },
            opacity: image.opacity,
            nearest: image.interpolation == ImageInterpolation::Nearest,
            clip,
            visible,
        });
    }
    Ok(plotx_render::Document {
        width,
        height,
        background: canvas.background,
        items,
    })
}

fn decode_asset(asset: &AssetRecord, page_index: u32) -> Result<DecodedAsset, ExportError> {
    let actual: [u8; 32] = Sha256::digest(&asset.bytes).into();
    if actual != asset.sha256 {
        return Err(ExportError::CorruptImageAsset {
            asset: asset.id,
            reason: "embedded bytes do not match the recorded SHA-256".to_owned(),
        });
    }
    let probe =
        plotx_io::image::probe(&asset.bytes).map_err(|error| ExportError::CorruptImageAsset {
            asset: asset.id,
            reason: error.to_string(),
        })?;
    let source_pixel_size = if page_index == 0 {
        [probe.width, probe.height]
    } else {
        plotx_io::image::tiff_page_dimensions(&asset.bytes, page_index).ok_or_else(|| {
            ExportError::CorruptImageAsset {
                asset: asset.id,
                reason: format!("image page {page_index} is unavailable or unsupported"),
            }
        })?
    };

    let decoded = plotx_io::image::decode_rgba8_page(&asset.bytes, page_index, false);
    match decoded {
        Ok(decoded) => Ok(DecodedAsset {
            sha256: decoded.sha256,
            pixel_size: [decoded.probe.width, decoded.probe.height],
            source_pixel_size,
            pixels: Arc::from(decoded.rgba8),
        }),
        Err(plotx_io::image::ImageError::TooLarge { .. }) if page_index == 0 => {
            let sample = plotx_io::image::decode_proxy_rgba8(&asset.bytes, MAX_EXPORT_SAMPLE_EDGE)
                .map_err(|error| ExportError::CorruptImageAsset {
                    asset: asset.id,
                    reason: error.to_string(),
                })?;
            Ok(DecodedAsset {
                sha256: actual,
                pixel_size: sample.pixel_size,
                source_pixel_size,
                pixels: Arc::from(sample.rgba8),
            })
        }
        Err(error) => Err(ExportError::CorruptImageAsset {
            asset: asset.id,
            reason: error.to_string(),
        }),
    }
}

#[derive(Clone)]
struct DecodedAsset {
    sha256: [u8; 32],
    pixel_size: [u32; 2],
    source_pixel_size: [u32; 2],
    pixels: Arc<[u8]>,
}

fn missing_image_placeholder(
    frame: plotx_render::Rect,
    visible: bool,
) -> plotx_render::DocumentItem<'static> {
    plotx_render::DocumentItem::Overlay(plotx_render::DocumentOverlay {
        frame,
        visible,
        kind: plotx_render::OverlayKind::Text(plotx_render::OverlayText {
            text: "Missing image - replace it in the inspector",
            font_size: 8.0,
            color: Color::rgb(180, 32, 32),
            align: plotx_render::OverlayAlign::Center,
            bold: true,
        }),
    })
}

fn raster_cache_hash(mut hash: [u8; 32], page_index: u32) -> [u8; 32] {
    for (target, byte) in hash.iter_mut().zip(page_index.to_le_bytes()) {
        *target ^= byte;
    }
    hash
}

#[cfg(test)]
#[path = "asset_render_tests.rs"]
mod tests;
