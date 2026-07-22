use crate::state::{CanvasDocument, render_document_svg};
use resvg::tiny_skia::{Pixmap, Transform};
use thiserror::Error;

/// Conservative defaults keep one RGBA result below 256 MiB and prevent
/// pathological dimensions even when the total pixel count is still small.
pub const DEFAULT_MAX_RASTER_WIDTH: u32 = 32_768;
pub const DEFAULT_MAX_RASTER_HEIGHT: u32 = 32_768;
pub const DEFAULT_MAX_RASTER_PIXELS: u64 = 67_108_864;
pub const DEFAULT_MAX_RASTER_BYTES: u64 = 268_435_456;

/// Resource limits applied before parsing or allocating the raster image.
///
/// `max_bytes` is the size of the straight-alpha RGBA8 result. The renderer
/// also holds a premultiplied RGBA8 working buffer of the same size while the
/// result is produced.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RasterLimits {
    pub max_width: u32,
    pub max_height: u32,
    pub max_pixels: u64,
    pub max_bytes: u64,
}

impl Default for RasterLimits {
    fn default() -> Self {
        Self {
            max_width: DEFAULT_MAX_RASTER_WIDTH,
            max_height: DEFAULT_MAX_RASTER_HEIGHT,
            max_pixels: DEFAULT_MAX_RASTER_PIXELS,
            max_bytes: DEFAULT_MAX_RASTER_BYTES,
        }
    }
}

/// Controls conversion from a physically sized SVG page to pixels.
///
/// The source SVG size is expressed in PostScript points (72 points per inch).
/// At natural size, each axis is `points / 72 * dpi` pixels. If
/// `target_width_mm` is set, both axes are uniformly scaled so the rendered
/// page has that physical width at the requested DPI.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RasterOptions {
    pub dpi: u16,
    pub target_width_mm: Option<f32>,
    pub limits: RasterLimits,
}

impl RasterOptions {
    pub fn new(dpi: u16) -> Self {
        Self {
            dpi,
            target_width_mm: None,
            limits: RasterLimits::default(),
        }
    }
}

/// A tightly packed, row-major RGBA8 image using straight (unpremultiplied)
/// alpha. The byte length is always `width * height * 4`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RasterImage {
    width: u32,
    height: u32,
    rgba: Vec<u8>,
}

impl RasterImage {
    pub(crate) fn from_rgba(width: u32, height: u32, rgba: Vec<u8>) -> Result<Self, RasterError> {
        let expected = usize::try_from(width)
            .ok()
            .and_then(|width| {
                usize::try_from(height)
                    .ok()
                    .and_then(|height| width.checked_mul(height))
            })
            .and_then(|pixels| pixels.checked_mul(4))
            .ok_or(RasterError::PixelDimensionsOverflow)?;
        if rgba.len() != expected {
            return Err(RasterError::InvalidBufferLength {
                width,
                height,
                expected,
                actual: rgba.len(),
            });
        }
        Ok(Self {
            width,
            height,
            rgba,
        })
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    #[cfg(test)]
    pub(crate) fn from_invalid_buffer(width: u32, height: u32, rgba: Vec<u8>) -> Self {
        Self {
            width,
            height,
            rgba,
        }
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    pub fn rgba(&self) -> &[u8] {
        &self.rgba
    }

    pub fn into_rgba(self) -> Vec<u8> {
        self.rgba
    }

    pub fn to_png(&self) -> Result<Vec<u8>, image::ImageError> {
        use image::ImageEncoder as _;
        let mut out = Vec::new();
        image::codecs::png::PngEncoder::new(&mut out).write_image(
            &self.rgba,
            self.width,
            self.height,
            image::ExtendedColorType::Rgba8,
        )?;
        Ok(out)
    }

    /// Discards alpha by compositing onto an opaque RGB background.
    pub fn to_rgb_over(&self, background: [u8; 3]) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.rgba.len() / 4 * 3);
        for pixel in self.rgba.chunks_exact(4) {
            let alpha = pixel[3] as u16;
            let inverse = 255 - alpha;
            for channel in 0..3 {
                out.push(
                    ((pixel[channel] as u16 * alpha + background[channel] as u16 * inverse + 127)
                        / 255) as u8,
                );
            }
        }
        out
    }
}

#[derive(Debug, Error)]
pub enum RasterError {
    #[error("raster DPI must be greater than zero")]
    InvalidDpi,
    #[error("source size must be finite and positive, got {width_pt}x{height_pt} pt")]
    InvalidSourceSize { width_pt: f32, height_pt: f32 },
    #[error("target width must be finite and positive, got {width_mm} mm")]
    InvalidTargetWidth { width_mm: f32 },
    #[error("calculated pixel dimensions are not representable")]
    PixelDimensionsOverflow,
    #[error(
        "raster dimensions {width}x{height} exceed the configured limit {max_width}x{max_height}"
    )]
    DimensionLimitExceeded {
        width: u32,
        height: u32,
        max_width: u32,
        max_height: u32,
    },
    #[error("raster has {pixels} pixels, exceeding the configured limit of {max_pixels}")]
    PixelLimitExceeded { pixels: u64, max_pixels: u64 },
    #[error("RGBA buffer requires {bytes} bytes, exceeding the configured limit of {max_bytes}")]
    BufferLimitExceeded { bytes: u64, max_bytes: u64 },
    #[error("SVG parse failed: {0}")]
    SvgParse(String),
    #[error("could not allocate renderer bitmap {width}x{height}")]
    PixmapAllocation { width: u32, height: u32 },
    #[error("could not allocate {bytes} bytes for the RGBA result")]
    OutputAllocation { bytes: u64 },
    #[error(
        "RGBA buffer length {actual} does not match {width}x{height} image (expected {expected})"
    )]
    InvalidBufferLength {
        width: u32,
        height: u32,
        expected: usize,
        actual: usize,
    },
}

/// Render a canvas into memory without performing filesystem I/O.
pub fn rasterize_canvas(
    canvas: &CanvasDocument,
    options: RasterOptions,
) -> Result<RasterImage, RasterError> {
    let svg = render_document_svg(canvas);
    rasterize_svg(&svg, canvas.size_pt(), options)
}

/// Render a physically sized SVG into a straight-alpha RGBA8 image.
///
/// `source_size_pt` must describe the SVG view box in points. The function
/// applies resource limits before SVG parsing and allocation.
pub fn rasterize_svg(
    svg: &str,
    source_size_pt: [f32; 2],
    options: RasterOptions,
) -> Result<RasterImage, RasterError> {
    let [width_pt, height_pt] = source_size_pt;
    if options.dpi == 0 {
        return Err(RasterError::InvalidDpi);
    }
    if !width_pt.is_finite() || !height_pt.is_finite() || width_pt <= 0.0 || height_pt <= 0.0 {
        return Err(RasterError::InvalidSourceSize {
            width_pt,
            height_pt,
        });
    }

    let size_scale = match options.target_width_mm {
        Some(width_mm) if !width_mm.is_finite() || width_mm <= 0.0 => {
            return Err(RasterError::InvalidTargetWidth { width_mm });
        }
        Some(width_mm) => width_mm as f64 / points_to_mm(width_pt) as f64,
        None => 1.0,
    };
    let width = pixels_from_points(width_pt, options.dpi, size_scale)?;
    let height = pixels_from_points(height_pt, options.dpi, size_scale)?;
    enforce_limits(width, height, options.limits)?;

    let mut usvg_options = resvg::usvg::Options::default();
    usvg_options.fontdb_mut().load_system_fonts();
    let tree = resvg::usvg::Tree::from_str(svg, &usvg_options)
        .map_err(|error| RasterError::SvgParse(error.to_string()))?;
    let mut pixmap =
        Pixmap::new(width, height).ok_or(RasterError::PixmapAllocation { width, height })?;
    let scale_x = width as f32 / tree.size().width().max(f32::MIN_POSITIVE);
    let scale_y = height as f32 / tree.size().height().max(f32::MIN_POSITIVE);
    resvg::render(
        &tree,
        Transform::from_scale(scale_x, scale_y),
        &mut pixmap.as_mut(),
    );

    let bytes = u64::from(width) * u64::from(height) * 4;
    let mut rgba = Vec::new();
    rgba.try_reserve_exact(bytes as usize)
        .map_err(|_| RasterError::OutputAllocation { bytes })?;
    for pixel in pixmap.pixels() {
        let color = pixel.demultiply();
        rgba.extend_from_slice(&[color.red(), color.green(), color.blue(), color.alpha()]);
    }

    Ok(RasterImage {
        width,
        height,
        rgba,
    })
}

fn points_to_mm(points: f32) -> f32 {
    points * 25.4 / 72.0
}

fn pixels_from_points(points: f32, dpi: u16, size_scale: f64) -> Result<u32, RasterError> {
    let pixels = (points as f64 / 72.0 * f64::from(dpi) * size_scale).round();
    if !pixels.is_finite() || pixels > u32::MAX as f64 {
        return Err(RasterError::PixelDimensionsOverflow);
    }
    Ok(pixels.max(1.0) as u32)
}

fn enforce_limits(width: u32, height: u32, limits: RasterLimits) -> Result<(), RasterError> {
    if width > limits.max_width || height > limits.max_height {
        return Err(RasterError::DimensionLimitExceeded {
            width,
            height,
            max_width: limits.max_width,
            max_height: limits.max_height,
        });
    }
    let pixels = u64::from(width) * u64::from(height);
    if pixels > limits.max_pixels {
        return Err(RasterError::PixelLimitExceeded {
            pixels,
            max_pixels: limits.max_pixels,
        });
    }
    let bytes = pixels * 4;
    if bytes > limits.max_bytes {
        return Err(RasterError::BufferLimitExceeded {
            bytes,
            max_bytes: limits.max_bytes,
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const EMPTY_SVG: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" width="72pt" height="36pt" viewBox="0 0 72 36"/>"#;

    #[test]
    fn target_width_and_dpi_define_pixel_dimensions() {
        let image = rasterize_svg(
            EMPTY_SVG,
            [72.0, 36.0],
            RasterOptions {
                dpi: 100,
                target_width_mm: Some(50.8),
                limits: RasterLimits::default(),
            },
        )
        .unwrap();
        assert_eq!((image.width(), image.height()), (200, 100));
        assert_eq!(image.rgba().len(), 200 * 100 * 4);
    }

    #[test]
    fn rgba_result_uses_straight_alpha() {
        let svg = r##"<svg xmlns="http://www.w3.org/2000/svg" width="1pt" height="1pt" viewBox="0 0 1 1"><rect width="1" height="1" fill="#ff0000" fill-opacity="0.5"/></svg>"##;
        let image = rasterize_svg(svg, [1.0, 1.0], RasterOptions::new(72)).unwrap();
        assert_eq!(image.width(), 1);
        assert_eq!(image.height(), 1);
        assert_eq!(&image.rgba()[..3], &[255, 0, 0]);
        assert!((127..=128).contains(&image.rgba()[3]));
    }

    #[test]
    fn pt_sized_svg_content_fills_the_whole_pixmap() {
        let svg = r##"<svg xmlns="http://www.w3.org/2000/svg" width="100pt" height="50pt" viewBox="0 0 100 50"><rect x="0" y="0" width="100" height="50" fill="#00ff00"/></svg>"##;
        let image = rasterize_svg(svg, [100.0, 50.0], RasterOptions::new(300)).unwrap();
        let (w, h) = (image.width() as usize, image.height() as usize);
        assert_eq!((w, h), (417, 208));
        let px = |x: usize, y: usize| &image.rgba()[(y * w + x) * 4..(y * w + x) * 4 + 4];
        assert_eq!(px(0, 0), &[0, 255, 0, 255]);
        assert_eq!(px(w - 1, 0), &[0, 255, 0, 255]);
        assert_eq!(px(0, h - 1), &[0, 255, 0, 255]);
        assert_eq!(px(w - 1, h - 1), &[0, 255, 0, 255]);
    }

    #[test]
    fn alpha_can_be_composited_onto_an_explicit_background() {
        let image = RasterImage {
            width: 1,
            height: 1,
            rgba: vec![255, 0, 0, 128],
        };
        assert_eq!(image.to_rgb_over([255, 255, 255]), vec![255, 127, 127]);
    }

    #[test]
    fn resource_limits_are_checked_before_svg_parsing() {
        let error = rasterize_svg(
            "not SVG",
            [72.0, 72.0],
            RasterOptions {
                dpi: 100,
                target_width_mm: None,
                limits: RasterLimits {
                    max_width: 10,
                    ..RasterLimits::default()
                },
            },
        )
        .unwrap_err();
        assert!(matches!(error, RasterError::DimensionLimitExceeded { .. }));
    }

    #[test]
    fn zero_dpi_is_rejected_instead_of_silently_clamped() {
        let error = rasterize_svg(EMPTY_SVG, [72.0, 36.0], RasterOptions::new(0)).unwrap_err();
        assert!(matches!(error, RasterError::InvalidDpi));
    }
}
