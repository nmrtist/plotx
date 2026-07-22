//! Rendering: turns a [`plotx_figure::Figure`] into egui pixels ([`screen`]),
//! an SVG document ([`svg`]), or a Windows metafile ([`emf`]). All back-ends
//! share [`Projector`] and [`ticks`].

pub mod contour;
pub mod integral;
pub mod svg;

#[cfg(feature = "screen")]
pub mod screen;

#[cfg(all(windows, feature = "emf"))]
pub mod emf;

use plotx_figure::{Axis, AxisFrame, AxisTrace, Color, ErrorBar, Figure, HeatmapGrid, Polygon};

/// Fraction of the plot dimension reserved for a marginal axis-projection band.
/// A fraction (not an absolute size) so bands scale with zoom like the margins.
pub const PROJECTION_BAND_FRAC: f32 = 0.15;

pub const AXIS_LINE_WIDTH: f32 = 1.0;
pub const TICK_LENGTH: f32 = 3.0;
pub const TICK_LABEL_PAD: f32 = 2.5;
pub const AXIS_LABEL_GAP: f32 = 5.0;
pub const OUTER_PAD: f32 = 4.0;

/// A rectangle in output space (pixels or points), y-axis pointing down.
#[derive(Debug, Clone, Copy)]
pub struct Rect {
    pub left: f32,
    pub top: f32,
    pub width: f32,
    pub height: f32,
}

/// Screen-only view transform for a fixed-size page document.
#[derive(Debug, Clone, Copy)]
pub struct DocumentViewport {
    pub zoom: f32,
    pub pan: [f32; 2],
}

/// A render-ready page. `items` are painted in order — index 0 is the backmost,
/// the last item is frontmost — so z-order is just the item list order.
pub struct Document<'a> {
    pub width: f32,
    pub height: f32,
    pub background: Color,
    pub items: Vec<DocumentItem<'a>>,
}

/// One paintable page entry: either a materialized plot or an authoring overlay.
pub enum DocumentItem<'a> {
    Plot(DocumentObject<'a>),
    Overlay(DocumentOverlay<'a>),
}

pub struct DocumentObject<'a> {
    pub id: String,
    pub frame: Rect,
    pub figure: &'a Figure,
    pub visible: bool,
    pub title: Option<DocumentText>,
}

/// A plot's in-frame panel letter (a/b/c…), drawn bold at the top-left corner.
pub struct DocumentText {
    pub text: String,
    pub position: [f32; 2],
    pub font_size: f32,
}

/// A non-figure authoring primitive (text label or shape) drawn in page space.
pub struct DocumentOverlay<'a> {
    pub frame: Rect,
    pub visible: bool,
    pub kind: OverlayKind<'a>,
}

pub enum OverlayKind<'a> {
    Text(OverlayText<'a>),
    Shape(OverlayShape),
}

pub struct OverlayText<'a> {
    pub text: &'a str,
    pub font_size: f32,
    pub color: Color,
    pub align: OverlayAlign,
    pub bold: bool,
}

#[derive(Clone, Copy)]
pub enum OverlayAlign {
    Left,
    Center,
    Right,
}

pub struct OverlayShape {
    pub shape: OverlayShapeKind,
    pub stroke: Color,
    pub stroke_width: f32,
    pub fill: Option<Color>,
}

#[derive(Clone, Copy)]
pub enum OverlayShapeKind {
    Rect,
    Ellipse,
    Line,
    Arrow,
}

/// The two barb endpoints of an arrowhead at `tip`, pointing away from `origin`.
/// The head length scales with the shaft but caps (in page units, times `scale`)
/// so short arrows stay legible.
pub fn arrow_head(origin: (f32, f32), tip: (f32, f32), scale: f32) -> [(f32, f32); 2] {
    let dx = tip.0 - origin.0;
    let dy = tip.1 - origin.1;
    let len = (dx * dx + dy * dy).sqrt().max(1e-3);
    let (ux, uy) = (dx / len, dy / len);
    let head = (len * 0.28).min(14.0 * scale);
    let (ca, sa) = (0.5f32.cos(), 0.5f32.sin());
    [
        (
            tip.0 - head * (ux * ca - uy * sa),
            tip.1 - head * (uy * ca + ux * sa),
        ),
        (
            tip.0 - head * (ux * ca + uy * sa),
            tip.1 - head * (uy * ca - ux * sa),
        ),
    ]
}

impl Rect {
    pub fn new(left: f32, top: f32, width: f32, height: f32) -> Self {
        Self {
            left,
            top,
            width,
            height,
        }
    }

    pub fn right(&self) -> f32 {
        self.left + self.width
    }

    pub fn bottom(&self) -> f32 {
        self.top + self.height
    }
}

pub struct Margins {
    pub left: f32,
    pub right: f32,
    pub top: f32,
    pub bottom: f32,
}

impl Default for Margins {
    fn default() -> Self {
        Self {
            left: 36.0,
            right: 10.0,
            top: 8.0,
            bottom: 28.0,
        }
    }
}

impl Margins {
    /// Publication-sized margins derived from the labels that will actually be
    /// drawn. This keeps the y title a stable distance from the widest tick
    /// label instead of retaining whitespace sized for a larger preview font.
    pub fn for_figure(fig: &Figure) -> Self {
        let ty = fig.typography;
        if fig.axis_frame == AxisFrame::Hidden {
            // No ticks or axis titles to clear — only the outer pad and title.
            let title_clearance = if fig.title.trim().is_empty() {
                0.0
            } else {
                ty.title_pt + AXIS_LABEL_GAP
            };
            return Self {
                left: OUTER_PAD,
                right: OUTER_PAD,
                top: OUTER_PAD + title_clearance,
                bottom: OUTER_PAD,
            };
        }
        let x_ticks = axis_ticks_for(&fig.x, 8);
        let y_ticks = axis_ticks_for(&fig.y, 5);
        let widest_y_tick = y_ticks
            .labels
            .iter()
            .map(|label| estimated_text_width(label, ty.tick_pt))
            .fold(0.0, f32::max);
        let widest_x_tick = x_ticks
            .labels
            .iter()
            .map(|label| estimated_text_width(label, ty.tick_pt))
            .fold(0.0, f32::max);

        let left =
            OUTER_PAD + ty.label_pt + AXIS_LABEL_GAP + widest_y_tick + TICK_LENGTH + TICK_LABEL_PAD;
        let right = (OUTER_PAD + widest_x_tick * 0.5).max(8.0);
        let title_clearance = if fig.title.trim().is_empty() {
            0.0
        } else {
            ty.title_pt + AXIS_LABEL_GAP
        };
        let top = OUTER_PAD + title_clearance + y_ticks.multiplier_clearance(ty.tick_pt);
        let bottom = OUTER_PAD
            + x_ticks.multiplier_clearance(ty.tick_pt)
            + ty.label_pt
            + AXIS_LABEL_GAP
            + ty.tick_pt
            + TICK_LABEL_PAD
            + TICK_LENGTH;

        Self {
            left,
            right,
            top,
            bottom,
        }
    }

    pub fn scaled(&self, s: f32) -> Margins {
        Margins {
            left: self.left * s,
            right: self.right * s,
            top: self.top * s,
            bottom: self.bottom * s,
        }
    }
}

/// Maps figure data-space coordinates into an output-space plot rectangle.
pub struct Projector<'a> {
    pub fig: &'a Figure,
    pub plot: Rect,
    /// The band above `plot` reserved for the top (F2) projection, when present.
    pub top_band: Option<Rect>,
    /// The band left of `plot` reserved for the left (F1) projection, when present.
    pub left_band: Option<Rect>,
}

impl<'a> Projector<'a> {
    pub fn new(fig: &'a Figure, outer: Rect, margins: &Margins) -> Self {
        let mut plot = Rect::new(
            outer.left + margins.left,
            outer.top + margins.top,
            (outer.width - margins.left - margins.right).max(1.0),
            (outer.height - margins.top - margins.bottom).max(1.0),
        );
        // Reserve the projection bands from the inner rect before aspect-locking,
        // so the contour keeps its aspect and the bands hug its final edges.
        let band_top = fig
            .top_projection
            .as_ref()
            .map(|_| plot.height * PROJECTION_BAND_FRAC);
        let band_left = fig
            .left_projection
            .as_ref()
            .map(|_| plot.width * PROJECTION_BAND_FRAC);
        if let Some(bt) = band_top {
            plot.top += bt;
            plot.height = (plot.height - bt).max(1.0);
        }
        if let Some(bl) = band_left {
            plot.left += bl;
            plot.width = (plot.width - bl).max(1.0);
        }
        if fig.lock_aspect {
            // Shrink to the largest centered sub-rect with equal data-units-per-
            // pixel on both axes (width/height == x-span/y-span), letterboxing.
            let target = (fig.x.span() / fig.y.span()).abs() as f32;
            if target.is_finite() && target > 0.0 {
                if plot.width / plot.height > target {
                    let w = plot.height * target;
                    plot.left += (plot.width - w) * 0.5;
                    plot.width = w;
                } else {
                    let h = plot.width / target;
                    plot.top += (plot.height - h) * 0.5;
                    plot.height = h;
                }
            }
        }
        // Bands hug the final contour edges, sharing its along-axis extent.
        let top_band = band_top.map(|bt| Rect::new(plot.left, plot.top - bt, plot.width, bt));
        let left_band = band_left.map(|bl| Rect::new(plot.left - bl, plot.top, bl, plot.height));
        Self {
            fig,
            plot,
            top_band,
            left_band,
        }
    }

    /// Data `[x, y]` → output `(px, py)`; y is flipped so larger values sit higher.
    pub fn project(&self, p: [f64; 2]) -> (f32, f32) {
        let tx = self.fig.x.normalize(p[0]) as f32;
        let ty = self.fig.y.normalize(p[1]) as f32;
        let px = self.plot.left + tx * self.plot.width;
        let py = self.plot.top + (1.0 - ty) * self.plot.height;
        (px, py)
    }
}

/// Project one vertical uncertainty whisker into its stem and lower/upper caps.
/// `output_scale` converts the cap's logical-unit width for zoomed screen output.
pub(crate) fn error_bar_segments(
    proj: &Projector<'_>,
    error_bar: &ErrorBar,
    output_scale: f32,
) -> Option<[[(f32, f32); 2]; 3]> {
    if !error_bar.center[0].is_finite()
        || !error_bar.center[1].is_finite()
        || !error_bar.negative.is_finite()
        || error_bar.negative < 0.0
        || !error_bar.positive.is_finite()
        || error_bar.positive < 0.0
        || !error_bar.width.is_finite()
        || error_bar.width <= 0.0
        || !error_bar.cap_width.is_finite()
        || error_bar.cap_width < 0.0
        || !output_scale.is_finite()
        || output_scale <= 0.0
    {
        return None;
    }
    let x = error_bar.center[0];
    let low_y = error_bar.center[1] - error_bar.negative;
    let high_y = error_bar.center[1] + error_bar.positive;
    if !low_y.is_finite() || !high_y.is_finite() {
        return None;
    }
    let low = proj.project([x, low_y]);
    let high = proj.project([x, high_y]);
    let half_cap = error_bar.cap_width * output_scale * 0.5;
    Some([
        [low, high],
        [(low.0 - half_cap, low.1), (low.0 + half_cap, low.1)],
        [(high.0 - half_cap, high.1), (high.0 + half_cap, high.1)],
    ])
}

/// Project a filled polygon's vertices; `None` when degenerate (< 3 points) or
/// fully transparent, so backends can skip it uniformly.
pub(crate) fn polygon_outline(proj: &Projector<'_>, poly: &Polygon) -> Option<Vec<(f32, f32)>> {
    if poly.points.len() < 3 || poly.opacity <= 0.0 {
        return None;
    }
    if poly
        .points
        .iter()
        .any(|p| !p[0].is_finite() || !p[1].is_finite())
    {
        return None;
    }
    Some(poly.points.iter().map(|p| proj.project(*p)).collect())
}

/// Project every finite heatmap cell to a normalized output rect plus its
/// sampled color. Rects are min/max-normalized so reversed axes still yield
/// positive extents.
pub(crate) fn heatmap_cells(proj: &Projector<'_>, grid: &HeatmapGrid) -> Vec<(Rect, Color)> {
    let mut cells = Vec::new();
    for row in 0..grid.rows {
        for col in 0..grid.cols {
            let Some(t) = grid.normalized(row, col) else {
                continue;
            };
            let [x0, x1, y0, y1] = grid.cell_bounds(row, col);
            let (ax, ay) = proj.project([x0, y0]);
            let (bx, by) = proj.project([x1, y1]);
            let (left, right) = (ax.min(bx), ax.max(bx));
            let (top, bottom) = (ay.min(by), ay.max(by));
            cells.push((
                Rect::new(left, top, right - left, bottom - top),
                grid.colormap.sample(t),
            ));
        }
    }
    cells
}

/// How a legend entry's swatch is drawn.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum LegendMark {
    Line,
    Points,
    Rect,
}

/// Legend entries across series and named polygons (deduplicated by name, in
/// first-appearance order), shared by every backend so legends stay identical.
pub(crate) fn legend_entries(fig: &Figure) -> Vec<(&str, Color, LegendMark)> {
    let mut entries: Vec<(&str, Color, LegendMark)> = fig
        .series
        .iter()
        .filter(|s| !s.points.is_empty() && !s.name.is_empty())
        .map(|s| {
            let mark = match s.kind {
                plotx_figure::SeriesKind::Line => LegendMark::Line,
                plotx_figure::SeriesKind::Points => LegendMark::Points,
            };
            (s.name.as_str(), s.color, mark)
        })
        .collect();
    for poly in &fig.polygons {
        if poly.name.is_empty() || entries.iter().any(|(n, _, _)| *n == poly.name) {
            continue;
        }
        entries.push((poly.name.as_str(), poly.fill, LegendMark::Rect));
    }
    entries
}

/// Lay a marginal projection `trace` into its `band` as output-space points,
/// sharing `plot`'s along-axis mapping. `along_x` selects the top band (shares
/// the x/F2 mapping, autoscaled vertically); otherwise the left band (shares the
/// y/F1 mapping, autoscaled horizontally). Intensity autoscale spans only points
/// within the shared axis range, so an attached trace wider than the contour
/// still fills its band. A small inset keeps the trace off the band edges.
pub fn projection_points(
    fig: &Figure,
    trace: &AxisTrace,
    plot: Rect,
    band: Rect,
    along_x: bool,
) -> Vec<(f32, f32)> {
    let axis = if along_x { &fig.x } else { &fig.y };
    let (lo, hi) = axis_intensity_bounds(axis, trace);
    let span = (hi - lo).max(f64::MIN_POSITIVE);
    trace
        .points
        .iter()
        .map(|p| {
            let n = axis.normalize(p[0]) as f32;
            let t = (((p[1] - lo) / span).clamp(0.0, 1.0)) as f32;
            if along_x {
                let x = plot.left + n * plot.width;
                let y = band.bottom() - (0.06 + 0.9 * t) * band.height;
                (x, y)
            } else {
                let y = plot.top + (1.0 - n) * plot.height;
                let x = band.left + (0.06 + 0.9 * t) * band.width;
                (x, y)
            }
        })
        .collect()
}

fn axis_intensity_bounds(axis: &plotx_figure::Axis, trace: &AxisTrace) -> (f64, f64) {
    let (a, b) = (axis.min.min(axis.max), axis.min.max(axis.max));
    let mut lo = f64::INFINITY;
    let mut hi = f64::NEG_INFINITY;
    for p in &trace.points {
        if p[0] >= a && p[0] <= b {
            lo = lo.min(p[1]);
            hi = hi.max(p[1]);
        }
    }
    if lo.is_finite() { (lo, hi) } else { (0.0, 1.0) }
}

/// Up to `target` "nice" tick values covering `[min, max]`, using 1/2/5×10ⁿ
/// rounding so ticks land on human-friendly numbers.
pub fn ticks(min: f64, max: f64, target: usize) -> Vec<f64> {
    let (lo, hi) = if min <= max { (min, max) } else { (max, min) };
    let span = hi - lo;
    if !span.is_finite() || span <= 0.0 || target == 0 {
        return vec![lo];
    }
    let raw_step = span / target as f64;
    let mag = 10f64.powf(raw_step.log10().floor());
    let norm = raw_step / mag;
    let nice = if norm < 1.5 {
        1.0
    } else if norm < 3.0 {
        2.0
    } else if norm < 7.0 {
        5.0
    } else {
        10.0
    };
    let step = nice * mag;

    let eps = step * 1e-9;
    let first = ((lo - eps) / step).ceil() * step;
    let mut out = Vec::new();
    let mut v = first;
    let mut guard = 0;
    while v <= hi + eps && guard < 1000 {
        out.push(if v.abs() < step * 1e-9 { 0.0 } else { v }); // snap fp crud to 0
        v += step;
        guard += 1;
    }
    out
}

/// Tick positions plus labels formatted as one axis-wide system. Decimal
/// precision follows the tick interval, and very large or small values share a
/// single power-of-ten multiplier instead of repeating scientific notation.
#[derive(Debug, Clone, PartialEq)]
pub struct AxisTicks {
    pub values: Vec<f64>,
    pub labels: Vec<String>,
    pub scale_exponent: Option<i32>,
}

impl AxisTicks {
    pub fn multiplier(&self) -> Option<String> {
        self.scale_exponent
            .map(|exponent| format!("×10{}", superscript(exponent)))
    }

    /// Margin height reserved for the multiplier's own text row, set in the
    /// figure's tick font size.
    pub fn multiplier_clearance(&self, tick_pt: f32) -> f32 {
        if self.scale_exponent.is_some() {
            tick_pt + AXIS_LABEL_GAP
        } else {
            0.0
        }
    }
}

fn estimated_text_width(text: &str, font_size: f32) -> f32 {
    text.chars()
        .map(|ch| match ch {
            '0'..='9' => 0.56,
            '.' | ',' => 0.28,
            '-' | '−' => 0.36,
            _ => 0.58,
        })
        .sum::<f32>()
        * font_size
}

/// Ticks for an axis honoring its ordinal mode: categorical axes label integer
/// slot positions with their category names (thinned to stay legible), numeric
/// axes fall through to [`axis_ticks`].
pub fn axis_ticks_for(axis: &Axis, target: usize) -> AxisTicks {
    let Some(names) = &axis.categories else {
        return axis_ticks(axis.min, axis.max, target);
    };
    let (lo, hi) = (axis.min.min(axis.max), axis.min.max(axis.max));
    let visible: Vec<(f64, &str)> = names
        .iter()
        .enumerate()
        .map(|(i, name)| (i as f64, name.as_str()))
        .filter(|(v, _)| *v >= lo - 1e-9 && *v <= hi + 1e-9)
        .collect();
    // Label every k-th slot when there are more categories than tick budget.
    let stride = visible.len().div_ceil(target.max(1)).max(1);
    let mut values = Vec::new();
    let mut labels = Vec::new();
    for (v, name) in visible.iter().step_by(stride) {
        values.push(*v);
        labels.push((*name).to_owned());
    }
    AxisTicks {
        values,
        labels,
        scale_exponent: None,
    }
}

pub fn axis_ticks(min: f64, max: f64, target: usize) -> AxisTicks {
    let values = ticks(min, max, target);
    let max_abs = min.abs().max(max.abs());
    let exponent = if max_abs.is_finite() && max_abs > 0.0 {
        max_abs.log10().floor() as i32
    } else {
        0
    };
    let scale_exponent = (exponent >= 4 || exponent <= -4).then_some(exponent);
    let scale = scale_exponent.map_or(1.0, |value| 10f64.powi(value));
    let scaled_step = values
        .windows(2)
        .map(|pair| ((pair[1] - pair[0]) / scale).abs())
        .find(|step| step.is_finite() && *step > 0.0)
        .unwrap_or(1.0);
    let precision = decimal_places(scaled_step);
    let zero_threshold = 0.5 * 10f64.powi(-(precision as i32));
    let labels = values
        .iter()
        .map(|value| {
            let scaled = value / scale;
            let clean = if scaled.abs() < zero_threshold {
                0.0
            } else {
                scaled
            };
            format!("{clean:.precision$}")
        })
        .collect();

    AxisTicks {
        values,
        labels,
        scale_exponent,
    }
}

fn decimal_places(step: f64) -> usize {
    if !step.is_finite() || step <= 0.0 {
        return 0;
    }
    (-(step.log10().floor() as i32)).clamp(0, 8) as usize
}

fn superscript(value: i32) -> String {
    value
        .to_string()
        .chars()
        .map(|ch| match ch {
            '-' => '⁻',
            '0' => '⁰',
            '1' => '¹',
            '2' => '²',
            '3' => '³',
            '4' => '⁴',
            '5' => '⁵',
            '6' => '⁶',
            '7' => '⁷',
            '8' => '⁸',
            '9' => '⁹',
            _ => ch,
        })
        .collect()
}

#[cfg(test)]
mod tests;
