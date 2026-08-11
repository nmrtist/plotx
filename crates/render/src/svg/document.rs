use super::{
    Document, DocumentItem, Rect, write_document_object, write_overlay, write_panel_letter,
};
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
    for item in &document.items {
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
            DocumentItem::Raster(_) => {}
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{DocumentObject, DocumentText};
    use plotx_figure::{Axis, AxisFrame, Color, Figure};

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
