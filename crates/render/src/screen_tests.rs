use super::{MAX_LINE_COLUMNS, MIN_LINE_COLUMNS, line_columns, screen_line_points};

#[test]
fn long_trace_is_bounded_and_keeps_narrow_extrema() {
    let mut points: Vec<_> = (0..2_000_000).map(|index| [index as f64, 0.0]).collect();
    points[1_234_567][1] = -42.0;
    let pooled = screen_line_points(&points, 0.0, 2_000_000.0, 2_000);
    assert!(pooled.len() <= 4_002);
    assert!(pooled.iter().any(|point| point[1] == -42.0));
}

#[test]
fn spectrum_sized_trace_is_pooled_to_the_screen_budget() {
    let points: Vec<_> = (0..32_768)
        .map(|index| [index as f64, (index % 7) as f64])
        .collect();
    let drawn = screen_line_points(&points, 0.0, 32_768.0, MIN_LINE_COLUMNS);
    assert!(drawn.len() <= MIN_LINE_COLUMNS * 2 + 2);
    assert!(matches!(drawn, std::borrow::Cow::Owned(_)));
}

#[test]
fn short_trace_keeps_its_real_samples() {
    let points: Vec<_> = (0..2_000)
        .map(|index| [index as f64, (index % 7) as f64])
        .collect();
    let drawn = screen_line_points(&points, 0.0, 2_000.0, MIN_LINE_COLUMNS);
    assert!(drawn.as_ref() == points.as_slice());
    assert!(matches!(drawn, std::borrow::Cow::Borrowed(_)));
}

#[test]
fn pooling_keeps_positive_and_negative_extrema() {
    let mut points: Vec<_> = (0..20_000).map(|index| [index as f64, 0.0]).collect();
    points[4_321][1] = -17.0;
    points[12_345][1] = 23.0;
    let drawn = screen_line_points(&points, 0.0, 20_000.0, MIN_LINE_COLUMNS);
    assert!(drawn.iter().any(|point| point[1] == -17.0));
    assert!(drawn.iter().any(|point| point[1] == 23.0));
}

#[test]
fn zoomed_view_keeps_only_visible_samples() {
    let points: Vec<_> = (0..100_000).map(|index| [index as f64, 1.0]).collect();
    let drawn = screen_line_points(&points, 40_000.0, 41_000.0, MIN_LINE_COLUMNS);
    assert!(drawn.len() < 1_100);
    assert!(drawn.first().unwrap()[0] < 40_000.0);
    assert!(drawn.last().unwrap()[0] > 41_000.0);
}

#[test]
fn descending_x_view_clips_like_nmr_ppm() {
    let points: Vec<_> = (0..100_000)
        .map(|index| [(100_000 - index) as f64, 1.0])
        .collect();
    let drawn = screen_line_points(&points, 40_000.0, 41_000.0, MIN_LINE_COLUMNS);
    assert!(drawn.len() < 1_100);
    assert!(drawn.first().unwrap()[0] > 41_000.0);
    assert!(drawn.last().unwrap()[0] < 40_000.0);
}

#[test]
fn non_monotonic_x_is_pooled_without_unsafe_clipping() {
    let mut points: Vec<_> = (0..20_000)
        .map(|index| [((index * 37) % 101) as f64, index as f64])
        .collect();
    let first_x = points[0][0];
    points.last_mut().unwrap()[0] = first_x;
    let drawn = screen_line_points(&points, 4_000.0, 5_000.0, MIN_LINE_COLUMNS);
    assert!(drawn.len() <= MIN_LINE_COLUMNS * 2 + 2);
    assert_eq!(drawn.first(), points.first());
    assert_eq!(drawn.last(), points.last());
}

#[test]
fn columns_track_device_pixels_within_bounds() {
    assert_eq!(line_columns(320.0, 1.0), MIN_LINE_COLUMNS);
    assert_eq!(line_columns(900.0, 2.0), 3_600);
    assert_eq!(line_columns(9_000.0, 2.0), MAX_LINE_COLUMNS);
}
