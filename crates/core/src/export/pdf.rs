use super::{ExportError, fonts};
use pdf_writer::{Content, Finish, Name, Pdf, Rect, Ref, TextStr};
use std::collections::HashMap;

pub(super) fn render(svgs: &[String]) -> Result<Vec<u8>, ExportError> {
    if svgs.len() == 1 {
        let tree = parse_svg(&svgs[0])?;
        return svg2pdf::to_pdf(
            &tree,
            svg2pdf::ConversionOptions::default(),
            svg2pdf::PageOptions::default(),
        )
        .map_err(|error| ExportError::Pdf(error.to_string()));
    }
    render_multi_page(svgs)
}

fn render_multi_page(svgs: &[String]) -> Result<Vec<u8>, ExportError> {
    let mut alloc = Ref::new(1);
    let catalog_id = alloc.bump();
    let page_tree_id = alloc.bump();
    let document_info_id = alloc.bump();
    let page_ids: Vec<_> = svgs.iter().map(|_| alloc.bump()).collect();
    let content_ids: Vec<_> = svgs.iter().map(|_| alloc.bump()).collect();

    let mut embedded = Vec::with_capacity(svgs.len());
    for svg in svgs {
        let tree = parse_svg(svg)?;
        let size = tree.size();
        let (chunk, svg_id) = svg2pdf::to_chunk(&tree, svg2pdf::ConversionOptions::default())
            .map_err(|error| ExportError::Pdf(error.to_string()))?;
        let mut ref_map = HashMap::new();
        let chunk = chunk.renumber(|old| *ref_map.entry(old).or_insert_with(|| alloc.bump()));
        let svg_id = *ref_map
            .get(&svg_id)
            .ok_or_else(|| ExportError::Pdf("could not renumber SVG PDF object".into()))?;
        // usvg reports CSS pixels (96/in), while a PDF MediaBox uses points
        // (72/in). The SVG XObject is normalized and then placed at this size.
        embedded.push((
            chunk,
            svg_id,
            size.width() * 72.0 / 96.0,
            size.height() * 72.0 / 96.0,
        ));
    }

    let mut pdf = Pdf::new();
    pdf.catalog(catalog_id).pages(page_tree_id);
    pdf.document_info(document_info_id)
        .producer(TextStr("plotx svg2pdf"));
    pdf.pages(page_tree_id)
        .kids(page_ids.iter().copied())
        .count(page_ids.len() as i32);

    for (index, ((chunk, svg_id, width, height), (&page_id, &content_id))) in embedded
        .iter()
        .zip(page_ids.iter().zip(&content_ids))
        .enumerate()
    {
        let name = format!("S{}", index + 1);
        let svg_name = Name(name.as_bytes());
        let mut page = pdf.page(page_id);
        page.media_box(Rect::new(0.0, 0.0, *width, *height));
        page.parent(page_tree_id);
        page.contents(content_id);
        let mut resources = page.resources();
        resources.x_objects().pair(svg_name, *svg_id);
        resources.finish();
        page.finish();

        let mut content = Content::new();
        content
            .transform([*width, 0.0, 0.0, *height, 0.0, 0.0])
            .x_object(svg_name);
        pdf.stream(content_id, &content.finish());
        pdf.extend(chunk);
    }

    Ok(pdf.finish())
}

fn parse_svg(svg: &str) -> Result<svg2pdf::usvg::Tree, ExportError> {
    let mut options = svg2pdf::usvg::Options::default();
    fonts::load_system_fonts(options.fontdb_mut());
    svg2pdf::usvg::Tree::from_str(svg, &options)
        .map_err(|error| ExportError::SvgParse(error.to_string()))
}
