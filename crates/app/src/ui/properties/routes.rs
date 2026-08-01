//! Canonical homes for property presentations.

use super::*;
use plotx_core::state::SettingsCategory;

pub(super) const CONTOUR_HOME: HomeRoute = HomeRoute {
    panel: PanelRoute::SecondarySidebar,
    section: panel::CONTOUR_SECTION,
};
pub(super) const HEATMAP_HOME: HomeRoute = HomeRoute {
    panel: PanelRoute::SecondarySidebar,
    section: panel::HEATMAP_SECTION,
};
pub(super) const LINE_HOME: HomeRoute = HomeRoute {
    panel: PanelRoute::SecondarySidebar,
    section: panel::LINE_SECTION,
};
pub(super) const AXIS_HOME: HomeRoute = HomeRoute {
    panel: PanelRoute::SecondarySidebar,
    section: panel::AXIS_SECTION,
};
pub(super) const GUIDE_HOME: HomeRoute = HomeRoute {
    panel: PanelRoute::SecondarySidebar,
    section: panel::GUIDE_SECTION,
};
pub(super) const STACK_HOME: HomeRoute = HomeRoute {
    panel: PanelRoute::SecondarySidebar,
    section: panel::STACK_SECTION,
};
pub(super) const CHART_HOME: HomeRoute = HomeRoute {
    panel: PanelRoute::SecondarySidebar,
    section: panel::CHART_SECTION,
};
pub(super) const TEXT_HOME: HomeRoute = HomeRoute {
    panel: PanelRoute::SecondarySidebar,
    section: panel::TEXT_SECTION,
};
pub(super) const SHAPE_HOME: HomeRoute = HomeRoute {
    panel: PanelRoute::SecondarySidebar,
    section: panel::SHAPE_SECTION,
};
pub(super) const PANEL_HOME: HomeRoute = HomeRoute {
    panel: PanelRoute::SecondarySidebar,
    section: panel::PANEL_SECTION,
};
pub(super) const OBJECT_HOME: HomeRoute = HomeRoute {
    panel: PanelRoute::SecondarySidebar,
    section: panel::OBJECT_SECTION,
};
pub(super) const TYPOGRAPHY_HOME: HomeRoute = HomeRoute {
    panel: PanelRoute::SecondarySidebar,
    section: panel::TYPOGRAPHY_SECTION,
};
pub(super) const CANVAS_MARGINS_HOME: HomeRoute = HomeRoute {
    panel: PanelRoute::CanvasSettings,
    section: panel::CANVAS_MARGINS_SECTION,
};
pub(super) const CANVAS_GRID_HOME: HomeRoute = HomeRoute {
    panel: PanelRoute::CanvasSettings,
    section: panel::CANVAS_GRID_SECTION,
};
pub(super) const CANVAS_SIZE_HOME: HomeRoute = HomeRoute {
    panel: PanelRoute::CanvasSettings,
    section: panel::CANVAS_SIZE_SECTION,
};
pub(super) const CANVAS_CAPTION_HOME: HomeRoute = HomeRoute {
    panel: PanelRoute::CanvasSettings,
    section: panel::CANVAS_CAPTION_SECTION,
};
pub(super) const APODIZATION_HOME: HomeRoute = HomeRoute {
    panel: PanelRoute::Processing,
    section: panel::APODIZATION_SECTION,
};
pub(super) const ZERO_FILL_HOME: HomeRoute = HomeRoute {
    panel: PanelRoute::Processing,
    section: panel::ZERO_FILL_SECTION,
};
pub(super) const PHASE_HOME: HomeRoute = HomeRoute {
    panel: PanelRoute::Processing,
    section: panel::PHASE_SECTION,
};
pub(super) const BASELINE_HOME: HomeRoute = HomeRoute {
    panel: PanelRoute::Processing,
    section: panel::BASELINE_SECTION,
};
pub(super) const REFERENCE_HOME: HomeRoute = HomeRoute {
    panel: PanelRoute::Processing,
    section: panel::REFERENCE_SECTION,
};
pub(super) const SMOOTH_HOME: HomeRoute = HomeRoute {
    panel: PanelRoute::Processing,
    section: panel::SMOOTH_SECTION,
};
pub(super) const NORMALIZE_HOME: HomeRoute = HomeRoute {
    panel: PanelRoute::Processing,
    section: panel::NORMALIZE_SECTION,
};
pub(super) const BIN_HOME: HomeRoute = HomeRoute {
    panel: PanelRoute::Processing,
    section: panel::BIN_SECTION,
};
pub(super) const PROCESSING_STEP_HOME: HomeRoute = HomeRoute {
    panel: PanelRoute::Processing,
    section: panel::PROCESSING_STEP_SECTION,
};
pub(super) const PROCESSING_ADVANCED_HOME: HomeRoute = HomeRoute {
    panel: PanelRoute::Processing,
    section: panel::PROCESSING_ADVANCED_SECTION,
};
pub(super) const EXPORT_PREFERENCES_HOME: HomeRoute = HomeRoute {
    panel: PanelRoute::Preferences,
    section: SettingsCategory::Export.section_id(),
};
pub(super) const GENERAL_PREFERENCES_HOME: HomeRoute = HomeRoute {
    panel: PanelRoute::Preferences,
    section: SettingsCategory::General.section_id(),
};
pub(super) const APPEARANCE_PREFERENCES_HOME: HomeRoute = HomeRoute {
    panel: PanelRoute::Preferences,
    section: SettingsCategory::Appearance.section_id(),
};
pub(super) const UPDATES_PREFERENCES_HOME: HomeRoute = HomeRoute {
    panel: PanelRoute::Preferences,
    section: panel::PREFERENCES_UPDATES_SECTION,
};
pub(super) const PROCESSING_PREFERENCES_HOME: HomeRoute = HomeRoute {
    panel: PanelRoute::Preferences,
    section: SettingsCategory::Processing.section_id(),
};

pub(super) const fn object_entry(
    id: PropertyId,
    label: &'static str,
    home_route: HomeRoute,
) -> PropertyPresentation {
    PropertyPresentation {
        id,
        localized_label: LocalizedText(label),
        localized_aliases: &[],
        home_route,
        canvas_step: false,
        uses_canvas_length_unit: false,
    }
}
