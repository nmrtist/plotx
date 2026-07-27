//! Presentation for the property catalog: what a property is called in the
//! interface and where it lives.
//!
//! The semantic half — identity, schema, applicability, tier, default policy —
//! stays in `plotx_core::properties`. This table adds only what the interface
//! needs, and may never introduce an entry that has no definition behind it. In
//! particular the tier is *read* from the definition rather than repeated here,
//! so a property cannot be Essential in the panel and Advanced in the catalog.

pub(crate) mod discovery;
#[path = "groups.rs"]
mod groups;
pub(crate) mod panel;
pub(crate) mod readout;
mod search;
mod types;

#[cfg(test)]
pub(crate) mod fixture;

pub(crate) use groups::GROUPS;
pub(crate) use search::property_hits;
pub use types::*;

#[cfg(test)]
use plotx_core::properties::definition;
use plotx_core::properties::{
    PropertyId, Tier, apodization, app_preferences, axis, baseline, bin, canvas, contour,
    export_dpi, group_delay, ilt, line, normalize, object, phase, reference, smooth, step_enabled,
    typography, zero_fill,
};
use plotx_core::state::{SettingsCategory, WorkflowTab};

const CONTOUR_HOME: HomeRoute = HomeRoute {
    panel: PanelRoute::SecondarySidebar,
    section: panel::CONTOUR_SECTION,
};

const LINE_HOME: HomeRoute = HomeRoute {
    panel: PanelRoute::SecondarySidebar,
    section: panel::LINE_SECTION,
};

const AXIS_HOME: HomeRoute = HomeRoute {
    panel: PanelRoute::SecondarySidebar,
    section: panel::AXIS_SECTION,
};

const STACK_HOME: HomeRoute = HomeRoute {
    panel: PanelRoute::SecondarySidebar,
    section: panel::STACK_SECTION,
};
const CHART_HOME: HomeRoute = HomeRoute {
    panel: PanelRoute::SecondarySidebar,
    section: panel::CHART_SECTION,
};
const TEXT_HOME: HomeRoute = HomeRoute {
    panel: PanelRoute::SecondarySidebar,
    section: panel::TEXT_SECTION,
};
const SHAPE_HOME: HomeRoute = HomeRoute {
    panel: PanelRoute::SecondarySidebar,
    section: panel::SHAPE_SECTION,
};
const PANEL_HOME: HomeRoute = HomeRoute {
    panel: PanelRoute::SecondarySidebar,
    section: panel::PANEL_SECTION,
};
const OBJECT_HOME: HomeRoute = HomeRoute {
    panel: PanelRoute::SecondarySidebar,
    section: panel::OBJECT_SECTION,
};

const fn object_entry(
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

const TYPOGRAPHY_HOME: HomeRoute = HomeRoute {
    panel: PanelRoute::SecondarySidebar,
    section: panel::TYPOGRAPHY_SECTION,
};

const CANVAS_MARGINS_HOME: HomeRoute = HomeRoute {
    panel: PanelRoute::CanvasSettings,
    section: panel::CANVAS_MARGINS_SECTION,
};

const CANVAS_GRID_HOME: HomeRoute = HomeRoute {
    panel: PanelRoute::CanvasSettings,
    section: panel::CANVAS_GRID_SECTION,
};

const CANVAS_SIZE_HOME: HomeRoute = HomeRoute {
    panel: PanelRoute::CanvasSettings,
    section: panel::CANVAS_SIZE_SECTION,
};

const CANVAS_CAPTION_HOME: HomeRoute = HomeRoute {
    panel: PanelRoute::CanvasSettings,
    section: panel::CANVAS_CAPTION_SECTION,
};

const APODIZATION_HOME: HomeRoute = HomeRoute {
    panel: PanelRoute::Processing,
    section: panel::APODIZATION_SECTION,
};

const ZERO_FILL_HOME: HomeRoute = HomeRoute {
    panel: PanelRoute::Processing,
    section: panel::ZERO_FILL_SECTION,
};

const PHASE_HOME: HomeRoute = HomeRoute {
    panel: PanelRoute::Processing,
    section: panel::PHASE_SECTION,
};

const BASELINE_HOME: HomeRoute = HomeRoute {
    panel: PanelRoute::Processing,
    section: panel::BASELINE_SECTION,
};

const REFERENCE_HOME: HomeRoute = HomeRoute {
    panel: PanelRoute::Processing,
    section: panel::REFERENCE_SECTION,
};

const SMOOTH_HOME: HomeRoute = HomeRoute {
    panel: PanelRoute::Processing,
    section: panel::SMOOTH_SECTION,
};

const NORMALIZE_HOME: HomeRoute = HomeRoute {
    panel: PanelRoute::Processing,
    section: panel::NORMALIZE_SECTION,
};

const BIN_HOME: HomeRoute = HomeRoute {
    panel: PanelRoute::Processing,
    section: panel::BIN_SECTION,
};

const PROCESSING_STEP_HOME: HomeRoute = HomeRoute {
    panel: PanelRoute::Processing,
    section: panel::PROCESSING_STEP_SECTION,
};

const PROCESSING_ADVANCED_HOME: HomeRoute = HomeRoute {
    panel: PanelRoute::Processing,
    section: panel::PROCESSING_ADVANCED_SECTION,
};

const EXPORT_PREFERENCES_HOME: HomeRoute = HomeRoute {
    panel: PanelRoute::Preferences,
    section: SettingsCategory::Export.section_id(),
};

const GENERAL_PREFERENCES_HOME: HomeRoute = HomeRoute {
    panel: PanelRoute::Preferences,
    section: SettingsCategory::General.section_id(),
};

const APPEARANCE_PREFERENCES_HOME: HomeRoute = HomeRoute {
    panel: PanelRoute::Preferences,
    section: SettingsCategory::Appearance.section_id(),
};

const UPDATES_PREFERENCES_HOME: HomeRoute = HomeRoute {
    panel: PanelRoute::Preferences,
    section: panel::PREFERENCES_UPDATES_SECTION,
};

const PROCESSING_PREFERENCES_HOME: HomeRoute = HomeRoute {
    panel: PanelRoute::Preferences,
    section: SettingsCategory::Processing.section_id(),
};

pub const PRESENTATIONS: &[PropertyPresentation] = &[
    object_entry(object::STACK_MODE, "Mode", STACK_HOME),
    object_entry(object::STACK_SPACING_Y, "Vertical spacing", STACK_HOME),
    object_entry(object::STACK_SHEAR_X, "3D shear", STACK_HOME),
    object_entry(object::STACK_NORMALIZE, "Normalize", STACK_HOME),
    object_entry(object::SERIES_VISIBLE, "Visible", STACK_HOME),
    object_entry(object::CHART_TYPE_ID, "Type", CHART_HOME),
    object_entry(object::CHART_BINS_AUTO, "Auto bins", CHART_HOME),
    object_entry(object::CHART_BINS_COUNT, "Bins", CHART_HOME),
    object_entry(object::CHART_STACKED, "Stacked", CHART_HOME),
    object_entry(object::CHART_COLORMAP, "Colormap", CHART_HOME),
    object_entry(object::CHART_VIEW_AZIMUTH, "Azimuth", CHART_HOME),
    object_entry(object::CHART_VIEW_ELEVATION, "Elevation", CHART_HOME),
    object_entry(object::PANEL_USER_NOTE, "Note", PANEL_HOME),
    object_entry(object::PANEL_VISIBLE, "Show letter", PANEL_HOME),
    object_entry(object::TEXT, "Text", TEXT_HOME),
    object_entry(object::TEXT_FONT_SIZE, "Size", TEXT_HOME),
    object_entry(object::TEXT_BOLD, "Bold", TEXT_HOME),
    object_entry(object::TEXT_ALIGN, "Align", TEXT_HOME),
    object_entry(object::TEXT_COLOR, "Color", TEXT_HOME),
    object_entry(object::SHAPE_KIND, "Kind", SHAPE_HOME),
    object_entry(object::SHAPE_STROKE, "Stroke", SHAPE_HOME),
    object_entry(object::SHAPE_STROKE_WIDTH, "Stroke width", SHAPE_HOME),
    object_entry(object::SHAPE_FILL_ENABLED, "Fill", SHAPE_HOME),
    object_entry(object::SHAPE_FILL_COLOR, "Fill color", SHAPE_HOME),
    object_entry(object::LOCKED, "Locked", OBJECT_HOME),
    PropertyPresentation {
        id: axis::X_LABEL,
        localized_label: LocalizedText("X title"),
        localized_aliases: &[LocalizedText("x-axis label")],
        home_route: AXIS_HOME,
        canvas_step: false,
        uses_canvas_length_unit: false,
    },
    PropertyPresentation {
        id: axis::Y_LABEL,
        localized_label: LocalizedText("Y title"),
        localized_aliases: &[LocalizedText("y-axis label")],
        home_route: AXIS_HOME,
        canvas_step: false,
        uses_canvas_length_unit: false,
    },
    PropertyPresentation {
        id: axis::X_SHOW_TICK_LABELS,
        localized_label: LocalizedText("X tick labels"),
        localized_aliases: &[LocalizedText("show x ticks")],
        home_route: AXIS_HOME,
        canvas_step: false,
        uses_canvas_length_unit: false,
    },
    PropertyPresentation {
        id: axis::X_SHOW_LABEL,
        localized_label: LocalizedText("Show X title"),
        localized_aliases: &[LocalizedText("x title visibility")],
        home_route: AXIS_HOME,
        canvas_step: false,
        uses_canvas_length_unit: false,
    },
    PropertyPresentation {
        id: axis::Y_SHOW_TICK_LABELS,
        localized_label: LocalizedText("Y tick labels"),
        localized_aliases: &[LocalizedText("show y ticks")],
        home_route: AXIS_HOME,
        canvas_step: false,
        uses_canvas_length_unit: false,
    },
    PropertyPresentation {
        id: axis::Y_SHOW_LABEL,
        localized_label: LocalizedText("Show Y title"),
        localized_aliases: &[LocalizedText("y title visibility")],
        home_route: AXIS_HOME,
        canvas_step: false,
        uses_canvas_length_unit: false,
    },
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
        uses_canvas_length_unit: false,
    },
    PropertyPresentation {
        id: contour::BASE_POLICY,
        localized_label: LocalizedText("Anchor"),
        localized_aliases: &[LocalizedText("level anchor"), LocalizedText("base policy")],
        home_route: CONTOUR_HOME,
        canvas_step: false,
        uses_canvas_length_unit: false,
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
        uses_canvas_length_unit: false,
    },
    PropertyPresentation {
        id: contour::RATIO,
        localized_label: LocalizedText("Level ratio"),
        localized_aliases: &[LocalizedText("contour spacing")],
        home_route: CONTOUR_HOME,
        canvas_step: false,
        uses_canvas_length_unit: false,
    },
    PropertyPresentation {
        id: contour::NEGATIVE_ENABLED,
        localized_label: LocalizedText("Negative contours"),
        localized_aliases: &[LocalizedText("negative peaks")],
        home_route: CONTOUR_HOME,
        canvas_step: false,
        uses_canvas_length_unit: false,
    },
    PropertyPresentation {
        id: contour::POSITIVE_COLOR,
        localized_label: LocalizedText("Positive colour"),
        localized_aliases: &[LocalizedText("contour colour")],
        home_route: CONTOUR_HOME,
        canvas_step: false,
        uses_canvas_length_unit: false,
    },
    PropertyPresentation {
        id: contour::NEGATIVE_COLOR,
        localized_label: LocalizedText("Negative colour"),
        localized_aliases: &[],
        home_route: CONTOUR_HOME,
        canvas_step: false,
        uses_canvas_length_unit: false,
    },
    PropertyPresentation {
        id: contour::LINE_WIDTH,
        localized_label: LocalizedText("Line width"),
        localized_aliases: &[LocalizedText("contour width")],
        home_route: CONTOUR_HOME,
        canvas_step: false,
        uses_canvas_length_unit: false,
    },
    PropertyPresentation {
        id: line::STROKE_WIDTH,
        localized_label: LocalizedText("Stroke width"),
        localized_aliases: &[LocalizedText("line thickness")],
        home_route: LINE_HOME,
        canvas_step: false,
        uses_canvas_length_unit: false,
    },
    PropertyPresentation {
        id: typography::TICK_PT,
        localized_label: LocalizedText("Tick-label size"),
        localized_aliases: &[LocalizedText("figure font size")],
        home_route: TYPOGRAPHY_HOME,
        canvas_step: false,
        uses_canvas_length_unit: false,
    },
    PropertyPresentation {
        id: typography::LABEL_PT,
        localized_label: LocalizedText("Axis titles"),
        localized_aliases: &[LocalizedText("axis label size")],
        home_route: TYPOGRAPHY_HOME,
        canvas_step: false,
        uses_canvas_length_unit: false,
    },
    PropertyPresentation {
        id: typography::TITLE_PT,
        localized_label: LocalizedText("Figure title"),
        localized_aliases: &[LocalizedText("figure title size")],
        home_route: TYPOGRAPHY_HOME,
        canvas_step: false,
        uses_canvas_length_unit: false,
    },
    PropertyPresentation {
        id: canvas::MARGIN_TOP_MM,
        localized_label: LocalizedText("Top margin"),
        localized_aliases: &[],
        home_route: CANVAS_MARGINS_HOME,
        canvas_step: false,
        uses_canvas_length_unit: true,
    },
    PropertyPresentation {
        id: canvas::MARGIN_RIGHT_MM,
        localized_label: LocalizedText("Right margin"),
        localized_aliases: &[],
        home_route: CANVAS_MARGINS_HOME,
        canvas_step: false,
        uses_canvas_length_unit: true,
    },
    PropertyPresentation {
        id: canvas::MARGIN_BOTTOM_MM,
        localized_label: LocalizedText("Bottom margin"),
        localized_aliases: &[],
        home_route: CANVAS_MARGINS_HOME,
        canvas_step: false,
        uses_canvas_length_unit: true,
    },
    PropertyPresentation {
        id: canvas::MARGIN_LEFT_MM,
        localized_label: LocalizedText("Left margin"),
        localized_aliases: &[],
        home_route: CANVAS_MARGINS_HOME,
        canvas_step: false,
        uses_canvas_length_unit: true,
    },
    PropertyPresentation {
        id: canvas::GUTTER_MM,
        localized_label: LocalizedText("Minimum spacing"),
        localized_aliases: &[LocalizedText("gutter")],
        home_route: CANVAS_MARGINS_HOME,
        canvas_step: false,
        uses_canvas_length_unit: true,
    },
    PropertyPresentation {
        id: canvas::ROWS,
        localized_label: LocalizedText("Rows"),
        localized_aliases: &[LocalizedText("grid rows")],
        home_route: CANVAS_GRID_HOME,
        canvas_step: false,
        uses_canvas_length_unit: false,
    },
    PropertyPresentation {
        id: canvas::COLS,
        localized_label: LocalizedText("Columns"),
        localized_aliases: &[LocalizedText("grid columns")],
        home_route: CANVAS_GRID_HOME,
        canvas_step: false,
        uses_canvas_length_unit: false,
    },
    PropertyPresentation {
        id: canvas::SHOW_GRID,
        localized_label: LocalizedText("Show layout grid"),
        localized_aliases: &[LocalizedText("grid overlay")],
        home_route: CANVAS_GRID_HOME,
        canvas_step: false,
        uses_canvas_length_unit: false,
    },
    PropertyPresentation {
        id: canvas::SPACING_MODE,
        localized_label: LocalizedText("Spacing basis"),
        localized_aliases: &[LocalizedText("visual spacing")],
        home_route: CANVAS_GRID_HOME,
        canvas_step: false,
        uses_canvas_length_unit: false,
    },
    PropertyPresentation {
        id: canvas::WIDTH_MM,
        localized_label: LocalizedText("Width"),
        localized_aliases: &[LocalizedText("page width")],
        home_route: CANVAS_SIZE_HOME,
        canvas_step: false,
        uses_canvas_length_unit: true,
    },
    PropertyPresentation {
        id: canvas::HEIGHT_MM,
        localized_label: LocalizedText("Height"),
        localized_aliases: &[LocalizedText("page height")],
        home_route: CANVAS_SIZE_HOME,
        canvas_step: false,
        uses_canvas_length_unit: true,
    },
    PropertyPresentation {
        id: canvas::AUTO_HEIGHT,
        localized_label: LocalizedText("Auto height"),
        localized_aliases: &[LocalizedText("content height")],
        home_route: CANVAS_SIZE_HOME,
        canvas_step: false,
        uses_canvas_length_unit: false,
    },
    PropertyPresentation {
        id: canvas::CAPTION_VISIBLE,
        localized_label: LocalizedText("Show caption below page"),
        localized_aliases: &[LocalizedText("caption visibility")],
        home_route: CANVAS_CAPTION_HOME,
        canvas_step: false,
        uses_canvas_length_unit: false,
    },
    PropertyPresentation {
        id: canvas::PANEL_LABEL_STYLE,
        localized_label: LocalizedText("Panel label style"),
        localized_aliases: &[LocalizedText("panel letters")],
        home_route: CANVAS_CAPTION_HOME,
        canvas_step: false,
        uses_canvas_length_unit: false,
    },
    PropertyPresentation {
        id: apodization::KIND,
        localized_label: LocalizedText("Window"),
        localized_aliases: &[LocalizedText("apodization window")],
        home_route: APODIZATION_HOME,
        canvas_step: false,
        uses_canvas_length_unit: false,
    },
    PropertyPresentation {
        id: apodization::LB_HZ,
        localized_label: LocalizedText("LB"),
        localized_aliases: &[LocalizedText("line broadening")],
        home_route: APODIZATION_HOME,
        canvas_step: false,
        uses_canvas_length_unit: false,
    },
    PropertyPresentation {
        id: apodization::GB_HZ,
        localized_label: LocalizedText("GB"),
        localized_aliases: &[LocalizedText("gaussian broadening")],
        home_route: APODIZATION_HOME,
        canvas_step: false,
        uses_canvas_length_unit: false,
    },
    PropertyPresentation {
        id: zero_fill::MODE,
        localized_label: LocalizedText("Zero fill"),
        localized_aliases: &[LocalizedText("FFT size")],
        home_route: ZERO_FILL_HOME,
        canvas_step: false,
        uses_canvas_length_unit: false,
    },
    PropertyPresentation {
        id: zero_fill::POINTS,
        localized_label: LocalizedText("Points"),
        localized_aliases: &[LocalizedText("custom FFT points")],
        home_route: ZERO_FILL_HOME,
        canvas_step: false,
        uses_canvas_length_unit: false,
    },
    PropertyPresentation {
        id: phase::MODE,
        localized_label: LocalizedText("Mode"),
        localized_aliases: &[LocalizedText("phase method")],
        home_route: PHASE_HOME,
        canvas_step: false,
        uses_canvas_length_unit: false,
    },
    PropertyPresentation {
        id: phase::PHASE0,
        localized_label: LocalizedText("φ0"),
        localized_aliases: &[LocalizedText("zero-order phase")],
        home_route: PHASE_HOME,
        canvas_step: false,
        uses_canvas_length_unit: false,
    },
    PropertyPresentation {
        id: phase::PHASE1,
        localized_label: LocalizedText("φ1"),
        localized_aliases: &[LocalizedText("first-order phase")],
        home_route: PHASE_HOME,
        canvas_step: false,
        uses_canvas_length_unit: false,
    },
    PropertyPresentation {
        id: phase::PIVOT,
        localized_label: LocalizedText("Pivot"),
        localized_aliases: &[LocalizedText("phase pivot fraction")],
        home_route: PHASE_HOME,
        canvas_step: false,
        uses_canvas_length_unit: false,
    },
    PropertyPresentation {
        id: baseline::METHOD,
        localized_label: LocalizedText("Method"),
        localized_aliases: &[LocalizedText("baseline correction")],
        home_route: BASELINE_HOME,
        canvas_step: false,
        uses_canvas_length_unit: false,
    },
    PropertyPresentation {
        id: baseline::POLYNOMIAL_ORDER,
        localized_label: LocalizedText("Order"),
        localized_aliases: &[LocalizedText("polynomial order")],
        home_route: BASELINE_HOME,
        canvas_step: false,
        uses_canvas_length_unit: false,
    },
    PropertyPresentation {
        id: baseline::SMOOTHNESS,
        localized_label: LocalizedText("Smoothness"),
        localized_aliases: &[LocalizedText("AsLS lambda")],
        home_route: BASELINE_HOME,
        canvas_step: false,
        uses_canvas_length_unit: false,
    },
    PropertyPresentation {
        id: baseline::ASYMMETRY,
        localized_label: LocalizedText("Peak weight"),
        localized_aliases: &[LocalizedText("AsLS asymmetry")],
        home_route: BASELINE_HOME,
        canvas_step: false,
        uses_canvas_length_unit: false,
    },
    PropertyPresentation {
        id: baseline::ITERATIONS,
        localized_label: LocalizedText("Iterations"),
        localized_aliases: &[LocalizedText("AsLS passes")],
        home_route: BASELINE_HOME,
        canvas_step: false,
        uses_canvas_length_unit: false,
    },
    PropertyPresentation {
        id: reference::AT_PPM,
        localized_label: LocalizedText("At"),
        localized_aliases: &[LocalizedText("reference source")],
        home_route: REFERENCE_HOME,
        canvas_step: false,
        uses_canvas_length_unit: false,
    },
    PropertyPresentation {
        id: reference::TARGET_PPM,
        localized_label: LocalizedText("Target"),
        localized_aliases: &[LocalizedText("reference destination")],
        home_route: REFERENCE_HOME,
        canvas_step: false,
        uses_canvas_length_unit: false,
    },
    PropertyPresentation {
        id: smooth::METHOD,
        localized_label: LocalizedText("Method"),
        localized_aliases: &[LocalizedText("smoothing method")],
        home_route: SMOOTH_HOME,
        canvas_step: false,
        uses_canvas_length_unit: false,
    },
    PropertyPresentation {
        id: smooth::WINDOW,
        localized_label: LocalizedText("Window"),
        localized_aliases: &[LocalizedText("window points")],
        home_route: SMOOTH_HOME,
        canvas_step: false,
        uses_canvas_length_unit: false,
    },
    PropertyPresentation {
        id: smooth::POLYNOMIAL_ORDER,
        localized_label: LocalizedText("Polynomial order"),
        localized_aliases: &[LocalizedText("Savitzky-Golay order")],
        home_route: SMOOTH_HOME,
        canvas_step: false,
        uses_canvas_length_unit: false,
    },
    PropertyPresentation {
        id: normalize::METHOD,
        localized_label: LocalizedText("Method"),
        localized_aliases: &[LocalizedText("normalization method")],
        home_route: NORMALIZE_HOME,
        canvas_step: false,
        uses_canvas_length_unit: false,
    },
    PropertyPresentation {
        id: normalize::DIVISOR,
        localized_label: LocalizedText("Divisor"),
        localized_aliases: &[LocalizedText("divide by constant")],
        home_route: NORMALIZE_HOME,
        canvas_step: false,
        uses_canvas_length_unit: false,
    },
    PropertyPresentation {
        id: bin::WIDTH,
        localized_label: LocalizedText("Bin width"),
        localized_aliases: &[LocalizedText("bucket width")],
        home_route: BIN_HOME,
        canvas_step: false,
        uses_canvas_length_unit: false,
    },
    PropertyPresentation {
        id: bin::METHOD,
        localized_label: LocalizedText("Aggregate"),
        localized_aliases: &[LocalizedText("bin aggregation")],
        home_route: BIN_HOME,
        canvas_step: false,
        uses_canvas_length_unit: false,
    },
    PropertyPresentation {
        id: step_enabled::ENABLED,
        localized_label: LocalizedText("Enabled"),
        localized_aliases: &[LocalizedText("enable processing step")],
        home_route: PROCESSING_STEP_HOME,
        canvas_step: false,
        uses_canvas_length_unit: false,
    },
    PropertyPresentation {
        id: group_delay::CORRECT,
        localized_label: LocalizedText("Group-delay correction"),
        localized_aliases: &[LocalizedText("digital filter correction")],
        home_route: PROCESSING_ADVANCED_HOME,
        canvas_step: false,
        uses_canvas_length_unit: false,
    },
    preference_entry(
        app_preferences::SNAP_ENABLED,
        "Object snapping",
        &[LocalizedText("snap to guides")],
        GENERAL_PREFERENCES_HOME,
    ),
    preference_entry(
        app_preferences::KEEP_EMPTY_SOURCE_CANVAS,
        "Keep empty source canvas",
        &[LocalizedText("keep source canvas when tiling")],
        GENERAL_PREFERENCES_HOME,
    ),
    preference_entry(
        app_preferences::PROJECT_BACKUP_GENERATIONS,
        "Project backup copies",
        &[LocalizedText("backup generations")],
        GENERAL_PREFERENCES_HOME,
    ),
    preference_entry(
        app_preferences::THEME,
        "Chrome theme",
        &[LocalizedText("appearance theme")],
        APPEARANCE_PREFERENCES_HOME,
    ),
    preference_entry(
        app_preferences::GRAPHICS_POWER,
        "Graphics processor",
        &[LocalizedText("GPU preference")],
        APPEARANCE_PREFERENCES_HOME,
    ),
    preference_entry(
        app_preferences::ACCENT_COLOR,
        "Canvas accent",
        &[LocalizedText("selection colour")],
        APPEARANCE_PREFERENCES_HOME,
    ),
    preference_entry(
        app_preferences::INCLUDE_VIEW_SNAPSHOTS,
        "Embed view snapshots",
        &[LocalizedText("save view snapshots")],
        EXPORT_PREFERENCES_HOME,
    ),
    preference_entry(
        app_preferences::TRIM_TO_VISIBLE_CONTENT,
        "Trim to visible content",
        &[LocalizedText("remove page whitespace")],
        EXPORT_PREFERENCES_HOME,
    ),
    preference_entry(
        app_preferences::SCALE_CONTENT,
        "Scale content with page size",
        &[LocalizedText("resize canvas content")],
        EXPORT_PREFERENCES_HOME,
    ),
    preference_entry(
        app_preferences::AUTO_CHECK_UPDATES,
        "Automatic updates",
        &[LocalizedText("check for updates")],
        UPDATES_PREFERENCES_HOME,
    ),
    preference_entry(
        app_preferences::UPDATE_CHANNEL,
        "Update channel",
        &[LocalizedText("release channel")],
        UPDATES_PREFERENCES_HOME,
    ),
    PropertyPresentation {
        id: export_dpi::DPI,
        localized_label: LocalizedText("Raster resolution"),
        localized_aliases: &[LocalizedText("export DPI"), LocalizedText("bitmap DPI")],
        home_route: EXPORT_PREFERENCES_HOME,
        canvas_step: false,
        uses_canvas_length_unit: false,
    },
    PropertyPresentation {
        id: ilt::DEFAULT_LAMBDA,
        localized_label: LocalizedText("Default ILT regularization"),
        localized_aliases: &[
            LocalizedText("ILT lambda"),
            LocalizedText("DOSY regularization"),
        ],
        home_route: PROCESSING_PREFERENCES_HOME,
        canvas_step: false,
        uses_canvas_length_unit: false,
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
