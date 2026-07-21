use super::{ComplianceThresholds, DEFAULT_BITMAP_DPI, ExportFormat};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExportPreset {
    SingleColumnTiff,
    DoubleColumnTiff,
    SingleColumnPng,
    VectorPdf,
    VectorSvg,
}

impl ExportPreset {
    pub fn all() -> &'static [Self] {
        &[
            Self::SingleColumnTiff,
            Self::DoubleColumnTiff,
            Self::SingleColumnPng,
            Self::VectorPdf,
            Self::VectorSvg,
        ]
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::SingleColumnTiff => "Single column · 89 mm · 600 dpi · TIFF",
            Self::DoubleColumnTiff => "Double column · 183 mm · 600 dpi · TIFF",
            Self::SingleColumnPng => "Single column · 89 mm · 300 dpi · PNG",
            Self::VectorPdf => "Vector · PDF (scalable)",
            Self::VectorSvg => "Vector · SVG (scalable)",
        }
    }

    pub fn format(self) -> ExportFormat {
        match self {
            Self::SingleColumnTiff | Self::DoubleColumnTiff => ExportFormat::Tiff,
            Self::SingleColumnPng => ExportFormat::Png,
            Self::VectorPdf => ExportFormat::Pdf,
            Self::VectorSvg => ExportFormat::Svg,
        }
    }

    /// Target physical page width in mm, or `None` to keep the page's own size
    /// (the vector presets are scalable, so they impose no fixed width).
    /// Widths come from the canvas-size preset catalog so the export scaler and
    /// the page presets can never drift apart.
    pub fn target_width_mm(self) -> Option<f32> {
        match self {
            Self::SingleColumnTiff | Self::SingleColumnPng => {
                Some(crate::state::NATURE_SINGLE_COLUMN.width_mm)
            }
            Self::DoubleColumnTiff => Some(crate::state::NATURE_DOUBLE_COLUMN.width_mm),
            Self::VectorPdf | Self::VectorSvg => None,
        }
    }

    /// The export preset matching a canvas's current size in the requested
    /// format, used to pre-select the export dialog: a page authored at a
    /// journal column width defaults to exporting at that width.
    pub fn matching_canvas(
        format: ExportFormat,
        size_mm: [f32; 2],
        preset_id: Option<&str>,
    ) -> Option<Self> {
        let width = crate::state::matching_preset(size_mm, preset_id)?.width_mm;
        Self::all().iter().copied().find(|preset| {
            preset.format() == format
                && preset
                    .target_width_mm()
                    .is_some_and(|w| (w - width).abs() < 0.01)
        })
    }

    pub fn dpi(self) -> u16 {
        match self {
            Self::SingleColumnTiff | Self::DoubleColumnTiff => 600,
            Self::SingleColumnPng => 300,
            Self::VectorPdf | Self::VectorSvg => DEFAULT_BITMAP_DPI,
        }
    }

    pub fn thresholds(self) -> ComplianceThresholds {
        ComplianceThresholds {
            min_font_pt: 7.0,
            min_line_pt: 0.5,
        }
    }
}
