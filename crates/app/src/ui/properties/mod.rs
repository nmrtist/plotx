//! Presentation for the property catalog: what a property is called in the
//! interface and where it lives.
//!
//! The semantic half — identity, schema, applicability, tier, default policy —
//! stays in `plotx_core::properties`. This table adds only what the interface
//! needs, and may never introduce an entry that has no definition behind it. In
//! particular the tier is *read* from the definition rather than repeated here,
//! so a property cannot be Essential in the panel and Advanced in the catalog.

pub(crate) mod discovery;
pub(crate) mod panel;
pub(crate) mod readout;
mod search;

#[cfg(test)]
pub(crate) mod fixture;

pub(crate) use search::property_hits;

use plotx_core::properties::{
    PropertyDefinition, PropertyId, Tier, apodization, contour, definition, line, typography,
};
use plotx_core::state::WorkflowTab;

/// A user-facing string in the active locale. PlotX ships one locale today; the
/// type marks which strings are translatable so adding another is a table edit
/// rather than a rework of the search index.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LocalizedText(pub &'static str);

impl LocalizedText {
    pub const fn get(self) -> &'static str {
        self.0
    }
}

/// Which panel owns a property's canonical home.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PanelRoute {
    SecondarySidebar,
    Processing,
}

impl PanelRoute {
    /// The section ids this panel actually renders. A home route naming
    /// anything else could not be navigated to, which is what the consistency
    /// test checks.
    pub const fn sections(self) -> &'static [&'static str] {
        match self {
            Self::SecondarySidebar => &[
                panel::CONTOUR_SECTION,
                panel::LINE_SECTION,
                panel::TYPOGRAPHY_SECTION,
            ],
            Self::Processing => &[panel::APODIZATION_SECTION],
        }
    }

    pub const fn title(self) -> &'static str {
        match self {
            Self::SecondarySidebar => "Object inspector",
            Self::Processing => "Processing tools",
        }
    }
}

/// Where a property is edited. This is data, not code: navigation opens the
/// panel, expands the section and scrolls to the row named by the property id.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HomeRoute {
    pub panel: PanelRoute,
    pub section: &'static str,
}

/// The interface half of one catalog entry.
#[derive(Clone, Copy, Debug)]
pub struct PropertyPresentation {
    pub id: PropertyId,
    pub localized_label: LocalizedText,
    pub localized_aliases: &'static [LocalizedText],
    pub home_route: HomeRoute,
    /// Whether the canvas `+` / `-` gesture drives this property (§8.5
    /// channel 3). Declared here, on the property's single registration, so the
    /// gesture is derived rather than listed in a table of its own. Most
    /// properties have no natural direction, which is why this is an opt-in and
    /// not an inference.
    pub canvas_step: bool,
}

/// Where a group of properties appears in the Ribbon.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RibbonSpot {
    pub tab: WorkflowTab,
    pub group: &'static str,
    /// Lower values survive longer as the Ribbon's width budget tightens.
    pub priority: u8,
}

/// One group of properties with a shared home (§8.5 channel 2 and 4).
///
/// The Ribbon and the context menu address groups, never single parameters:
/// they are entry maps that jump to the panel section where the controls
/// already live. Membership is not listed here — it is read off the members'
/// home routes — so adding a property to an existing group requires no edit to
/// this table.
#[derive(Clone, Copy, Debug)]
pub struct PropertyGroup {
    /// The home-route section its members share.
    pub section: &'static str,
    pub label: LocalizedText,
    pub icon: &'static str,
    pub ribbon: RibbonSpot,
    /// Why the entry is disabled when nothing in the selection has a member of
    /// this group. Starts with a verb and says how to unblock it.
    pub unavailable_reason: &'static str,
}

pub const GROUPS: &[PropertyGroup] = &[
    PropertyGroup {
        section: panel::CONTOUR_SECTION,
        label: LocalizedText("Contour"),
        icon: egui_phosphor::regular::CHART_POLAR,
        ribbon: RibbonSpot {
            tab: WorkflowTab::Figure,
            group: "Style",
            priority: 2,
        },
        unavailable_reason: "Select a plot whose series draws contours before changing contour levels.",
    },
    PropertyGroup {
        section: panel::LINE_SECTION,
        label: LocalizedText("Line"),
        icon: egui_phosphor::regular::LINE_SEGMENT,
        ribbon: RibbonSpot {
            tab: WorkflowTab::Figure,
            group: "Style",
            priority: 3,
        },
        unavailable_reason: "Select a plot whose series draws lines before changing line style.",
    },
    PropertyGroup {
        section: panel::TYPOGRAPHY_SECTION,
        label: LocalizedText("Figure typography"),
        icon: egui_phosphor::regular::TEXT_T,
        ribbon: RibbonSpot {
            tab: WorkflowTab::Figure,
            group: "Style",
            priority: 3,
        },
        unavailable_reason: "Open a PlotX document before changing figure typography.",
    },
    PropertyGroup {
        section: panel::APODIZATION_SECTION,
        label: LocalizedText("Apodization"),
        icon: egui_phosphor::regular::WAVEFORM,
        ribbon: RibbonSpot {
            tab: WorkflowTab::Process,
            group: "Processing",
            priority: 1,
        },
        unavailable_reason: "Select a dataset with an apodization processing step.",
    },
];

impl PropertyPresentation {
    /// The tier lives on the definition; presentation reads it so the panel
    /// budget and the catalog can never disagree.
    pub fn tier(&self) -> Option<Tier> {
        definition(self.id).map(|definition| definition.tier)
    }

    pub fn definition(&self) -> Option<&'static PropertyDefinition> {
        definition(self.id)
    }
}

const CONTOUR_HOME: HomeRoute = HomeRoute {
    panel: PanelRoute::SecondarySidebar,
    section: panel::CONTOUR_SECTION,
};

const LINE_HOME: HomeRoute = HomeRoute {
    panel: PanelRoute::SecondarySidebar,
    section: panel::LINE_SECTION,
};

const TYPOGRAPHY_HOME: HomeRoute = HomeRoute {
    panel: PanelRoute::SecondarySidebar,
    section: panel::TYPOGRAPHY_SECTION,
};

const APODIZATION_HOME: HomeRoute = HomeRoute {
    panel: PanelRoute::Processing,
    section: panel::APODIZATION_SECTION,
};

pub const PRESENTATIONS: &[PropertyPresentation] = &[
    PropertyPresentation {
        id: contour::BASE_MAGNITUDE,
        localized_label: LocalizedText("Lowest level"),
        localized_aliases: &[
            LocalizedText("threshold"),
            LocalizedText("contour threshold"),
            LocalizedText("noise multiple"),
        ],
        home_route: CONTOUR_HOME,
        // The one contour setting worth reaching without leaving the plot:
        // §1 principle 4(c) — the best parameter is the one you never look for.
        canvas_step: true,
    },
    PropertyPresentation {
        id: contour::BASE_POLICY,
        localized_label: LocalizedText("Anchor"),
        localized_aliases: &[LocalizedText("level anchor"), LocalizedText("base policy")],
        home_route: CONTOUR_HOME,
        canvas_step: false,
    },
    PropertyPresentation {
        id: contour::COUNT,
        localized_label: LocalizedText("Levels"),
        localized_aliases: &[
            LocalizedText("contour levels"),
            LocalizedText("number of contours"),
        ],
        home_route: CONTOUR_HOME,
        canvas_step: false,
    },
    PropertyPresentation {
        id: contour::RATIO,
        localized_label: LocalizedText("Level ratio"),
        localized_aliases: &[LocalizedText("contour spacing")],
        home_route: CONTOUR_HOME,
        canvas_step: false,
    },
    PropertyPresentation {
        id: contour::NEGATIVE_ENABLED,
        localized_label: LocalizedText("Negative contours"),
        localized_aliases: &[LocalizedText("negative peaks")],
        home_route: CONTOUR_HOME,
        canvas_step: false,
    },
    PropertyPresentation {
        id: contour::POSITIVE_COLOR,
        localized_label: LocalizedText("Positive colour"),
        localized_aliases: &[LocalizedText("contour colour")],
        home_route: CONTOUR_HOME,
        canvas_step: false,
    },
    PropertyPresentation {
        id: contour::NEGATIVE_COLOR,
        localized_label: LocalizedText("Negative colour"),
        localized_aliases: &[],
        home_route: CONTOUR_HOME,
        canvas_step: false,
    },
    PropertyPresentation {
        id: contour::LINE_WIDTH,
        localized_label: LocalizedText("Line width"),
        localized_aliases: &[LocalizedText("contour width")],
        home_route: CONTOUR_HOME,
        canvas_step: false,
    },
    PropertyPresentation {
        id: line::STROKE_WIDTH,
        localized_label: LocalizedText("Stroke width"),
        localized_aliases: &[LocalizedText("line thickness")],
        home_route: LINE_HOME,
        canvas_step: false,
    },
    PropertyPresentation {
        id: typography::TICK_PT,
        localized_label: LocalizedText("Tick-label size"),
        localized_aliases: &[LocalizedText("figure font size")],
        home_route: TYPOGRAPHY_HOME,
        canvas_step: false,
    },
    PropertyPresentation {
        id: apodization::KIND,
        localized_label: LocalizedText("Window"),
        localized_aliases: &[LocalizedText("apodization window")],
        home_route: APODIZATION_HOME,
        canvas_step: false,
    },
    PropertyPresentation {
        id: apodization::LB_HZ,
        localized_label: LocalizedText("LB"),
        localized_aliases: &[LocalizedText("line broadening")],
        home_route: APODIZATION_HOME,
        canvas_step: false,
    },
    PropertyPresentation {
        id: apodization::GB_HZ,
        localized_label: LocalizedText("GB"),
        localized_aliases: &[LocalizedText("gaussian broadening")],
        home_route: APODIZATION_HOME,
        canvas_step: false,
    },
];

pub fn presentation(id: PropertyId) -> Option<&'static PropertyPresentation> {
    PRESENTATIONS
        .iter()
        .find(|presentation| presentation.id == id)
}

/// Essential entries a single panel section renders by default. The budget
/// check counts these.
pub fn essential_in(section: &str) -> Vec<&'static PropertyPresentation> {
    PRESENTATIONS
        .iter()
        .filter(|presentation| {
            presentation.home_route.section == section
                && presentation.tier() == Some(Tier::Essential)
        })
        .collect()
}

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
