use super::{
    Document, DocumentItem, Rect, write_document_object, write_overlay, write_panel_letter,
};
use crate::{DocumentRaster, RasterFit};
use base64::Engine as _;
use image::ImageEncoder as _;
use std::fmt::Write as _;

/// Render a page document to SVG using page points as the geometry space.
pub fn export_document(document: &Document<'_>) -> String {
    export_document_with_page(
        document,
        None,
        [document.width, document.height],
        true,
        false,
    )
}

/// Render the document without visually redundant backgrounds for painted-bounds analysis.
pub fn export_document_for_bounds(document: &Document<'_>) -> String {
    export_document_with_page(
        document,
        None,
        [document.width, document.height],
        false,
        true,
    )
}

/// Render the complete document against a cropped page without moving its geometry.
pub fn export_document_page(
    document: &Document<'_>,
    view_box: Rect,
    physical_size: [f32; 2],
) -> String {
    export_document_with_page(document, Some(view_box), physical_size, true, false)
}

fn export_document_with_page(
    document: &Document<'_>,
    view_box: Option<Rect>,
    physical_size: [f32; 2],
    include_page_background: bool,
    omit_redundant_figure_background: bool,
) -> String {
    let w = document.width;
    let h = document.height;
    let page = view_box.unwrap_or_else(|| Rect::new(0.0, 0.0, w, h));
    let [physical_width, physical_height] = physical_size;
    let mut s = String::new();
    let _ = write!(
        s,
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{physical_width}pt" height="{physical_height}pt" viewBox="{x} {y} {vw} {vh}" font-family="sans-serif">"#,
        x = page.left,
        y = page.top,
        vw = page.width,
        vh = page.height,
    );
    if include_page_background {
        let _ = write!(
            s,
            r#"<rect x="{x}" y="{y}" width="{vw}" height="{vh}" fill="{}"/>"#,
            document.background.to_hex(),
            x = page.left,
            y = page.top,
            vw = page.width,
            vh = page.height,
        );
    }
    for (item_index, item) in document.items.iter().enumerate() {
        match item {
            DocumentItem::Plot(object) => write_document_object(
                &mut s,
                object,
                omit_redundant_figure_background.then_some(document.background),
            ),
            DocumentItem::Overlay(overlay) => {
                if overlay.visible {
                    write_overlay(&mut s, overlay);
                }
            }
            DocumentItem::Raster(raster) => write_raster(&mut s, raster, item_index),
            DocumentItem::PanelLabel {
                frame,
                text,
                visible,
            } => {
                if *visible {
                    write_panel_letter(
                        &mut s,
                        &text.text,
                        [frame.left + text.position[0], frame.top + text.position[1]],
                        text.font_size,
                    );
                }
            }
        }
    }
    let _ = write!(s, "</svg>");
    s
}

fn write_raster(svg: &mut String, raster: &DocumentRaster, item_index: usize) {
    if !raster.visible || raster.opacity <= 0.0 {
        return;
    }
    let Some(image) = prepared_raster(raster) else {
        return;
    };
    let mut png = Vec::new();
    if image::codecs::png::PngEncoder::new(&mut png)
        .write_image(
            image.as_raw(),
            image.width(),
            image.height(),
            image::ExtendedColorType::Rgba8,
        )
        .is_err()
    {
        return;
    }
    let href = base64::engine::general_purpose::STANDARD.encode(png);
    let Some(clip) = intersect(raster.clip, raster.frame) else {
        return;
    };
    let clip_id = format!("raster-clip-{item_index}");
    let _ = write!(
        svg,
        r#"<defs><clipPath id="{clip_id}"><rect x="{x:.3}" y="{y:.3}" width="{w:.3}" height="{h:.3}"/></clipPath></defs>"#,
        x = clip.left,
        y = clip.top,
        w = clip.width,
        h = clip.height,
    );
    let preserve = match raster.fit {
        RasterFit::Contain => "xMidYMid meet",
        RasterFit::Cover => "xMidYMid slice",
        RasterFit::Stretch => "none",
    };
    let rendering = if raster.nearest { "pixelated" } else { "auto" };
    let _ = write!(
        svg,
        r#"<image x="{x:.3}" y="{y:.3}" width="{w:.3}" height="{h:.3}" preserveAspectRatio="{preserve}" opacity="{opacity:.4}" image-rendering="{rendering}" clip-path="url(#{clip_id})" href="data:image/png;base64,{href}"/>"#,
        x = raster.frame.left,
        y = raster.frame.top,
        w = raster.frame.width,
        h = raster.frame.height,
        opacity = raster.opacity.clamp(0.0, 1.0),
    );
}

pub(crate) fn prepared_raster(raster: &DocumentRaster) -> Option<image::RgbaImage> {
    let width = usize::try_from(raster.pixel_size[0]).ok()?;
    let height = usize::try_from(raster.pixel_size[1]).ok()?;
    let expected = width.checked_mul(height)?.checked_mul(4)?;
    if raster.pixel_size.contains(&0) || raster.pixels.len() != expected {
        return None;
    }
    let source = image::RgbaImage::from_raw(
        raster.pixel_size[0],
        raster.pixel_size[1],
        raster.pixels.as_ref().to_vec(),
    )?;
    let [left, top, right, bottom] = raster.crop;
    if !raster.crop.into_iter().all(f32::is_finite)
        || left < 0.0
        || top < 0.0
        || right > 1.0
        || bottom > 1.0
        || left >= right
        || top >= bottom
    {
        return None;
    }
    let x0 = (left * source.width() as f32).floor() as u32;
    let y0 = (top * source.height() as f32).floor() as u32;
    let x1 = ((right * source.width() as f32).ceil() as u32).min(source.width());
    let y1 = ((bottom * source.height() as f32).ceil() as u32).min(source.height());
    let cropped = image::imageops::crop_imm(
        &source,
        x0,
        y0,
        x1.saturating_sub(x0).max(1),
        y1.saturating_sub(y0).max(1),
    )
    .to_image();
    Some(match raster.quarter_turns % 4 {
        0 => cropped,
        1 => image::imageops::rotate90(&cropped),
        2 => image::imageops::rotate180(&cropped),
        _ => image::imageops::rotate270(&cropped),
    })
}

fn intersect(clip: Option<Rect>, frame: Rect) -> Option<Rect> {
    let Some(clip) = clip else {
        return Some(frame);
    };
    let left = clip.left.max(frame.left);
    let top = clip.top.max(frame.top);
    let right = clip.right().min(frame.right());
    let bottom = clip.bottom().min(frame.bottom());
    (right > left && bottom > top).then(|| Rect::new(left, top, right - left, bottom - top))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{DocumentObject, DocumentText};
    use plotx_figure::{Axis, AxisFrame, Color, Figure};
    use std::sync::Arc;

    #[test]
    fn raster_crop_and_rotation_use_source_pixels() {
        let raster = DocumentRaster {
            source_hash: [7; 32],
            frame: Rect::new(0.0, 0.0, 20.0, 20.0),
            pixels: Arc::from([1, 0, 0, 255, 2, 0, 0, 255, 3, 0, 0, 255, 4, 0, 0, 255]),
            pixel_size: [2, 2],
            source_pixel_size: [2, 2],
            crop: [0.0, 0.0, 0.5, 1.0],
            fit: RasterFit::Cover,
            quarter_turns: 1,
            opacity: 0.5,
            nearest: true,
            clip: None,
            visible: true,
        };

        let prepared = prepared_raster(&raster).unwrap();
        assert_eq!(prepared.dimensions(), (2, 1));
        assert_eq!(prepared.into_raw(), [3, 0, 0, 255, 1, 0, 0, 255]);
        let svg = export_document(&Document {
            width: 20.0,
            height: 20.0,
            background: Color::rgb(255, 255, 255),
            items: vec![DocumentItem::Raster(raster)],
        });
        assert!(svg.contains("preserveAspectRatio=\"xMidYMid slice\""));
        assert!(svg.contains("opacity=\"0.5000\""));
        assert!(svg.contains("image-rendering=\"pixelated\""));
    }

    #[test]
    fn raster_clip_ids_are_unique_when_source_and_frame_match() {
        let raster = |clip| DocumentRaster {
            source_hash: [9; 32],
            frame: Rect::new(0.0, 0.0, 20.0, 20.0),
            pixels: Arc::from([1, 2, 3, 255]),
            pixel_size: [1, 1],
            source_pixel_size: [1, 1],
            crop: [0.0, 0.0, 1.0, 1.0],
            fit: RasterFit::Stretch,
            quarter_turns: 0,
            opacity: 1.0,
            nearest: false,
            clip: Some(clip),
            visible: true,
        };
        let svg = export_document(&Document {
            width: 20.0,
            height: 20.0,
            background: Color::rgb(255, 255, 255),
            items: vec![
                DocumentItem::Raster(raster(Rect::new(0.0, 0.0, 8.0, 20.0))),
                DocumentItem::Raster(raster(Rect::new(12.0, 0.0, 8.0, 20.0))),
            ],
        });

        assert!(svg.contains(
            r#"<clipPath id="raster-clip-0"><rect x="0.000" y="0.000" width="8.000" height="20.000"/>"#
        ));
        assert!(svg.contains(
            r#"<clipPath id="raster-clip-1"><rect x="12.000" y="0.000" width="8.000" height="20.000"/>"#
        ));
        assert_eq!(svg.matches(r#"clip-path="url(#raster-clip-0)""#).count(), 1);
        assert_eq!(svg.matches(r#"clip-path="url(#raster-clip-1)""#).count(), 1);
    }

    #[test]
    fn bounds_document_omits_only_visually_redundant_backgrounds() {
        let page_color = Color::rgb(255, 255, 255);
        let mut matching = Figure::new("", Axis::new("", 0.0, 1.0), Axis::new("", 0.0, 1.0));
        matching.background = page_color;
        matching.axis_frame = AxisFrame::Hidden;
        let matching_doc = Document {
            width: 100.0,
            height: 80.0,
            background: page_color,
            items: vec![DocumentItem::Plot(DocumentObject {
                id: "matching".into(),
                frame: Rect::new(10.0, 10.0, 50.0, 40.0),
                figure: &matching,
                visible: true,
                title: None,
            })],
        };
        assert!(!export_document_for_bounds(&matching_doc).contains("fill=\"#ffffff\""));
        assert!(export_document(&matching_doc).contains("fill=\"#ffffff\""));

        let mut contrasting = matching.clone();
        contrasting.background = Color::rgb(1, 2, 3);
        let contrasting_doc = Document {
            items: vec![DocumentItem::Plot(DocumentObject {
                id: "contrasting".into(),
                frame: Rect::new(10.0, 10.0, 50.0, 40.0),
                figure: &contrasting,
                visible: true,
                title: None,
            })],
            ..matching_doc
        };
        assert!(export_document_for_bounds(&contrasting_doc).contains("fill=\"#010203\""));
    }

    #[test]
    fn panel_label_uses_panel_frame_and_escapes_text() {
        let document = Document {
            width: 100.0,
            height: 80.0,
            background: Color::rgb(255, 255, 255),
            items: vec![DocumentItem::PanelLabel {
                frame: Rect::new(10.0, 20.0, 50.0, 40.0),
                text: DocumentText {
                    text: "a<&".to_owned(),
                    position: [3.0, 4.0],
                    font_size: 9.0,
                },
                visible: true,
            }],
        };

        let svg = export_document(&document);
        assert!(svg.contains(r#"x="13.00" y="33.00""#));
        assert!(svg.contains("a&lt;&amp;"));
        assert!(!svg.contains("a<&"));

        let hidden = Document {
            items: vec![DocumentItem::PanelLabel {
                frame: Rect::new(10.0, 20.0, 50.0, 40.0),
                text: DocumentText {
                    text: "HIDDEN_PANEL_LABEL".to_owned(),
                    position: [3.0, 4.0],
                    font_size: 9.0,
                },
                visible: false,
            }],
            ..document
        };
        assert!(!export_document(&hidden).contains("HIDDEN_PANEL_LABEL"));
    }
}
