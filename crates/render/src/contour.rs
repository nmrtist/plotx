//! Marching-squares contour extraction: turns a row-major intensity grid and a
//! set of levels into data-space line segments for a contour plot.

/// One contour line segment in data space, `[[x0, y0], [x1, y1]]`.
pub type Segment = [[f64; 2]; 2];

/// The most contour line segments one figure's geometry may carry.
///
/// This is a hard device limit, not a taste threshold. Every drawn segment is
/// tessellated into its own feathered quad strip, which costs 30 `u32` indices
/// in the frame's index buffer — a figure of 8.4 million segments, measured on
/// a 2048×8192 spectrum whose lowest level sat 5σ above a thermal-noise
/// estimate, asked for a 1.0 GB buffer against wgpu's 256 MiB default
/// `max_buffer_size`. Exceeding that maximum is a validation error that aborts
/// the process, so it must be impossible to reach rather than merely unlikely.
///
/// 256 MiB is 67,108,864 indices, about 2.2 million segments for *everything* a
/// frame draws. Budgeting 250,000 per contour geometry keeps a page of eight
/// contour plots inside the limit alongside the rest of the interface, and
/// keeps one plot's tessellation tractable besides.
///
/// The budget bounds a whole geometry rather than a single level: the levels of
/// one ladder are drawn together or not at all, so a per-level cap would still
/// let a ladder overrun the buffer.
pub const MAX_CONTOUR_SEGMENTS: usize = 250_000;

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
) -> Vec<Segment> {
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
) -> Option<Vec<Segment>> {
    let mut out = Vec::new();
    for &level in levels {
        // These callers draw everything they ask for, so the budget is opted
        // out of rather than ignored: `usize::MAX` can never be reached, and
        // the "fits" answer is constant.
        level_segments_into(
            z,
            rows,
            cols,
            x0,
            x1,
            y0,
            y1,
            level,
            &mut out,
            usize::MAX,
            cancelled,
        )?;
    }
    Some(out)
}

/// Append one `level`'s crossings of the `rows × cols` grid `z` to `out`,
/// stopping once `out` would grow past `limit` segments.
///
/// Extracting a single level is what lets a caller decide *per level* whether
/// the result still fits a budget it owns: a caller that must stay under
/// [`MAX_CONTOUR_SEGMENTS`] can measure a level, then keep or discard the whole
/// level. Truncating mid-level would be indistinguishable from a contour that
/// genuinely stops there, which is why this is the smallest unit offered — and
/// why `limit` reports rather than returns a short level. A level that crosses
/// a whole large grid is tens of millions of segments and a gigabyte of
/// scratch, so a caller that is going to reject it must be able to stop paying
/// for it as soon as the verdict is settled.
///
/// Returns `Some(true)` when the level is complete, `Some(false)` when `limit`
/// stopped it part-way — `out` then holds a partial level the caller must
/// discard — and `None` when `cancelled` fired, with the same obligation.
#[allow(clippy::too_many_arguments)]
pub fn level_segments_into(
    z: &[f32],
    rows: usize,
    cols: usize,
    x0: f64,
    x1: f64,
    y0: f64,
    y1: f64,
    level: f64,
    out: &mut Vec<Segment>,
    limit: usize,
    cancelled: &impl Fn() -> bool,
) -> Option<bool> {
    if rows < 2 || cols < 2 || z.len() < rows * cols {
        return Some(true);
    }
    let gx = |colf: f64| x0 + (x1 - x0) * colf / (cols - 1) as f64;
    let gy = |rowf: f64| y0 + (y1 - y0) * rowf / (rows - 1) as f64;
    let at = |r: usize, c: usize| z[r * cols + c] as f64;

    for r in 0..rows - 1 {
        if cancelled() {
            return None;
        }
        // Tested once per row rather than once per cell: the overshoot is at
        // most one row of crossings, and the check costs nothing against the
        // row it guards.
        if out.len() > limit {
            return Some(false);
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
    Some(out.len() <= limit)
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
