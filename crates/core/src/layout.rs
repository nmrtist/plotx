//! Page-layout geometry: a panel grid plus the snapping math that pulls a dragged
//! or resized object onto page edges, centre lines, margins and cell boundaries.

use crate::state::{MM_TO_PT, ObjectFrame, ObjectId};

/// Grid presets offered in the Arrange menu, as `(label, rows, cols)`.
pub const GRID_PRESETS: &[(&str, u32, u32)] = &[
    ("1 × 1", 1, 1),
    ("1 × 2", 1, 2),
    ("2 × 1", 2, 1),
    ("2 × 2", 2, 2),
    ("2 × 3", 2, 3),
    ("3 × 2", 3, 2),
    ("4 × 2", 4, 2),
];

/// Per-canvas layout: outer margins, the gutter between cells, and the current
/// panel grid divisions. `show_grid` is a view preference kept alongside so it
/// travels with the page in `.plotx`. Sizes are stored in mm to match the
/// canvas size and its unit conversions.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PageLayout {
    /// Outer margins in mm, ordered top, right, bottom, left.
    pub margin_mm: [f32; 4],
    pub gutter_mm: f32,
    pub rows: u32,
    pub cols: u32,
    pub show_grid: bool,
}

impl Default for PageLayout {
    fn default() -> Self {
        Self {
            margin_mm: [0.0, 0.0, 0.0, 0.0],
            gutter_mm: 5.0,
            rows: 1,
            cols: 1,
            show_grid: false,
        }
    }
}

impl PageLayout {
    fn margins_pt(&self) -> [f32; 4] {
        [
            self.margin_mm[0] * MM_TO_PT,
            self.margin_mm[1] * MM_TO_PT,
            self.margin_mm[2] * MM_TO_PT,
            self.margin_mm[3] * MM_TO_PT,
        ]
    }

    fn gutter_pt(&self) -> f32 {
        self.gutter_mm * MM_TO_PT
    }

    fn cell_size(&self, page_pt: [f32; 2]) -> (f32, f32) {
        let rows = self.rows.max(1) as f32;
        let cols = self.cols.max(1) as f32;
        let [mt, mr, mb, ml] = self.margins_pt();
        let g = self.gutter_pt();
        let avail_w = (page_pt[0] - ml - mr - g * (cols - 1.0)).max(1.0);
        let avail_h = (page_pt[1] - mt - mb - g * (rows - 1.0)).max(1.0);
        (avail_w / cols, avail_h / rows)
    }
}

/// The `row * cols` cell rectangles in row-major order (left→right, top→bottom).
pub fn grid_frames(page_pt: [f32; 2], layout: &PageLayout) -> Vec<ObjectFrame> {
    let rows = layout.rows.max(1);
    let cols = layout.cols.max(1);
    let [mt, _mr, _mb, ml] = layout.margins_pt();
    let g = layout.gutter_pt();
    let (cell_w, cell_h) = layout.cell_size(page_pt);
    let mut frames = Vec::with_capacity((rows * cols) as usize);
    for r in 0..rows {
        for c in 0..cols {
            let x = ml + c as f32 * (cell_w + g);
            let y = mt + r as f32 * (cell_h + g);
            frames.push(ObjectFrame::new(x, y, cell_w, cell_h));
        }
    }
    frames
}

/// A guide line drawn while a drag snaps. `vertical` lines are at a fixed x
/// (page space); horizontal lines are at a fixed y.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SnapGuide {
    pub vertical: bool,
    pub pos: f32,
}

/// Candidate snap lines in page space: `xs` for vertical edges, `ys` for
/// horizontal edges.
#[derive(Clone, Debug, Default)]
pub struct SnapTargets {
    pub xs: Vec<f32>,
    pub ys: Vec<f32>,
}

impl SnapTargets {
    /// Page-derived targets: edges, centre lines, margins and cell boundaries.
    pub fn from_page(page_pt: [f32; 2], layout: &PageLayout) -> Self {
        let w = page_pt[0];
        let h = page_pt[1];
        let [mt, mr, mb, ml] = layout.margins_pt();
        let mut xs = vec![0.0, w, w * 0.5, ml, w - mr];
        let mut ys = vec![0.0, h, h * 0.5, mt, h - mb];
        let g = layout.gutter_pt();
        let (cell_w, cell_h) = layout.cell_size(page_pt);
        for c in 0..layout.cols.max(1) {
            let left = ml + c as f32 * (cell_w + g);
            xs.push(left);
            xs.push(left + cell_w);
        }
        for r in 0..layout.rows.max(1) {
            let top = mt + r as f32 * (cell_h + g);
            ys.push(top);
            ys.push(top + cell_h);
        }
        Self { xs, ys }
    }

    pub fn push_object(&mut self, frame: ObjectFrame) {
        self.xs.push(frame.x);
        self.xs.push(frame.x + frame.width * 0.5);
        self.xs.push(frame.x + frame.width);
        self.ys.push(frame.y);
        self.ys.push(frame.y + frame.height * 0.5);
        self.ys.push(frame.y + frame.height);
    }
}

/// Which edges of a frame a resize moves; the opposite edges stay anchored.
#[derive(Clone, Copy, Debug, Default)]
pub struct MovableEdges {
    pub left: bool,
    pub right: bool,
    pub top: bool,
    pub bottom: bool,
}

/// The best correction pulling one of `edges` onto a target within `threshold`,
/// as `(delta_to_apply, snapped_target_pos)`; `None` when nothing is in range.
fn best_snap(edges: &[f32], targets: &[f32], threshold: f32) -> Option<(f32, f32)> {
    let mut best_dist = threshold;
    let mut result = None;
    for &e in edges {
        for &t in targets {
            let dist = (e - t).abs();
            if dist <= best_dist {
                best_dist = dist;
                result = Some((t - e, t));
            }
        }
    }
    result
}

/// Snap a moving frame by translating it so its nearest edge/centre lands on a
/// target line, independently per axis. Returns the corrected frame and the
/// guide lines to draw.
pub fn snap_move(
    frame: ObjectFrame,
    targets: &SnapTargets,
    threshold: f32,
) -> (ObjectFrame, Vec<SnapGuide>) {
    let cx = frame.x + frame.width * 0.5;
    let cy = frame.y + frame.height * 0.5;
    let mut out = frame;
    let mut guides = Vec::new();

    if let Some((dx, pos)) = best_snap(
        &[frame.x, cx, frame.x + frame.width],
        &targets.xs,
        threshold,
    ) {
        out.x += dx;
        guides.push(SnapGuide {
            vertical: true,
            pos,
        });
    }
    if let Some((dy, pos)) = best_snap(
        &[frame.y, cy, frame.y + frame.height],
        &targets.ys,
        threshold,
    ) {
        out.y += dy;
        guides.push(SnapGuide {
            vertical: false,
            pos,
        });
    }
    (out, guides)
}

/// Snap a resizing frame by pulling only the moving edges onto targets, keeping
/// the anchored edges fixed. A snap that would shrink the frame below
/// `min_size` in that axis is skipped.
pub fn snap_resize(
    frame: ObjectFrame,
    edges: MovableEdges,
    targets: &SnapTargets,
    threshold: f32,
    min_size: f32,
) -> (ObjectFrame, Vec<SnapGuide>) {
    let mut left = frame.x;
    let mut right = frame.x + frame.width;
    let mut top = frame.y;
    let mut bottom = frame.y + frame.height;
    let mut guides = Vec::new();

    let mut moving_x = Vec::new();
    if edges.left {
        moving_x.push(left);
    }
    if edges.right {
        moving_x.push(right);
    }
    if let Some((dx, pos)) = best_snap(&moving_x, &targets.xs, threshold) {
        let (nl, nr) = if edges.left {
            (left + dx, right)
        } else {
            (left, right + dx)
        };
        if nr - nl >= min_size {
            left = nl;
            right = nr;
            guides.push(SnapGuide {
                vertical: true,
                pos,
            });
        }
    }

    let mut moving_y = Vec::new();
    if edges.top {
        moving_y.push(top);
    }
    if edges.bottom {
        moving_y.push(bottom);
    }
    if let Some((dy, pos)) = best_snap(&moving_y, &targets.ys, threshold) {
        let (nt, nb) = if edges.top {
            (top + dy, bottom)
        } else {
            (top, bottom + dy)
        };
        if nb - nt >= min_size {
            top = nt;
            bottom = nb;
            guides.push(SnapGuide {
                vertical: false,
                pos,
            });
        }
    }

    (
        ObjectFrame::new(left, top, right - left, bottom - top),
        guides,
    )
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Align {
    Left,
    HCenter,
    Right,
    Top,
    VCenter,
    Bottom,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Distribute {
    Horizontal,
    Vertical,
}

fn bounds(frames: &[(ObjectId, ObjectFrame)]) -> Option<(f32, f32, f32, f32)> {
    let mut it = frames.iter();
    let (_, f0) = it.next()?;
    let mut min_x = f0.x;
    let mut min_y = f0.y;
    let mut max_x = f0.x + f0.width;
    let mut max_y = f0.y + f0.height;
    for (_, f) in it {
        min_x = min_x.min(f.x);
        min_y = min_y.min(f.y);
        max_x = max_x.max(f.x + f.width);
        max_y = max_y.max(f.y + f.height);
    }
    Some((min_x, min_y, max_x, max_y))
}

/// Align every frame to a shared edge or centre of the selection's bounding box.
/// Sizes are unchanged; input order is preserved. Needs ≥2 frames.
pub fn align(frames: &[(ObjectId, ObjectFrame)], mode: Align) -> Vec<(ObjectId, ObjectFrame)> {
    let Some((min_x, min_y, max_x, max_y)) = bounds(frames) else {
        return frames.to_vec();
    };
    frames
        .iter()
        .map(|&(id, f)| {
            let mut out = f;
            match mode {
                Align::Left => out.x = min_x,
                Align::HCenter => out.x = 0.5 * (min_x + max_x) - f.width * 0.5,
                Align::Right => out.x = max_x - f.width,
                Align::Top => out.y = min_y,
                Align::VCenter => out.y = 0.5 * (min_y + max_y) - f.height * 0.5,
                Align::Bottom => out.y = max_y - f.height,
            }
            (id, out)
        })
        .collect()
}

/// Space frames evenly along one axis by equalising their centre positions
/// between the two extreme centres; the endpoints stay put. Input order is
/// preserved. Needs ≥3 frames.
pub fn distribute(
    frames: &[(ObjectId, ObjectFrame)],
    axis: Distribute,
) -> Vec<(ObjectId, ObjectFrame)> {
    if frames.len() < 3 {
        return frames.to_vec();
    }
    let center = |f: &ObjectFrame| match axis {
        Distribute::Horizontal => f.x + f.width * 0.5,
        Distribute::Vertical => f.y + f.height * 0.5,
    };
    let mut order: Vec<usize> = (0..frames.len()).collect();
    order.sort_by(|&a, &b| {
        center(&frames[a].1)
            .partial_cmp(&center(&frames[b].1))
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let first = center(&frames[order[0]].1);
    let last = center(&frames[order[order.len() - 1]].1);
    let step = (last - first) / (order.len() as f32 - 1.0);
    let mut out: Vec<(ObjectId, ObjectFrame)> = frames.to_vec();
    for (rank, &idx) in order.iter().enumerate() {
        let target = first + step * rank as f32;
        let f = &mut out[idx].1;
        match axis {
            Distribute::Horizontal => f.x = target - f.width * 0.5,
            Distribute::Vertical => f.y = target - f.height * 0.5,
        }
    }
    out
}

/// Assign objects (in order) to the first grid cells, returning
/// `(id, cell_frame)` for the placed objects. Objects beyond `rows * cols` keep
/// their current frame and are omitted.
pub fn assign_grid(
    page_pt: [f32; 2],
    layout: &PageLayout,
    object_ids: &[ObjectId],
) -> Vec<(ObjectId, ObjectFrame)> {
    let cells = grid_frames(page_pt, layout);
    object_ids
        .iter()
        .zip(cells)
        .map(|(&id, frame)| (id, frame))
        .collect()
}

pub struct TilingPlan {
    pub newcomer: ObjectFrame,
    pub existing: Vec<(ObjectId, ObjectFrame)>,
}

/// Compute where a dropped plot and the target's existing plots tile a page:
/// - 0 existing → the newcomer fills the page.
/// - 1 existing → a two-way split; the pointer's half goes to the newcomer, the
///   complementary half to the existing plot, separated by the layout gutter.
/// - 2+ existing → an even grid re-tile of all N+1 plots (newcomer appended last).
pub fn compute_tiling_plan(
    page_pt: [f32; 2],
    layout: &PageLayout,
    existing_ids: &[ObjectId],
    pointer_page: [f32; 2],
) -> TilingPlan {
    let [w, h] = page_pt;
    let gutter = layout.gutter_mm * MM_TO_PT;
    match existing_ids.len() {
        0 => TilingPlan {
            newcomer: ObjectFrame::new(0.0, 0.0, w, h),
            existing: Vec::new(),
        },
        1 => {
            let (newcomer, other) = split_two(page_pt, gutter, pointer_page);
            TilingPlan {
                newcomer,
                existing: vec![(existing_ids[0], other)],
            }
        }
        _ => grid_retile(page_pt, layout, existing_ids),
    }
}

/// Two-way page split. The pointer decides the axis and side: whichever of x/y is
/// further from the page centre picks left/right vs top/bottom (ties favour
/// left/right). Returns `(pointer_side_frame, opposite_frame)` separated by `g`.
fn split_two(page_pt: [f32; 2], g: f32, p: [f32; 2]) -> (ObjectFrame, ObjectFrame) {
    let [w, h] = page_pt;
    let nx = if w > 0.0 { p[0] / w } else { 0.5 };
    let ny = if h > 0.0 { p[1] / h } else { 0.5 };
    if (nx - 0.5).abs() >= (ny - 0.5).abs() {
        let half = ((w - g) * 0.5).max(1.0);
        let left = ObjectFrame::new(0.0, 0.0, half, h);
        let right = ObjectFrame::new(half + g, 0.0, (w - half - g).max(1.0), h);
        if nx >= 0.5 {
            (right, left)
        } else {
            (left, right)
        }
    } else {
        let half = ((h - g) * 0.5).max(1.0);
        let top = ObjectFrame::new(0.0, 0.0, w, half);
        let bottom = ObjectFrame::new(0.0, half + g, w, (h - half - g).max(1.0));
        if ny >= 0.5 {
            (bottom, top)
        } else {
            (top, bottom)
        }
    }
}

/// Even grid re-tile of all N+1 plots into a near-square grid (existing keep their
/// order, newcomer takes the next free cell).
fn grid_retile(page_pt: [f32; 2], layout: &PageLayout, existing_ids: &[ObjectId]) -> TilingPlan {
    let n = existing_ids.len() + 1;
    let (rows, cols) = even_grid_dims(n);
    let grid_layout = PageLayout {
        rows,
        cols,
        ..*layout
    };
    let cells = grid_frames(page_pt, &grid_layout);
    let existing = existing_ids
        .iter()
        .zip(&cells)
        .map(|(&id, &cell)| (id, cell))
        .collect();
    TilingPlan {
        newcomer: cells[existing_ids.len()],
        existing,
    }
}

/// A near-square `(rows, cols)` grid holding at least `n` cells.
fn even_grid_dims(n: usize) -> (u32, u32) {
    let cols = ((n as f64).sqrt().ceil() as u32).max(1);
    let rows = (n as u32).div_ceil(cols).max(1);
    (rows, cols)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn approx(a: f32, b: f32) -> bool {
        (a - b).abs() < 0.01
    }

    #[test]
    fn split_places_newcomer_on_pointer_side_with_gutter() {
        let page = [400.0, 300.0];
        let layout = PageLayout {
            gutter_mm: 5.0,
            ..PageLayout::default()
        };
        let g = 5.0 * MM_TO_PT;
        // Pointer well to the right of centre → left/right split, newcomer right.
        let plan = compute_tiling_plan(page, &layout, &[7], [360.0, 150.0]);
        assert_eq!(plan.existing.len(), 1);
        let (id, ex) = plan.existing[0];
        assert_eq!(id, 7);
        let nc = plan.newcomer;
        assert!(nc.x > ex.x, "newcomer takes the right half");
        assert!(
            (nc.x - (ex.x + ex.width) - g).abs() < 0.5,
            "one gutter between the two halves"
        );
        assert!(ex.x >= -0.01 && nc.x + nc.width <= page[0] + 0.01);
        assert!((ex.height - page[1]).abs() < 0.01 && (nc.height - page[1]).abs() < 0.01);
    }

    #[test]
    fn multi_plot_retile_grids_all_including_newcomer() {
        let page = [400.0, 300.0];
        let plan = compute_tiling_plan(page, &PageLayout::default(), &[1, 2], [5.0, 5.0]);
        assert_eq!(plan.existing.len(), 2, "both existing plots reframed");
        let frames = [plan.existing[0].1, plan.existing[1].1, plan.newcomer];
        for i in 0..frames.len() {
            for j in (i + 1)..frames.len() {
                assert_ne!(frames[i], frames[j], "3 distinct grid cells");
            }
        }
        for f in frames {
            assert!(f.x >= -0.01 && f.y >= -0.01);
            assert!(f.x + f.width <= page[0] + 0.01 && f.y + f.height <= page[1] + 0.01);
        }
    }

    #[test]
    fn grid_frames_partition_page_with_margins_and_gutter() {
        let layout = PageLayout {
            margin_mm: [0.0, 0.0, 0.0, 0.0],
            gutter_mm: 0.0,
            rows: 2,
            cols: 2,
            show_grid: false,
        };
        let frames = grid_frames([200.0, 100.0], &layout);
        assert_eq!(frames.len(), 4);
        assert!(approx(frames[0].width, 100.0) && approx(frames[0].height, 50.0));
        // Row-major: index 1 is top-right, index 2 is bottom-left.
        assert!(approx(frames[1].x, 100.0) && approx(frames[1].y, 0.0));
        assert!(approx(frames[2].x, 0.0) && approx(frames[2].y, 50.0));
        assert!(approx(frames[3].x, 100.0) && approx(frames[3].y, 50.0));
    }

    #[test]
    fn gutter_reduces_cell_width() {
        let layout = PageLayout {
            margin_mm: [0.0, 0.0, 0.0, 0.0],
            gutter_mm: 0.0,
            rows: 1,
            cols: 2,
            show_grid: false,
        };
        let no_gutter = grid_frames([200.0, 100.0], &layout)[0].width;
        let with_gutter = grid_frames(
            [200.0, 100.0],
            &PageLayout {
                gutter_mm: 10.0,
                ..layout
            },
        )[0]
        .width;
        assert!(with_gutter < no_gutter);
    }

    #[test]
    fn snap_move_pulls_edge_to_target_within_threshold() {
        let mut targets = SnapTargets::default();
        targets.xs.push(100.0);
        let frame = ObjectFrame::new(96.0, 10.0, 20.0, 20.0);
        let (snapped, guides) = snap_move(frame, &targets, 6.0);
        assert!(approx(snapped.x, 100.0));
        assert_eq!(guides.len(), 1);
        assert!(guides[0].vertical && approx(guides[0].pos, 100.0));
    }

    #[test]
    fn snap_move_ignores_targets_outside_threshold() {
        let mut targets = SnapTargets::default();
        targets.xs.push(100.0);
        // left 80, centre 85, right 90 — all more than the threshold from 100.
        let frame = ObjectFrame::new(80.0, 10.0, 10.0, 20.0);
        let (snapped, guides) = snap_move(frame, &targets, 6.0);
        assert!(approx(snapped.x, 80.0));
        assert!(guides.is_empty());
    }

    #[test]
    fn snap_move_picks_nearest_target() {
        let mut targets = SnapTargets::default();
        targets.xs.push(100.0);
        targets.xs.push(104.0);
        let frame = ObjectFrame::new(103.0, 0.0, 20.0, 20.0);
        let (snapped, _) = snap_move(frame, &targets, 6.0);
        assert!(approx(snapped.x, 104.0));
    }

    #[test]
    fn snap_resize_moves_only_the_dragged_edge() {
        let mut targets = SnapTargets::default();
        targets.xs.push(100.0);
        let frame = ObjectFrame::new(10.0, 10.0, 88.0, 40.0); // right edge at 98
        let edges = MovableEdges {
            right: true,
            ..MovableEdges::default()
        };
        let (snapped, guides) = snap_resize(frame, edges, &targets, 6.0, 24.0);
        assert!(approx(snapped.x, 10.0));
        assert!(approx(snapped.x + snapped.width, 100.0));
        assert_eq!(guides.len(), 1);
    }

    #[test]
    fn align_left_and_right_pin_to_bounding_edges() {
        let frames = vec![
            (1u64, ObjectFrame::new(10.0, 0.0, 20.0, 10.0)),
            (2, ObjectFrame::new(50.0, 40.0, 40.0, 10.0)),
        ];
        let left = align(&frames, Align::Left);
        assert!(approx(left[0].1.x, 10.0) && approx(left[1].1.x, 10.0));
        let right = align(&frames, Align::Right);
        // Bounding box right edge is 90; each frame's right edge lands there.
        assert!(approx(right[0].1.x, 70.0) && approx(right[1].1.x, 50.0));
    }

    #[test]
    fn align_hcenter_centres_each_frame_on_bbox_centre() {
        let frames = vec![
            (1u64, ObjectFrame::new(0.0, 0.0, 20.0, 10.0)),
            (2, ObjectFrame::new(80.0, 0.0, 20.0, 10.0)),
        ];
        // bbox spans 0..100, centre 50.
        let out = align(&frames, Align::HCenter);
        assert!(approx(out[0].1.x, 40.0) && approx(out[1].1.x, 40.0));
    }

    #[test]
    fn distribute_horizontal_equalises_centre_spacing() {
        let frames = vec![
            (1u64, ObjectFrame::new(0.0, 0.0, 10.0, 10.0)),
            (2, ObjectFrame::new(12.0, 0.0, 10.0, 10.0)),
            (3, ObjectFrame::new(90.0, 0.0, 10.0, 10.0)),
        ];
        // Centres: 5, 17, 95 → after: 5, 50, 95 (step 45); middle x = 45.
        let out = distribute(&frames, Distribute::Horizontal);
        assert!(approx(out[0].1.x, 0.0));
        assert!(approx(out[1].1.x, 45.0));
        assert!(approx(out[2].1.x, 90.0));
    }

    #[test]
    fn distribute_needs_three_frames() {
        let frames = vec![
            (1u64, ObjectFrame::new(0.0, 0.0, 10.0, 10.0)),
            (2, ObjectFrame::new(90.0, 0.0, 10.0, 10.0)),
        ];
        let out = distribute(&frames, Distribute::Horizontal);
        assert_eq!(out, frames);
    }

    #[test]
    fn snap_resize_skips_when_below_min_size() {
        let mut targets = SnapTargets::default();
        targets.xs.push(30.0);
        let frame = ObjectFrame::new(10.0, 10.0, 25.0, 40.0); // right edge at 35, snapping to 30 → width 20 < 24
        let edges = MovableEdges {
            right: true,
            ..MovableEdges::default()
        };
        let (snapped, guides) = snap_resize(frame, edges, &targets, 6.0, 24.0);
        assert!(approx(snapped.width, 25.0));
        assert!(guides.is_empty());
    }
}
