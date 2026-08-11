//! Raw Win32 clipboard writer: publishes one figure as bitmap (CF_DIBV5,
//! "PNG") and vector ("image/svg+xml", CF_ENHMETAFILE) formats at once, so
//! each paste target picks the richest one it understands.

use std::cell::Cell;
use std::fmt;
use windows_sys::Win32::Foundation::{GetLastError, GlobalFree, HANDLE, HGLOBAL};
use windows_sys::Win32::Graphics::Gdi::{DeleteEnhMetaFile, SetEnhMetaFileBits};
use windows_sys::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, GetClipboardData, IsClipboardFormatAvailable, OpenClipboard,
    RegisterClipboardFormatW, SetClipboardData,
};
use windows_sys::Win32::System::Memory::{
    GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock,
};
use windows_sys::Win32::System::Ole::{CF_DIBV5, CF_ENHMETAFILE, CF_UNICODETEXT};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, VK_CONTROL, VK_SHIFT, VK_V,
};
use windows_sys::Win32::UI::Shell::DragQueryFileW;

const CF_DIB_FORMAT: u32 = 8;
const CF_HDROP_FORMAT: u32 = 15;

thread_local! {
    static PASTE_CHORD_DOWN: Cell<bool> = const { Cell::new(false) };
}

pub(super) const PLOTX_TABLE_SCHEMA_MIME: &str =
    "application/vnd.plotx.table-schema+json;version=1";

pub(super) struct FormatOutcome {
    pub name: &'static str,
    pub ok: bool,
    pub error: Option<String>,
}

#[derive(Debug)]
pub(super) enum NativeClipboardError {
    OpenFailed {
        last_error: u32,
    },
    ReadFailed {
        operation: &'static str,
        last_error: u32,
    },
    BitmapDecode(String),
}

impl fmt::Display for NativeClipboardError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::OpenFailed { last_error } => {
                write!(f, "the clipboard could not be opened (error {last_error})")
            }
            Self::ReadFailed {
                operation,
                last_error,
            } => write!(f, "{operation} failed (error {last_error})"),
            Self::BitmapDecode(error) => write!(f, "could not decode clipboard bitmap: {error}"),
        }
    }
}

impl std::error::Error for NativeClipboardError {}

/// Empties the clipboard once, then publishes every provided format. Formats
/// fail independently; the call errors only if the clipboard never opened.
pub(super) fn set_clipboard_formats(
    dibv5: &[u8],
    png: &[u8],
    svg: &str,
    emf: Option<&[u8]>,
) -> Result<Vec<FormatOutcome>, NativeClipboardError> {
    let _guard = ClipboardGuard::open()?;
    unsafe {
        EmptyClipboard();
    }
    let mut outcomes = Vec::new();
    outcomes.push(set_bytes(CF_DIBV5 as u32, "dibv5", dibv5));
    outcomes.push(set_bytes(register_format("PNG"), "png", png));
    // Trailing NUL: some consumers treat the payload as a C string.
    let mut svg_bytes = svg.as_bytes().to_vec();
    svg_bytes.push(0);
    outcomes.push(set_bytes(
        register_format("image/svg+xml"),
        "svg",
        &svg_bytes,
    ));
    if let Some(emf_bytes) = emf {
        outcomes.push(set_emf(emf_bytes));
    }
    Ok(outcomes)
}

/// Publish standard Unicode TSV and, when available, PlotX's typed schema
/// contract in one atomic clipboard ownership change.
pub(super) fn set_table_formats(
    text: &str,
    schema_json: Option<&str>,
) -> Result<Vec<FormatOutcome>, NativeClipboardError> {
    let _guard = ClipboardGuard::open()?;
    unsafe {
        EmptyClipboard();
    }
    let utf16 = text
        .encode_utf16()
        .chain(std::iter::once(0))
        .flat_map(u16::to_le_bytes)
        .collect::<Vec<_>>();
    let mut outcomes = vec![set_bytes(CF_UNICODETEXT as u32, "tsv", &utf16)];
    if let Some(schema_json) = schema_json {
        let mut schema_bytes = schema_json.as_bytes().to_vec();
        schema_bytes.push(0);
        outcomes.push(set_bytes(
            register_format(PLOTX_TABLE_SCHEMA_MIME),
            "plotx_schema",
            &schema_bytes,
        ));
    }
    Ok(outcomes)
}

pub(super) fn get_table_schema() -> Result<Option<String>, NativeClipboardError> {
    let format = register_format(PLOTX_TABLE_SCHEMA_MIME);
    if format == 0 || unsafe { IsClipboardFormatAvailable(format) } == 0 {
        return Ok(None);
    }
    let _guard = ClipboardGuard::open()?;
    let handle = unsafe { GetClipboardData(format) };
    if handle.is_null() {
        return Ok(None);
    }
    let memory = handle as HGLOBAL;
    let size = unsafe { GlobalSize(memory) };
    let pointer = unsafe { GlobalLock(memory) };
    if pointer.is_null() {
        return Ok(None);
    }
    let allocation = unsafe { std::slice::from_raw_parts(pointer.cast::<u8>(), size) };
    let payload_len = allocation
        .iter()
        .position(|byte| *byte == 0)
        .unwrap_or(allocation.len());
    let bytes = allocation[..payload_len].to_vec();
    unsafe {
        GlobalUnlock(memory);
    }
    Ok(String::from_utf8(bytes).ok())
}

pub(super) fn get_file_list() -> Result<Vec<std::path::PathBuf>, NativeClipboardError> {
    if unsafe { IsClipboardFormatAvailable(CF_HDROP_FORMAT) } == 0 {
        return Ok(Vec::new());
    }
    let _guard = ClipboardGuard::open()?;
    let handle = unsafe { GetClipboardData(CF_HDROP_FORMAT) };
    if handle.is_null() {
        return Err(read_failed("GetClipboardData(CF_HDROP)"));
    }
    let count = unsafe { DragQueryFileW(handle, u32::MAX, std::ptr::null_mut(), 0) };
    let mut paths = Vec::with_capacity(count as usize);
    for index in 0..count {
        let len = unsafe { DragQueryFileW(handle, index, std::ptr::null_mut(), 0) };
        let mut buffer = vec![0_u16; len as usize + 1];
        let written =
            unsafe { DragQueryFileW(handle, index, buffer.as_mut_ptr(), buffer.len() as u32) };
        buffer.truncate(written as usize);
        paths.push(std::path::PathBuf::from(String::from_utf16_lossy(&buffer)));
    }
    Ok(paths)
}

pub(super) fn get_dib_image() -> Result<Option<image::RgbaImage>, NativeClipboardError> {
    let format = if unsafe { IsClipboardFormatAvailable(CF_DIBV5 as u32) } != 0 {
        CF_DIBV5 as u32
    } else if unsafe { IsClipboardFormatAvailable(CF_DIB_FORMAT) } != 0 {
        CF_DIB_FORMAT
    } else {
        return Ok(None);
    };
    let bytes = {
        let _guard = ClipboardGuard::open()?;
        let handle = unsafe { GetClipboardData(format) };
        if handle.is_null() {
            return Err(read_failed("GetClipboardData(CF_DIB)"));
        }
        let memory = handle as HGLOBAL;
        let size = unsafe { GlobalSize(memory) };
        if size == 0 {
            return Err(read_failed("GlobalSize(CF_DIB)"));
        }
        let pointer = unsafe { GlobalLock(memory) };
        if pointer.is_null() {
            return Err(read_failed("GlobalLock(CF_DIB)"));
        }
        let bytes = unsafe { std::slice::from_raw_parts(pointer.cast::<u8>(), size) }.to_vec();
        unsafe {
            GlobalUnlock(memory);
        }
        bytes
    };
    decode_dib(&bytes).map(Some)
}

fn decode_dib(bytes: &[u8]) -> Result<image::RgbaImage, NativeClipboardError> {
    let invalid = |reason: &str| NativeClipboardError::BitmapDecode(reason.to_owned());
    let header_size = read_u32(bytes, 0).ok_or_else(|| invalid("missing DIB header"))? as usize;
    if header_size < 40 || bytes.len() < header_size {
        return Err(invalid("unsupported or truncated DIB header"));
    }
    let width = read_i32(bytes, 4).ok_or_else(|| invalid("missing DIB width"))?;
    let signed_height = read_i32(bytes, 8).ok_or_else(|| invalid("missing DIB height"))?;
    if width <= 0 || signed_height == 0 {
        return Err(invalid("DIB dimensions are invalid"));
    }
    let width = width as u32;
    let height = signed_height.unsigned_abs();
    let top_down = signed_height < 0;
    let planes = read_u16(bytes, 12).ok_or_else(|| invalid("missing DIB plane count"))?;
    let bits = read_u16(bytes, 14).ok_or_else(|| invalid("missing DIB bit depth"))?;
    let compression = read_u32(bytes, 16).ok_or_else(|| invalid("missing DIB compression"))?;
    if planes != 1 || !matches!(bits, 16 | 24 | 32) || !matches!(compression, 0 | 3 | 6) {
        return Err(invalid("DIB pixel layout is not a supported RGB format"));
    }

    let external_masks = usize::from(header_size == 40 && matches!(compression, 3 | 6))
        * if compression == 6 { 16 } else { 12 };
    let colors_used = read_u32(bytes, 32).unwrap_or(0) as usize;
    let palette_entries = if bits <= 8 {
        if colors_used == 0 {
            1usize << bits
        } else {
            colors_used
        }
    } else {
        0
    };
    let pixel_offset = header_size
        .checked_add(external_masks)
        .and_then(|offset| offset.checked_add(palette_entries * 4))
        .ok_or_else(|| invalid("DIB pixel offset overflowed"))?;
    let row_stride = (u64::from(width) * u64::from(bits)).div_ceil(32) * 4;
    let pixel_bytes = row_stride
        .checked_mul(u64::from(height))
        .and_then(|size| usize::try_from(size).ok())
        .ok_or_else(|| invalid("DIB pixel size overflowed"))?;
    if pixel_offset
        .checked_add(pixel_bytes)
        .is_none_or(|end| end > bytes.len())
    {
        return Err(invalid("DIB pixel buffer is truncated"));
    }

    let masks = dib_color_masks(bytes, header_size, bits, compression)?;
    let rgba_len = usize::try_from(u64::from(width) * u64::from(height) * 4)
        .map_err(|_| invalid("decoded DIB size overflowed"))?;
    let mut rgba = vec![0; rgba_len];
    for output_y in 0..height as usize {
        let source_y = if top_down {
            output_y
        } else {
            height as usize - output_y - 1
        };
        let row = &bytes[pixel_offset + source_y * row_stride as usize..];
        for x in 0..width as usize {
            let output = (output_y * width as usize + x) * 4;
            match bits {
                24 => {
                    let input = x * 3;
                    rgba[output..output + 4].copy_from_slice(&[
                        row[input + 2],
                        row[input + 1],
                        row[input],
                        255,
                    ]);
                }
                16 => {
                    let input = x * 2;
                    let pixel = u16::from_le_bytes([row[input], row[input + 1]]) as u32;
                    write_masked_pixel(&mut rgba[output..output + 4], pixel, masks);
                }
                32 if compression == 0 => {
                    let input = x * 4;
                    rgba[output..output + 4].copy_from_slice(&[
                        row[input + 2],
                        row[input + 1],
                        row[input],
                        255,
                    ]);
                }
                32 => {
                    let input = x * 4;
                    let pixel = u32::from_le_bytes(
                        row[input..input + 4]
                            .try_into()
                            .map_err(|_| invalid("DIB pixel row ended unexpectedly"))?,
                    );
                    write_masked_pixel(&mut rgba[output..output + 4], pixel, masks);
                }
                _ => unreachable!("validated DIB bit depth"),
            }
        }
    }
    image::RgbaImage::from_raw(width, height, rgba)
        .ok_or_else(|| invalid("decoded DIB dimensions did not match its pixels"))
}

fn dib_color_masks(
    bytes: &[u8],
    header_size: usize,
    bits: u16,
    compression: u32,
) -> Result<[u32; 4], NativeClipboardError> {
    if compression == 0 {
        return Ok(if bits == 16 {
            [0x7C00, 0x03E0, 0x001F, 0]
        } else {
            [0x00FF_0000, 0x0000_FF00, 0x0000_00FF, 0]
        });
    }
    let offset = 40;
    let red = read_u32(bytes, offset)
        .ok_or_else(|| NativeClipboardError::BitmapDecode("missing red mask".to_owned()))?;
    let green = read_u32(bytes, offset + 4)
        .ok_or_else(|| NativeClipboardError::BitmapDecode("missing green mask".to_owned()))?;
    let blue = read_u32(bytes, offset + 8)
        .ok_or_else(|| NativeClipboardError::BitmapDecode("missing blue mask".to_owned()))?;
    let alpha = if header_size >= 56 || compression == 6 {
        read_u32(bytes, offset + 12).unwrap_or(0)
    } else {
        0
    };
    Ok([red, green, blue, alpha])
}

fn write_masked_pixel(output: &mut [u8], pixel: u32, masks: [u32; 4]) {
    output[0] = masked_component(pixel, masks[0]);
    output[1] = masked_component(pixel, masks[1]);
    output[2] = masked_component(pixel, masks[2]);
    output[3] = if masks[3] == 0 {
        255
    } else {
        masked_component(pixel, masks[3])
    };
}

fn masked_component(pixel: u32, mask: u32) -> u8 {
    if mask == 0 {
        return 0;
    }
    let shift = mask.trailing_zeros();
    let maximum = mask >> shift;
    let value = (pixel & mask) >> shift;
    (u64::from(value) * 255 / u64::from(maximum)) as u8
}

fn read_u16(bytes: &[u8], offset: usize) -> Option<u16> {
    Some(u16::from_le_bytes(
        bytes.get(offset..offset + 2)?.try_into().ok()?,
    ))
}

fn read_u32(bytes: &[u8], offset: usize) -> Option<u32> {
    Some(u32::from_le_bytes(
        bytes.get(offset..offset + 4)?.try_into().ok()?,
    ))
}

fn read_i32(bytes: &[u8], offset: usize) -> Option<i32> {
    Some(i32::from_le_bytes(
        bytes.get(offset..offset + 4)?.try_into().ok()?,
    ))
}

/// egui-winit consumes Ctrl+V before emitting a key event and emits
/// `Event::Paste` only when it can read non-empty text. Restore the swallowed
/// press for native file-list and bitmap clipboards.
pub(crate) fn restore_missing_paste_shortcut(raw_input: &mut egui::RawInput) {
    let v = unsafe { GetAsyncKeyState(VK_V as i32) } as u16;
    let control = unsafe { GetAsyncKeyState(VK_CONTROL as i32) } as u16;
    let shift = unsafe { GetAsyncKeyState(VK_SHIFT as i32) } as u16;
    let chord_down = v & 0x8000 != 0 && control & 0x8000 != 0;
    let pressed_since_poll = v & 1 != 0 && control & (0x8000 | 1) != 0;
    let shift_active = shift & 0x8000 != 0;
    PASTE_CHORD_DOWN.with(|was_down| {
        restore_paste_from_snapshot(
            raw_input,
            was_down,
            chord_down,
            pressed_since_poll,
            shift_active,
        );
    });
}

fn restore_paste_from_snapshot(
    raw_input: &mut egui::RawInput,
    was_down: &Cell<bool>,
    chord_down: bool,
    pressed_since_poll: bool,
    shift_active: bool,
) {
    let shift_active = shift_active
        || raw_input.modifiers.shift
        || raw_input.events.iter().any(|event| {
            matches!(
                event,
                egui::Event::Key {
                    key: egui::Key::V,
                    modifiers,
                    ..
                } if modifiers.shift
            )
        });
    let already_reported = raw_input.events.iter().any(|event| {
        matches!(event, egui::Event::Paste(_))
            || matches!(
                event,
                egui::Event::Key {
                    key: egui::Key::V,
                    pressed: true,
                    ..
                }
            )
    });
    if !was_down.get() && (chord_down || pressed_since_poll) && !shift_active && !already_reported {
        let mut modifiers = raw_input.modifiers;
        modifiers.ctrl = true;
        modifiers.command = true;
        modifiers.shift = false;
        raw_input.events.push(egui::Event::Key {
            key: egui::Key::V,
            physical_key: Some(egui::Key::V),
            pressed: true,
            repeat: false,
            modifiers,
        });
    }
    was_down.set(chord_down);
}

fn read_failed(operation: &'static str) -> NativeClipboardError {
    NativeClipboardError::ReadFailed {
        operation,
        last_error: unsafe { GetLastError() },
    }
}

struct ClipboardGuard;

impl ClipboardGuard {
    fn open() -> Result<Self, NativeClipboardError> {
        for attempt in 0..10 {
            if unsafe { OpenClipboard(std::ptr::null_mut()) } != 0 {
                return Ok(Self);
            }
            if attempt < 9 {
                std::thread::sleep(std::time::Duration::from_millis(15));
            }
        }
        Err(NativeClipboardError::OpenFailed {
            last_error: unsafe { GetLastError() },
        })
    }
}

impl Drop for ClipboardGuard {
    fn drop(&mut self) {
        unsafe {
            CloseClipboard();
        }
    }
}

fn register_format(name: &str) -> u32 {
    let wide: Vec<u16> = name.encode_utf16().chain(std::iter::once(0)).collect();
    unsafe { RegisterClipboardFormatW(wide.as_ptr()) }
}

fn set_bytes(format: u32, name: &'static str, bytes: &[u8]) -> FormatOutcome {
    if format == 0 {
        return failure(name, "clipboard format registration failed");
    }
    unsafe {
        let hglobal: HGLOBAL = GlobalAlloc(GMEM_MOVEABLE, bytes.len());
        if hglobal.is_null() {
            return failure(name, "GlobalAlloc failed");
        }
        let dst = GlobalLock(hglobal);
        if dst.is_null() {
            GlobalFree(hglobal);
            return failure(name, "GlobalLock failed");
        }
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), dst as *mut u8, bytes.len());
        GlobalUnlock(hglobal);
        if SetClipboardData(format, hglobal as HANDLE).is_null() {
            GlobalFree(hglobal);
            return failure(name, "SetClipboardData failed");
        }
    }
    FormatOutcome {
        name,
        ok: true,
        error: None,
    }
}

fn set_emf(bytes: &[u8]) -> FormatOutcome {
    unsafe {
        let hemf = SetEnhMetaFileBits(bytes.len() as u32, bytes.as_ptr());
        if hemf.is_null() {
            return failure("emf", "SetEnhMetaFileBits failed");
        }
        if SetClipboardData(CF_ENHMETAFILE as u32, hemf as HANDLE).is_null() {
            DeleteEnhMetaFile(hemf);
            return failure("emf", "SetClipboardData failed");
        }
    }
    FormatOutcome {
        name: "emf",
        ok: true,
        error: None,
    }
}

fn failure(name: &'static str, error: &str) -> FormatOutcome {
    FormatOutcome {
        name,
        ok: false,
        error: Some(format!("{error} (error {})", unsafe { GetLastError() })),
    }
}

/// A CF_DIBV5 payload: 124-byte BITMAPV5HEADER followed by top-down BGRA rows
/// with straight alpha. No BITMAPFILEHEADER on the clipboard.
pub(super) fn build_dibv5(width: u32, height: u32, rgba: &[u8], dpi: u16) -> Vec<u8> {
    let mut out = Vec::with_capacity(124 + rgba.len());
    let px_per_meter = (dpi as i32 * 10_000 + 127) / 254;
    let u32le = |out: &mut Vec<u8>, v: u32| out.extend_from_slice(&v.to_le_bytes());
    let i32le = |out: &mut Vec<u8>, v: i32| out.extend_from_slice(&v.to_le_bytes());
    u32le(&mut out, 124);
    i32le(&mut out, width as i32);
    i32le(&mut out, -(height as i32));
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&32u16.to_le_bytes());
    u32le(&mut out, 3); // BI_BITFIELDS: masks + alpha are honored
    u32le(&mut out, width * height * 4);
    i32le(&mut out, px_per_meter);
    i32le(&mut out, px_per_meter);
    u32le(&mut out, 0);
    u32le(&mut out, 0);
    u32le(&mut out, 0x00FF_0000); // red
    u32le(&mut out, 0x0000_FF00); // green
    u32le(&mut out, 0x0000_00FF); // blue
    u32le(&mut out, 0xFF00_0000); // alpha
    u32le(&mut out, 0x7352_4742); // LCS_sRGB ("sRGB")
    out.extend_from_slice(&[0u8; 36]); // CIE endpoints
    u32le(&mut out, 0); // gamma r/g/b
    u32le(&mut out, 0);
    u32le(&mut out, 0);
    u32le(&mut out, 4); // LCS_GM_IMAGES
    u32le(&mut out, 0);
    u32le(&mut out, 0);
    u32le(&mut out, 0);
    for px in rgba.chunks_exact(4) {
        out.extend_from_slice(&[px[2], px[1], px[0], px[3]]);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dibv5_header_layout_is_correct() {
        let rgba = [10u8, 20, 30, 40, 50, 60, 70, 80];
        let dib = build_dibv5(2, 1, &rgba, 300);
        assert_eq!(dib.len(), 124 + 8);
        assert_eq!(u32::from_le_bytes(dib[0..4].try_into().unwrap()), 124);
        assert_eq!(i32::from_le_bytes(dib[4..8].try_into().unwrap()), 2);
        assert_eq!(i32::from_le_bytes(dib[8..12].try_into().unwrap()), -1);
        assert_eq!(u16::from_le_bytes(dib[12..14].try_into().unwrap()), 1);
        assert_eq!(u16::from_le_bytes(dib[14..16].try_into().unwrap()), 32);
        assert_eq!(u32::from_le_bytes(dib[16..20].try_into().unwrap()), 3);
        assert_eq!(u32::from_le_bytes(dib[20..24].try_into().unwrap()), 8);
        assert_eq!(
            u32::from_le_bytes(dib[40..44].try_into().unwrap()),
            0x00FF_0000
        );
        assert_eq!(
            u32::from_le_bytes(dib[52..56].try_into().unwrap()),
            0xFF00_0000
        );
        assert_eq!(
            u32::from_le_bytes(dib[56..60].try_into().unwrap()),
            0x7352_4742
        );
        assert_eq!(&dib[124..128], &[30, 20, 10, 40]);
        assert_eq!(&dib[128..132], &[70, 60, 50, 80]);
    }

    #[test]
    fn native_dib_decoder_accepts_standard_dibv5_pixels() {
        let rgba = [10u8, 20, 30, 255, 50, 60, 70, 128];
        let decoded = decode_dib(&build_dibv5(2, 1, &rgba, 96)).unwrap();
        assert_eq!(decoded.dimensions(), (2, 1));
        assert_eq!(decoded.into_raw(), rgba);
    }

    #[test]
    fn native_dib_decoder_handles_bottom_up_24_bit_rows_and_padding() {
        let mut dib = vec![0_u8; 40];
        dib[0..4].copy_from_slice(&40_u32.to_le_bytes());
        dib[4..8].copy_from_slice(&2_i32.to_le_bytes());
        dib[8..12].copy_from_slice(&2_i32.to_le_bytes());
        dib[12..14].copy_from_slice(&1_u16.to_le_bytes());
        dib[14..16].copy_from_slice(&24_u16.to_le_bytes());
        dib[20..24].copy_from_slice(&16_u32.to_le_bytes());
        dib.extend_from_slice(&[
            255, 0, 0, 255, 255, 255, 0, 0, // bottom: blue, white
            0, 0, 255, 0, 255, 0, 0, 0, // top: red, green
        ]);

        let decoded = decode_dib(&dib).unwrap();
        assert_eq!(
            decoded.into_raw(),
            [
                255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 255, 255,
            ]
        );
    }

    #[test]
    fn missing_paste_press_is_restored_once_per_chord() {
        let state = Cell::new(false);
        let mut raw = egui::RawInput::default();
        restore_paste_from_snapshot(&mut raw, &state, true, true, false);
        assert!(matches!(
            raw.events.as_slice(),
            [egui::Event::Key {
                key: egui::Key::V,
                pressed: true,
                ..
            }]
        ));

        raw.events.clear();
        restore_paste_from_snapshot(&mut raw, &state, true, false, false);
        assert!(raw.events.is_empty());
        restore_paste_from_snapshot(&mut raw, &state, false, false, false);
        restore_paste_from_snapshot(&mut raw, &state, true, true, false);
        assert_eq!(raw.events.len(), 1);
    }

    #[test]
    fn existing_paste_event_and_shift_paste_are_not_rewritten() {
        let state = Cell::new(false);
        let mut raw = egui::RawInput {
            events: vec![egui::Event::Paste("text".to_owned())],
            ..Default::default()
        };
        restore_paste_from_snapshot(&mut raw, &state, true, true, false);
        assert_eq!(raw.events.len(), 1);

        raw.events.clear();
        restore_paste_from_snapshot(&mut raw, &state, false, false, false);
        restore_paste_from_snapshot(&mut raw, &state, true, true, true);
        assert!(raw.events.is_empty());
    }

    /// Puts a test pattern on the real clipboard for manual inspection.
    #[test]
    #[ignore]
    fn manual_clipboard_probe() {
        let (w, h) = (200u32, 100u32);
        let mut rgba = Vec::with_capacity((w * h * 4) as usize);
        for y in 0..h {
            for x in 0..w {
                let (r, g, b) = match (x >= w / 2, y >= h / 2) {
                    (false, false) => (255, 0, 0),
                    (true, false) => (0, 255, 0),
                    (false, true) => (0, 0, 255),
                    (true, true) => (255, 255, 255),
                };
                rgba.extend_from_slice(&[r, g, b, 255u8]);
            }
        }
        let dib = build_dibv5(w, h, &rgba, 300);
        let outcomes =
            set_clipboard_formats(&dib, &[0x89, b'P'], "<svg/>", None).expect("clipboard open");
        for o in &outcomes {
            println!("{}: ok={} {:?}", o.name, o.ok, o.error);
        }
    }
}
