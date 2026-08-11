mod asset_render;
mod fonts;
mod pdf;
mod precheck;
mod preset;
mod raster;
#[cfg(test)]
mod state_tests;
mod trim;

pub use asset_render::{MissingImagePolicy, prepare_render_document};
pub use precheck::{
    ComplianceStatus, ComplianceThresholds, PrecheckReport, image_precheck_items, page_metrics,
    precheck_report,
};
pub use preset::ExportPreset;
pub use raster::{
    DEFAULT_MAX_RASTER_BYTES, DEFAULT_MAX_RASTER_HEIGHT, DEFAULT_MAX_RASTER_PIXELS,
    DEFAULT_MAX_RASTER_WIDTH, RasterError, RasterImage, RasterLimits, RasterOptions,
    rasterize_canvas, rasterize_canvas_with_assets, rasterize_svg,
};

use crate::state::{AssetId, AssetRecord, CanvasDocument};
use image::codecs::jpeg::{JpegEncoder, PixelDensity};
use image::{ExtendedColorType, ImageFormat};
use std::collections::BTreeMap;
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
    pub trim_to_visible_content: bool,
    pub allow_missing_images: bool,
}

impl ExportDialogState {
    pub fn new(format: ExportFormat) -> Self {
        Self {
            format,
            scope: ExportPageScope::Current,
            dpi: DEFAULT_BITMAP_DPI,
            preset: None,
            trim_to_visible_content: false,
            allow_missing_images: false,
        }
    }

    pub fn from_defaults(format: ExportFormat, defaults: &crate::settings::ExportDefaults) -> Self {
        let mut state = Self::new(format);
        state.dpi = defaults.dpi;
        state.trim_to_visible_content = defaults.trim_to_visible_content;
        state
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
    pub target_width_mm: Option<f32>,
    pub trim_to_visible_content: bool,
    pub allow_missing_images: bool,
}

impl From<&ExportDialogState> for ExportSettings {
    fn from(value: &ExportDialogState) -> Self {
        Self {
            format: value.format,
            scope: value.scope,
            dpi: value.dpi,
            target_width_mm: value.target_width_mm(),
            trim_to_visible_content: value.trim_to_visible_content,
            allow_missing_images: value.allow_missing_images,
        }
    }
}

#[derive(Debug, Error)]
pub enum ExportError {
    #[error("no pages are available to export")]
    EmptyDocument,
    #[error("current page is no longer available")]
    MissingCurrentPage,
    #[error("image asset {asset} is missing; replace it or allow placeholder export")]
    MissingImageAsset { asset: crate::state::AssetId },
    #[error("image asset {asset} is damaged: {reason}")]
    CorruptImageAsset {
        asset: crate::state::AssetId,
        reason: String,
    },
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
    export_canvases_with_assets(canvases, &BTreeMap::new(), active_page, settings, base_path)
}

pub fn export_canvases_with_assets(
    canvases: &[CanvasDocument],
    assets: &BTreeMap<AssetId, AssetRecord>,
    active_page: Option<usize>,
    settings: &ExportSettings,
    base_path: &Path,
) -> Result<Vec<PathBuf>, ExportError> {
    let pages = resolve_page_scope(settings.scope, active_page, canvases.len())?;
    let target = settings.target_width_mm;
    let missing_policy = if settings.allow_missing_images {
        MissingImagePolicy::Placeholder
    } else {
        MissingImagePolicy::Block
    };
    match settings.format {
        ExportFormat::Svg => export_svg(
            canvases,
            assets,
            &pages,
            target,
            settings.trim_to_visible_content,
            missing_policy,
            base_path,
        ),
        ExportFormat::Pdf => export_pdf(
            canvases,
            assets,
            &pages,
            target,
            settings.trim_to_visible_content,
            missing_policy,
            base_path,
        ),
        ExportFormat::Png | ExportFormat::Jpeg | ExportFormat::Tiff => export_bitmap(
            canvases,
            assets,
            &pages,
            settings,
            missing_policy,
            base_path,
        ),
    }
}

fn document_svg_with_assets(
    canvas: &CanvasDocument,
    assets: &BTreeMap<AssetId, AssetRecord>,
    target_width_mm: Option<f32>,
    missing_policy: MissingImagePolicy,
) -> Result<String, ExportError> {
    let document = prepare_render_document(canvas, assets, missing_policy)?;
    let svg = plotx_render::svg::export_document(&document);
    let Some(target) = target_width_mm else {
        return Ok(svg);
    };
    let [w, h] = canvas.size_pt();
    let scale = target / canvas.size_mm[0].max(f32::MIN_POSITIVE);
    let from = format!(r#"width="{w}pt" height="{h}pt""#);
    let to = format!(r#"width="{}pt" height="{}pt""#, w * scale, h * scale);
    Ok(svg.replacen(&from, &to, 1))
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
    assets: &BTreeMap<AssetId, AssetRecord>,
    pages: &[usize],
    target_width_mm: Option<f32>,
    trim_to_visible_content: bool,
    missing_policy: MissingImagePolicy,
    base_path: &Path,
) -> Result<Vec<PathBuf>, ExportError> {
    let paths = export_output_paths(base_path, ExportFormat::Svg, pages.len());
    for (&page, path) in pages.iter().zip(&paths) {
        let svg = if trim_to_visible_content {
            trim::trim_document_svg_with_assets(
                &canvases[page],
                assets,
                target_width_mm,
                missing_policy,
            )?
        } else {
            document_svg_with_assets(&canvases[page], assets, target_width_mm, missing_policy)?
        };
        std::fs::write(path, svg)?;
    }
    Ok(paths)
}

fn export_pdf(
    canvases: &[CanvasDocument],
    assets: &BTreeMap<AssetId, AssetRecord>,
    pages: &[usize],
    target_width_mm: Option<f32>,
    trim_to_visible_content: bool,
    missing_policy: MissingImagePolicy,
    base_path: &Path,
) -> Result<Vec<PathBuf>, ExportError> {
    let path = with_extension(base_path, ExportFormat::Pdf.extension());
    let svgs: Vec<String> = pages
        .iter()
        .map(|&page| {
            if trim_to_visible_content {
                trim::trim_document_svg_with_assets(
                    &canvases[page],
                    assets,
                    target_width_mm,
                    missing_policy,
                )
            } else {
                document_svg_with_assets(&canvases[page], assets, target_width_mm, missing_policy)
            }
        })
        .collect::<Result<_, ExportError>>()?;
    let pdf = pdf::render(&svgs)?;
    std::fs::write(&path, pdf)?;
    Ok(vec![path])
}

fn export_bitmap(
    canvases: &[CanvasDocument],
    assets: &BTreeMap<AssetId, AssetRecord>,
    pages: &[usize],
    settings: &ExportSettings,
    missing_policy: MissingImagePolicy,
    base_path: &Path,
) -> Result<Vec<PathBuf>, ExportError> {
    let paths = export_output_paths(base_path, settings.format, pages.len());
    for (&page, path) in pages.iter().zip(&paths) {
        let raster = rasterize_canvas_with_assets(
            &canvases[page],
            assets,
            RasterOptions {
                dpi: settings.dpi,
                target_width_mm: settings.target_width_mm,
                limits: RasterLimits::default(),
            },
            missing_policy,
        )?;
        let raster = if settings.trim_to_visible_content {
            let background = canvases[page].background;
            trim::crop_raster(
                raster,
                [background.r, background.g, background.b, 255],
                trim::raster_trim_padding(settings.dpi),
            )?
        } else {
            raster
        };
        match settings.format {
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
                encoder.set_pixel_density(PixelDensity::dpi(settings.dpi));
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
    use crate::state::{
        CanvasDocument, CanvasObject, CanvasObjectKind, ObjectFrame, ObjectId, ShapeKind,
        ShapeObject,
    };

    fn canvas(name: &str, size_mm: [f32; 2]) -> CanvasDocument {
        CanvasDocument::new(name.to_owned(), size_mm)
    }

    fn document_svg(canvas: &CanvasDocument, target_width_mm: Option<f32>) -> String {
        document_svg_with_assets(
            canvas,
            &BTreeMap::new(),
            target_width_mm,
            MissingImagePolicy::Block,
        )
        .expect("a document without image assets renders")
    }

    fn test_dir() -> PathBuf {
        std::env::var_os("CARGO_TARGET_TMPDIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| std::env::temp_dir().join("plotx-export-tests"))
    }

    fn canvas_with_shape(frame: ObjectFrame) -> CanvasDocument {
        let mut canvas = canvas("page", [100.0, 80.0]);
        canvas.objects.push(CanvasObject {
            id: ObjectId::new(1),
            name: "shape".into(),
            frame,
            locked: false,
            visible: true,
            kind: CanvasObjectKind::Shape(ShapeObject::new(ShapeKind::Rect)),
        });
        canvas
    }

    #[test]
    fn svg_export_is_invariant_to_board_pos() {
        let mut c = canvas("page", [80.0, 60.0]);
        let baseline = document_svg(&c, None);
        c.board_pos = [1234.0, -567.0];
        assert_eq!(document_svg(&c, None), baseline);
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
                trim_to_visible_content: false,
                allow_missing_images: false,
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
                trim_to_visible_content: false,
                allow_missing_images: false,
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
                    trim_to_visible_content: false,
                    allow_missing_images: false,
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
                trim_to_visible_content: false,
                allow_missing_images: false,
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

    #[test]
    fn trimmed_svg_uses_painted_bounds_and_keeps_page_background() {
        let doc = canvas_with_shape(ObjectFrame::new(100.0, 80.0, 40.0, 30.0));
        let svg = trim::trim_document_svg(&doc, None).unwrap();
        let [page_width, page_height] = doc.size_pt();
        assert!(svg.contains("<rect x=\""));
        assert!(!svg.contains(&format!("viewBox=\"0 0 {page_width} {page_height}\"")));
        assert!(svg.contains("viewBox=\""));
    }

    fn svg_number(svg: &str, attribute: &str) -> f32 {
        let start = svg.find(attribute).unwrap() + attribute.len();
        let end = svg[start..].find('"').unwrap() + start;
        svg[start..end].trim_end_matches("pt").parse().unwrap()
    }

    fn pdf_media_boxes(bytes: &[u8]) -> Vec<[f32; 2]> {
        let text = String::from_utf8_lossy(bytes);
        text.match_indices("/MediaBox [")
            .filter_map(|(start, _)| {
                let values = text[start + 11..].split_once(']')?.0;
                let numbers = values
                    .split_whitespace()
                    .map(str::parse::<f32>)
                    .collect::<Result<Vec<_>, _>>()
                    .ok()?;
                (numbers.len() == 4).then_some([numbers[2], numbers[3]])
            })
            .collect()
    }

    #[test]
    fn preset_scale_precedes_trim_without_refitting_and_padding_is_physical_point() {
        let doc = canvas_with_shape(ObjectFrame::new(100.0, 80.0, 40.0, 30.0));
        let svg = trim::trim_document_svg(&doc, Some(50.0)).unwrap();
        let width_pt = svg_number(&svg, "width=\"");
        let preset_width_pt = 50.0 * 72.0 / 25.4;
        assert!(width_pt < preset_width_pt);

        let view_box = svg
            .split_once("viewBox=\"")
            .unwrap()
            .1
            .split_once('"')
            .unwrap()
            .0
            .split_whitespace()
            .map(|value| value.parse::<f32>().unwrap())
            .collect::<Vec<_>>();
        let authored_scale = 0.5;
        assert!((width_pt - view_box[2] * authored_scale).abs() < 0.01);
        let bounds_svg = plotx_render::svg::export_document_for_bounds(
            &crate::state::build_render_document(&doc),
        );
        let mut options = resvg::usvg::Options::default();
        fonts::load_system_fonts(options.fontdb_mut());
        let tree = resvg::usvg::Tree::from_str(&bounds_svg, &options).unwrap();
        let painted = tree.root().abs_stroke_bounding_box();
        let painted_x = painted.x() * doc.size_pt()[0] / tree.size().width();
        // Two authored points at 0.5 scale are one physical point.
        assert!(((painted_x - view_box[0]) * authored_scale - 1.0).abs() < 0.01);
    }

    #[test]
    fn empty_trimmed_svg_keeps_original_page() {
        let doc = canvas("empty", [100.0, 80.0]);
        assert_eq!(
            trim::trim_document_svg(&doc, None).unwrap(),
            document_svg(&doc, None)
        );
    }

    #[test]
    fn bitmap_formats_encode_the_shared_trimmed_dimensions() {
        let canvases = vec![canvas_with_shape(ObjectFrame::new(100.0, 80.0, 40.0, 30.0))];
        let dir = test_dir();
        std::fs::create_dir_all(&dir).unwrap();
        let mut dimensions = Vec::new();
        for format in [ExportFormat::Png, ExportFormat::Jpeg, ExportFormat::Tiff] {
            let out = dir.join(format!("trimmed.{}", format.extension()));
            let paths = export_canvases(
                &canvases,
                Some(0),
                &ExportSettings {
                    format,
                    scope: ExportPageScope::Current,
                    dpi: 72,
                    target_width_mm: None,
                    trim_to_visible_content: true,
                    allow_missing_images: false,
                },
                &out,
            )
            .unwrap();
            let image = image::open(&paths[0]).unwrap();
            dimensions.push((image.width(), image.height()));
            assert!(image.width() < 284 && image.height() < 227);
        }
        assert!(dimensions.windows(2).all(|pair| pair[0] == pair[1]));
    }

    #[test]
    fn pdf_media_boxes_follow_each_trimmed_svg_page_and_empty_page_stays_full_size() {
        let shaped = canvas_with_shape(ObjectFrame::new(100.0, 80.0, 40.0, 30.0));
        let mut wider = canvas_with_shape(ObjectFrame::new(60.0, 50.0, 100.0, 35.0));
        wider.name = "wider".into();
        let empty = canvas("empty", [100.0, 80.0]);
        let canvases = vec![shaped, wider, empty];
        let dir = test_dir();
        std::fs::create_dir_all(&dir).unwrap();
        let out = dir.join("trimmed-pages.pdf");
        let paths = export_canvases(
            &canvases,
            Some(0),
            &ExportSettings {
                format: ExportFormat::Pdf,
                scope: ExportPageScope::All,
                dpi: DEFAULT_BITMAP_DPI,
                target_width_mm: None,
                trim_to_visible_content: true,
                allow_missing_images: false,
            },
            &out,
        )
        .unwrap();
        let boxes = pdf_media_boxes(&std::fs::read(&paths[0]).unwrap());
        assert_eq!(boxes.len(), 3);
        assert_ne!(boxes[0], boxes[1]);
        let [page_width, page_height] = canvases[2].size_pt();
        assert!((boxes[2][0] - page_width).abs() < 0.01);
        assert!((boxes[2][1] - page_height).abs() < 0.01);
    }
}
