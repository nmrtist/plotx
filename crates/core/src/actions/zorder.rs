use crate::state::ObjectId;

/// The active-canvas index after deleting `deleted` from `len` canvases: `None`
/// when the last page goes, else the same slot clamped to the new final index.
pub fn active_canvas_after_delete(len: usize, deleted: usize) -> Option<usize> {
    if len <= 1 {
        None
    } else {
        Some(deleted.min(len - 2))
    }
}

/// A z-order move applied to a set of target objects, preserving their relative
/// order. Front = later in the `objects` vec (painted on top).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ZOrder {
    Front,
    Back,
    Forward,
    Backward,
}

/// Compute a new full id order after applying `op` to the `targets` within
/// `order`. Targets keep their relative order; `Forward`/`Backward` step one
/// slot past the nearest non-target neighbour.
pub fn reorder_z(order: &[ObjectId], targets: &[ObjectId], op: ZOrder) -> Vec<ObjectId> {
    let is_target = |id: &ObjectId| targets.contains(id);
    match op {
        ZOrder::Front => {
            let mut v: Vec<ObjectId> = order.iter().copied().filter(|id| !is_target(id)).collect();
            v.extend(order.iter().copied().filter(|id| is_target(id)));
            v
        }
        ZOrder::Back => {
            let mut v: Vec<ObjectId> = order.iter().copied().filter(is_target).collect();
            v.extend(order.iter().copied().filter(|id| !is_target(id)));
            v
        }
        ZOrder::Forward => {
            let mut v = order.to_vec();
            for i in (0..v.len().saturating_sub(1)).rev() {
                if is_target(&v[i]) && !is_target(&v[i + 1]) {
                    v.swap(i, i + 1);
                }
            }
            v
        }
        ZOrder::Backward => {
            let mut v = order.to_vec();
            for i in 1..v.len() {
                if is_target(&v[i]) && !is_target(&v[i - 1]) {
                    v.swap(i, i - 1);
                }
            }
            v
        }
    }
}
