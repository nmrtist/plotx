//! Raw Win32 clipboard writer: publishes one figure as bitmap (CF_DIBV5,
//! "PNG") and vector ("image/svg+xml", CF_ENHMETAFILE) formats at once, so
//! each paste target picks the richest one it understands.

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

pub(super) const PLOTX_TABLE_SCHEMA_MIME: &str =
    "application/vnd.plotx.table-schema+json;version=1";

pub(super) struct FormatOutcome {
    pub name: &'static str,
    pub ok: bool,
    pub error: Option<String>,
}

#[derive(Debug)]
pub(super) enum NativeClipboardError {
    OpenFailed { last_error: u32 },
}

impl fmt::Display for NativeClipboardError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::OpenFailed { last_error } => {
                write!(f, "the clipboard could not be opened (error {last_error})")
            }
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
