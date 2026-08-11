use super::{HARD_DECODED_BYTES, ImageError, ImageProbe, ProxyImage, ResourceClass, probe};
use image::ImageDecoder;
use std::io::Cursor;

pub fn decode_proxy_rgba8(bytes: &[u8], max_edge: u32) -> Result<ProxyImage, ImageError> {
    let probe = probe(bytes)?;
    if probe.class == ResourceClass::Rejected {
        return Err(ImageError::TooLarge {
            width: probe.width,
            height: probe.height,
            decoded_bytes: probe.decoded_bytes,
        });
    }
    if probe.class == ResourceClass::ProxyRequired {
        return decode_file_backed_proxy(bytes, max_edge, probe);
    }
    let decoded =
        image::load_from_memory(bytes).map_err(|error| ImageError::Decode(error.to_string()))?;
    let edge = max_edge.max(1);
    let proxy = decoded.thumbnail(edge, edge).into_rgba8();
    let pixel_size = [proxy.width(), proxy.height()];
    let rgba8 = proxy.into_raw();
    let expected = usize::try_from(u64::from(pixel_size[0]) * u64::from(pixel_size[1]) * 4)
        .map_err(|_| ImageError::InvalidDecodedSize)?;
    if rgba8.len() != expected {
        return Err(ImageError::InvalidDecodedSize);
    }
    Ok(ProxyImage { pixel_size, rgba8 })
}

pub(super) fn decode_file_backed_proxy(
    bytes: &[u8],
    max_edge: u32,
    probe: ImageProbe,
) -> Result<ProxyImage, ImageError> {
    let reader = image::ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|error| ImageError::Decode(error.to_string()))?;
    let decoder = reader
        .into_decoder()
        .map_err(|error| ImageError::Decode(error.to_string()))?;
    let color = decoder.color_type();
    let source_bytes = decoder.total_bytes();
    if source_bytes > HARD_DECODED_BYTES {
        return Err(ImageError::TooLarge {
            width: probe.width,
            height: probe.height,
            decoded_bytes: source_bytes,
        });
    }
    let mapped_len = usize::try_from(source_bytes).map_err(|_| ImageError::TooLarge {
        width: probe.width,
        height: probe.height,
        decoded_bytes: source_bytes,
    })?;
    let file = tempfile::tempfile().map_err(|error| ImageError::Decode(error.to_string()))?;
    file.set_len(source_bytes)
        .map_err(|error| ImageError::Decode(error.to_string()))?;
    // The mapping cannot outlive `file`, and its exact decoder-reported length
    // has passed the project hard limit above.
    let mut mapped = unsafe {
        memmap2::MmapOptions::new()
            .len(mapped_len)
            .map_mut(&file)
            .map_err(|error| ImageError::Decode(error.to_string()))?
    };
    decoder
        .read_image(&mut mapped)
        .map_err(|error| ImageError::Decode(error.to_string()))?;
    proxy_from_pixels(&mapped, [probe.width, probe.height], color, max_edge)
}

fn proxy_from_pixels(
    pixels: &[u8],
    source: [u32; 2],
    color: image::ColorType,
    max_edge: u32,
) -> Result<ProxyImage, ImageError> {
    let edge = max_edge.max(1);
    let scale = (edge as f64 / f64::from(source[0].max(source[1]))).min(1.0);
    let width = (f64::from(source[0]) * scale).round().max(1.0) as u32;
    let height = (f64::from(source[1]) * scale).round().max(1.0) as u32;
    let bytes_per_pixel = usize::from(color.bytes_per_pixel());
    let expected = usize::try_from(u64::from(source[0]) * u64::from(source[1]))
        .ok()
        .and_then(|count| count.checked_mul(bytes_per_pixel))
        .ok_or(ImageError::InvalidDecodedSize)?;
    if pixels.len() != expected {
        return Err(ImageError::InvalidDecodedSize);
    }
    let output_len = usize::try_from(u64::from(width) * u64::from(height) * 4)
        .map_err(|_| ImageError::InvalidDecodedSize)?;
    let mut rgba8 = Vec::with_capacity(output_len);
    for y in 0..height {
        let source_y = (u64::from(y) * u64::from(source[1]) / u64::from(height)) as u32;
        for x in 0..width {
            let source_x = (u64::from(x) * u64::from(source[0]) / u64::from(width)) as u32;
            let pixel_index = u64::from(source_y) * u64::from(source[0]) + u64::from(source_x);
            let offset = usize::try_from(pixel_index * bytes_per_pixel as u64)
                .map_err(|_| ImageError::InvalidDecodedSize)?;
            rgba8.extend_from_slice(&pixel_to_rgba8(
                &pixels[offset..offset + bytes_per_pixel],
                color,
            )?);
        }
    }
    Ok(ProxyImage {
        pixel_size: [width, height],
        rgba8,
    })
}

fn pixel_to_rgba8(pixel: &[u8], color: image::ColorType) -> Result<[u8; 4], ImageError> {
    let u16_sample =
        |offset: usize| u16::from_ne_bytes([pixel[offset], pixel[offset + 1]]).to_be_bytes()[0];
    let f32_sample = |offset: usize| {
        let bytes = [
            pixel[offset],
            pixel[offset + 1],
            pixel[offset + 2],
            pixel[offset + 3],
        ];
        (f32::from_ne_bytes(bytes).clamp(0.0, 1.0) * 255.0).round() as u8
    };
    let rgba = match color {
        image::ColorType::L8 => [pixel[0], pixel[0], pixel[0], 255],
        image::ColorType::La8 => [pixel[0], pixel[0], pixel[0], pixel[1]],
        image::ColorType::Rgb8 => [pixel[0], pixel[1], pixel[2], 255],
        image::ColorType::Rgba8 => [pixel[0], pixel[1], pixel[2], pixel[3]],
        image::ColorType::L16 => {
            let value = u16_sample(0);
            [value, value, value, 255]
        }
        image::ColorType::La16 => {
            let value = u16_sample(0);
            [value, value, value, u16_sample(2)]
        }
        image::ColorType::Rgb16 => [u16_sample(0), u16_sample(2), u16_sample(4), 255],
        image::ColorType::Rgba16 => [u16_sample(0), u16_sample(2), u16_sample(4), u16_sample(6)],
        image::ColorType::Rgb32F => [f32_sample(0), f32_sample(4), f32_sample(8), 255],
        image::ColorType::Rgba32F => [f32_sample(0), f32_sample(4), f32_sample(8), f32_sample(12)],
        _ => {
            return Err(ImageError::Decode(format!(
                "unsupported proxy color type {color:?}"
            )));
        }
    };
    Ok(rgba)
}
