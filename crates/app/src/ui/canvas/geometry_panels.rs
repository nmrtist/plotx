use super::*;

#[derive(Clone, Copy)]
pub(crate) struct ObjectHit {
    pub(crate) object: ObjectId,
    pub(crate) kind: ObjectDragKind,
}

#[derive(Clone, Copy)]
pub(crate) struct PanelHit {
    pub(crate) panel: PanelId,
    pub(crate) kind: ObjectDragKind,
}

pub(crate) fn hit_object(canvas: &CanvasDocument, p: Pos2, zoom: f32) -> Option<ObjectHit> {
    hit_objects(canvas, p, zoom).into_iter().next()
}

pub(crate) fn hit_objects(canvas: &CanvasDocument, p: Pos2, zoom: f32) -> Vec<ObjectHit> {
    hit_frames(canvas, p, zoom, |canvas, id| canvas.layout_frame(id))
}

/// Child hit-testing uses page-space content frames. In page scope callers hit
/// the Panel first; after entering a Panel this is the only content target.
pub(crate) fn hit_content_objects(
    canvas: &CanvasDocument,
    p: Pos2,
    zoom: f32,
    panel: Option<PanelId>,
) -> Vec<ObjectHit> {
    hit_frames(canvas, p, zoom, |canvas, id| {
        (panel.is_none_or(|panel_id| canvas.parent_panel(id) == Some(panel_id)))
            .then(|| canvas.content_page_frame(id))
            .flatten()
    })
}

pub(crate) fn hit_content_object(
    canvas: &CanvasDocument,
    p: Pos2,
    zoom: f32,
    panel: Option<PanelId>,
) -> Option<ObjectHit> {
    hit_content_objects(canvas, p, zoom, panel)
        .into_iter()
        .next()
}

fn hit_frames(
    canvas: &CanvasDocument,
    p: Pos2,
    zoom: f32,
    frame: impl Fn(&CanvasDocument, ObjectId) -> Option<ObjectFrame>,
) -> Vec<ObjectHit> {
    let handle_radius = (HANDLE_SIZE_PX / zoom.max(0.01)).max(3.0);
    canvas
        .objects
        .iter()
        .rev()
        .filter_map(|object| {
            if !object.visible {
                return None;
            }
            let frame = frame(canvas, object.id)?;
            let r = egui::Rect::from_min_size(
                Pos2::new(frame.x, frame.y),
                egui::vec2(frame.width, frame.height),
            );
            let handles = [
                (r.left_top(), ResizeHandle::TopLeft),
                (r.right_top(), ResizeHandle::TopRight),
                (r.left_bottom(), ResizeHandle::BottomLeft),
                (r.right_bottom(), ResizeHandle::BottomRight),
            ];
            handles
                .into_iter()
                .find_map(|(pos, handle)| {
                    (pos.distance(p) <= handle_radius).then_some(ObjectHit {
                        object: object.id,
                        kind: ObjectDragKind::Resize(handle),
                    })
                })
                .or_else(|| {
                    r.contains(p).then_some(ObjectHit {
                        object: object.id,
                        kind: ObjectDragKind::Move,
                    })
                })
        })
        .collect()
}

/// Hit-test the semantic Panel frame, including empty Panels and resize
/// handles. The topmost visible Panel wins on overlap.
pub(crate) fn hit_panel(canvas: &CanvasDocument, p: Pos2, zoom: f32) -> Option<PanelHit> {
    let handle_radius = (HANDLE_SIZE_PX / zoom.max(0.01)).max(3.0);
    canvas
        .panels
        .iter()
        .rev()
        .filter(|panel| panel.visible)
        .find_map(|panel| {
            let r = egui::Rect::from_min_size(
                Pos2::new(panel.frame.x, panel.frame.y),
                egui::vec2(panel.frame.width, panel.frame.height),
            );
            let handles = [
                (r.left_top(), ResizeHandle::TopLeft),
                (r.right_top(), ResizeHandle::TopRight),
                (r.left_bottom(), ResizeHandle::BottomLeft),
                (r.right_bottom(), ResizeHandle::BottomRight),
            ];
            handles
                .into_iter()
                .find_map(|(pos, handle)| {
                    (pos.distance(p) <= handle_radius).then_some(PanelHit {
                        panel: panel.id,
                        kind: ObjectDragKind::Resize(handle),
                    })
                })
                .or_else(|| {
                    r.contains(p).then_some(PanelHit {
                        panel: panel.id,
                        kind: ObjectDragKind::Move,
                    })
                })
        })
}

pub(crate) fn content_screen_rect(
    board: BoardViewport,
    canvas: &CanvasDocument,
    object_id: ObjectId,
    screen: egui::Rect,
) -> Option<PlotRect> {
    let page = BoardTransform::from_board(board, screen).page_screen_rect(canvas);
    let frame = canvas.content_page_frame(object_id)?;
    Some(PlotRect::new(
        page.left() + frame.x * board.zoom,
        page.top() + frame.y * board.zoom,
        frame.width * board.zoom,
        frame.height * board.zoom,
    ))
}
