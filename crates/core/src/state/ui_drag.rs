//! In-progress measurement-band gestures.
//!
//! Each of these mirrors [`super::ObjectDrag`]: the live geometry is recomputed
//! absolutely from the grab state every frame so nothing accumulates drift, and
//! `before` snapshots the dataset so the gesture commits as one undoable step.

use super::{AxisOverrides, DatasetId, ObjectId, Region, RegionId};
use crate::{Integral2D, IntegralResult};

/// An in-progress region-band edit on a series plot. `region_id` names the band
/// being resized or moved (`None` while drawing a new one).
#[derive(Clone, Debug)]
pub struct RegionDrag {
    pub canvas: usize,
    pub object: ObjectId,
    pub dataset: DatasetId,
    pub kind: RegionDragKind,
    pub region_id: Option<RegionId>,
    pub before: Vec<Region>,
    /// Pointer ppm at grab time (for `Move`) or the fixed anchor (for `NewBand`).
    pub anchor_ppm: f64,
    /// The dragged band's lo/hi at grab time (for `Move`).
    pub grab_lo: f64,
    pub grab_hi: f64,
    /// Live pointer ppm, used to paint the `NewBand` preview.
    pub current_ppm: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RegionDragKind {
    NewBand,
    EdgeLo,
    EdgeHi,
    Move,
}

#[derive(Clone, Debug)]
pub struct FurnitureDrag {
    pub canvas: usize,
    pub object: ObjectId,
    pub target: FurnitureTarget,
}

#[derive(Clone, Debug)]
pub enum FurnitureTarget {
    Legend {
        before: AxisOverrides,
        grab_offset: [f32; 2],
    },
    RegionLabel {
        dataset: DatasetId,
        region: RegionId,
        before: Vec<Region>,
        grab_offset: [f32; 2],
    },
}

/// An in-progress integral-band edit on a 1D spectrum — the direct analogue of
/// [`RegionDrag`], reusing [`RegionDragKind`].
#[derive(Clone, Debug)]
pub struct IntegralDrag {
    pub canvas: usize,
    pub object: ObjectId,
    pub dataset: usize,
    pub kind: RegionDragKind,
    pub integral_id: Option<u64>,
    pub before: Vec<IntegralResult>,
    pub anchor_ppm: f64,
    pub grab_lo: f64,
    pub grab_hi: f64,
    pub current_ppm: f64,
}

/// An in-progress true-2D integral rectangle edit. Geometry is updated live,
/// while volume recomputation is deferred until the gesture commits.
#[derive(Clone, Debug)]
pub struct Integral2DDrag {
    pub canvas: usize,
    pub object: ObjectId,
    pub dataset: usize,
    pub kind: Integral2DDragKind,
    pub integral_id: Option<u64>,
    pub before: Vec<Integral2D>,
    /// Pointer coordinates at grab time, or the fixed corner for a new rectangle.
    pub anchor: [f64; 2],
    /// Rectangle bounds at grab time for moves and resizes.
    pub grab_f2: (f64, f64),
    pub grab_f1: (f64, f64),
    /// Live pointer coordinates, used for the new-rectangle preview.
    pub current: [f64; 2],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Integral2DDragKind {
    NewRect,
    EdgeF2Lo,
    EdgeF2Hi,
    EdgeF1Lo,
    EdgeF1Hi,
    CornerF2LoF1Lo,
    CornerF2LoF1Hi,
    CornerF2HiF1Lo,
    CornerF2HiF1Hi,
    Move,
}
