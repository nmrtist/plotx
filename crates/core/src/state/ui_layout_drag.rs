use super::*;

#[derive(Clone, Debug)]
pub struct ObjectDrag {
    pub canvas: usize,
    pub object: ObjectId,
    pub kind: ObjectDragKind,
    pub before: ObjectFrame,
    /// Pointer position in page space (pt) when the drag began, so the live
    /// frame is recomputed absolutely each frame — a snap correction on one
    /// frame is not re-perturbed by the next frame's incremental delta.
    pub start_pointer: [f32; 2],
    /// Pointer position in screen px when the drag began. The move dead-zone is
    /// measured against screen-space pointer travel: intent to drag is about how
    /// far the cursor moved, not how far the page moved under it, so a view
    /// change can never trip a drag the user never made.
    pub start_pointer_screen: [f32; 2],
    /// Start frames of the other selected objects moving with the primary (group
    /// move). Empty for a single-object drag; populated only for `Move`.
    pub others: Vec<(ObjectId, ObjectFrame)>,
    /// Whether the gesture has cleared the move dead-zone. A `Move` starts `false`
    /// and only moves/commits once the pointer travels past the threshold, so a
    /// click with a few px of jitter selects without nudging the frame. Resize
    /// grabs are deliberate and start `true`.
    pub active: bool,
    /// Coordinate space in which the live gesture edits the content frame.
    pub space: ObjectDragSpace,
}

/// A content frame is either expressed in page coordinates or in the local
/// coordinate system of its owning Panel. Keeping this explicit prevents a
/// loose item from being mistaken for Panel-local content merely because both
/// cases used to be represented by `false`/`true` conditionals.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ObjectDragSpace {
    Page,
    Panel(PanelId),
}

/// A live drag of a whole Panel frame. Child frames are local snapshots so a
/// resize scales them with the Panel and cancellation restores the gesture.
#[derive(Clone, Debug)]
pub struct PanelDrag {
    pub canvas: usize,
    pub panel: PanelId,
    pub kind: ObjectDragKind,
    pub before: ObjectFrame,
    pub others: Vec<(PanelId, ObjectFrame)>,
    pub children: Vec<(ContentId, ObjectFrame)>,
    pub start_pointer: [f32; 2],
    pub start_pointer_screen: [f32; 2],
    pub active: bool,
}
