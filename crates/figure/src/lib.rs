//! Figure model: a pure-data description of what to draw, in data-space
//! coordinates. Both renderers (egui screen, SVG export) consume this model.

mod colormap;

pub use colormap::ColormapId;

/// An RGB color, 0–255 per channel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Color {
    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b }
    }

    pub const BLACK: Color = Color::rgb(0, 0, 0);
    pub const AXIS: Color = Color::rgb(0x27, 0x27, 0x27);
    pub const TRACE: Color = Color::rgb(0x0f, 0x4d, 0x92);
    pub const GRID: Color = Color::rgb(220, 222, 228);

    pub fn to_hex(self) -> String {
        format!("#{:02x}{:02x}{:02x}", self.r, self.g, self.b)
    }
}

/// Point sizes of a figure's axis furniture text. Absolute typographic points
/// (1/72"), independent of the plot's frame size — the journal convention — so
/// resizing a panel never changes its type size. Defaults sit mid-band for
/// journal figures (most ask for 5–7 pt minimum at final size) and pass the
/// export precheck's 7 pt floor.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct FigureTypography {
    /// Tick value labels.
    pub tick_pt: f32,
    /// Axis titles (the x/y labels).
    pub label_pt: f32,
    /// Figure title above the plot.
    pub title_pt: f32,
}

impl Default for FigureTypography {
    fn default() -> Self {
        Self {
            tick_pt: 7.0,
            label_pt: 8.0,
            title_pt: 8.0,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Axis {
    pub label: String,
    #[serde(default = "default_true")]
    pub show_tick_labels: bool,
    #[serde(default = "default_true")]
    pub show_label: bool,
    pub min: f64,
    pub max: f64,
    /// If true, larger values draw toward the lower screen coordinate (left for
    /// x, the NMR ppm convention; top for y).
    pub reversed: bool,
    /// Ordinal mode: `Some(names)` places one category per integer position
    /// (0, 1, …) and renderers label those positions with the names instead of
    /// generating numeric ticks. `min`/`max` still define the visible window,
    /// conventionally `[-0.5, n - 0.5]` so slots get equal half-unit padding.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub categories: Option<Vec<String>>,
}

impl Axis {
    pub fn new(label: impl Into<String>, min: f64, max: f64) -> Self {
        Self {
            label: label.into(),
            show_tick_labels: true,
            show_label: true,
            min,
            max,
            reversed: false,
            categories: None,
        }
    }

    /// An ordinal axis with one slot per category name, windowed to give every
    /// slot equal padding.
    pub fn categorical(label: impl Into<String>, names: Vec<String>) -> Self {
        let n = names.len().max(1) as f64;
        Self {
            label: label.into(),
            show_tick_labels: true,
            show_label: true,
            min: -0.5,
            max: n - 0.5,
            reversed: false,
            categories: Some(names),
        }
    }

    pub fn reversed(mut self, yes: bool) -> Self {
        self.reversed = yes;
        self
    }

    /// Axis span, forced non-zero to avoid divide-by-zero in renderers.
    pub fn span(&self) -> f64 {
        let s = self.max - self.min;
        if s.abs() < f64::EPSILON { 1.0 } else { s }
    }

    /// Map a value to a normalized `[0,1]` position, honoring `reversed`.
    pub fn normalize(&self, v: f64) -> f64 {
        let t = (v - self.min) / self.span();
        if self.reversed { 1.0 - t } else { t }
    }
}

const fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Series {
    pub name: String,
    pub points: Vec<[f64; 2]>,
    pub color: Color,
    pub width: f32,
    #[serde(default)]
    pub kind: SeriesKind,
}

/// A stored 1D NMR integral description. Renderers derive the cumulative trace
/// from `Figure::series[source_series]`, keeping high-resolution spectrum points
/// in one place in project snapshots.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct IntegralCurve {
    pub start_ppm: f64,
    pub end_ppm: f64,
    pub normalized_area: f64,
    pub label: String,
    pub color: Color,
    pub width: f32,
    pub source_series: usize,
}

/// A vertical uncertainty whisker in data-space coordinates. `center` is the
/// plotted observation and `negative`/`positive` are non-negative distances
/// below and above it. Cap width is expressed in output-space logical units so
/// it stays legible independently of the x-axis scale.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ErrorBar {
    pub center: [f64; 2],
    pub negative: f64,
    pub positive: f64,
    pub color: Color,
    pub width: f32,
    pub cap_width: f32,
    /// Paint after the corresponding data series. Bars use this so their wide
    /// stroke cannot obscure the lower whisker; point markers leave it false so
    /// they remain visually on top of the whisker center.
    #[serde(default)]
    pub draw_over_data: bool,
}

impl ErrorBar {
    pub fn symmetric(center: [f64; 2], error: f64) -> Self {
        Self {
            center,
            negative: error,
            positive: error,
            color: Color::TRACE,
            width: 1.0,
            cap_width: 8.0,
            draw_over_data: false,
        }
    }

    pub fn colored(mut self, color: Color) -> Self {
        self.color = color;
        self
    }

    pub fn over_data(mut self) -> Self {
        self.draw_over_data = true;
        self
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum SeriesKind {
    #[default]
    Line,
    Points,
}

impl Series {
    pub fn line(name: impl Into<String>, points: Vec<[f64; 2]>) -> Self {
        Self {
            name: name.into(),
            points,
            color: Color::TRACE,
            width: 1.0,
            kind: SeriesKind::Line,
        }
    }

    pub fn points(name: impl Into<String>, points: Vec<[f64; 2]>) -> Self {
        Self {
            name: name.into(),
            points,
            color: Color::TRACE,
            width: 3.0,
            kind: SeriesKind::Points,
        }
    }

    pub fn colored(mut self, color: Color) -> Self {
        self.color = color;
        self
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Annotation {
    pub text: String,
    pub at: [f64; 2],
    pub color: Color,
    pub size: f32,
}

/// A 2D contour overlay as pre-computed data-space line segments (each
/// `[[x0,y0],[x1,y1]]`). The heavy marching-squares pass runs once when the
/// figure is built, so both renderers only project and stroke.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Contour {
    pub segments: Vec<[[f64; 2]; 2]>,
    pub color: Color,
    pub width: f32,
}

/// A 1D trace pinned alongside a 2D contour's axis (the standard NMR marginal
/// projection). `points` are `[ppm, intensity]` on the contour's corresponding
/// axis — the top track shares the x (F2) mapping, the left track the y (F1)
/// mapping. Intensity is raw; the renderer autoscales it within the reserved
/// band, so an attached trace wider than the contour window still fills it.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AxisTrace {
    pub points: Vec<[f64; 2]>,
    pub color: Color,
    pub width: f32,
}

/// A filled polygon in data-space coordinates: bars, box bodies, violin
/// strips, stacked segments, and pie wedges all lower to this one primitive.
///
/// Invariant: renderers assume the points describe a **convex** polygon (their
/// fill paths are simple fans). Builders must decompose concave shapes — e.g.
/// a violin outline — into convex pieces (strips, wedges) before emitting them.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Polygon {
    /// Legend entry; polygons sharing a name legend once. Empty = no legend.
    pub name: String,
    pub points: Vec<[f64; 2]>,
    pub fill: Color,
    /// Fill opacity in `[0, 1]`. Exporters without native alpha (EMF)
    /// pre-blend against the figure background.
    #[serde(default = "default_opacity")]
    pub opacity: f32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stroke: Option<(Color, f32)>,
}

fn default_opacity() -> f32 {
    1.0
}

impl Polygon {
    pub fn new(name: impl Into<String>, points: Vec<[f64; 2]>, fill: Color) -> Self {
        Self {
            name: name.into(),
            points,
            fill,
            opacity: 1.0,
            stroke: None,
        }
    }

    /// An axis-aligned rectangle spanning `[x0, x1] × [y0, y1]` — the bar/box
    /// workhorse. Callers may pass bounds in either order.
    pub fn rect(name: impl Into<String>, x0: f64, x1: f64, y0: f64, y1: f64, fill: Color) -> Self {
        Self::new(name, vec![[x0, y0], [x1, y0], [x1, y1], [x0, y1]], fill)
    }

    pub fn with_opacity(mut self, opacity: f32) -> Self {
        self.opacity = opacity.clamp(0.0, 1.0);
        self
    }

    pub fn with_stroke(mut self, color: Color, width: f32) -> Self {
        self.stroke = Some((color, width));
        self
    }
}

/// A dense value grid rendered as filled cells through a colormap. Unlike
/// [`Contour`] — which keeps only extracted line segments — the raw matrix is
/// retained so renderers can fill every cell.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct HeatmapGrid {
    pub rows: usize,
    pub cols: usize,
    /// Row-major `rows × cols` values (the [`crate::Contour`]/marching-squares
    /// convention). Non-finite cells are skipped (drawn as background).
    pub values: Vec<f32>,
    /// Data-space x of the leftmost and rightmost cell edges; columns divide
    /// this span evenly.
    pub x_bounds: [f64; 2],
    /// Data-space y of the row-0 edge and the last-row edge; rows divide this
    /// span evenly.
    pub y_bounds: [f64; 2],
    pub colormap: ColormapId,
    /// Value range mapped to the colormap ends; cells clamp outside it.
    pub value_range: [f32; 2],
}

impl HeatmapGrid {
    /// The normalized `[0, 1]` colormap position of one cell, or `None` for a
    /// missing / non-finite cell.
    pub fn normalized(&self, row: usize, col: usize) -> Option<f32> {
        let v = *self.values.get(row * self.cols + col)?;
        if !v.is_finite() {
            return None;
        }
        let [lo, hi] = self.value_range;
        let span = hi - lo;
        if span.abs() < f32::EPSILON {
            return Some(0.5);
        }
        Some(((v - lo) / span).clamp(0.0, 1.0))
    }

    /// Data-space rectangle `[x0, x1, y0, y1]` covered by one cell.
    pub fn cell_bounds(&self, row: usize, col: usize) -> [f64; 4] {
        let dx = (self.x_bounds[1] - self.x_bounds[0]) / self.cols.max(1) as f64;
        let dy = (self.y_bounds[1] - self.y_bounds[0]) / self.rows.max(1) as f64;
        let x0 = self.x_bounds[0] + col as f64 * dx;
        let y0 = self.y_bounds[0] + row as f64 * dy;
        [x0, x0 + dx, y0, y0 + dy]
    }
}

/// Border treatment for the data area. Open axes suit 1D traces and ordinary
/// quantitative charts; a box defines the bounded field of a 2D contour while
/// ticks and labels remain on the left and bottom only. Hidden suppresses the
/// frame, ticks, and axis labels entirely (pie charts, 3D projections).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum AxisFrame {
    #[default]
    Open,
    Box,
    Hidden,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Figure {
    pub title: String,
    pub x: Axis,
    pub y: Axis,
    pub series: Vec<Series>,
    /// Persistent descriptions of result-bearing 1D NMR integral curves.
    #[serde(default)]
    pub integral_curves: Vec<IntegralCurve>,
    /// Filled polygons, painted after the heatmap and before contours/series so
    /// bodies (bars, boxes, violins, wedges) sit under outlines and markers.
    #[serde(default)]
    pub polygons: Vec<Polygon>,
    /// Backmost layer: a dense colormapped cell grid.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub heatmap: Option<HeatmapGrid>,
    /// Vertical uncertainty whiskers, painted behind the corresponding series.
    #[serde(default)]
    pub error_bars: Vec<ErrorBar>,
    pub annotations: Vec<Annotation>,
    /// Zero, one, or many contour overlays painted on the shared axes (e.g. one
    /// per dataset in a 2D color-overlay stack), each with its own colour/width.
    #[serde(default)]
    pub contours: Vec<Contour>,
    /// Marginal 1D projection along the top (F2/x) axis of a 2D contour, drawn in
    /// a reserved band above the plot. `None` = no top projection.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub top_projection: Option<AxisTrace>,
    /// Marginal 1D projection along the left (F1/y) axis, drawn in a reserved band
    /// to the left of the plot. `None` = no left projection.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub left_projection: Option<AxisTrace>,
    /// Preferred output size in logical units (px for screen, pt for export).
    pub width: f32,
    pub height: f32,
    pub background: Color,
    pub show_grid: bool,
    /// Draw a legend box (series name + colour swatch). Populated only for
    /// multi-series overlays; defaults off so single-trace figures are unchanged.
    #[serde(default)]
    pub show_legend: bool,
    /// Render the data area with equal data-units-per-pixel on both axes,
    /// letterboxed within the frame. Set for homonuclear 2D (square COSY/NOESY).
    #[serde(default)]
    pub lock_aspect: bool,
    #[serde(default)]
    pub axis_frame: AxisFrame,
    /// Text sizes (pt) of the axis furniture; see [`FigureTypography`].
    #[serde(default)]
    pub typography: FigureTypography,
}

impl Figure {
    pub fn new(title: impl Into<String>, x: Axis, y: Axis) -> Self {
        Self {
            title: title.into(),
            x,
            y,
            series: Vec::new(),
            integral_curves: Vec::new(),
            polygons: Vec::new(),
            heatmap: None,
            error_bars: Vec::new(),
            annotations: Vec::new(),
            contours: Vec::new(),
            top_projection: None,
            left_projection: None,
            width: 900.0,
            height: 520.0,
            background: Color::rgb(255, 255, 255),
            show_grid: false,
            show_legend: false,
            lock_aspect: false,
            axis_frame: AxisFrame::Open,
            typography: FigureTypography::default(),
        }
    }

    pub fn with_series(mut self, s: Series) -> Self {
        self.series.push(s);
        self
    }

    pub fn with_polygon(mut self, p: Polygon) -> Self {
        self.polygons.push(p);
        self
    }

    pub fn with_error_bar(mut self, error_bar: ErrorBar) -> Self {
        self.error_bars.push(error_bar);
        self
    }

    pub fn with_annotation(mut self, a: Annotation) -> Self {
        self.annotations.push(a);
        self
    }

    pub fn with_contour(mut self, c: Contour) -> Self {
        self.contours.push(c);
        self
    }

    pub fn with_axis_frame(mut self, frame: AxisFrame) -> Self {
        self.axis_frame = frame;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::Axis;

    #[test]
    fn axis_text_visibility_defaults_to_visible() {
        let axis = Axis::new("x", 0.0, 1.0);
        assert!(axis.show_tick_labels);
        assert!(axis.show_label);
    }

    #[test]
    fn missing_axis_visibility_fields_deserialize_as_visible() {
        let axis: Axis =
            serde_json::from_str(r#"{"label":"x","min":0.0,"max":1.0,"reversed":false}"#).unwrap();
        assert!(axis.show_tick_labels);
        assert!(axis.show_label);
    }
}
