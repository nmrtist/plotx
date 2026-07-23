use super::{Session, SharedDocument};

pub struct PlotxApp {
    pub doc: SharedDocument,
    pub session: Session,
    /// Live, already-applied tiling preference. This deliberately does not read
    /// the Preferences draft or disk during pointer movement.
    pub keep_empty_source_canvas: bool,
}

/// Live UI-scale state of the monitor under the window: the settings key it is
/// recorded under, the derived automatic zoom, any manual override, and what
/// the physical probe reported (for the Preferences description).
#[derive(Clone, Debug, PartialEq)]
pub struct MonitorScaleStatus {
    /// Key into `settings.appearance.ui_scale.monitors`.
    pub key: String,
    /// Automatic zoom derived from the display's physical density.
    pub auto: f32,
    /// Manual override mirrored from settings; `None` follows `auto`.
    pub user: Option<f32>,
    /// Physical pixels per inch when the display reported its dimensions.
    pub ppi: Option<f32>,
}

impl MonitorScaleStatus {
    pub fn effective(&self) -> f32 {
        self.user.unwrap_or(self.auto)
    }
}
