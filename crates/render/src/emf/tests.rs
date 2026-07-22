use super::*;
use plotx_figure::{Axis, ErrorBar, Figure, Series};

fn demo_document(fig: &Figure) -> Document<'_> {
    Document {
        width: 400.0,
        height: 300.0,
        background: Color::rgb(255, 255, 255),
        items: vec![DocumentItem::Plot(DocumentObject {
            id: "obj".into(),
            frame: Rect::new(20.0, 20.0, 360.0, 260.0),
            figure: fig,
            visible: true,
            title: None,
        })],
    }
}

#[test]
fn exports_valid_emf_header() {
    let fig = Figure::new(
        "Demo",
        Axis::new("ppm", 0.0, 10.0).reversed(true),
        Axis::new("intensity", 0.0, 1.0),
    )
    .with_series(Series::line(
        "trace",
        vec![[0.0, 0.0], [5.0, 1.0], [10.0, 0.0]],
    ))
    .with_error_bar(ErrorBar::symmetric([5.0, 0.5], 0.2));
    let doc = demo_document(&fig);
    let bytes = export_document_emf(&doc).expect("export");
    assert!(bytes.len() >= 88);
    assert_eq!(u32::from_le_bytes(bytes[0..4].try_into().unwrap()), 1);
    assert_eq!(&bytes[40..44], b" EMF");
    assert_eq!(
        u32::from_le_bytes(bytes[48..52].try_into().unwrap()) as usize,
        bytes.len()
    );
    let frame_right = i32::from_le_bytes(bytes[32..36].try_into().unwrap());
    let frame_bottom = i32::from_le_bytes(bytes[36..40].try_into().unwrap());
    assert_eq!(frame_right, (400.0f64 * 2540.0 / 72.0).round() as i32);
    assert_eq!(frame_bottom, (300.0f64 * 2540.0 / 72.0).round() as i32);
}

#[test]
fn round_trips_through_set_enh_meta_file_bits() {
    let fig = Figure::new("t", Axis::new("x", 0.0, 1.0), Axis::new("y", 0.0, 1.0));
    let doc = demo_document(&fig);
    let bytes = export_document_emf(&doc).expect("export");
    unsafe {
        let hemf = windows_sys::Win32::Graphics::Gdi::SetEnhMetaFileBits(
            bytes.len() as u32,
            bytes.as_ptr(),
        );
        assert!(!hemf.is_null());
        DeleteEnhMetaFile(hemf);
    }
}

#[test]
fn hidden_axis_text_is_absent_from_emf_while_drawing_records_remain() {
    let mut fig = Figure::new(
        "",
        Axis::new("UNIQUE_X_TITLE", 0.0, 90_000.0),
        Axis::new("UNIQUE_Y_TITLE", -90_000.0, 90_000.0),
    );
    fig.x.show_tick_labels = false;
    fig.x.show_label = false;
    fig.y.show_tick_labels = false;
    fig.y.show_label = false;
    let bytes = export_document_emf(&demo_document(&fig)).expect("export");
    let contains_utf16 = |needle: &str| {
        let encoded: Vec<u8> = needle.encode_utf16().flat_map(u16::to_le_bytes).collect();
        bytes.windows(encoded.len()).any(|window| window == encoded)
    };
    assert!(!contains_utf16("UNIQUE_X_TITLE"));
    assert!(!contains_utf16("UNIQUE_Y_TITLE"));
    assert!(!contains_utf16("×10"));
    assert!(
        bytes.len() > 88,
        "EMF still contains axis and tick drawing records"
    );
}
