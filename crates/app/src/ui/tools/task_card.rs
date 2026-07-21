//! Shared geometry for the canvas task cards (Regions, Curve Fit). Both anchor
//! to the same corner of the canvas, so the sizing rules live in one place.

use egui::{Pos2, Ui};

/// Width shared by every task card, and the gap it keeps from the canvas edges.
const WIDTH: f32 = 310.0;
const MARGIN: f32 = 12.0;
const TOP_OFFSET: f32 = 8.0;
/// Room the card header, frame and footprint need outside the resizable body.
const CHROME: f32 = 64.0;
/// Below this the body is useless anyway; the card is allowed to overhang.
const FLOOR: f32 = 120.0;

pub(super) struct TaskCardGeometry {
    pub pos: Pos2,
    pub width: f32,
    pub min_body_height: f32,
    pub max_body_height: f32,
}

/// Anchors a card to the host's top-right corner and sizes its body to the
/// height the canvas actually has.
///
/// `preferred_min_body` is honoured only while it fits: `egui::Resize` applies
/// `at_least(min).at_most(max)`, so a min taller than the host would win over
/// the fitted max and force the card past the canvas. `Area` then constrains it
/// to the screen and slides it up over the Ribbon, hiding the very buttons that
/// opened it. Clamping the min keeps a short window shrinking instead.
pub(super) fn geometry(host: &Ui, preferred_min_body: f32) -> TaskCardGeometry {
    let host_rect = host.max_rect();
    let pos = host_rect.right_top() + egui::vec2(-WIDTH - MARGIN, TOP_OFFSET);
    let max_body_height = (host_rect.bottom() - pos.y - CHROME).max(FLOOR);
    TaskCardGeometry {
        pos,
        width: WIDTH,
        min_body_height: preferred_min_body.min(max_body_height),
        max_body_height,
    }
}
