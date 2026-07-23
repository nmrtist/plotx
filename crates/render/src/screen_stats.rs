#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RenderStats {
    pub documents_painted: usize,
    pub line_series_visited: usize,
    /// Source points in the x-visible slice inspected by line pooling. This is
    /// zero when pooling is unnecessary.
    pub line_source_points_scanned: usize,
    pub line_points_emitted: usize,
}

pub(crate) fn visible_source_len(points: &[[f64; 2]], x_min: f64, x_max: f64) -> usize {
    let first_x = points.first().map(|p| p[0]);
    let last_x = points.last().map(|p| p[0]);
    let (start, end) = match (first_x, last_x) {
        (Some(first), Some(last)) if first < last => (
            points.partition_point(|p| p[0] < x_min).saturating_sub(1),
            points
                .partition_point(|p| p[0] <= x_max)
                .saturating_add(1)
                .min(points.len()),
        ),
        (Some(first), Some(last)) if first > last => (
            points.partition_point(|p| p[0] > x_max).saturating_sub(1),
            points
                .partition_point(|p| p[0] >= x_min)
                .saturating_add(1)
                .min(points.len()),
        ),
        _ => (0, points.len()),
    };
    end.saturating_sub(start.min(end))
}
