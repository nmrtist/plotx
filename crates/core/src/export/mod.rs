mod precheck;
mod preset;
mod raster;

pub use precheck::{
    ComplianceStatus, ComplianceThresholds, PrecheckReport, page_metrics, precheck_report,
};
pub use preset::ExportPreset;
pub use raster::{
    DEFAULT_MAX_RASTER_BYTES, DEFAULT_MAX_RASTER_HEIGHT, DEFAULT_MAX_RASTER_PIXELS,
    DEFAULT_MAX_RASTER_WIDTH, RasterError, RasterImage, RasterLimits, RasterOptions,
    rasterize_canvas, rasterize_svg,
};

use crate::state::{CanvasDocument, render_document_svg};
use image::codecs::jpeg::{JpegEncoder, PixelDensity};
use image::{ExtendedColorType, ImageFormat};
use pdf_writer::{Content, Finish, Name, Pdf, Rect, Ref, TextStr};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use thiserror::Error;

pub const DEFAULT_BITMAP_DPI: u16 = 300;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExportFormat {
    Svg,
    Pdf,
    Png,
    Jpeg,
    Tiff,
}

impl ExportFormat {
    pub fn label(self) -> &'static str {
        match self {
            Self::Svg => "SVG",
            Self::Pdf => "PDF",
            Self::Png => "PNG",
            Self::Jpeg => "JPEG",
            Self::Tiff => "TIFF",
        }
    }

    pub fn extension(self) -> &'static str {
        match self {
            Self::Svg => "svg",
            Self::Pdf => "pdf",
            Self::Png => "png",
            Self::Jpeg => "jpg",
            Self::Tiff => "tif",
        }
    }

    pub fn default_file_name(self) -> &'static str {
        match self {
            Self::Svg => "spectrum.svg",
            Self::Pdf => "spectrum.pdf",
            Self::Png => "spectrum.png",
            Self::Jpeg => "spectrum.jpg",
            Self::Tiff => "spectrum.tif",
        }
    }

    pub fn dialog_title(self) -> String {
        format!("Export figure as {}", self.label())
    }

    pub fn is_bitmap(self) -> bool {
        matches!(self, Self::Png | Self::Jpeg | Self::Tiff)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExportPageScope {
    Current,
    All,
    Range { start: usize, end: usize },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExportScopeKind {
    Current,
    All,
    Range,
}

#[derive(Clone, Debug)]
pub struct ExportDialogState {
    pub format: ExportFormat,
    pub scope: ExportPageScope,
    pub dpi: u16,
    pub preset: Option<ExportPreset>,
}

impl ExportDialogState {
    pub fn new(format: ExportFormat) -> Self {
        Self {
            format,
            scope: ExportPageScope::Current,
            dpi: DEFAULT_BITMAP_DPI,
            preset: None,
        }
    }

    pub fn apply_preset(&mut self, preset: Option<ExportPreset>) {
        self.preset = preset;
        if let Some(preset) = preset {
            self.format = preset.format();
            if preset.format().is_bitmap() {
                self.dpi = preset.dpi();
            }
        }
    }

    pub fn target_width_mm(&self) -> Option<f32> {
        self.preset.and_then(ExportPreset::target_width_mm)
    }

    pub fn scope_kind(&self) -> ExportScopeKind {
        match self.scope {
            ExportPageScope::Current => ExportScopeKind::Current,
            ExportPageScope::All => ExportScopeKind::All,
            ExportPageScope::Range { .. } => ExportScopeKind::Range,
        }
    }

    pub fn set_scope_kind(&mut self, kind: ExportScopeKind, active_page: usize, page_count: usize) {
        self.scope = match kind {
            ExportScopeKind::Current => ExportPageScope::Current,
            ExportScopeKind::All => ExportPageScope::All,
            ExportScopeKind::Range => {
                let max_page = page_count.max(1);
                let page = active_page.saturating_add(1).clamp(1, max_page);
                match self.scope {
                    ExportPageScope::Range { start, end } => ExportPageScope::Range {
                        start: start.clamp(1, max_page),
                        end: end.clamp(1, max_page),
                    },
                    _ => ExportPageScope::Range {
                        start: page,
                        end: page,
                    },
                }
            }
        };
    }
}

#[derive(Clone, Debug)]
pub struct ExportSettings {
    pub format: ExportFormat,
    pub scope: ExportPageScope,
    pub dpi: u16,
    /// When set, each page is scaled (uniformly, preserving aspect ratio) so its
    /// output width equals this many millimetres. `None` keeps the page's size.
    pub target_width_mm: Option<f32>,
}

impl From<&ExportDialogState> for ExportSettings {
    fn from(value: &ExportDialogState) -> Self {
        Self {
            format: value.format,
            scope: value.scope,
            dpi: value.dpi,
            target_width_mm: value.target_width_mm(),
        }
    }
}

#[derive(Debug, Error)]
pub enum ExportError {
    #[error("no pages are available to export")]
    EmptyDocument,
    #[error("current page is no longer available")]
    MissingCurrentPage,
    #[error("page range must be between 1 and {page_count}")]
    InvalidRange { page_count: usize },
    #[error("SVG parse failed: {0}")]
    SvgParse(String),
    #[error("PDF conversion failed: {0}")]
    Pdf(String),
    #[error("image encoding failed: {0}")]
    Image(#[from] image::ImageError),
    #[error("I/O failed: {0}")]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Raster(#[from] RasterError),
}

pub fn resolve_page_scope(
    scope: ExportPageScope,
    active_page: Option<usize>,
    page_count: usize,
) -> Result<Vec<usize>, ExportError> {
    if page_count == 0 {
        return Err(ExportError::EmptyDocument);
    }

    match scope {
        ExportPageScope::Current => active_page
            .filter(|&page| page < page_count)
            .map(|page| vec![page])
            .ok_or(ExportError::MissingCurrentPage),
        ExportPageScope::All => Ok((0..page_count).collect()),
        ExportPageScope::Range { start, end } => {
            if start == 0 || end == 0 || start > end || end > page_count {
                return Err(ExportError::InvalidRange { page_count });
            }
            Ok((start - 1..end).collect())
        }
    }
}

pub fn export_canvases(
    canvases: &[CanvasDocument],
    active_page: Option<usize>,
    settings: &ExportSettings,
    base_path: &Path,
) -> Result<Vec<PathBuf>, ExportError> {
    let pages = resolve_page_scope(settings.scope, active_page, canvases.len())?;
    let target = settings.target_width_mm;
    match settings.format {
        ExportFormat::Svg => export_svg(canvases, &pages, target, base_path),
        ExportFormat::Pdf => export_pdf(canvases, &pages, target, base_path),
        ExportFormat::Png | ExportFormat::Jpeg | ExportFormat::Tiff => export_bitmap(
            canvases,
            &pages,
            settings.format,
            settings.dpi,
            target,
            base_path,
        ),
    }
}

/// The page's SVG with its declared physical size scaled to `target_width_mm`
/// (leaving the `viewBox` — and thus all geometry — untouched, a uniform scale).
/// `None` keeps the page's own size. Reproduces the exact `width/height` header
/// `render_document_svg` emits so the rewrite is a single deterministic replace.
fn document_svg(canvas: &CanvasDocument, target_width_mm: Option<f32>) -> String {
    let svg = render_document_svg(canvas);
    let Some(target) = target_width_mm else {
        return svg;
    };
    let [w, h] = canvas.size_pt();
    let scale = target / canvas.size_mm[0].max(f32::MIN_POSITIVE);
    let from = format!(r#"width="{w}pt" height="{h}pt""#);
    let to = format!(r#"width="{}pt" height="{}pt""#, w * scale, h * scale);
    svg.replacen(&from, &to, 1)
}

pub fn export_output_paths(
    base_path: &Path,
    format: ExportFormat,
    output_count: usize,
) -> Vec<PathBuf> {
    if output_count <= 1 || matches!(format, ExportFormat::Pdf) {
        return vec![with_extension(base_path, format.extension())];
    }

    (1..=output_count)
        .map(|ordinal| numbered_output_path(base_path, ordinal, format.extension()))
        .collect()
}

fn export_svg(
    canvases: &[CanvasDocument],
    pages: &[usize],
    target_width_mm: Option<f32>,
    base_path: &Path,
) -> Result<Vec<PathBuf>, ExportError> {
    let paths = export_output_paths(base_path, ExportFormat::Svg, pages.len());
    for (&page, path) in pages.iter().zip(&paths) {
        std::fs::write(path, document_svg(&canvases[page], target_width_mm))?;
    }
    Ok(paths)
}

fn export_pdf(
    canvases: &[CanvasDocument],
    pages: &[usize],
    target_width_mm: Option<f32>,
    base_path: &Path,
) -> Result<Vec<PathBuf>, ExportError> {
    let path = with_extension(base_path, ExportFormat::Pdf.extension());
    let svgs: Vec<String> = pages
        .iter()
        .map(|&page| document_svg(&canvases[page], target_width_mm))
        .collect();
    let pdf = if svgs.len() == 1 {
        let tree = parse_pdf_svg(&svgs[0])?;
        svg2pdf::to_pdf(
            &tree,
            svg2pdf::ConversionOptions::default(),
            svg2pdf::PageOptions::default(),
        )
        .map_err(|e| ExportError::Pdf(e.to_string()))?
    } else {
        render_multi_page_pdf(&svgs)?
    };
    std::fs::write(&path, pdf)?;
    Ok(vec![path])
}

fn export_bitmap(
    canvases: &[CanvasDocument],
    pages: &[usize],
    format: ExportFormat,
    dpi: u16,
    target_width_mm: Option<f32>,
    base_path: &Path,
) -> Result<Vec<PathBuf>, ExportError> {
    let paths = export_output_paths(base_path, format, pages.len());
    for (&page, path) in pages.iter().zip(&paths) {
        let raster = rasterize_canvas(
            &canvases[page],
            RasterOptions {
                dpi,
                target_width_mm,
                limits: RasterLimits::default(),
            },
        )?;
        match format {
            ExportFormat::Png => {
                image::save_buffer_with_format(
                    path,
                    raster.rgba(),
                    raster.width(),
                    raster.height(),
                    image::ColorType::Rgba8,
                    ImageFormat::Png,
                )?;
            }
            ExportFormat::Tiff => {
                image::save_buffer_with_format(
                    path,
                    raster.rgba(),
                    raster.width(),
                    raster.height(),
                    image::ColorType::Rgba8,
                    ImageFormat::Tiff,
                )?;
            }
            ExportFormat::Jpeg => {
                let rgb = raster.to_rgb_over([255, 255, 255]);
                let file = std::fs::File::create(path)?;
                let mut encoder = JpegEncoder::new_with_quality(file, 90);
                encoder.set_pixel_density(PixelDensity::dpi(dpi));
                encoder.encode(
                    &rgb,
                    raster.width(),
                    raster.height(),
                    ExtendedColorType::Rgb8,
                )?;
            }
            _ => unreachable!("bitmap export called for non-bitmap format"),
        }
    }
    Ok(paths)
}

fn render_multi_page_pdf(svgs: &[String]) -> Result<Vec<u8>, ExportError> {
    let mut alloc = Ref::new(1);
    let catalog_id = alloc.bump();
    let page_tree_id = alloc.bump();
    let document_info_id = alloc.bump();
    let page_ids: Vec<_> = svgs.iter().map(|_| alloc.bump()).collect();
    let content_ids: Vec<_> = svgs.iter().map(|_| alloc.bump()).collect();

    let mut embedded = Vec::with_capacity(svgs.len());
    for svg in svgs {
        let tree = parse_pdf_svg(svg)?;
        let size = tree.size();
        let (chunk, svg_id) = svg2pdf::to_chunk(&tree, svg2pdf::ConversionOptions::default())
            .map_err(|e| ExportError::Pdf(e.to_string()))?;
        let mut ref_map = HashMap::new();
        let chunk = chunk.renumber(|old| *ref_map.entry(old).or_insert_with(|| alloc.bump()));
        let svg_id = *ref_map
            .get(&svg_id)
            .ok_or_else(|| ExportError::Pdf("could not renumber SVG PDF object".into()))?;
        embedded.push((chunk, svg_id, size.width(), size.height()));
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

fn parse_pdf_svg(svg: &str) -> Result<svg2pdf::usvg::Tree, ExportError> {
    let mut options = svg2pdf::usvg::Options::default();
    options.fontdb_mut().load_system_fonts();
    svg2pdf::usvg::Tree::from_str(svg, &options).map_err(|e| ExportError::SvgParse(e.to_string()))
}

fn with_extension(path: &Path, extension: &str) -> PathBuf {
    let mut path = path.to_path_buf();
    path.set_extension(extension);
    path
}

fn numbered_output_path(base_path: &Path, ordinal: usize, extension: &str) -> PathBuf {
    let parent = base_path.parent();
    let stem = base_path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .filter(|stem| !stem.is_empty())
        .unwrap_or("export");
    let file_name = format!("{stem}-{ordinal:03}.{extension}");
    if let Some(parent) = parent {
        parent.join(file_name)
    } else {
        PathBuf::from(file_name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::CanvasDocument;

    fn canvas(name: &str, size_mm: [f32; 2]) -> CanvasDocument {
        CanvasDocument::new(name.to_owned(), size_mm)
    }

    fn test_dir() -> PathBuf {
        std::env::var_os("CARGO_TARGET_TMPDIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| std::env::temp_dir().join("plotx-export-tests"))
    }

    #[test]
    fn svg_export_is_invariant_to_board_pos() {
        let mut c = canvas("page", [80.0, 60.0]);
        let baseline = document_svg(&c, None);
        c.board_pos = [1234.0, -567.0];
        assert_eq!(document_svg(&c, None), baseline);
    }

    #[test]
    fn resolves_page_scopes() {
        assert_eq!(
            resolve_page_scope(ExportPageScope::Current, Some(1), 3).unwrap(),
            vec![1]
        );
        assert_eq!(
            resolve_page_scope(ExportPageScope::All, Some(1), 3).unwrap(),
            vec![0, 1, 2]
        );
        assert_eq!(
            resolve_page_scope(ExportPageScope::Range { start: 2, end: 3 }, Some(0), 4).unwrap(),
            vec![1, 2]
        );
        assert!(
            resolve_page_scope(ExportPageScope::Range { start: 3, end: 2 }, Some(0), 4).is_err()
        );
    }

    #[test]
    fn bitmap_multi_page_paths_are_deterministic() {
        let paths = export_output_paths(Path::new("figure.png"), ExportFormat::Png, 2);
        assert_eq!(
            paths,
            vec![
                PathBuf::from("figure-001.png"),
                PathBuf::from("figure-002.png")
            ]
        );
        let paths = export_output_paths(Path::new("figure.jpeg"), ExportFormat::Jpeg, 2);
        assert_eq!(
            paths,
            vec![
                PathBuf::from("figure-001.jpg"),
                PathBuf::from("figure-002.jpg")
            ]
        );
    }

    #[test]
    fn pdf_single_page_starts_with_pdf_marker() {
        let canvases = vec![canvas("page", [25.4, 25.4])];
        std::fs::create_dir_all(test_dir()).unwrap();
        let out = test_dir().join("single.pdf");
        let paths = export_canvases(
            &canvases,
            Some(0),
            &ExportSettings {
                format: ExportFormat::Pdf,
                scope: ExportPageScope::Current,
                dpi: DEFAULT_BITMAP_DPI,
                target_width_mm: None,
            },
            &out,
        )
        .unwrap();
        let bytes = std::fs::read(&paths[0]).unwrap();
        assert!(bytes.starts_with(b"%PDF"));
    }

    #[test]
    fn pdf_multi_page_contains_selected_page_count() {
        let canvases = vec![canvas("one", [25.4, 25.4]), canvas("two", [25.4, 25.4])];
        std::fs::create_dir_all(test_dir()).unwrap();
        let out = test_dir().join("multi.pdf");
        let paths = export_canvases(
            &canvases,
            Some(0),
            &ExportSettings {
                format: ExportFormat::Pdf,
                scope: ExportPageScope::All,
                dpi: DEFAULT_BITMAP_DPI,
                target_width_mm: None,
            },
            &out,
        )
        .unwrap();
        let bytes = std::fs::read(&paths[0]).unwrap();
        let text = String::from_utf8_lossy(&bytes);
        assert_eq!(text.matches("/Type /Page\n").count(), 2);
    }

    #[test]
    fn bitmaps_decode_with_expected_pixel_dimensions() {
        let canvases = vec![canvas("page", [25.4, 12.7])];
        let dir = test_dir();
        std::fs::create_dir_all(&dir).unwrap();
        for format in [ExportFormat::Png, ExportFormat::Jpeg, ExportFormat::Tiff] {
            let out = dir.join(format!("bitmap.{}", format.extension()));
            let paths = export_canvases(
                &canvases,
                Some(0),
                &ExportSettings {
                    format,
                    scope: ExportPageScope::Current,
                    dpi: DEFAULT_BITMAP_DPI,
                    target_width_mm: None,
                },
                &out,
            )
            .unwrap();
            let image = image::open(&paths[0]).unwrap();
            assert_eq!((image.width(), image.height()), (300, 150));
        }
    }

    #[test]
    fn preset_target_width_drives_pixel_dimensions() {
        // 200 mm wide page, downscaled to an 89 mm column at 600 dpi:
        // width_px = 89/25.4 * 600 ≈ 2102, height preserves the 2:1 aspect ratio.
        let canvases = vec![canvas("page", [200.0, 100.0])];
        let dir = test_dir();
        std::fs::create_dir_all(&dir).unwrap();
        let out = dir.join("column.tif");
        let paths = export_canvases(
            &canvases,
            Some(0),
            &ExportSettings {
                format: ExportFormat::Tiff,
                scope: ExportPageScope::Current,
                dpi: 600,
                target_width_mm: Some(89.0),
            },
            &out,
        )
        .unwrap();
        let image = image::open(&paths[0]).unwrap();
        assert_eq!((image.width(), image.height()), (2102, 1051));
    }

    #[test]
    fn preset_scales_vector_physical_size() {
        let doc = canvas("page", [200.0, 100.0]);
        let [w, h] = doc.size_pt();
        let natural = document_svg(&doc, None);
        assert!(natural.contains(&format!(r#"width="{w}pt" height="{h}pt""#)));

        // Half-width target halves the declared physical size; the viewBox — and
        // hence all geometry — is untouched.
        let scaled = document_svg(&doc, Some(100.0));
        assert!(scaled.contains(&format!(r#"width="{}pt" height="{}pt""#, w * 0.5, h * 0.5)));
        assert!(scaled.contains(&format!(r#"viewBox="0 0 {w} {h}""#)));
    }
}
