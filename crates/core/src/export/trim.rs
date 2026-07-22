use super::ExportError;
use super::raster::{RasterError, RasterImage};
use crate::state::{CanvasDocument, render_document_svg_for_bounds, render_document_svg_page};
use plotx_render::Rect;

const TRIM_SAFETY_EDGE_PT: f32 = 1.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct PixelBounds {
    pub left: u32,
    pub top: u32,
    pub right: u32,
    pub bottom: u32,
}

pub(crate) fn raster_visible_bounds(
    image: &RasterImage,
    background: [u8; 4],
) -> Result<Option<PixelBounds>, RasterError> {
    let width = usize::try_from(image.width()).map_err(|_| RasterError::PixelDimensionsOverflow)?;
    let height =
        usize::try_from(image.height()).map_err(|_| RasterError::PixelDimensionsOverflow)?;
    let expected = width
        .checked_mul(height)
        .and_then(|pixels| pixels.checked_mul(4))
        .ok_or(RasterError::PixelDimensionsOverflow)?;
    if image.rgba().len() != expected {
        return Err(RasterError::InvalidBufferLength {
            width: image.width(),
            height: image.height(),
            expected,
            actual: image.rgba().len(),
        });
    }
    let mut bounds: Option<PixelBounds> = None;
    for (index, pixel) in image.rgba().chunks_exact(4).enumerate() {
        if pixel == background {
            continue;
        }
        let x = u32::try_from(index % width).map_err(|_| RasterError::PixelDimensionsOverflow)?;
        let y = u32::try_from(index / width).map_err(|_| RasterError::PixelDimensionsOverflow)?;
        bounds = Some(match bounds {
            Some(old) => PixelBounds {
                left: old.left.min(x),
                top: old.top.min(y),
                right: old.right.max(x),
                bottom: old.bottom.max(y),
            },
            None => PixelBounds {
                left: x,
                top: y,
                right: x,
                bottom: y,
            },
        });
    }
    Ok(bounds)
}

pub(crate) fn crop_raster(
    image: RasterImage,
    background: [u8; 4],
    padding_px: u32,
) -> Result<RasterImage, RasterError> {
    let Some(mut bounds) = raster_visible_bounds(&image, background)? else {
        return Ok(image);
    };
    bounds.left = bounds.left.saturating_sub(padding_px);
    bounds.top = bounds.top.saturating_sub(padding_px);
    bounds.right = bounds
        .right
        .saturating_add(padding_px)
        .min(image.width().saturating_sub(1));
    bounds.bottom = bounds
        .bottom
        .saturating_add(padding_px)
        .min(image.height().saturating_sub(1));

    let Some(width) = bounds
        .right
        .checked_sub(bounds.left)
        .and_then(|v| v.checked_add(1))
    else {
        return Err(RasterError::PixelDimensionsOverflow);
    };
    let Some(height) = bounds
        .bottom
        .checked_sub(bounds.top)
        .and_then(|v| v.checked_add(1))
    else {
        return Err(RasterError::PixelDimensionsOverflow);
    };
    let Some(row_bytes) = usize::try_from(width).ok().and_then(|v| v.checked_mul(4)) else {
        return Err(RasterError::PixelDimensionsOverflow);
    };
    let Some(capacity) = row_bytes.checked_mul(usize::try_from(height).unwrap_or(usize::MAX))
    else {
        return Err(RasterError::PixelDimensionsOverflow);
    };
    let Ok(source_width) = usize::try_from(image.width()) else {
        return Err(RasterError::PixelDimensionsOverflow);
    };
    let mut rgba = Vec::new();
    if rgba.try_reserve_exact(capacity).is_err() {
        return Err(RasterError::OutputAllocation {
            bytes: u64::try_from(capacity).unwrap_or(u64::MAX),
        });
    }
    for y in bounds.top..=bounds.bottom {
        let Some(start) = usize::try_from(y)
            .ok()
            .and_then(|y| y.checked_mul(source_width))
            .and_then(|v| v.checked_add(bounds.left as usize))
            .and_then(|v| v.checked_mul(4))
        else {
            return Err(RasterError::PixelDimensionsOverflow);
        };
        let Some(end) = start.checked_add(row_bytes) else {
            return Err(RasterError::PixelDimensionsOverflow);
        };
        let Some(row) = image.rgba().get(start..end) else {
            return Err(RasterError::InvalidBufferLength {
                width: image.width(),
                height: image.height(),
                expected: source_width
                    .checked_mul(image.height() as usize)
                    .and_then(|pixels| pixels.checked_mul(4))
                    .unwrap_or(usize::MAX),
                actual: image.rgba().len(),
            });
        };
        rgba.extend_from_slice(row);
    }
    RasterImage::from_rgba(width, height, rgba)
}

pub(crate) fn raster_trim_padding(dpi: u16) -> u32 {
    // Match the vector export's one-point physical safety edge. Rounding up
    // keeps the edge from becoming smaller than one point at any output DPI.
    u32::from(dpi).div_ceil(72).max(1)
}

pub(crate) fn trim_document_svg(
    canvas: &CanvasDocument,
    target_width_mm: Option<f32>,
) -> Result<String, ExportError> {
    let [page_width, page_height] = canvas.size_pt();
    let scale = target_width_mm
        .map(|target| target / canvas.size_mm[0].max(f32::MIN_POSITIVE))
        .unwrap_or(1.0);
    let bounds_svg = render_document_svg_for_bounds(canvas);
    let Some(bounds) = svg_content_bounds(&bounds_svg, [page_width, page_height])? else {
        return Ok(super::document_svg(canvas, target_width_mm));
    };
    let left = bounds.left;
    let top = bounds.top;
    let right = bounds.left + bounds.width;
    let bottom = bounds.top + bounds.height;

    // Padding is expressed in authored coordinates so it becomes exactly 1 pt
    // after the preset's physical scale has been applied.
    let padding = TRIM_SAFETY_EDGE_PT / scale.max(f32::MIN_POSITIVE);
    let view = Rect::new(
        (left - padding).max(0.0),
        (top - padding).max(0.0),
        (right + padding).min(page_width) - (left - padding).max(0.0),
        (bottom + padding).min(page_height) - (top - padding).max(0.0),
    );
    Ok(render_document_svg_page(
        canvas,
        view,
        [view.width * scale, view.height * scale],
    ))
}

fn svg_content_bounds(svg: &str, page: [f32; 2]) -> Result<Option<Rect>, ExportError> {
    let mut options = resvg::usvg::Options::default();
    options.fontdb_mut().load_system_fonts();
    let tree = resvg::usvg::Tree::from_str(svg, &options)
        .map_err(|error| ExportError::SvgParse(error.to_string()))?;
    if tree.root().children().is_empty() {
        return Ok(None);
    }
    let Some(bounds) = visible_group_bounds(tree.root()) else {
        return Ok(None);
    };
    // usvg reports absolute bounds in its CSS-pixel viewport. PlotX document
    // SVGs declare their physical size in points, so the viewport is 96/72
    // larger than the authored page coordinate system. Normalize through the
    // parsed tree size rather than baking in that ratio, which also keeps this
    // helper correct for unitless SVG fixtures and future physical units.
    let tree_size = tree.size();
    let scale_x = page[0] / tree_size.width();
    let scale_y = page[1] / tree_size.height();
    if !scale_x.is_finite() || !scale_y.is_finite() || scale_x <= 0.0 || scale_y <= 0.0 {
        return Ok(None);
    }
    let bounds = bounds.scaled(scale_x, scale_y);
    let left = bounds.left.max(0.0);
    let top = bounds.top.max(0.0);
    let right = bounds.right.min(page[0]);
    let bottom = bounds.bottom.min(page[1]);
    if !left.is_finite() || !top.is_finite() || right <= left || bottom <= top {
        return Ok(None);
    }
    Ok(Some(Rect::new(left, top, right - left, bottom - top)))
}

#[derive(Clone, Copy)]
struct SvgBounds {
    left: f32,
    top: f32,
    right: f32,
    bottom: f32,
}

impl SvgBounds {
    fn from_rect(rect: resvg::tiny_skia::Rect) -> Self {
        Self {
            left: rect.left(),
            top: rect.top(),
            right: rect.right(),
            bottom: rect.bottom(),
        }
    }

    fn union(self, other: Self) -> Self {
        Self {
            left: self.left.min(other.left),
            top: self.top.min(other.top),
            right: self.right.max(other.right),
            bottom: self.bottom.max(other.bottom),
        }
    }

    fn scaled(self, x: f32, y: f32) -> Self {
        Self {
            left: self.left * x,
            top: self.top * y,
            right: self.right * x,
            bottom: self.bottom * y,
        }
    }

    fn intersect(self, other: Self) -> Option<Self> {
        let bounds = Self {
            left: self.left.max(other.left),
            top: self.top.max(other.top),
            right: self.right.min(other.right),
            bottom: self.bottom.min(other.bottom),
        };
        (bounds.right > bounds.left && bounds.bottom > bounds.top).then_some(bounds)
    }
}

fn visible_group_bounds(group: &resvg::usvg::Group) -> Option<SvgBounds> {
    let clip_bounds = match group.clip_path() {
        Some(clip) => Some(clip_path_bounds(clip)?),
        None => None,
    };
    let mask_bounds = match group.mask() {
        Some(mask) => Some(mask_bounds(mask, group.abs_transform())?),
        None => None,
    };
    let mut bounds = group
        .children()
        .iter()
        .filter_map(visible_node_bounds)
        .filter_map(|bounds| match clip_bounds {
            Some(clip) => bounds.intersect(clip),
            None => Some(bounds),
        })
        .filter_map(|bounds| match mask_bounds {
            Some(mask) => bounds.intersect(mask),
            None => Some(bounds),
        })
        .reduce(SvgBounds::union)?;

    // usvg's layer bounds include filter regions but deliberately do not apply
    // clip paths. Preserve filter expansion, then explicitly constrain the
    // stroke-aware aggregate to the painted clip geometry.
    if !group.filters().is_empty() {
        bounds = SvgBounds::from_rect(group.abs_layer_bounding_box().to_rect());
        if let Some(clip) = clip_bounds {
            bounds = bounds.intersect(clip)?;
        }
        if let Some(mask) = mask_bounds {
            bounds = bounds.intersect(mask)?;
        }
    }
    Some(bounds)
}

fn visible_node_bounds(node: &resvg::usvg::Node) -> Option<SvgBounds> {
    match node {
        resvg::usvg::Node::Group(group) => visible_group_bounds(group),
        _ => Some(SvgBounds::from_rect(node.abs_stroke_bounding_box())),
    }
}

fn clip_path_bounds(clip: &resvg::usvg::ClipPath) -> Option<SvgBounds> {
    if clip.root().children().is_empty() {
        return None;
    }
    let mut bounds = SvgBounds::from_rect(clip.root().abs_bounding_box());
    if let Some(parent) = clip.clip_path() {
        bounds = bounds.intersect(clip_path_bounds(parent)?)?;
    }
    Some(bounds)
}

fn mask_bounds(
    mask: &resvg::usvg::Mask,
    transform: resvg::tiny_skia::Transform,
) -> Option<SvgBounds> {
    let region = mask.rect().to_rect().transform(transform)?;
    let content = visible_group_bounds(mask.root())?;
    let mut bounds = SvgBounds::from_rect(region).intersect(content)?;
    if let Some(parent) = mask.mask() {
        bounds = bounds.intersect(mask_bounds(parent, transform)?)?;
    }
    Some(bounds)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn image(width: u32, height: u32, pixels: Vec<u8>) -> RasterImage {
        RasterImage::from_rgba(width, height, pixels).unwrap()
    }

    #[test]
    fn raster_crop_adds_one_pixel_and_clamps_at_edges() {
        let mut pixels = vec![255; 10 * 8 * 4];
        pixels[(3 * 10 + 4) * 4..(3 * 10 + 4) * 4 + 4].copy_from_slice(&[0, 0, 0, 255]);
        let cropped = crop_raster(image(10, 8, pixels), [255; 4], 1).unwrap();
        assert_eq!((cropped.width(), cropped.height()), (3, 3));

        let mut pixels = vec![255; 4 * 4 * 4];
        pixels[..4].copy_from_slice(&[0, 0, 0, 255]);
        let cropped = crop_raster(image(4, 4, pixels), [255; 4], 1).unwrap();
        assert_eq!((cropped.width(), cropped.height()), (2, 2));
    }

    #[test]
    fn empty_raster_keeps_its_page() {
        let original = image(7, 5, vec![255; 7 * 5 * 4]);
        assert_eq!(
            crop_raster(original.clone(), [255; 4], 1).unwrap(),
            original
        );
    }

    #[test]
    fn raster_visibility_is_exact_final_pixel_color_and_padding_is_one_pixel() {
        let mut pixels = vec![255; 9 * 9 * 4];
        // A white-on-white authored mark is visually absent in the final raster.
        pixels[(2 * 9 + 2) * 4..(2 * 9 + 2) * 4 + 4].copy_from_slice(&[255; 4]);
        pixels[(4 * 9 + 4) * 4..(4 * 9 + 4) * 4 + 4].copy_from_slice(&[254, 255, 255, 255]);
        let cropped = crop_raster(image(9, 9, pixels), [255; 4], 1).unwrap();
        assert_eq!((cropped.width(), cropped.height()), (3, 3));
    }

    #[test]
    fn invalid_raster_buffer_is_reported() {
        let invalid = RasterImage::from_invalid_buffer(2, 2, vec![255; 15]);
        assert!(matches!(
            crop_raster(invalid, [255; 4], 1),
            Err(RasterError::InvalidBufferLength { .. })
        ));
    }

    #[test]
    fn raster_safety_edge_is_one_physical_point_rounded_up() {
        assert_eq!(raster_trim_padding(72), 1);
        assert_eq!(raster_trim_padding(300), 5);
        assert_eq!(raster_trim_padding(600), 9);
        assert_eq!(raster_trim_padding(1_200), 17);
    }

    #[test]
    fn raster_crop_applies_requested_padding() {
        let mut pixels = vec![255; 20 * 20 * 4];
        pixels[(10 * 20 + 10) * 4..(10 * 20 + 10) * 4 + 4].copy_from_slice(&[0, 0, 0, 255]);
        let cropped = crop_raster(image(20, 20, pixels), [255; 4], 5).unwrap();
        assert_eq!((cropped.width(), cropped.height()), (11, 11));
    }

    #[test]
    fn svg_layer_bounds_intersect_clipped_geometry_and_keep_visible_stroke() {
        let svg = r##"<svg xmlns="http://www.w3.org/2000/svg" width="200" height="120" viewBox="0 0 200 120">
          <defs><clipPath id="plot"><rect x="50" y="30" width="80" height="40"/></clipPath></defs>
          <g clip-path="url(#plot)">
            <path d="M-1000 50 H1000" fill="none" stroke="#000" stroke-width="10"/>
            <rect x="500" y="500" width="20" height="20" fill="#f00"/>
          </g>
        </svg>"##;
        let bounds = svg_content_bounds(svg, [200.0, 120.0]).unwrap().unwrap();
        assert!((bounds.left - 50.0).abs() < 0.01, "{bounds:?}");
        assert!((bounds.width - 80.0).abs() < 0.01);
        assert!((bounds.top - 45.0).abs() < 0.01);
        assert!((bounds.height - 10.0).abs() < 0.01, "{bounds:?}");
    }

    #[test]
    fn fully_clipped_svg_element_does_not_expand_visible_bounds() {
        let svg = r##"<svg xmlns="http://www.w3.org/2000/svg" width="200" height="120" viewBox="0 0 200 120">
          <defs><clipPath id="plot"><rect x="50" y="30" width="80" height="40"/></clipPath></defs>
          <circle cx="20" cy="20" r="4" fill="#000"/>
          <g clip-path="url(#plot)"><rect x="500" y="500" width="20" height="20" fill="#f00"/></g>
        </svg>"##;
        let bounds = svg_content_bounds(svg, [200.0, 120.0]).unwrap().unwrap();
        assert!((bounds.left - 16.0).abs() < 0.01);
        assert!((bounds.top - 16.0).abs() < 0.01);
        assert!((bounds.width - 8.0).abs() < 0.01, "{bounds:?}");
        assert!((bounds.height - 8.0).abs() < 0.01);
    }

    #[test]
    fn text_bounds_stay_inside_the_authored_page_coordinate_system() {
        let svg = r##"<svg xmlns="http://www.w3.org/2000/svg" width="200pt" height="120pt" viewBox="0 0 200 120" font-family="sans-serif">
          <text x="40" y="50" font-size="12">Axis label</text>
        </svg>"##;
        let bounds = svg_content_bounds(svg, [200.0, 120.0])
            .unwrap()
            .expect("text should have painted bounds");
        assert!(
            bounds.left >= 35.0
                && bounds.left <= 45.0
                && bounds.top >= 30.0
                && bounds.top <= 50.0
                && bounds.left + bounds.width < 150.0
                && bounds.top + bounds.height < 70.0,
            "unexpected text bounds: {bounds:?}"
        );
    }
}
