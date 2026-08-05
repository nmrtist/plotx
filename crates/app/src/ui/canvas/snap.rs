use super::*;

/// Distance (screen px) within which a dragged frame's edge snaps to a
/// neighbour's edge or to one gutter clear of it. Converted to world pt via the
/// live board zoom so the magnet feels the same at any zoom.
const FRAME_SNAP_TOL_PX: f32 = 8.0;

/// The snapped resting position for a frame being dragged to `candidate` (its
/// top-left, pt): edge/gutter magnetism against every other visible frame. An
/// axis with no nearby magnetic target stays exactly at the candidate position.
pub(crate) fn snap_dragged_frame(
    app: &PlotxApp,
    frame: FrameRef,
    moving: &[plotx_core::state::BoardFrameId],
    candidate: [f32; 2],
    bypass: bool,
) -> [f32; 2] {
    if !app.settings.general.snap_enabled || bypass {
        return candidate;
    }
    let Some(r) = frame_board_rect(app, frame) else {
        return candidate;
    };
    let size = [r.right() - r.left, r.bottom() - r.top];
    let others: Vec<PlotRect> = board_frames(app)
        .into_iter()
        .filter(|&f| f != frame && board_frame_id(app, f).is_none_or(|id| !moving.contains(&id)))
        .filter_map(|f| frame_board_rect(app, f))
        .collect();
    let tol = FRAME_SNAP_TOL_PX / app.session.board.zoom.max(0.01);
    snap_frame_pos(candidate, size, &others, BOARD_GUTTER_PT, tol)
}

/// Snap a dragged frame (top-left `candidate`, world `size`) to the `others`
/// frames per axis: align to a neighbour's edge, or sit one `gutter` clear of it.
/// An axis with no neighbour within `tol` remains at its candidate coordinate.
fn snap_frame_pos(
    candidate: [f32; 2],
    size: [f32; 2],
    others: &[PlotRect],
    gutter: f32,
    tol: f32,
) -> [f32; 2] {
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
    [x.unwrap_or(candidate[0]), y.unwrap_or(candidate[1])]
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
    if !app.settings.general.snap_enabled || alt {
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
    fn free_placement_keeps_the_candidate_without_a_magnetic_target() {
        let others = [PlotRect::new(0.0, 0.0, 100.0, 80.0)];
        let size = [100.0, 80.0];
        let far = [1000.0 + 5.0, 700.0 + 5.0];
        assert_eq!(
            snap_frame_pos(far, size, &others, BOARD_GUTTER_PT, 8.0),
            far
        );
    }

    #[test]
    fn magnetic_edges_and_gutters_snap_within_tolerance() {
        let others = [PlotRect::new(0.0, 0.0, 100.0, 80.0)];
        let size = [100.0, 80.0];
        let near = [100.0 + BOARD_GUTTER_PT + 3.0, 2.0];
        assert_eq!(
            snap_frame_pos(near, size, &others, BOARD_GUTTER_PT, 8.0),
            [100.0 + BOARD_GUTTER_PT, 0.0]
        );
    }

    fn app_with_two_pages() -> PlotxApp {
        let mut app = PlotxApp::new();
        for x in [0.0, 500.0] {
            let mut page = CanvasDocument::new("p".to_owned(), [100.0, 80.0]);
            page.board_pos = [x, 0.0];
            app.doc.canvases.push(page);
        }
        app.session.board.zoom = 1.0;
        app
    }

    #[test]
    fn frame_snapping_respects_the_general_preference() {
        let mut app = app_with_two_pages();
        let width = app.doc.canvases[0].board_rect_pt().width;
        let candidate = [width + BOARD_GUTTER_PT + 2.0, 3.0];
        app.settings.general.snap_enabled = false;
        assert_eq!(
            snap_dragged_frame(&app, FrameRef::Page(1), &[], candidate, false),
            candidate
        );
    }

    #[test]
    fn alt_bypasses_frame_snapping_for_the_current_drag() {
        let app = app_with_two_pages();
        let width = app.doc.canvases[0].board_rect_pt().width;
        let candidate = [width + BOARD_GUTTER_PT + 2.0, 3.0];
        assert_eq!(
            snap_dragged_frame(&app, FrameRef::Page(1), &[], candidate, true),
            candidate
        );
    }

    #[test]
    fn group_drag_does_not_snap_to_another_moving_page() {
        let mut app = app_with_two_pages();
        let width = app.doc.canvases[0].board_rect_pt().width;
        app.doc.canvases[1].board_pos = [width, 0.0];
        let moving = app
            .doc
            .canvases
            .iter()
            .map(|canvas| plotx_core::state::BoardFrameId::Page(canvas.resource_id))
            .collect::<Vec<_>>();
        let candidate = [3.0, 3.0];

        assert_eq!(
            snap_dragged_frame(&app, FrameRef::Page(0), &moving, candidate, false),
            candidate
        );
    }
}
