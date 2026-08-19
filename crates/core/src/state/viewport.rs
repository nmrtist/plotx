use super::BoardFrameId;

/// Layout-independent camera for the board that holds every page-frame.
///
/// `world_center` is expressed in board points. The UI maps it to the center of
/// the current visible workspace, so changing sidebars or other chrome never
/// changes what point the camera is looking at.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BoardViewport {
    pub zoom: f32,
    pub world_center: [f32; 2],
}

impl Default for BoardViewport {
    fn default() -> Self {
        Self {
            zoom: 1.0,
            world_center: [0.0, 0.0],
        }
    }
}

/// What a persistent board fit intent targets.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum BoardFitTarget {
    /// A single frame, re-read each tick so the glide tracks it if it moves.
    Frame(BoardFrameId),
    /// A fixed world-pt region `(min_x, min_y, max_x, max_y)`.
    Region([f32; 4]),
    /// Every board frame, resolved again whenever the workspace changes.
    AllFrames,
    /// An exact layout-independent camera, e.g. a saved named view.
    Viewport { zoom: f32, world_center: [f32; 2] },
}

/// Ownership of the board camera. Fit intent persists across layout changes;
/// direct pan or zoom returns control to the user.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum ViewportMode {
    Manual,
    Fit(BoardFitTarget),
}

/// A saved board bookmark that can be restored independently of UI layout.
#[derive(Clone, Debug, PartialEq)]
pub struct NamedView {
    pub name: String,
    pub zoom: f32,
    pub world_center: [f32; 2],
}
