pub(crate) fn point_ranges(points: &[[f64; 2]], include_zero: bool) -> ([f64; 2], [f64; 2]) {
    let mut x = [f64::INFINITY, f64::NEG_INFINITY];
    let mut y = if include_zero {
        [0.0, 0.0]
    } else {
        [f64::INFINITY, f64::NEG_INFINITY]
    };
    for point in points {
        if point[0].is_finite() {
            x = [x[0].min(point[0]), x[1].max(point[0])]
        }
        if point[1].is_finite() {
            y = [y[0].min(point[1]), y[1].max(point[1])]
        }
    }
    if !x[0].is_finite() || !x[1].is_finite() {
        x = [0.0, 1.0]
    } else if x[0] == x[1] {
        x = [x[0], x[0] + 1.0]
    }
    if !y[0].is_finite() || !y[1].is_finite() {
        y = [0.0, 1.0]
    } else if y[0] == y[1] {
        y = [y[0].min(0.0), y[0].max(0.0) + 1.0]
    }
    (x, y)
}
