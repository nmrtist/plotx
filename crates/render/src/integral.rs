//! Shared 1D NMR integral geometry and label layout.

use crate::Rect;
use plotx_figure::{Axis, Color, Figure, IntegralCurve};

const CANCELLATION_EPS: f64 = 1e-12;
const CURVE_EDGE_INSET: f32 = 3.0;
const LABEL_FONT_SIZE: f32 = 6.0;
const LABEL_GAP: f32 = 2.0;

#[derive(Debug, Clone, PartialEq)]
pub struct IntegralLabelLayout {
    pub text: String,
    pub position: (f32, f32),
    pub font_size: f32,
    pub color: Color,
}

#[derive(Debug, Clone, PartialEq)]
pub struct IntegralPathLayout {
    pub points: Vec<(f32, f32)>,
    pub color: Color,
    pub width: f32,
    pub label: IntegralLabelLayout,
}

/// Cumulative values in screen-left-to-right order. Trapezoids use absolute
/// x spacing, so reversing the ppm axis changes presentation order, not sign.
pub fn cumulative_values(
    curve: &IntegralCurve,
    source: &[[f64; 2]],
    x_axis: &Axis,
) -> Vec<[f64; 2]> {
    let lo = curve.start_ppm.min(curve.end_ppm);
    let hi = curve.start_ppm.max(curve.end_ppm);
    let mut samples: Vec<[f64; 2]> = source
        .iter()
        .copied()
        .filter(|p| p[0] >= lo && p[0] <= hi)
        .collect();

    // Spectrum points are normally monotonic in ppm. Preserve that order (or
    // reverse it for the screen axis) instead of sorting every integral on every
    // repaint. Retain a robust fallback for hand-built, non-monotonic figures.
    let monotonic = samples.windows(2).all(|pair| pair[0][0] <= pair[1][0])
        || samples.windows(2).all(|pair| pair[0][0] >= pair[1][0]);
    if monotonic {
        if samples
            .first()
            .zip(samples.last())
            .is_some_and(|(first, last)| x_axis.normalize(first[0]) > x_axis.normalize(last[0]))
        {
            samples.reverse();
        }
    } else {
        samples.sort_by(|a, b| x_axis.normalize(a[0]).total_cmp(&x_axis.normalize(b[0])));
    }

    if samples.len() < 2 || samples.iter().flatten().any(|v| !v.is_finite()) {
        return zero_values(curve, x_axis);
    }

    let mut cumulative = Vec::with_capacity(samples.len());
    cumulative.push([samples[0][0], 0.0]);
    let mut total = 0.0;
    let mut absolute_scale = 0.0;
    for pair in samples.windows(2) {
        let dx = (pair[1][0] - pair[0][0]).abs();
        let contribution = 0.5 * (pair[0][1] + pair[1][1]) * dx;
        total += contribution;
        absolute_scale += contribution.abs();
        cumulative.push([pair[1][0], total]);
    }
    if !total.is_finite()
        || !absolute_scale.is_finite()
        || !curve.normalized_area.is_finite()
        || total.abs() <= CANCELLATION_EPS * absolute_scale
    {
        return zero_values(curve, x_axis);
    }
    let scale = curve.normalized_area / total;
    for point in &mut cumulative {
        point[1] *= scale;
    }
    cumulative
}

fn zero_values(curve: &IntegralCurve, x_axis: &Axis) -> Vec<[f64; 2]> {
    let mut endpoints = [[curve.start_ppm, 0.0], [curve.end_ppm, 0.0]];
    if x_axis.normalize(endpoints[0][0]) > x_axis.normalize(endpoints[1][0]) {
        endpoints.reverse();
    }
    endpoints.to_vec()
}

/// Lay out every integral in output space. All paths share a zero line and
/// vertical scale; their band grows gently with count and remains 20%–50% of
/// the viewport height.
pub fn layout(fig: &Figure, plot: Rect, output_scale: f32) -> Vec<IntegralPathLayout> {
    let values: Vec<(&IntegralCurve, Vec<[f64; 2]>)> = fig
        .integral_curves
        .iter()
        .map(|curve| {
            let points = fig.series.get(curve.source_series).map_or_else(
                || zero_values(curve, &fig.x),
                |series| cumulative_values(curve, &series.points, &fig.x),
            );
            (curve, points)
        })
        .collect();
    if values.is_empty() {
        return Vec::new();
    }

    let (mut min_value, mut max_value) = (0.0f64, 0.0f64);
    for (_, points) in &values {
        for point in points {
            min_value = min_value.min(point[1]);
            max_value = max_value.max(point[1]);
        }
    }
    let band_fraction = (0.2 + values.len().saturating_sub(1) as f32 * 0.03).clamp(0.2, 0.5);
    let band_height = plot.height * band_fraction;
    let edge_inset = (CURVE_EDGE_INSET * output_scale).min(band_height * 0.1);
    let curve_height = (band_height - edge_inset * 2.0).max(0.0);
    let curve_top = plot.top + edge_inset;
    let zero_y = if min_value < 0.0 && max_value > 0.0 {
        curve_top + curve_height * (max_value / (max_value - min_value)) as f32
    } else if min_value < 0.0 {
        curve_top
    } else {
        curve_top + curve_height
    };
    let scale = if min_value < 0.0 && max_value > 0.0 {
        curve_height / (max_value - min_value) as f32
    } else {
        let extent = max_value.abs().max(min_value.abs());
        if extent.is_finite() && extent > 0.0 {
            curve_height / extent as f32
        } else {
            0.0
        }
    };

    let mut paths: Vec<_> = values
        .into_iter()
        .map(|(curve, points)| {
            let points: Vec<(f32, f32)> = points
                .into_iter()
                .map(|point| {
                    let x = plot.left + fig.x.normalize(point[0]) as f32 * plot.width;
                    (x, zero_y - point[1] as f32 * scale)
                })
                .collect();
            let end = points.last().copied().unwrap_or((plot.left, zero_y));
            IntegralPathLayout {
                label: label_layout(&curve.label, end, curve.color, plot, output_scale),
                points,
                color: curve.color,
                width: curve.width,
            }
        })
        .collect();
    avoid_label_overlaps(&mut paths, plot, output_scale);
    paths
}

/// Backend-independent vertical placement immediately to the right of the
/// screen-right curve end.
pub fn label_layout(
    text: &str,
    end: (f32, f32),
    color: Color,
    plot: Rect,
    output_scale: f32,
) -> IntegralLabelLayout {
    let font_size = LABEL_FONT_SIZE * output_scale;
    let label_height = estimated_rotated_height(text, font_size);
    let half_width = font_size * 0.5;
    let half_height = label_height * 0.5;
    IntegralLabelLayout {
        text: text.to_owned(),
        position: (
            (end.0 + LABEL_GAP * output_scale + half_width)
                .clamp(plot.left + half_width, plot.right() - half_width),
            end.1
                .clamp(plot.top + half_height, plot.bottom() - half_height),
        ),
        font_size,
        color,
    }
}

fn estimated_rotated_height(text: &str, font_size: f32) -> f32 {
    text.chars().count() as f32 * font_size * 0.58
}

fn avoid_label_overlaps(paths: &mut [IntegralPathLayout], plot: Rect, output_scale: f32) {
    let gap = LABEL_GAP * output_scale;
    let mut placed: Vec<(f32, f32, f32, f32)> = Vec::new();
    let max_attempts = paths.len() * 2;
    for path in paths {
        let width = path.label.font_size;
        let height = estimated_rotated_height(&path.label.text, path.label.font_size);
        let original_y = path.label.position.1;
        let step = height + gap * 2.0;
        let mut chosen = original_y;
        for attempt in 0..=max_attempts {
            let offset = if attempt == 0 {
                0.0
            } else {
                let distance = attempt.div_ceil(2) as f32 * step;
                if attempt % 2 == 1 {
                    distance
                } else {
                    -distance
                }
            };
            let candidate =
                (original_y + offset).clamp(plot.top + height * 0.5, plot.bottom() - height * 0.5);
            let bounds = (
                path.label.position.0 - width * 0.5 - gap,
                candidate - height * 0.5 - gap,
                path.label.position.0 + width * 0.5 + gap,
                candidate + height * 0.5 + gap,
            );
            if placed.iter().all(|other| !rects_overlap(bounds, *other)) {
                chosen = candidate;
                placed.push(bounds);
                break;
            }
        }
        path.label.position.1 = chosen;
    }
}

fn rects_overlap(a: (f32, f32, f32, f32), b: (f32, f32, f32, f32)) -> bool {
    a.0 < b.2 && a.2 > b.0 && a.1 < b.3 && a.3 > b.1
}

#[cfg(test)]
mod tests {
    use super::*;

    fn curve(target: f64) -> IntegralCurve {
        IntegralCurve {
            start_ppm: 0.0,
            end_ppm: 3.0,
            normalized_area: target,
            label: format!("{target:.3}"),
            color: Color::TRACE,
            width: 1.0,
            source_series: 0,
        }
    }

    #[test]
    fn screen_order_honors_forward_and_reversed_axes() {
        let source = [[0.0, 1.0], [1.0, 1.0], [3.0, 1.0]];
        let forward = Axis::new("x", 0.0, 3.0);
        let reversed = Axis::new("x", 0.0, 3.0).reversed(true);
        assert_eq!(cumulative_values(&curve(3.0), &source, &forward)[0][0], 0.0);
        assert_eq!(
            cumulative_values(&curve(3.0), &source, &reversed)[0][0],
            3.0
        );
    }

    #[test]
    fn nonuniform_trapezoids_reach_the_exact_target() {
        let source = [[0.0, 0.0], [1.0, 2.0], [3.0, 2.0]];
        let values = cumulative_values(&curve(7.0), &source, &Axis::new("x", 0.0, 3.0));
        assert_eq!(values[1][1], 1.4);
        assert_eq!(values.last().unwrap()[1], 7.0);
    }

    #[test]
    fn negative_target_accumulates_downward() {
        let values = cumulative_values(
            &curve(-2.0),
            &[[0.0, 1.0], [1.0, 1.0], [3.0, 1.0]],
            &Axis::new("x", 0.0, 3.0),
        );
        assert_eq!(values.last().unwrap()[1], -2.0);
    }

    #[test]
    fn unstable_and_short_inputs_become_horizontal_zero() {
        let axis = Axis::new("x", 0.0, 3.0);
        for source in [
            vec![[0.0, 1.0]],
            vec![[0.0, f64::NAN], [3.0, 1.0]],
            vec![[0.0, 0.0], [3.0, 0.0]],
            vec![[0.0, 1.0], [1.0, -1.0]],
            vec![
                [0.0, 1.0],
                [1.0, 1.0],
                [2.0, -1.0 + 1e-13],
                [3.0, -1.0 + 1e-13],
            ],
        ] {
            assert!(
                cumulative_values(&curve(1.0), &source, &axis)
                    .iter()
                    .all(|p| p[1] == 0.0)
            );
        }
    }

    #[test]
    fn non_monotonic_sources_use_the_ordering_fallback() {
        let values = cumulative_values(
            &curve(3.0),
            &[[3.0, 1.0], [0.0, 1.0], [1.0, 1.0]],
            &Axis::new("x", 0.0, 3.0),
        );
        assert_eq!(
            values.iter().map(|point| point[0]).collect::<Vec<_>>(),
            vec![0.0, 1.0, 3.0]
        );
    }

    #[test]
    fn all_zero_layout_has_only_finite_coordinates() {
        use plotx_figure::{Figure, Series};
        let mut fig =
            Figure::new("", Axis::new("x", 0.0, 3.0), Axis::new("y", 0.0, 1.0)).with_series(
                Series::line("spectrum", vec![[0.0, 0.0], [1.0, 0.0], [3.0, 0.0]]),
            );
        fig.integral_curves = vec![curve(1.0)];

        let paths = layout(&fig, Rect::new(0.0, 0.0, 300.0, 100.0), 1.0);

        assert!(
            paths[0]
                .points
                .iter()
                .all(|(x, y)| x.is_finite() && y.is_finite())
        );
        assert!(
            paths[0]
                .points
                .windows(2)
                .all(|points| points[0].1 == points[1].1)
        );
    }

    #[test]
    fn layouts_share_zero_and_scale_for_mixed_signs() {
        use plotx_figure::{Figure, Series};
        let mut fig =
            Figure::new("", Axis::new("x", 0.0, 3.0), Axis::new("y", 0.0, 1.0)).with_series(
                Series::line("spectrum", vec![[0.0, 1.0], [1.0, 1.0], [3.0, 1.0]]),
            );
        fig.integral_curves = vec![curve(2.0), curve(-1.0)];
        let paths = layout(&fig, Rect::new(0.0, 0.0, 300.0, 100.0), 1.0);
        assert_eq!(paths[0].points[0].1, paths[1].points[0].1);
        let zero = paths[0].points[0].1;
        let positive = paths[0].points.last().unwrap().1;
        let negative = paths[1].points.last().unwrap().1;
        assert!(((zero - positive) / (negative - zero) - 2.0).abs() < 1e-5);
    }

    #[test]
    fn tallest_curve_keeps_clear_of_the_viewport_edge() {
        use plotx_figure::{Figure, Series};
        let mut fig =
            Figure::new("", Axis::new("x", 0.0, 3.0), Axis::new("y", 0.0, 1.0)).with_series(
                Series::line("spectrum", vec![[0.0, 1.0], [1.0, 1.0], [3.0, 1.0]]),
            );
        fig.integral_curves = vec![curve(2.0)];
        let paths = layout(&fig, Rect::new(0.0, 0.0, 300.0, 100.0), 1.0);
        assert!(paths[0].points.iter().all(|point| point.1 > 0.0));
    }

    #[test]
    fn coincident_vertical_labels_are_separated() {
        use plotx_figure::{Figure, Series};
        let mut fig =
            Figure::new("", Axis::new("x", 0.0, 4.0), Axis::new("y", 0.0, 1.0)).with_series(
                Series::line("spectrum", vec![[0.0, 1.0], [1.0, 1.0], [3.0, 1.0]]),
            );
        fig.integral_curves = vec![curve(2.0), curve(2.0)];
        let paths = layout(&fig, Rect::new(0.0, 0.0, 400.0, 140.0), 1.0);
        assert_eq!(paths[0].label.position.0, paths[1].label.position.0);
        assert_ne!(paths[0].label.position.1, paths[1].label.position.1);
        assert_eq!(paths[0].label.font_size, 6.0);
    }
}
