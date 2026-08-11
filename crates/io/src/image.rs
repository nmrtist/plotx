//! Header-first probing and bounded decoding for embedded raster assets.

use image::{GenericImageView, ImageDecoder};
use sha2::{Digest, Sha256};
use std::io::Cursor;

mod proxy;
pub use proxy::decode_proxy_rgba8;

pub const SOFT_PIXEL_LIMIT: u64 = 100_000_000;
pub const HARD_PIXEL_LIMIT: u64 = 500_000_000;
pub const SOFT_DECODED_BYTES: u64 = 512 * 1024 * 1024;
pub const HARD_DECODED_BYTES: u64 = 2 * 1024 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RasterFormat {
    Png,
    Jpeg,
    Tiff,
    WebP,
    Bmp,
    Gif,
    Svg,
    Unknown,
}

impl RasterFormat {
    pub fn name(self) -> &'static str {
        match self {
            Self::Png => "PNG",
            Self::Jpeg => "JPEG",
            Self::Tiff => "TIFF",
            Self::WebP => "WebP",
            Self::Bmp => "BMP",
            Self::Gif => "GIF",
            Self::Svg => "SVG",
            Self::Unknown => "unknown",
        }
    }

    pub fn supported(self) -> bool {
        matches!(
            self,
            Self::Png | Self::Jpeg | Self::Tiff | Self::WebP | Self::Bmp
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResourceClass {
    Normal,
    ProxyRequired,
    Rejected,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ImageProbe {
    pub format: RasterFormat,
    pub width: u32,
    pub height: u32,
    pub decoded_bytes: u64,
    pub class: ResourceClass,
    pub animated: bool,
    pub has_icc: bool,
    pub has_exif: bool,
    pub high_precision: bool,
    pub pages: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DecodedImage {
    pub probe: ImageProbe,
    pub rgba8: Vec<u8>,
    pub sha256: [u8; 32],
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProxyImage {
    pub pixel_size: [u32; 2],
    pub rgba8: Vec<u8>,
}

pub fn sha256(bytes: &[u8]) -> [u8; 32] {
    Sha256::digest(bytes).into()
}

/// Reads physical pixel density without decoding pixels. Returns `None` when
/// the source carries no density, uses non-physical units, or is malformed.
pub fn metadata_dpi(bytes: &[u8]) -> Option<[f32; 2]> {
    match sniff(bytes) {
        RasterFormat::Png => png_dpi(bytes),
        RasterFormat::Jpeg => jpeg_dpi(bytes),
        RasterFormat::Bmp => bmp_dpi(bytes),
        _ => None,
    }
}

fn png_dpi(bytes: &[u8]) -> Option<[f32; 2]> {
    let mut offset = 8usize;
    while offset.checked_add(12)? <= bytes.len() {
        let length = u32::from_be_bytes(bytes.get(offset..offset + 4)?.try_into().ok()?) as usize;
        let data = offset.checked_add(8)?;
        let end = data.checked_add(length)?;
        if end.checked_add(4)? > bytes.len() {
            return None;
        }
        if bytes.get(offset + 4..offset + 8)? == b"pHYs" && length == 9 {
            if bytes[end - 1] != 1 {
                return None;
            }
            let x = u32::from_be_bytes(bytes.get(data..data + 4)?.try_into().ok()?);
            let y = u32::from_be_bytes(bytes.get(data + 4..data + 8)?.try_into().ok()?);
            return (x > 0 && y > 0).then_some([x as f32 * 0.0254, y as f32 * 0.0254]);
        }
        offset = end.checked_add(4)?;
    }
    None
}

fn jpeg_dpi(bytes: &[u8]) -> Option<[f32; 2]> {
    let marker = bytes.windows(5).position(|window| window == b"JFIF\0")?;
    let units = *bytes.get(marker + 7)?;
    let x = u16::from_be_bytes(bytes.get(marker + 8..marker + 10)?.try_into().ok()?) as f32;
    let y = u16::from_be_bytes(bytes.get(marker + 10..marker + 12)?.try_into().ok()?) as f32;
    match units {
        1 if x > 0.0 && y > 0.0 => Some([x, y]),
        2 if x > 0.0 && y > 0.0 => Some([x * 2.54, y * 2.54]),
        _ => None,
    }
}

fn bmp_dpi(bytes: &[u8]) -> Option<[f32; 2]> {
    let x = i32::from_le_bytes(bytes.get(38..42)?.try_into().ok()?);
    let y = i32::from_le_bytes(bytes.get(42..46)?.try_into().ok()?);
    (x > 0 && y > 0).then_some([x as f32 * 0.0254, y as f32 * 0.0254])
}

#[derive(Debug, thiserror::Error)]
pub enum ImageError {
    #[error("detected {format}; supported image formats are PNG, JPEG, TIFF, WebP, and BMP")]
    Unsupported { format: &'static str },
    #[error("animated {format} is not supported by this import path")]
    Animated { format: &'static str },
    #[error("image dimensions could not be read: {0}")]
    Probe(String),
    #[error("image is too large ({width} x {height}, estimated {decoded_bytes} decoded bytes)")]
    TooLarge {
        width: u32,
        height: u32,
        decoded_bytes: u64,
    },
    #[error("image decode failed: {0}")]
    Decode(String),
    #[error("decoded image size is inconsistent with its dimensions")]
    InvalidDecodedSize,
    #[error("image page {page} is unavailable or unsupported: {reason}")]
    Page { page: u32, reason: String },
}

pub fn sniff(bytes: &[u8]) -> RasterFormat {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        RasterFormat::Png
    } else if bytes.starts_with(&[0xff, 0xd8, 0xff]) {
        RasterFormat::Jpeg
    } else if bytes.starts_with(b"II*\0") || bytes.starts_with(b"MM\0*") {
        RasterFormat::Tiff
    } else if bytes.len() >= 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        RasterFormat::WebP
    } else if bytes.starts_with(b"BM") {
        RasterFormat::Bmp
    } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        RasterFormat::Gif
    } else if looks_like_svg(bytes) {
        RasterFormat::Svg
    } else {
        RasterFormat::Unknown
    }
}

pub fn probe(bytes: &[u8]) -> Result<ImageProbe, ImageError> {
    let format = sniff(bytes);
    if !format.supported() {
        return Err(ImageError::Unsupported {
            format: format.name(),
        });
    }
    let animated = animation_marker(format, bytes);
    let reader = image::ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|error| ImageError::Probe(error.to_string()))?;
    let mut decoder = reader
        .into_decoder()
        .map_err(|error| ImageError::Probe(error.to_string()))?;
    let (width, height) = decoder.dimensions();
    let color_type = decoder.color_type();
    let has_icc = decoder
        .icc_profile()
        .map_err(|error| ImageError::Probe(error.to_string()))?
        .is_some();
    let has_exif = decoder
        .exif_metadata()
        .map_err(|error| ImageError::Probe(error.to_string()))?
        .is_some();
    let pixels = u64::from(width)
        .checked_mul(u64::from(height))
        .ok_or(ImageError::TooLarge {
            width,
            height,
            decoded_bytes: u64::MAX,
        })?;
    let decoded_bytes = pixels.checked_mul(4).ok_or(ImageError::TooLarge {
        width,
        height,
        decoded_bytes: u64::MAX,
    })?;
    let class = classify(pixels, decoded_bytes);
    Ok(ImageProbe {
        format,
        width,
        height,
        decoded_bytes,
        class,
        animated,
        has_icc,
        has_exif,
        high_precision: matches!(
            color_type,
            image::ColorType::L16
                | image::ColorType::La16
                | image::ColorType::Rgb16
                | image::ColorType::Rgba16
                | image::ColorType::Rgb32F
                | image::ColorType::Rgba32F
        ),
        pages: if format == RasterFormat::Tiff {
            tiff_page_count(bytes).unwrap_or(1)
        } else {
            1
        },
    })
}

pub fn tiff_page_count(bytes: &[u8]) -> Option<u32> {
    if sniff(bytes) != RasterFormat::Tiff {
        return None;
    }
    let mut decoder = tiff::decoder::Decoder::new(Cursor::new(bytes)).ok()?;
    let mut pages = 1_u32;
    while decoder.more_images() {
        decoder.next_image().ok()?;
        pages = pages.checked_add(1)?;
    }
    Some(pages)
}

pub fn tiff_page_dimensions(bytes: &[u8], page_index: u32) -> Option<[u32; 2]> {
    if sniff(bytes) != RasterFormat::Tiff {
        return None;
    }
    let mut decoder = tiff::decoder::Decoder::new(Cursor::new(bytes)).ok()?;
    for _ in 0..page_index {
        decoder.next_image().ok()?;
    }
    let (width, height) = decoder.dimensions().ok()?;
    Some([width, height])
}

fn classify(pixels: u64, decoded_bytes: u64) -> ResourceClass {
    if pixels > HARD_PIXEL_LIMIT || decoded_bytes > HARD_DECODED_BYTES {
        ResourceClass::Rejected
    } else if pixels > SOFT_PIXEL_LIMIT || decoded_bytes > SOFT_DECODED_BYTES {
        ResourceClass::ProxyRequired
    } else {
        ResourceClass::Normal
    }
}

pub fn decode_rgba8(bytes: &[u8], allow_first_frame: bool) -> Result<DecodedImage, ImageError> {
    decode_rgba8_page(bytes, 0, allow_first_frame)
}

pub fn decode_rgba8_page(
    bytes: &[u8],
    page_index: u32,
    allow_first_frame: bool,
) -> Result<DecodedImage, ImageError> {
    let probe = probe(bytes)?;
    if page_index >= probe.pages || (probe.format != RasterFormat::Tiff && page_index != 0) {
        return Err(ImageError::Page {
            page: page_index,
            reason: "page index is outside the source".to_owned(),
        });
    }
    if probe.animated && !allow_first_frame {
        return Err(ImageError::Animated {
            format: probe.format.name(),
        });
    }
    if probe.class == ResourceClass::Rejected {
        return Err(ImageError::TooLarge {
            width: probe.width,
            height: probe.height,
            decoded_bytes: probe.decoded_bytes,
        });
    }
    // Proxy-required inputs need a format-specific reduced decoder. Refuse a
    // full allocation here so callers cannot accidentally defeat the limit.
    if probe.class == ResourceClass::ProxyRequired {
        return Err(ImageError::TooLarge {
            width: probe.width,
            height: probe.height,
            decoded_bytes: probe.decoded_bytes,
        });
    }
    if probe.format == RasterFormat::Tiff && page_index != 0 {
        return decode_tiff_page(bytes, page_index, probe);
    }
    let decoded =
        image::load_from_memory(bytes).map_err(|error| ImageError::Decode(error.to_string()))?;
    let (width, height) = decoded.dimensions();
    if width != probe.width || height != probe.height {
        return Err(ImageError::InvalidDecodedSize);
    }
    let rgba8 = decoded.into_rgba8().into_raw();
    let expected =
        usize::try_from(probe.decoded_bytes).map_err(|_| ImageError::InvalidDecodedSize)?;
    if rgba8.len() != expected {
        return Err(ImageError::InvalidDecodedSize);
    }
    let sha256 = sha256(bytes);
    Ok(DecodedImage {
        probe,
        rgba8,
        sha256,
    })
}

fn decode_tiff_page(
    bytes: &[u8],
    page_index: u32,
    mut probe: ImageProbe,
) -> Result<DecodedImage, ImageError> {
    use tiff::ColorType;
    use tiff::decoder::DecodingResult;
    let mut decoder =
        tiff::decoder::Decoder::new(Cursor::new(bytes)).map_err(|error| ImageError::Page {
            page: page_index,
            reason: error.to_string(),
        })?;
    for _ in 0..page_index {
        decoder.next_image().map_err(|error| ImageError::Page {
            page: page_index,
            reason: error.to_string(),
        })?;
    }
    let (width, height) = decoder.dimensions().map_err(|error| ImageError::Page {
        page: page_index,
        reason: error.to_string(),
    })?;
    let pixels = u64::from(width)
        .checked_mul(u64::from(height))
        .ok_or(ImageError::InvalidDecodedSize)?;
    let decoded_bytes = pixels
        .checked_mul(4)
        .ok_or(ImageError::InvalidDecodedSize)?;
    let class = classify(pixels, decoded_bytes);
    if class != ResourceClass::Normal {
        return Err(ImageError::TooLarge {
            width,
            height,
            decoded_bytes,
        });
    }
    let color = decoder.colortype().map_err(|error| ImageError::Page {
        page: page_index,
        reason: error.to_string(),
    })?;
    let data = decoder.read_image().map_err(|error| ImageError::Page {
        page: page_index,
        reason: error.to_string(),
    })?;
    let rgba8 = match data {
        DecodingResult::U8(values) => samples_to_rgba8(&values, color, |value| value)?,
        DecodingResult::U16(values) => {
            samples_to_rgba8(&values, color, |value| (value >> 8) as u8)?
        }
        _ => {
            return Err(ImageError::Page {
                page: page_index,
                reason: "only unsigned 8-bit and 16-bit TIFF samples are supported".to_owned(),
            });
        }
    };
    probe.width = width;
    probe.height = height;
    probe.decoded_bytes = decoded_bytes;
    probe.class = class;
    probe.high_precision = matches!(
        color,
        ColorType::Gray(16) | ColorType::GrayA(16) | ColorType::RGB(16) | ColorType::RGBA(16)
    );
    Ok(DecodedImage {
        probe,
        rgba8,
        sha256: sha256(bytes),
    })
}

fn samples_to_rgba8<T: Copy>(
    values: &[T],
    color: tiff::ColorType,
    convert: impl Fn(T) -> u8,
) -> Result<Vec<u8>, ImageError> {
    let channels = match color {
        tiff::ColorType::Gray(_) => 1,
        tiff::ColorType::GrayA(_) => 2,
        tiff::ColorType::RGB(_) => 3,
        tiff::ColorType::RGBA(_) | tiff::ColorType::CMYK(_) => 4,
        _ => {
            return Err(ImageError::Decode(format!(
                "unsupported TIFF color type {color:?}"
            )));
        }
    };
    if !values.len().is_multiple_of(channels) {
        return Err(ImageError::InvalidDecodedSize);
    }
    let mut out = Vec::with_capacity(values.len() / channels * 4);
    for sample in values.chunks_exact(channels) {
        match color {
            tiff::ColorType::Gray(_) => {
                let value = convert(sample[0]);
                out.extend_from_slice(&[value, value, value, 255]);
            }
            tiff::ColorType::GrayA(_) => {
                let value = convert(sample[0]);
                out.extend_from_slice(&[value, value, value, convert(sample[1])]);
            }
            tiff::ColorType::RGB(_) => out.extend_from_slice(&[
                convert(sample[0]),
                convert(sample[1]),
                convert(sample[2]),
                255,
            ]),
            tiff::ColorType::RGBA(_) => out.extend(sample.iter().map(|value| convert(*value))),
            tiff::ColorType::CMYK(_) => {
                let c = u16::from(convert(sample[0]));
                let m = u16::from(convert(sample[1]));
                let y = u16::from(convert(sample[2]));
                let k = u16::from(convert(sample[3]));
                out.extend_from_slice(&[
                    255_u16.saturating_sub((c + k).min(255)) as u8,
                    255_u16.saturating_sub((m + k).min(255)) as u8,
                    255_u16.saturating_sub((y + k).min(255)) as u8,
                    255,
                ]);
            }
            _ => unreachable!(),
        }
    }
    Ok(out)
}

pub fn strip_metadata_to_png(bytes: &[u8], allow_first_frame: bool) -> Result<Vec<u8>, ImageError> {
    let decoded = decode_rgba8(bytes, allow_first_frame)?;
    let image =
        image::RgbaImage::from_raw(decoded.probe.width, decoded.probe.height, decoded.rgba8)
            .ok_or(ImageError::InvalidDecodedSize)?;
    let mut output = Cursor::new(Vec::new());
    image::DynamicImage::ImageRgba8(image)
        .write_to(&mut output, image::ImageFormat::Png)
        .map_err(|error| ImageError::Decode(error.to_string()))?;
    Ok(output.into_inner())
}

fn animation_marker(format: RasterFormat, bytes: &[u8]) -> bool {
    match format {
        RasterFormat::Png => bytes.windows(4).any(|window| window == b"acTL"),
        RasterFormat::WebP => bytes
            .windows(4)
            .any(|window| window == b"ANIM" || window == b"ANMF"),
        _ => false,
    }
}

fn looks_like_svg(bytes: &[u8]) -> bool {
    let prefix = &bytes[..bytes.len().min(1024)];
    std::str::from_utf8(prefix)
        .ok()
        .map(str::trim_start)
        .is_some_and(|text| {
            text.starts_with("<svg") || (text.starts_with("<?xml") && text.contains("<svg"))
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn magic_bytes_win_without_an_extension() {
        assert_eq!(sniff(b"\x89PNG\r\n\x1a\nrest"), RasterFormat::Png);
        assert_eq!(sniff(b"BMrest"), RasterFormat::Bmp);
        assert_eq!(
            sniff(b"<svg xmlns='http://www.w3.org/2000/svg'>"),
            RasterFormat::Svg
        );
    }

    #[test]
    fn detects_animation_markers() {
        assert!(animation_marker(RasterFormat::Png, b"png acTL chunk"));
        assert!(animation_marker(RasterFormat::WebP, b"webp ANMF frame"));
    }

    #[test]
    fn decodes_every_supported_first_release_format() {
        let cases = [
            (image::ImageFormat::Png, RasterFormat::Png),
            (image::ImageFormat::Jpeg, RasterFormat::Jpeg),
            (image::ImageFormat::Tiff, RasterFormat::Tiff),
            (image::ImageFormat::WebP, RasterFormat::WebP),
            (image::ImageFormat::Bmp, RasterFormat::Bmp),
        ];
        for (encoded, expected) in cases {
            let source =
                image::RgbaImage::from_raw(2, 1, vec![255, 0, 0, 128, 0, 255, 0, 255]).unwrap();
            let mut cursor = Cursor::new(Vec::new());
            image::DynamicImage::ImageRgba8(source)
                .write_to(&mut cursor, encoded)
                .unwrap();
            let decoded = decode_rgba8(cursor.get_ref(), false).unwrap();
            assert_eq!(decoded.probe.format, expected);
            assert_eq!([decoded.probe.width, decoded.probe.height], [2, 1]);
            assert_eq!(decoded.rgba8.len(), 8);
        }
    }

    #[test]
    fn preserves_alpha_and_expands_grayscale_to_rgba8() {
        let alpha = image::RgbaImage::from_raw(1, 1, vec![10, 20, 30, 40]).unwrap();
        let gray = image::GrayImage::from_raw(1, 1, vec![73]).unwrap();
        let mut alpha_png = Cursor::new(Vec::new());
        let mut gray_png = Cursor::new(Vec::new());
        image::DynamicImage::ImageRgba8(alpha)
            .write_to(&mut alpha_png, image::ImageFormat::Png)
            .unwrap();
        image::DynamicImage::ImageLuma8(gray)
            .write_to(&mut gray_png, image::ImageFormat::Png)
            .unwrap();
        assert_eq!(
            decode_rgba8(alpha_png.get_ref(), false).unwrap().rgba8,
            [10, 20, 30, 40]
        );
        assert_eq!(
            decode_rgba8(gray_png.get_ref(), false).unwrap().rgba8,
            [73, 73, 73, 255]
        );
    }

    #[test]
    fn rejects_corrupt_and_truncated_supported_payloads() {
        for bytes in [
            b"\x89PNG\r\n\x1a\ntruncated".as_slice(),
            b"\xff\xd8\xfftruncated".as_slice(),
            b"BMtruncated".as_slice(),
        ] {
            assert!(probe(bytes).is_err());
        }
    }

    #[test]
    fn extension_cannot_override_magic_bytes() {
        let source = image::RgbaImage::from_pixel(1, 1, image::Rgba([1, 2, 3, 4]));
        let mut bytes_named_jpeg = Cursor::new(Vec::new());
        image::DynamicImage::ImageRgba8(source)
            .write_to(&mut bytes_named_jpeg, image::ImageFormat::Png)
            .unwrap();
        assert_eq!(
            probe(bytes_named_jpeg.get_ref()).unwrap().format,
            RasterFormat::Png
        );
    }

    #[test]
    fn soft_and_hard_limits_are_strict_at_the_documented_boundaries() {
        assert_eq!(classify(SOFT_PIXEL_LIMIT, 4), ResourceClass::Normal);
        assert_eq!(
            classify(SOFT_PIXEL_LIMIT + 1, 4),
            ResourceClass::ProxyRequired
        );
        assert_eq!(
            classify(1, SOFT_DECODED_BYTES + 1),
            ResourceClass::ProxyRequired
        );
        assert_eq!(classify(HARD_PIXEL_LIMIT + 1, 4), ResourceClass::Rejected);
        assert_eq!(classify(1, HARD_DECODED_BYTES + 1), ResourceClass::Rejected);
    }

    #[test]
    fn proxy_decode_is_bounded_and_preserves_aspect() {
        let source = image::RgbaImage::from_pixel(10, 5, image::Rgba([1, 2, 3, 4]));
        let mut encoded = Cursor::new(Vec::new());
        image::DynamicImage::ImageRgba8(source)
            .write_to(&mut encoded, image::ImageFormat::Png)
            .unwrap();
        let proxy = decode_proxy_rgba8(encoded.get_ref(), 4).unwrap();
        assert_eq!(proxy.pixel_size, [4, 2]);
        assert_eq!(proxy.rgba8.len(), 32);
    }

    #[test]
    fn file_backed_proxy_handles_every_supported_format() {
        for format in [
            image::ImageFormat::Png,
            image::ImageFormat::Jpeg,
            image::ImageFormat::Tiff,
            image::ImageFormat::WebP,
            image::ImageFormat::Bmp,
        ] {
            let source =
                image::RgbaImage::from_fn(8, 4, |x, y| image::Rgba([x as u8, y as u8, 17, 200]));
            let mut encoded = Cursor::new(Vec::new());
            image::DynamicImage::ImageRgba8(source)
                .write_to(&mut encoded, format)
                .unwrap();
            let probe = probe(encoded.get_ref()).unwrap();
            let proxy = proxy::decode_file_backed_proxy(encoded.get_ref(), 4, probe).unwrap();
            assert_eq!(proxy.pixel_size, [4, 2], "{format:?}");
            assert_eq!(proxy.rgba8.len(), 32, "{format:?}");
        }
    }

    #[test]
    fn explicit_metadata_strip_reencodes_pixels_as_png() {
        let source = image::RgbaImage::from_pixel(3, 2, image::Rgba([7, 8, 9, 10]));
        let mut encoded = Cursor::new(Vec::new());
        image::DynamicImage::ImageRgba8(source)
            .write_to(&mut encoded, image::ImageFormat::Bmp)
            .unwrap();
        let stripped = strip_metadata_to_png(encoded.get_ref(), false).unwrap();
        let result = decode_rgba8(&stripped, false).unwrap();
        assert_eq!(result.probe.format, RasterFormat::Png);
        assert_eq!([result.probe.width, result.probe.height], [3, 2]);
        assert!(!result.probe.has_icc);
        assert!(!result.probe.has_exif);
        assert_eq!(result.rgba8, [7, 8, 9, 10].repeat(6));
    }

    #[test]
    fn reports_bmp_physical_density_without_decoding_pixels() {
        let mut bytes = vec![0_u8; 54];
        bytes[..2].copy_from_slice(b"BM");
        bytes[38..42].copy_from_slice(&11_811_i32.to_le_bytes());
        bytes[42..46].copy_from_slice(&5_906_i32.to_le_bytes());
        let dpi = metadata_dpi(&bytes).unwrap();
        assert!((dpi[0] - 300.0).abs() < 0.1);
        assert!((dpi[1] - 150.0).abs() < 0.1);
    }

    #[test]
    fn counts_and_decodes_each_multipage_tiff_page() {
        use tiff::encoder::{TiffEncoder, colortype};
        let mut encoded = Cursor::new(Vec::new());
        {
            let mut encoder = TiffEncoder::new(&mut encoded).unwrap();
            encoder
                .write_image::<colortype::Gray8>(2, 1, &[10, 20])
                .unwrap();
            encoder
                .write_image::<colortype::RGB8>(1, 2, &[1, 2, 3, 4, 5, 6])
                .unwrap();
        }
        assert_eq!(tiff_page_count(encoded.get_ref()), Some(2));
        assert_eq!(probe(encoded.get_ref()).unwrap().pages, 2);
        let second = decode_rgba8_page(encoded.get_ref(), 1, false).unwrap();
        assert_eq!([second.probe.width, second.probe.height], [1, 2]);
        assert_eq!(second.rgba8, [1, 2, 3, 255, 4, 5, 6, 255]);
        assert!(decode_rgba8_page(encoded.get_ref(), 2, false).is_err());
    }
}
