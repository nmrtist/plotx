use crate::{AXIS_LABEL_GAP, Margins, OUTER_PAD, Projector, Rect, TICK_LABEL_PAD, TICK_LENGTH};
use plotx_figure::{Axis, AxisFrame, Figure};
use unicode_width::UnicodeWidthChar;

const MAX_X_TARGET: usize = 8;
const MAX_Y_TARGET: usize = 5;
const LABEL_GAP_FACTOR: f32 = 0.8;
const Y_TICK_SPACING_FACTOR: f32 = 1.8;

/// Margins and tick sets computed together for a particular output size.
#[derive(Debug, Clone, PartialEq)]
pub struct AxisLayout {
    pub margins: Margins,
    pub x_ticks: AxisTicks,
    pub y_ticks: AxisTicks,
}

/// Compute axes in page units, adapting tick density to the final plot rect.
///
/// The first pass uses the normal maximum tick budgets to establish margins.
/// The second and final pass chooses ticks from the resulting plot size and
/// recomputes margins. Avoiding another geometry pass keeps the result stable;
/// the second-pass margins can only make the available plot area less
/// constrained than the conservative first pass.
pub fn axis_layout(fig: &Figure, outer_width: f32, outer_height: f32) -> AxisLayout {
    if fig.axis_frame == AxisFrame::Hidden {
        let x_ticks = AxisTicks::empty();
        let y_ticks = AxisTicks::empty();
        return AxisLayout {
            margins: margins_for_ticks(fig, &x_ticks, &y_ticks),
            x_ticks,
            y_ticks,
        };
    }

    let initial_x = axis_ticks_for(&fig.x, MAX_X_TARGET);
    let initial_y = axis_ticks_for(&fig.y, MAX_Y_TARGET);
    // Categorical thinning can select a different subset at a lower target.
    // Size the first pass for every visible category so no second-pass label
    // can make the final margins grow and invalidate the fit checks below.
    // This deliberately reserves names that may not ultimately be drawn; on a
    // narrow figure that conservative cost can reduce the x tick budget.
    let initial_widths = TickLabelWidths {
        x: widest_layout_label(&fig.x, &initial_x, fig.typography.tick_pt),
        y: widest_layout_label(&fig.y, &initial_y, fig.typography.tick_pt),
    };
    let initial_margins =
        margins_for_ticks_with_widths(fig, &initial_x, &initial_y, initial_widths);
    let outer = Rect::new(0.0, 0.0, outer_width.max(0.0), outer_height.max(0.0));
    let plot = Projector::new(fig, outer, &initial_margins).plot;
    let gap = fig.typography.tick_pt * LABEL_GAP_FACTOR;

    let x_ticks = adaptive_x_ticks(fig, plot.width, gap, initial_widths.x);
    let y_ticks = adaptive_y_ticks(fig, plot.height);
    let margins = margins_for_ticks(fig, &x_ticks, &y_ticks);

    AxisLayout {
        margins,
        x_ticks,
        y_ticks,
    }
}

fn adaptive_x_ticks(fig: &Figure, plot_width: f32, gap: f32, widest: f32) -> AxisTicks {
    let slot_width = widest + gap;
    if !plot_width.is_finite() || slot_width <= 0.0 || plot_width < 2.0 * slot_width {
        return AxisTicks::empty();
    }

    let target = ((plot_width / slot_width).floor() as usize).clamp(2, MAX_X_TARGET);
    for candidate in (2..=target).rev() {
        let ticks = axis_ticks_for(&fig.x, candidate);
        if horizontal_labels_fit(&fig.x, &ticks, plot_width, fig.typography.tick_pt, gap) {
            return ticks;
        }
    }
    AxisTicks::empty()
}

fn adaptive_y_ticks(fig: &Figure, plot_height: f32) -> AxisTicks {
    let spacing = fig.typography.tick_pt * Y_TICK_SPACING_FACTOR;
    if !plot_height.is_finite() || spacing <= 0.0 || plot_height < 2.0 * spacing {
        return AxisTicks::empty();
    }

    let target = ((plot_height / spacing).floor() as usize).clamp(2, MAX_Y_TARGET);
    for candidate in (2..=target).rev() {
        let ticks = axis_ticks_for(&fig.y, candidate);
        if vertical_labels_fit(&fig.y, &ticks, plot_height, fig.typography.tick_pt) {
            return ticks;
        }
    }
    AxisTicks::empty()
}

fn widest_layout_label(axis: &Axis, ticks: &AxisTicks, tick_pt: f32) -> f32 {
    if axis.categories.is_some() {
        visible_categories(axis)
            .map(|(_, label)| estimated_text_width(label, tick_pt))
            .fold(0.0, f32::max)
    } else {
        widest_tick_label(ticks, tick_pt)
    }
}

fn widest_tick_label(ticks: &AxisTicks, tick_pt: f32) -> f32 {
    ticks
        .labels
        .iter()
        .map(|label| estimated_text_width(label, tick_pt))
        .fold(0.0, f32::max)
}

fn horizontal_labels_fit(
    axis: &Axis,
    ticks: &AxisTicks,
    width: f32,
    tick_pt: f32,
    gap: f32,
) -> bool {
    let mut intervals: Vec<(f32, f32)> = ticks
        .values
        .iter()
        .zip(&ticks.labels)
        .map(|(&value, label)| {
            let center = axis.normalize(value) as f32 * width;
            let half = estimated_text_width(label, tick_pt) * 0.5;
            (center - half, center + half)
        })
        .collect();
    intervals.sort_by(|a, b| a.0.total_cmp(&b.0));
    intervals
        .windows(2)
        .all(|pair| pair[0].1 + gap <= pair[1].0)
}

fn vertical_labels_fit(axis: &Axis, ticks: &AxisTicks, height: f32, tick_pt: f32) -> bool {
    let mut centers: Vec<f32> = ticks
        .values
        .iter()
        .map(|&value| axis.normalize(value) as f32 * height)
        .collect();
    centers.sort_by(f32::total_cmp);
    centers
        .windows(2)
        .all(|pair| pair[1] - pair[0] >= tick_pt * Y_TICK_SPACING_FACTOR)
}

pub(crate) fn margins_for_ticks(fig: &Figure, x_ticks: &AxisTicks, y_ticks: &AxisTicks) -> Margins {
    let widths = TickLabelWidths {
        x: widest_tick_label(x_ticks, fig.typography.tick_pt),
        y: widest_tick_label(y_ticks, fig.typography.tick_pt),
    };
    margins_for_ticks_with_widths(fig, x_ticks, y_ticks, widths)
}

#[derive(Clone, Copy)]
struct TickLabelWidths {
    x: f32,
    y: f32,
}

fn margins_for_ticks_with_widths(
    fig: &Figure,
    x_ticks: &AxisTicks,
    y_ticks: &AxisTicks,
    widths: TickLabelWidths,
) -> Margins {
    let ty = fig.typography;
    if fig.axis_frame == AxisFrame::Hidden {
        let title_clearance = if fig.title.trim().is_empty() {
            0.0
        } else {
            ty.title_pt + AXIS_LABEL_GAP
        };
        return Margins {
            left: OUTER_PAD,
            right: OUTER_PAD,
            top: OUTER_PAD + title_clearance,
            bottom: OUTER_PAD,
        };
    }

    let y_tick_clearance = if y_ticks.values.is_empty() {
        0.0
    } else if fig.y.show_tick_labels {
        widths.y + TICK_LENGTH + TICK_LABEL_PAD
    } else {
        TICK_LENGTH
    };
    let x_tick_clearance = if x_ticks.values.is_empty() {
        0.0
    } else if fig.x.show_tick_labels {
        ty.tick_pt + TICK_LABEL_PAD + TICK_LENGTH
    } else {
        TICK_LENGTH
    };

    // Keep a left-end x label out of the rotated y-title lane even when the
    // y ticks themselves have been dropped on a short panel.
    let x_endpoint_clearance = if !fig.x.show_tick_labels || x_ticks.labels.is_empty() {
        0.0
    } else {
        widths.x * 0.5
    };
    let y_title_clearance = if fig.y.show_label {
        ty.label_pt + AXIS_LABEL_GAP
    } else {
        0.0
    };
    let left = OUTER_PAD + y_title_clearance + y_tick_clearance.max(x_endpoint_clearance);
    let x_width = if fig.x.show_tick_labels {
        widths.x
    } else {
        0.0
    };
    let right = (OUTER_PAD + x_width * 0.5).max(8.0);
    let title_clearance = if fig.title.trim().is_empty() {
        0.0
    } else {
        ty.title_pt + AXIS_LABEL_GAP
    };
    let y_multiplier = if fig.y.show_tick_labels {
        y_ticks.multiplier_clearance(ty.tick_pt)
    } else {
        0.0
    };
    let x_multiplier = if fig.x.show_tick_labels {
        x_ticks.multiplier_clearance(ty.tick_pt)
    } else {
        0.0
    };
    let x_title_clearance = if fig.x.show_label {
        ty.label_pt + AXIS_LABEL_GAP
    } else {
        0.0
    };
    let top = OUTER_PAD + title_clearance + y_multiplier;
    let bottom = OUTER_PAD + x_multiplier + x_title_clearance + x_tick_clearance;

    Margins {
        left,
        right,
        top,
        bottom,
    }
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
    let mut value = first;
    let mut guard = 0;
    while value <= hi + eps && guard < 1000 {
        out.push(if value.abs() < step * 1e-9 {
            0.0
        } else {
            value
        });
        value += step;
        guard += 1;
    }
    out
}

/// Tick positions plus labels formatted as one axis-wide system.
#[derive(Debug, Clone, PartialEq)]
pub struct AxisTicks {
    pub values: Vec<f64>,
    pub labels: Vec<String>,
    pub scale_exponent: Option<i32>,
}

impl AxisTicks {
    pub fn empty() -> Self {
        Self {
            values: Vec::new(),
            labels: Vec::new(),
            scale_exponent: None,
        }
    }

    pub fn multiplier(&self) -> Option<String> {
        self.scale_exponent
            .map(|exponent| format!("×10{}", superscript(exponent)))
    }

    pub fn multiplier_clearance(&self, tick_pt: f32) -> f32 {
        if self.scale_exponent.is_some() {
            tick_pt + AXIS_LABEL_GAP
        } else {
            0.0
        }
    }
}

pub(crate) fn estimated_text_width(text: &str, font_size: f32) -> f32 {
    text.chars()
        .map(|ch| match ch {
            '0'..='9' => 0.56,
            '.' | ',' => 0.28,
            '-' | '−' => 0.36,
            _ => match ch.width() {
                None | Some(0) => 0.0,
                Some(1) => 0.58,
                Some(_) => 1.0,
            },
        })
        .sum::<f32>()
        * font_size
}

/// Ticks for an axis honoring its ordinal mode.
pub fn axis_ticks_for(axis: &Axis, target: usize) -> AxisTicks {
    if axis.categories.is_none() {
        return axis_ticks(axis.min, axis.max, target);
    }
    let visible: Vec<(f64, &str)> = visible_categories(axis).collect();
    let stride = visible.len().div_ceil(target.max(1)).max(1);
    let mut values = Vec::new();
    let mut labels = Vec::new();
    for (value, name) in visible.iter().step_by(stride) {
        values.push(*value);
        labels.push((*name).to_owned());
    }
    AxisTicks {
        values,
        labels,
        scale_exponent: None,
    }
}

fn visible_categories(axis: &Axis) -> impl Iterator<Item = (f64, &str)> {
    let (lo, hi) = (axis.min.min(axis.max), axis.min.max(axis.max));
    axis.categories
        .iter()
        .flat_map(|names| names.iter().enumerate())
        .filter_map(move |(index, name)| {
            let value = index as f64;
            (value >= lo - 1e-9 && value <= hi + 1e-9).then_some((value, name.as_str()))
        })
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
