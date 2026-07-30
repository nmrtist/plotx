use super::{CanvasId, DatasetId, ObjectId};

/// The current slice position of the Slice tool: which 2D dataset/plot it
/// targets, the cut orientation, and the snapped grid index (a row/column index
/// for a true-2D spectrum, or an increment index for a pseudo-2D stack). Drives
/// the live preview and the "Extract" button; transient (never serialized).
#[derive(Clone, Copy, PartialEq)]
pub struct SliceCursor {
    pub dataset: usize,
    pub object: ObjectId,
    pub kind: plotx_processing::SliceKind,
    pub index: usize,
}

/// One sampled plot position used by the transient Inspect and Delta cursors.
/// Stable owner IDs keep a pin attached to the same plot if collections move.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CursorPoint {
    pub canvas: CanvasId,
    pub object: ObjectId,
    pub dataset: DatasetId,
    pub x: f64,
    pub y: Option<f64>,
    pub intensity: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CursorDelta {
    pub first: CursorPoint,
    pub second: CursorPoint,
}
