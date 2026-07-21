//! Marching-squares contour extraction: turns a row-major intensity grid and a
//! set of levels into data-space line segments for a contour plot.

/// Contour line segments (each `[[x0,y0],[x1,y1]]`, in data space) for every
/// `level` crossing of the `rows × cols` grid `z`. Columns map linearly onto
/// `[x0, x1]` and rows onto `[y0, y1]`.
#[allow(clippy::too_many_arguments)]
pub fn segments(
    z: &[f32],
    rows: usize,
    cols: usize,
    x0: f64,
    x1: f64,
    y0: f64,
    y1: f64,
    levels: &[f64],
) -> Vec<[[f64; 2]; 2]> {
    segments_cancellable(z, rows, cols, x0, x1, y0, y1, levels, &|| false)
        .expect("non-cancelling contour extraction")
}

#[allow(clippy::too_many_arguments)]
pub fn segments_cancellable(
    z: &[f32],
    rows: usize,
    cols: usize,
    x0: f64,
    x1: f64,
    y0: f64,
    y1: f64,
    levels: &[f64],
    cancelled: &impl Fn() -> bool,
) -> Option<Vec<[[f64; 2]; 2]>> {
    let mut out = Vec::new();
    if rows < 2 || cols < 2 || z.len() < rows * cols {
        return Some(out);
    }
    let gx = |colf: f64| x0 + (x1 - x0) * colf / (cols - 1) as f64;
    let gy = |rowf: f64| y0 + (y1 - y0) * rowf / (rows - 1) as f64;
    let at = |r: usize, c: usize| z[r * cols + c] as f64;

    for &level in levels {
        for r in 0..rows - 1 {
            if cancelled() {
                return None;
            }
            for c in 0..cols - 1 {
                let nw = at(r, c);
                let ne = at(r, c + 1);
                let sw = at(r + 1, c);
                let se = at(r + 1, c + 1);

                let case = (sw >= level) as u8
                    | (((se >= level) as u8) << 1)
                    | (((ne >= level) as u8) << 2)
                    | (((nw >= level) as u8) << 3);
                if case == 0 || case == 15 {
                    continue;
                }

                // Edge crossings as fractional (row, col) grid coordinates.
                let interp = |a: f64, b: f64| (level - a) / (b - a);
                let top = || [r as f64, c as f64 + interp(nw, ne)];
                let bottom = || [r as f64 + 1.0, c as f64 + interp(sw, se)];
                let left = || [r as f64 + interp(nw, sw), c as f64];
                let right = || [r as f64 + interp(ne, se), c as f64 + 1.0];

                let mut push = |a: [f64; 2], b: [f64; 2]| {
                    out.push([[gx(a[1]), gy(a[0])], [gx(b[1]), gy(b[0])]]);
                };

                match case {
                    1 | 14 => push(left(), bottom()),
                    2 | 13 => push(bottom(), right()),
                    3 | 12 => push(left(), right()),
                    4 | 11 => push(top(), right()),
                    6 | 9 => push(bottom(), top()),
                    7 | 8 => push(left(), top()),
                    5 => {
                        push(left(), top());
                        push(bottom(), right());
                    }
                    10 => {
                        push(left(), bottom());
                        push(top(), right());
                    }
                    _ => {}
                }
            }
        }
    }
    Some(out)
}

/// A geometric ladder of `count` positive contour levels between `base` (the
/// lowest drawn contour) and `peak`, each `1/ratio` of the next. Returns nothing
/// if the inputs are degenerate.
pub fn geometric_levels(base: f64, peak: f64, count: usize, ratio: f64) -> Vec<f64> {
    if base <= 0.0 || base.is_nan() || peak <= base || count == 0 || ratio <= 1.0 {
        return Vec::new();
    }
    let mut levels = Vec::with_capacity(count);
    let mut v = base;
    for _ in 0..count {
        if v > peak {
            break;
        }
        levels.push(v);
        v *= ratio;
    }
    levels
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn geometric_levels_ladder() {
        let l = geometric_levels(1.0, 10.0, 5, 2.0);
        assert_eq!(l, vec![1.0, 2.0, 4.0, 8.0]);
        assert!(geometric_levels(0.0, 10.0, 5, 2.0).is_empty());
        assert!(geometric_levels(1.0, 1.0, 5, 2.0).is_empty());
    }

    #[test]
    fn flat_grid_has_no_contours() {
        let z = vec![0.0f32; 9];
        assert!(segments(&z, 3, 3, 0.0, 1.0, 0.0, 1.0, &[0.5]).is_empty());
    }

    #[test]
    fn single_peak_cell_yields_a_closed_ring() {
        // One central cell above the level: the contour circles it with 4 edges.
        let z = vec![
            0.0, 0.0, 0.0, //
            0.0, 1.0, 0.0, //
            0.0, 0.0, 0.0,
        ];
        let segs = segments(&z, 3, 3, 0.0, 2.0, 0.0, 2.0, &[0.5]);
        assert_eq!(segs.len(), 4, "a lone peak is ringed by 4 segments");
        // Every crossing sits at the mid-value (0.5) → midpoints of edges, all
        // inside the unit ring around the centre (1,1) in data space.
        for [a, b] in &segs {
            for p in [a, b] {
                assert!(p[0] >= 0.4 && p[0] <= 1.6 && p[1] >= 0.4 && p[1] <= 1.6);
            }
        }
    }
}
