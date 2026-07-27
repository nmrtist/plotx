use super::panel;
use plotx_core::properties::{PropertyDefinition, PropertyId, Tier, definition};
use plotx_core::state::{SettingsCategory, WorkflowTab};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LocalizedText(pub &'static str);

impl LocalizedText {
    pub const fn get(self) -> &'static str {
        self.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PanelRoute {
    SecondarySidebar,
    Processing,
    CanvasSettings,
    Preferences,
}

const PREFERENCES_SECTIONS: &[&str] = &[
    SettingsCategory::General.section_id(),
    SettingsCategory::Appearance.section_id(),
    SettingsCategory::Processing.section_id(),
    SettingsCategory::Export.section_id(),
    panel::PREFERENCES_UPDATES_SECTION,
    SettingsCategory::Recent.section_id(),
];

impl PanelRoute {
    pub const fn sections(self) -> &'static [&'static str] {
        match self {
            Self::SecondarySidebar => &[
                panel::CONTOUR_SECTION,
                panel::HEATMAP_SECTION,
                panel::LINE_SECTION,
                panel::AXIS_SECTION,
                panel::STACK_SECTION,
                panel::CHART_SECTION,
                panel::TEXT_SECTION,
                panel::SHAPE_SECTION,
                panel::PANEL_SECTION,
                panel::OBJECT_SECTION,
                panel::TYPOGRAPHY_SECTION,
            ],
            Self::Processing => &[
                panel::APODIZATION_SECTION,
                panel::ZERO_FILL_SECTION,
                panel::PHASE_SECTION,
                panel::BASELINE_SECTION,
                panel::REFERENCE_SECTION,
                panel::SMOOTH_SECTION,
                panel::NORMALIZE_SECTION,
                panel::BIN_SECTION,
                panel::PROCESSING_STEP_SECTION,
                panel::PROCESSING_ADVANCED_SECTION,
            ],
            Self::CanvasSettings => &[
                panel::CANVAS_MARGINS_SECTION,
                panel::CANVAS_GRID_SECTION,
                panel::CANVAS_SIZE_SECTION,
                panel::CANVAS_CAPTION_SECTION,
            ],
            Self::Preferences => PREFERENCES_SECTIONS,
        }
    }

    pub const fn title(self) -> &'static str {
        match self {
            Self::SecondarySidebar => "Object inspector",
            Self::Processing => "Processing tools",
            Self::CanvasSettings => "Canvas settings",
            Self::Preferences => "Preferences",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HomeRoute {
    pub panel: PanelRoute,
    pub section: &'static str,
}

#[derive(Clone, Copy, Debug)]
pub struct PropertyPresentation {
    pub id: PropertyId,
    pub localized_label: LocalizedText,
    pub localized_aliases: &'static [LocalizedText],
    pub home_route: HomeRoute,
    pub canvas_step: bool,
    pub uses_canvas_length_unit: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RibbonSpot {
    pub tab: WorkflowTab,
    pub group: &'static str,
    pub priority: u8,
}

#[derive(Clone, Copy, Debug)]
pub struct PropertyGroup {
    pub section: &'static str,
    pub label: LocalizedText,
    pub icon: &'static str,
    pub ribbon: RibbonSpot,
    pub unavailable_reason: &'static str,
}

pub(crate) const fn preference_entry(
    id: PropertyId,
    label: &'static str,
    aliases: &'static [LocalizedText],
    home_route: HomeRoute,
) -> PropertyPresentation {
    PropertyPresentation {
        id,
        localized_label: LocalizedText(label),
        localized_aliases: aliases,
        home_route,
        canvas_step: false,
        uses_canvas_length_unit: false,
    }
}

impl PropertyPresentation {
    pub fn tier(&self) -> Option<Tier> {
        definition(self.id).map(|definition| definition.tier)
    }

    pub fn definition(&self) -> Option<&'static PropertyDefinition> {
        definition(self.id)
    }
}
