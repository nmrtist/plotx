use crate::screen::ScreenRenderDetail;
use std::borrow::Cow;
use std::sync::Arc;

pub(super) const FULL_MIN_LINE_COLUMNS: usize = 2_048;
pub(super) const FULL_MAX_LINE_COLUMNS: usize = 16_384;
pub(super) const INTERACTIVE_MIN_LINE_COLUMNS: usize = 64;
pub(super) const INTERACTIVE_MAX_LINE_COLUMNS: usize = 2_048;
const INTERACTIVE_MIN_CONTOUR_SEGMENTS: usize = 512;
pub(super) const INTERACTIVE_MAX_CONTOUR_SEGMENTS: usize = 16_384;

pub(super) struct ScreenLinePoints<'a> {
    pub points: Cow<'a, [[f64; 2]]>,
    pub source_points_visited: usize,
    pub pooled: bool,
}

pub(super) type ContourSegment = [[f64; 2]; 2];

pub(super) struct ScreenContourSegments<'a> {
    source: &'a [ContourSegment],
    selected: Option<Arc<[usize]>>,
    budget: usize,
    source_segments_scanned: usize,
}

impl<'a> ScreenContourSegments<'a> {
    pub fn full(source: &'a [ContourSegment], source_segments_scanned: usize) -> Self {
        Self {
            source,
            selected: None,
            budget: usize::MAX,
            source_segments_scanned,
        }
    }

    pub fn from_lod(
        source: &'a [ContourSegment],
        selected: Arc<[usize]>,
        budget: usize,
        source_segments_scanned: usize,
    ) -> Self {
        Self {
            source,
            selected: Some(selected),
            budget,
            source_segments_scanned,
        }
    }

    pub fn source_segments_scanned(&self) -> usize {
        self.source_segments_scanned
    }

    pub fn len(&self) -> usize {
        self.selected
            .as_ref()
            .map_or(self.source.len(), |selected| {
                selected.len().min(self.budget)
            })
    }

    pub fn iter(&'a self) -> impl Iterator<Item = &'a ContourSegment> + 'a {
        (0..self.len()).map(|slot| {
            let index = self.selected.as_ref().map_or(slot, |selected| {
                if selected.len() <= self.budget {
                    selected[slot]
                } else {
                    selected[sampled_segment_index(selected.len(), self.budget, slot)]
                }
            });
            &self.source[index]
        })
    }
}

pub(super) fn line_columns(
    detail: ScreenRenderDetail,
    plot_width: f32,
    pixels_per_point: f32,
) -> usize {
    match detail {
        ScreenRenderDetail::Full => ((plot_width * pixels_per_point.max(1.0)).max(1.0) as usize)
            .saturating_mul(2)
            .clamp(FULL_MIN_LINE_COLUMNS, FULL_MAX_LINE_COLUMNS),
        ScreenRenderDetail::Interactive => {
            let scale = valid_pixel_scale(pixels_per_point);
            ((plot_width * scale).max(1.0).ceil() as usize)
                .clamp(INTERACTIVE_MIN_LINE_COLUMNS, INTERACTIVE_MAX_LINE_COLUMNS)
        }
    }
}

/// Clip to the viewport, then pool dense lines into min/max envelope buckets.
pub(super) fn screen_line_points(
    points: &[[f64; 2]],
    x_min: f64,
    x_max: f64,
    columns: usize,
) -> ScreenLinePoints<'_> {
    // Keep one neighbour on each side for continuity. Handles ascending traces
    // (time) and descending ones (NMR ppm); a flat or non-monotonic series keeps
    // its whole extent, which is safe but less selective.
    let first_x = points.first().map(|p| p[0]);
    let last_x = points.last().map(|p| p[0]);
    let (start, end) = match (first_x, last_x) {
        (Some(first), Some(last)) if first < last => {
            let start = points
                .partition_point(|point| point[0] < x_min)
                .saturating_sub(1);
            let end = points
                .partition_point(|point| point[0] <= x_max)
                .saturating_add(1)
                .min(points.len());
            (start.min(end), end)
        }
        (Some(first), Some(last)) if first > last => {
            let start = points
                .partition_point(|point| point[0] > x_max)
                .saturating_sub(1);
            let end = points
                .partition_point(|point| point[0] >= x_min)
                .saturating_add(1)
                .min(points.len());
            (start.min(end), end)
        }
        _ => (0, points.len()),
    };
    let visible = &points[start..end];
    let source_points_visited = visible.len();
    if visible.len() <= columns.saturating_mul(2) {
        return ScreenLinePoints {
            points: Cow::Borrowed(visible),
            source_points_visited,
            pooled: false,
        };
    }

    let bucket_count = columns.max(1);
    let bucket_size = visible.len().div_ceil(bucket_count);
    let mut pooled = Vec::with_capacity(bucket_count * 2 + 2);
    pooled.push(visible[0]);
    for bucket in visible.chunks(bucket_size) {
        let mut min_index = 0;
        let mut max_index = 0;
        for index in 1..bucket.len() {
            if bucket[index][1] < bucket[min_index][1] {
                min_index = index;
            }
            if bucket[index][1] > bucket[max_index][1] {
                max_index = index;
            }
        }
        if min_index <= max_index {
            pooled.push(bucket[min_index]);
            if max_index != min_index {
                pooled.push(bucket[max_index]);
            }
        } else {
            pooled.push(bucket[max_index]);
            pooled.push(bucket[min_index]);
        }
    }
    if let Some(last) = visible.last()
        && pooled.last() != Some(last)
    {
        pooled.push(*last);
    }
    ScreenLinePoints {
        points: Cow::Owned(pooled),
        source_points_visited,
        pooled: true,
    }
}

pub(super) fn contour_segment_budget(
    detail: ScreenRenderDetail,
    plot_width: f32,
    plot_height: f32,
    pixels_per_point: f32,
) -> usize {
    if detail == ScreenRenderDetail::Full {
        return usize::MAX;
    }
    let scale = valid_pixel_scale(pixels_per_point);
    let physical_area = (plot_width * scale).max(1.0) * (plot_height * scale).max(1.0);
    ((physical_area / 8.0) as usize).clamp(
        INTERACTIVE_MIN_CONTOUR_SEGMENTS,
        INTERACTIVE_MAX_CONTOUR_SEGMENTS,
    )
}

pub(super) fn screen_contour_segments(
    segments: &[ContourSegment],
    detail: ScreenRenderDetail,
    viewport: [f64; 4],
    budget: usize,
) -> ScreenContourSegments<'_> {
    if detail == ScreenRenderDetail::Full {
        return ScreenContourSegments::full(segments, 0);
    }
    let selected = prepare_contour_lod(segments, viewport);
    ScreenContourSegments::from_lod(segments, selected, budget, segments.len())
}

pub(super) fn prepare_contour_lod(segments: &[ContourSegment], viewport: [f64; 4]) -> Arc<[usize]> {
    let [x_min, x_max, y_min, y_max] = viewport;
    let mut selected = segments
        .iter()
        .enumerate()
        .filter_map(|(index, segment)| {
            segment_intersects_viewport(segment, x_min, x_max, y_min, y_max).then_some(index)
        })
        .collect::<Vec<_>>();
    if selected.len() > INTERACTIVE_MAX_CONTOUR_SEGMENTS {
        let source_len = selected.len();
        for slot in 0..INTERACTIVE_MAX_CONTOUR_SEGMENTS {
            selected[slot] =
                selected[sampled_segment_index(source_len, INTERACTIVE_MAX_CONTOUR_SEGMENTS, slot)];
        }
        selected.truncate(INTERACTIVE_MAX_CONTOUR_SEGMENTS);
    }
    selected.into()
}

fn segment_intersects_viewport(
    segment: &ContourSegment,
    x_min: f64,
    x_max: f64,
    y_min: f64,
    y_max: f64,
) -> bool {
    let [a, b] = segment;
    if ![a[0], a[1], b[0], b[1], x_min, x_max, y_min, y_max]
        .into_iter()
        .all(f64::is_finite)
    {
        return false;
    }
    let (x_min, x_max) = (x_min.min(x_max), x_min.max(x_max));
    let (y_min, y_max) = (y_min.min(y_max), y_min.max(y_max));
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    let mut enter = 0.0_f64;
    let mut leave = 1.0_f64;
    for (direction, distance) in [
        (-dx, a[0] - x_min),
        (dx, x_max - a[0]),
        (-dy, a[1] - y_min),
        (dy, y_max - a[1]),
    ] {
        if direction == 0.0 {
            if distance < 0.0 {
                return false;
            }
            continue;
        }
        let crossing = distance / direction;
        if direction < 0.0 {
            enter = enter.max(crossing);
        } else {
            leave = leave.min(crossing);
        }
        if enter > leave {
            return false;
        }
    }
    true
}

fn valid_pixel_scale(pixels_per_point: f32) -> f32 {
    if pixels_per_point.is_finite() && pixels_per_point > 0.0 {
        pixels_per_point
    } else {
        1.0
    }
}

/// Select the middle segment from each equal-length source block. Integer
/// boundaries make the selection stable and spread it across the full contour.
pub(super) fn sampled_segment_index(source_len: usize, budget: usize, slot: usize) -> usize {
    debug_assert!(budget > 0 && budget < source_len && slot < budget);
    let start = slot * source_len / budget;
    let end = (slot + 1) * source_len / budget;
    start + (end - start) / 2
}
