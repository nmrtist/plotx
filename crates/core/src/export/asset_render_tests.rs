use super::*;
use crate::export::{ExportFormat, ExportPageScope, ExportSettings, export_canvases_with_assets};
use crate::state::{CanvasObject, ObjectFrame, ObjectId, RasterImageContent, TextBox};
use image::{DynamicImage, ImageFormat, Rgba, RgbaImage};
use std::io::Cursor;

fn image_asset() -> AssetRecord {
    let image = RgbaImage::from_pixel(4, 3, Rgba([210, 20, 30, 255]));
    let mut bytes = Cursor::new(Vec::new());
    DynamicImage::ImageRgba8(image)
        .write_to(&mut bytes, ImageFormat::Png)
        .unwrap();
    let bytes = bytes.into_inner();
    let id = AssetId::new();
    AssetRecord {
        id,
        sha256: Sha256::digest(&bytes).into(),
        format: "png".to_owned(),
        pixel_size: [4, 3],
        bytes,
    }
}

fn image_canvas(asset: AssetId) -> CanvasDocument {
    let mut canvas = CanvasDocument::new("image page".to_owned(), [25.4, 25.4]);
    canvas.objects.push(CanvasObject {
        id: ObjectId::new(1),
        name: "source image".to_owned(),
        frame: ObjectFrame::new(0.0, 0.0, 72.0, 72.0),
        locked: false,
        visible: true,
        kind: CanvasObjectKind::RasterImage(RasterImageContent::new(asset)),
    });
    canvas.objects.push(CanvasObject {
        id: ObjectId::new(2),
        name: "front label".to_owned(),
        frame: ObjectFrame::new(2.0, 2.0, 40.0, 12.0),
        locked: false,
        visible: true,
        kind: CanvasObjectKind::Text(TextBox::label("FRONT_LAYER".to_owned())),
    });
    canvas
}

#[test]
fn repeated_image_items_share_one_original_decode() {
    let asset = image_asset();
    let mut canvas = image_canvas(asset.id);
    for id in 3..=258 {
        let mut repeated = canvas.objects[0].clone();
        repeated.id = ObjectId::new(id);
        canvas.objects.push(repeated);
    }
    let assets = BTreeMap::from([(asset.id, asset)]);

    let document = prepare_render_document(&canvas, &assets, MissingImagePolicy::Block).unwrap();
    let rasters: Vec<_> = document
        .items
        .iter()
        .filter_map(|item| match item {
            plotx_render::DocumentItem::Raster(raster) => Some(raster),
            _ => None,
        })
        .collect();
    assert_eq!(rasters.len(), 257);
    assert!(
        rasters
            .iter()
            .all(|raster| Arc::ptr_eq(&rasters[0].pixels, &raster.pixels))
    );
    assert_eq!(rasters[0].pixel_size, [4, 3]);
}

#[test]
fn svg_pdf_and_bitmap_exports_include_original_image_and_z_order() {
    let asset = image_asset();
    let canvas = image_canvas(asset.id);
    let assets = BTreeMap::from([(asset.id, asset)]);
    let dir = std::env::temp_dir().join(format!("plotx-image-export-{}", AssetId::new()));
    std::fs::create_dir_all(&dir).unwrap();

    for format in [
        ExportFormat::Svg,
        ExportFormat::Pdf,
        ExportFormat::Png,
        ExportFormat::Jpeg,
        ExportFormat::Tiff,
    ] {
        let base = dir.join(format!("figure.{}", format.extension()));
        let paths = export_canvases_with_assets(
            std::slice::from_ref(&canvas),
            &assets,
            Some(0),
            &ExportSettings {
                format,
                scope: ExportPageScope::Current,
                dpi: 72,
                target_width_mm: None,
                trim_to_visible_content: false,
                allow_missing_images: false,
            },
            &base,
        )
        .unwrap();
        let bytes = std::fs::read(&paths[0]).unwrap();
        match format {
            ExportFormat::Svg => {
                let svg = String::from_utf8(bytes).unwrap();
                let image = svg.find("<image ").expect("embedded SVG image");
                let label = svg.find("FRONT_LAYER").expect("front label");
                assert!(svg.contains("data:image/png;base64,"));
                assert!(image < label, "document order must remain the z order");
            }
            ExportFormat::Pdf => {
                assert!(bytes.starts_with(b"%PDF-"));
                assert!(bytes.windows(15).any(|window| window == b"/Subtype /Image"));
            }
            ExportFormat::Png | ExportFormat::Jpeg | ExportFormat::Tiff => {
                let decoded = image::load_from_memory(&bytes).unwrap();
                assert_eq!(decoded.width(), 72);
                assert_eq!(decoded.height(), 72);
                let center = decoded.to_rgb8().get_pixel(36, 36).0;
                assert!(center[0] > center[1] + 100);
            }
        }
        std::fs::remove_file(&paths[0]).unwrap();
    }
    std::fs::remove_dir(&dir).unwrap();
}

#[test]
fn unavailable_images_block_by_default_and_can_export_as_placeholders() {
    let asset = image_asset();
    let canvas = image_canvas(asset.id);
    let missing = prepare_render_document(&canvas, &BTreeMap::new(), MissingImagePolicy::Block)
        .err()
        .expect("missing asset must block export");
    assert!(matches!(missing, ExportError::MissingImageAsset { .. }));

    let placeholder =
        prepare_render_document(&canvas, &BTreeMap::new(), MissingImagePolicy::Placeholder)
            .unwrap();
    assert!(matches!(
        &placeholder.items[0],
        plotx_render::DocumentItem::Overlay(overlay)
            if matches!(&overlay.kind, plotx_render::OverlayKind::Text(text) if text.text.contains("Missing image"))
    ));

    let mut damaged = asset;
    damaged.bytes.push(0);
    let assets = BTreeMap::from([(damaged.id, damaged)]);
    let error = prepare_render_document(&canvas, &assets, MissingImagePolicy::Block)
        .err()
        .expect("damaged asset must block export");
    assert!(matches!(error, ExportError::CorruptImageAsset { .. }));
    assert!(prepare_render_document(&canvas, &assets, MissingImagePolicy::Placeholder).is_ok());
}
