use super::*;

pub(crate) fn preserve_aspect_frame(
    before: ObjectFrame,
    candidate: ObjectFrame,
    kind: ObjectDragKind,
) -> ObjectFrame {
    let ObjectDragKind::Resize(handle) = kind else {
        return candidate;
    };
    let width_scale = candidate.width / before.width;
    let height_scale = candidate.height / before.height;
    let scale = if (width_scale - 1.0).abs() >= (height_scale - 1.0).abs() {
        width_scale
    } else {
        height_scale
    }
    .max(MIN_OBJECT_SIZE_PT / before.width)
    .max(MIN_OBJECT_SIZE_PT / before.height);
    let width = before.width * scale;
    let height = before.height * scale;
    let right = before.x + before.width;
    let bottom = before.y + before.height;
    match handle {
        ResizeHandle::TopLeft => ObjectFrame::new(right - width, bottom - height, width, height),
        ResizeHandle::TopRight => ObjectFrame::new(before.x, bottom - height, width, height),
        ResizeHandle::BottomLeft => ObjectFrame::new(right - width, before.y, width, height),
        ResizeHandle::BottomRight => ObjectFrame::new(before.x, before.y, width, height),
    }
}
