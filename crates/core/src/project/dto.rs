use super::axis_overrides::AxisOverridesDto;
use super::*;

#[derive(Serialize, Deserialize)]
pub struct Manifest {
    pub format: String,
    pub schema_version: u32,
    pub app_version: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revision: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recovery: Option<RecoveryMetadata>,
    pub save_profile: SaveProfile,
    pub objects: Vec<Entry>,
    pub views: Vec<Entry>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub runs: Vec<Entry>,
    pub workspace: String,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct RecoveryMetadata {
    pub original_path: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_revision: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_file: Option<FileStamp>,
}

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileStamp {
    pub modified_nanos: u128,
    pub len: u64,
}

#[derive(Serialize, Deserialize)]
pub struct SaveProfile {
    pub include_view_snapshots: bool,
    pub snapshot_kind: Option<String>,
}

#[derive(Serialize, Deserialize)]
pub struct Entry {
    pub id: String,
    pub role: String,
    pub path: String,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct Classification {
    pub domain: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub technique: Option<String>,
    pub object: String,
}

#[derive(Serialize, Deserialize)]
pub struct DataObject {
    pub id: String,
    pub role: String,
    pub classification: Classification,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    pub dimensions: Vec<Dimension>,
    pub payload: Payload,
    pub extensions: serde_json::Value,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct Dimension {
    pub id: String,
    pub role: String,
    pub size: usize,
    pub storage_axis: usize,
    pub quantity: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_quantity: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unit: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub nucleus: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub spectral_width_hz: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub observe_freq_mhz: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub carrier_ppm: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub group_delay: Option<f64>,
}

#[derive(Serialize, Deserialize)]
pub struct Payload {
    pub storage: String,
    pub blob: String,
    pub shape: Vec<usize>,
    pub domain: String,
}

#[derive(Serialize, Deserialize)]
pub struct RecipeObject {
    pub id: String,
    pub role: String,
    pub classification: Classification,
    pub input: String,
    pub parameters: RecipeParameters,
    #[serde(default)]
    pub extensions: serde_json::Value,
}

#[derive(Serialize, Deserialize)]
pub struct RecipeParameters {
    pub dimension_count: usize,
    /// One ordered step list per dimension (`[direct]` for 1D, `[f2, f1]` for 2D).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pipelines: Vec<AxisPipelineDto>,
    #[serde(default = "bool_true", skip_serializing_if = "is_true")]
    pub group_delay_correct: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub layout: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preset: Option<String>,
}

impl Default for RecipeParameters {
    fn default() -> Self {
        Self {
            dimension_count: 0,
            pipelines: Vec::new(),
            group_delay_correct: true,
            layout: None,
            preset: None,
        }
    }
}

/// Same representation as the standalone `*.plotxproc` scheme file.
#[derive(Serialize, Deserialize, Clone)]
pub struct AxisPipelineDto {
    pub steps: Vec<ProcessingStepDto>,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct ProcessingStepDto {
    /// Present in a project archive, where step identities must round-trip.
    /// Absent in a `.plotxproc` recipe: a detached recipe carries no identity —
    /// the adopting dataset remints every step from its own allocator — so
    /// requiring one would make hand-written recipes spell out a field that is
    /// immediately discarded.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<u64>,
    pub kind: StepKindDto,
    pub enabled: bool,
    pub source: StepSourceDto,
}

#[derive(Serialize, Deserialize, Clone)]
pub enum StepKindDto {
    Apodize(ApodizationDto),
    ZeroFill(String),
    Fft,
    Phase(PhaseParamsDto),
    Baseline(BaselineMethodDto),
    Reference(ReferenceParamsDto),
    Magnitude,
    Smooth(SmoothMethodDto),
    Normalize(NormalizeMethodDto),
    Bin { width: f64, method: BinMethodDto },
    Reverse,
    Invert,
}

#[derive(Serialize, Deserialize, Clone, Copy)]
pub enum SmoothMethodDto {
    MovingAverage { window: u16 },
    SavitzkyGolay { window: u16, poly_order: u8 },
}

#[derive(Serialize, Deserialize, Clone, Copy)]
pub enum NormalizeMethodDto {
    MaxPeak,
    TotalArea,
    Constant { divisor: f64 },
}

#[derive(Serialize, Deserialize, Clone, Copy)]
pub enum BinMethodDto {
    Sum,
    Mean,
}

#[derive(Serialize, Deserialize, Clone)]
pub enum ApodizationDto {
    None,
    CosineBell,
    Exponential { lb_hz: f64 },
    Gaussian { lb_hz: f64, gb_hz: f64 },
}

#[derive(Serialize, Deserialize, Clone)]
pub struct PhaseParamsDto {
    pub phase0: f64,
    pub phase1: f64,
    pub pivot_frac: f64,
    pub auto: Option<AutoPhaseMethodDto>,
}

#[derive(Serialize, Deserialize, Clone, Copy)]
pub enum AutoPhaseMethodDto {
    RobustConsensus,
    AbsorptivePeak,
    Entropy,
    NegativeMinimization,
    PeakRegression,
}

#[derive(Serialize, Deserialize, Clone)]
pub enum BaselineMethodDto {
    Offset,
    Polynomial {
        order: u8,
    },
    AsymmetricLeastSquares {
        smoothness: f64,
        asymmetry: f64,
        iterations: u16,
    },
}

#[derive(Serialize, Deserialize, Clone, Copy)]
pub struct ReferenceParamsDto {
    pub at_ppm: f64,
    pub target_ppm: f64,
}

#[derive(Serialize, Deserialize, Clone, Copy)]
pub enum StepSourceDto {
    Default,
    User,
    Imported,
}

/// Serialized in the 2D data extension so a save/load round-trip keeps a dataset
/// a `PseudoNmr` rather than degrading it to a plain 2D.
#[derive(Serialize, Deserialize, Clone)]
pub struct PseudoAxisDto {
    pub name: String,
    pub kind: String,
    pub values: Vec<f64>,
    pub unit: String,
    pub source: String,
}

/// Serialized alongside the pseudo axis so the Stejskal–Tanner b-factor survives
/// a round-trip.
#[derive(Serialize, Deserialize, Clone, Copy)]
pub struct DiffusionMetaDto {
    pub gamma: f64,
    pub delta: f64,
    pub big_delta: f64,
    pub tau: f64,
    pub shape_factor: f64,
}

#[derive(Serialize, Deserialize)]
pub struct ViewObject {
    pub id: String,
    pub role: String,
    pub classification: Classification,
    #[serde(default)]
    pub inputs: Vec<String>,
    pub name: String,
    pub next_object_id: u64,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub caption: String,
    #[serde(default = "caption_visible_default")]
    pub caption_visible: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub panel_label_style: Option<String>,
    pub layout: ViewLayout,
    #[serde(default)]
    pub objects: Vec<ViewCanvasObject>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub viewport: Option<ViewportDto>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub snapshot: Option<ViewSnapshot>,
}

#[derive(Serialize, Deserialize)]
pub struct ViewLayout {
    pub size_mm: [f32; 2],
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size_preset: Option<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub auto_height: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub grid: Option<PageLayoutDto>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub background: Option<[u8; 3]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub board_pos: Option<[f32; 2]>,
}

#[derive(Serialize, Deserialize, Clone, Copy)]
pub struct PageLayoutDto {
    pub margin_mm: [f32; 4],
    pub gutter_mm: f32,
    pub rows: u32,
    pub cols: u32,
    #[serde(default)]
    pub show_grid: bool,
    #[serde(default)]
    pub spacing_mode: crate::layout::SpacingMode,
}

impl PageLayoutDto {
    pub fn from_layout(l: &PageLayout) -> Self {
        Self {
            margin_mm: l.margin_mm,
            gutter_mm: l.gutter_mm,
            rows: l.rows,
            cols: l.cols,
            show_grid: l.show_grid,
            spacing_mode: l.spacing_mode,
        }
    }

    pub fn into_layout(self) -> PageLayout {
        PageLayout {
            margin_mm: self.margin_mm,
            gutter_mm: self.gutter_mm,
            rows: self.rows.max(1),
            cols: self.cols.max(1),
            show_grid: self.show_grid,
            spacing_mode: self.spacing_mode,
        }
    }
}

#[cfg(test)]
#[path = "dto_tests.rs"]
mod tests;

#[derive(Serialize, Deserialize)]
pub struct ViewCanvasObject {
    pub id: String,
    pub name: String,
    pub kind: String,
    #[serde(default)]
    pub input: String,
    /// Plot-only allocator high-water mark, derivable from `series` via
    /// `PlotObject::repair_series_allocator` (which load always runs). Omitting
    /// it when zero keeps it out of text/shape/label objects, where it is noise.
    #[serde(default, skip_serializing_if = "is_zero_u64")]
    pub next_series_id: u64,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub series: Vec<SeriesBindingDto>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chart_type: Option<String>,
    /// Stable column UUID for column-oriented charts; absent selects the first.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chart_column: Option<String>,
    /// Histogram bin count; absent = automatic binning.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chart_bins: Option<usize>,
    /// Multi-column bar charts: stacked instead of grouped.
    #[serde(default, skip_serializing_if = "is_false")]
    pub chart_stacked: bool,
    /// Colormap id for value-mapped charts; absent = default. Kept as a string
    /// so files from newer builds with unknown maps still open.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chart_colormap: Option<String>,
    /// 3D surface `[azimuth°, elevation°]`; absent = default view.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chart_view: Option<[f32; 2]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stack: Option<StackDto>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub projections: Option<ProjectionsDto>,
    pub frame: FrameDto,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub viewport: Option<ViewportDto>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub axis_overrides: Option<AxisOverridesDto>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub panel: Option<PanelDto>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<PanelDto>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<TextBoxDto>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shape: Option<ShapeDto>,
    pub locked: bool,
    pub visible: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub snapshot: Option<ViewSnapshot>,
}

fn is_false(b: &bool) -> bool {
    !*b
}

fn is_zero(n: &usize) -> bool {
    *n == 0
}

fn is_zero_u64(n: &u64) -> bool {
    *n == 0
}

fn caption_visible_default() -> bool {
    true
}

#[derive(Serialize, Deserialize, Clone)]
pub struct SeriesBindingDto {
    /// Owner-local series identity. Optional on read: a series list written
    /// without ids falls back to positional numbering, which keeps the entries
    /// distinct — a plain zero default would collapse every series onto id 0.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<u64>,
    pub input: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<[u8; 3]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// Multiplier applied before stacking; defaults to 1.0.
    #[serde(default = "one_f64", skip_serializing_if = "is_one_f64")]
    pub scale: f64,
    #[serde(default = "bool_true", skip_serializing_if = "is_true")]
    pub visible: bool,
}

fn one_f64() -> f64 {
    1.0
}

fn is_one_f64(v: &f64) -> bool {
    (*v - 1.0).abs() < f64::EPSILON
}

fn bool_true() -> bool {
    true
}

fn is_true(v: &bool) -> bool {
    *v
}

#[derive(Serialize, Deserialize, Clone)]
pub struct StackDto {
    pub mode: String,
    pub spacing_y: f64,
    pub shear_x: f64,
    pub normalize: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active: Option<usize>,
}

impl StackDto {
    pub fn from_spec(s: &StackSpec) -> Self {
        Self {
            mode: match s.mode {
                StackMode::Superimposed => "superimposed",
                StackMode::Offset => "offset",
                StackMode::ColorOverlay => "color_overlay",
            }
            .to_owned(),
            spacing_y: s.spacing_y,
            shear_x: s.shear_x,
            normalize: s.normalize,
            active: s.active,
        }
    }

    pub fn into_spec(self) -> StackSpec {
        StackSpec {
            mode: match self.mode.as_str() {
                "offset" => StackMode::Offset,
                "color_overlay" => StackMode::ColorOverlay,
                _ => StackMode::Superimposed,
            },
            spacing_y: self.spacing_y,
            shear_x: self.shear_x,
            normalize: self.normalize,
            active: self.active,
        }
    }
}

/// An `Attached` source persists the referenced dataset as a recipe id (the same
/// id scheme as series bindings), so it re-resolves on load.
#[derive(Serialize, Deserialize, Clone)]
pub struct ProjectionsDto {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub top: Option<AxisProjectionDto>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub left: Option<AxisProjectionDto>,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct AxisProjectionDto {
    /// One of "attached", "sum", "skyline", "slice".
    pub source: String,
    /// Recipe id of the attached 1D dataset, when `source == "attached"`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attached: Option<String>,
    /// Pinned grid index, when `source == "slice"`.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub slice_index: usize,
    #[serde(default = "bool_true", skip_serializing_if = "is_true")]
    pub visible: bool,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct TextBoxDto {
    pub text: String,
    pub font_size: f32,
    pub color: [u8; 3],
    pub align: String,
    pub bold: bool,
}

impl TextBoxDto {
    pub fn from_text_box(t: &TextBox) -> Self {
        Self {
            text: t.text.clone(),
            font_size: t.font_size,
            color: [t.color.r, t.color.g, t.color.b],
            align: match t.align {
                TextAlign::Left => "left",
                TextAlign::Center => "center",
                TextAlign::Right => "right",
            }
            .to_owned(),
            bold: t.bold,
        }
    }

    pub fn into_text_box(self) -> TextBox {
        TextBox {
            text: self.text,
            font_size: self.font_size,
            color: Color::rgb(self.color[0], self.color[1], self.color[2]),
            align: match self.align.as_str() {
                "center" => TextAlign::Center,
                "right" => TextAlign::Right,
                _ => TextAlign::Left,
            },
            bold: self.bold,
        }
    }
}

#[derive(Serialize, Deserialize, Clone)]
pub struct ShapeDto {
    pub shape: String,
    pub stroke: [u8; 3],
    pub stroke_width: f32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fill: Option<[u8; 3]>,
}

impl ShapeDto {
    pub fn from_shape(s: &ShapeObject) -> Self {
        Self {
            shape: match s.shape {
                ShapeKind::Rect => "rect",
                ShapeKind::Ellipse => "ellipse",
                ShapeKind::Line => "line",
                ShapeKind::Arrow => "arrow",
            }
            .to_owned(),
            stroke: [s.stroke.r, s.stroke.g, s.stroke.b],
            stroke_width: s.stroke_width,
            fill: s.fill.map(|c| [c.r, c.g, c.b]),
        }
    }

    pub fn into_shape(self) -> ShapeObject {
        ShapeObject {
            shape: match self.shape.as_str() {
                "ellipse" => ShapeKind::Ellipse,
                "line" => ShapeKind::Line,
                "arrow" => ShapeKind::Arrow,
                _ => ShapeKind::Rect,
            },
            stroke: Color::rgb(self.stroke[0], self.stroke[1], self.stroke[2]),
            stroke_width: self.stroke_width,
            fill: self.fill.map(|c| Color::rgb(c[0], c[1], c[2])),
        }
    }
}

#[derive(Serialize, Deserialize, Clone)]
pub struct PanelDto {
    #[serde(default, alias = "text")]
    pub note: String,
    #[serde(default = "default_panel_label_position", alias = "position")]
    pub label_position: [f32; 2],
    #[serde(default = "default_panel_label_font_size", alias = "font_size")]
    pub label_font_size: f32,
    #[serde(default = "bool_true", alias = "visible")]
    pub label_visible: bool,
}

impl PanelDto {
    pub fn from_panel(panel: &PanelMeta) -> Self {
        Self {
            note: panel.user_note.clone(),
            label_position: panel.position,
            label_font_size: panel.font_size,
            label_visible: panel.visible,
        }
    }

    pub fn into_panel(self) -> PanelMeta {
        PanelMeta {
            user_note: self.note,
            position: self.label_position,
            font_size: self.label_font_size,
            visible: self.label_visible,
        }
    }
}

fn default_panel_label_position() -> [f32; 2] {
    [6.0, 5.0]
}

fn default_panel_label_font_size() -> f32 {
    8.0
}

#[derive(Serialize, Deserialize, Clone, Copy)]
pub struct FrameDto {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

#[derive(Serialize, Deserialize)]
pub struct ViewSnapshot {
    pub kind: String,
    pub schema_version: u32,
    pub figure: String,
}

#[derive(Serialize, Deserialize)]
pub struct Workspace {
    pub dataset_order: Vec<DatasetBinding>,
    pub view_order: Vec<String>,
    #[serde(default)]
    pub automation_revision: u64,
    pub active_data: Option<String>,
    pub active_view: Option<String>,
    pub primary_view: String,
    pub tool: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub analysis_selection: Option<SelectionDto>,
    pub primary_sidebar_width: f32,
    pub primary_sidebar_visible: bool,
    pub secondary_sidebar_width: f32,
    pub secondary_sidebar_visible: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub board: Option<BoardDto>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub board_views: Vec<BoardViewDto>,
    /// Document-level axis text sizes; absent in older files → defaults.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub figure_typography: Option<plotx_figure::FigureTypography>,
}

#[derive(Serialize, Deserialize, Clone, Copy)]
pub struct BoardDto {
    pub zoom: f32,
    pub pan: [f32; 2],
}

#[derive(Serialize, Deserialize, Clone)]
pub struct BoardViewDto {
    pub name: String,
    pub zoom: f32,
    pub pan: [f32; 2],
}

#[derive(Serialize, Deserialize)]
pub struct DatasetBinding {
    pub data: String,
    pub recipe: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub derivation: Option<DerivationDto>,
}

#[derive(Serialize, Deserialize)]
pub struct DerivationDto {
    pub kind: String,
    #[serde(default)]
    pub sources: Vec<String>,
}

#[derive(Serialize, Deserialize, Clone, Copy)]
pub struct RangeDto {
    pub min: f64,
    pub max: f64,
}

#[derive(Serialize, Deserialize)]
pub struct ViewportDto {
    pub full_x: RangeDto,
    pub full_y: RangeDto,
    pub view_x: RangeDto,
    pub view_y: RangeDto,
    pub auto_y: bool,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct SelectionDto {
    pub dataset: String,
    pub canvas: String,
    pub object: String,
    pub x_range: RangeDto,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub y_range: Option<RangeDto>,
}

impl ViewportDto {
    pub fn from_viewport(v: &CanvasViewport) -> Self {
        Self {
            full_x: RangeDto::from_range(v.full_x),
            full_y: RangeDto::from_range(v.full_y),
            view_x: RangeDto::from_range(v.view_x),
            view_y: RangeDto::from_range(v.view_y),
            auto_y: v.auto_y,
        }
    }

    pub fn to_viewport(&self) -> CanvasViewport {
        CanvasViewport {
            full_x: self.full_x.into_range(),
            full_y: self.full_y.into_range(),
            view_x: self.view_x.into_range(),
            view_y: self.view_y.into_range(),
            auto_y: self.auto_y,
        }
    }
}

impl FrameDto {
    pub fn from_frame(frame: ObjectFrame) -> Self {
        Self {
            x: frame.x,
            y: frame.y,
            width: frame.width,
            height: frame.height,
        }
    }

    pub fn into_frame(self) -> ObjectFrame {
        ObjectFrame::new(self.x, self.y, self.width, self.height)
    }
}

impl RangeDto {
    pub fn from_range(r: AxisRange) -> Self {
        Self {
            min: r.min,
            max: r.max,
        }
    }

    pub fn into_range(self) -> AxisRange {
        AxisRange::new(self.min, self.max)
    }
}

impl SelectionDto {
    pub fn from_selection(selection: &AnalysisSelection) -> Self {
        Self {
            dataset: selection.dataset.to_string(),
            canvas: selection.canvas.to_string(),
            object: selection.object.to_string(),
            x_range: RangeDto::from_range(selection.x_range),
            y_range: selection.y_range.map(RangeDto::from_range),
        }
    }

    pub fn to_selection(&self) -> Option<AnalysisSelection> {
        Some(AnalysisSelection {
            dataset: self.dataset.parse().ok()?,
            canvas: self.canvas.parse().ok()?,
            object: self.object.parse().ok()?,
            x_range: self.x_range.into_range(),
            y_range: self.y_range.map(RangeDto::into_range),
        })
    }
}
