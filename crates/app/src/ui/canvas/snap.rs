use super::*;

/// Distance (screen px) within which a dragged frame's edge snaps to a
/// neighbour's edge or to one gutter clear of it. Converted to world pt via the
/// live board zoom so the magnet feels the same at any zoom.
const FRAME_SNAP_TOL_PX: f32 = 8.0;

pub(crate) fn snap_board_pos(pos: [f32; 2], cell: f32) -> [f32; 2] {
    if cell <= 0.0 {
        return pos;
    }
    [
        (pos[0] / cell).round() * cell,
        (pos[1] / cell).round() * cell,
    ]
}

/// The snapped resting position for a frame being dragged to `candidate` (its
/// top-left, pt): edge/gutter magnetism against every other frame, with the
/// coarse board grid as the per-axis fallback.
pub(crate) fn snap_dragged_frame(app: &PlotxApp, frame: FrameRef, candidate: [f32; 2]) -> [f32; 2] {
    let Some(r) = frame_board_rect(app, frame) else {
        return snap_board_pos(candidate, BOARD_GRID_PT);
    };
    let size = [r.right() - r.left, r.bottom() - r.top];
    let others: Vec<PlotRect> = board_frames(app)
        .into_iter()
        .filter(|&f| f != frame)
        .filter_map(|f| frame_board_rect(app, f))
        .collect();
    let tol = FRAME_SNAP_TOL_PX / app.session.board.zoom.max(0.01);
    snap_frame_pos(
        candidate,
        size,
        &others,
        BOARD_GUTTER_PT,
        BOARD_GRID_PT,
        tol,
    )
}

/// Snap a dragged frame (top-left `candidate`, world `size`) to the `others`
/// frames per axis: align to a neighbour's edge, or sit one `gutter` clear of it.
/// An axis with no neighbour within `tol` falls back to the coarse board `grid`.
fn snap_frame_pos(
    candidate: [f32; 2],
    size: [f32; 2],
    others: &[PlotRect],
    gutter: f32,
    grid: f32,
    tol: f32,
) -> [f32; 2] {
    let grid_pos = snap_board_pos(candidate, grid);
    let x = snap_edge(
        candidate[0],
        size[0],
        others.iter().map(|r| (r.left, r.right())),
        gutter,
        tol,
    );
    let y = snap_edge(
        candidate[1],
        size[1],
        others.iter().map(|r| (r.top, r.bottom())),
        gutter,
        tol,
    );
    [x.unwrap_or(grid_pos[0]), y.unwrap_or(grid_pos[1])]
}

/// The nearest snap target for a dragged frame's near edge `cand` (its extent
/// `extent` along this axis) against neighbour spans `lines` — each neighbour
/// offers aligning to either edge or sitting one `gutter` clear on either side.
/// `None` when nothing lands within `tol`.
fn snap_edge(
    cand: f32,
    extent: f32,
    lines: impl Iterator<Item = (f32, f32)>,
    gutter: f32,
    tol: f32,
) -> Option<f32> {
    let mut best = None;
    let mut best_d = tol;
    for (lo, hi) in lines {
        for target in [lo, hi - extent, hi + gutter, lo - gutter - extent] {
            let d = (target - cand).abs();
            if d < best_d {
                best_d = d;
                best = Some(target);
            }
        }
    }
    best
}

/// Snapping is skipped when disabled or Alt is held.
pub(crate) fn snap_object_frame(
    app: &PlotxApp,
    ci: usize,
    drag: &ObjectDrag,
    candidate: ObjectFrame,
    ui: &Ui,
) -> (ObjectFrame, Vec<SnapGuide>) {
    let alt = ui.input(|i| i.modifiers.alt);
    if !app.session.ui.snap_enabled || alt {
        return (candidate, Vec::new());
    }
    let canvas = &app.doc.canvases[ci];
    let zoom = app.session.board.zoom.max(0.01);
    let threshold = SNAP_PX / zoom;
    let mut targets = SnapTargets::from_page(canvas.size_pt(), &canvas.layout);
    for object in &canvas.objects {
        let moving =
            object.id == drag.object || drag.others.iter().any(|(oid, _)| *oid == object.id);
        if !moving && object.visible {
            targets.push_object(object.frame);
        }
    }
    match drag.kind {
        ObjectDragKind::Move => layout::snap_move(candidate, &targets, threshold),
        ObjectDragKind::Resize(handle) => layout::snap_resize(
            candidate,
            movable_edges(handle),
            &targets,
            threshold,
            MIN_OBJECT_SIZE_PT,
        ),
    }
}

pub(crate) fn movable_edges(handle: ResizeHandle) -> MovableEdges {
    let (left, right) = (
        matches!(handle, ResizeHandle::TopLeft | ResizeHandle::BottomLeft),
        matches!(handle, ResizeHandle::TopRight | ResizeHandle::BottomRight),
    );
    let (top, bottom) = (
        matches!(handle, ResizeHandle::TopLeft | ResizeHandle::TopRight),
        matches!(handle, ResizeHandle::BottomLeft | ResizeHandle::BottomRight),
    );
    MovableEdges {
        left,
        right,
        top,
        bottom,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snap_frame_pos_prefers_neighbour_gutter_then_grid() {
        let others = [PlotRect::new(0.0, 0.0, 100.0, 80.0)];
        let size = [100.0, 80.0];
        let near = [100.0 + BOARD_GUTTER_PT + 3.0, 2.0];
        let snapped = snap_frame_pos(near, size, &others, BOARD_GUTTER_PT, BOARD_GRID_PT, 8.0);
        assert_eq!(snapped, [100.0 + BOARD_GUTTER_PT, 0.0]);
        let far = [1000.0 + 5.0, 700.0 + 5.0];
        let g = snap_frame_pos(far, size, &others, BOARD_GUTTER_PT, BOARD_GRID_PT, 8.0);
        assert_eq!(g, snap_board_pos(far, BOARD_GRID_PT));
    }

    #[test]
    fn snap_board_pos_rounds_to_grid() {
        assert_eq!(snap_board_pos([10.0, -10.0], 360.0), [0.0, 0.0]);
        assert_eq!(snap_board_pos([190.0, 181.0], 360.0), [360.0, 360.0]);
        assert_eq!(snap_board_pos([540.0, -540.0], 360.0), [720.0, -720.0]);
        // A non-positive cell is a no-op guard, never a divide-by-zero.
        assert_eq!(snap_board_pos([7.0, 3.0], 0.0), [7.0, 3.0]);
    }
}
