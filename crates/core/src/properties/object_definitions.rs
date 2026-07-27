use super::*;
use plotx_figure::Color;

pub const STACK_MODE: PropertyId = PropertyId("object.stack.mode");
pub const STACK_SPACING_Y: PropertyId = PropertyId("object.stack.spacing_y");
pub const STACK_SHEAR_X: PropertyId = PropertyId("object.stack.shear_x");
pub const STACK_NORMALIZE: PropertyId = PropertyId("object.stack.normalize");
pub const CHART_TYPE_ID: PropertyId = PropertyId("object.chart.type_id");
pub const CHART_BINS_AUTO: PropertyId = PropertyId("object.chart.bins.auto");
pub const CHART_BINS_COUNT: PropertyId = PropertyId("object.chart.bins.count");
pub const CHART_STACKED: PropertyId = PropertyId("object.chart.stacked");
pub const CHART_COLORMAP: PropertyId = PropertyId("object.chart.colormap");
pub const CHART_VIEW_AZIMUTH: PropertyId = PropertyId("object.chart.view_angles.0");
pub const CHART_VIEW_ELEVATION: PropertyId = PropertyId("object.chart.view_angles.1");
pub const PANEL_USER_NOTE: PropertyId = PropertyId("object.panel.user_note");
pub const PANEL_VISIBLE: PropertyId = PropertyId("object.panel.visible");
pub const SERIES_VISIBLE: PropertyId = PropertyId("series.visible");
pub const TEXT: PropertyId = PropertyId("object.text.text");
pub const TEXT_FONT_SIZE: PropertyId = PropertyId("object.text.font_size");
pub const TEXT_BOLD: PropertyId = PropertyId("object.text.bold");
pub const TEXT_ALIGN: PropertyId = PropertyId("object.text.align");
pub const TEXT_COLOR: PropertyId = PropertyId("object.text.color");
pub const SHAPE_KIND: PropertyId = PropertyId("object.shape.shape");
pub const SHAPE_STROKE: PropertyId = PropertyId("object.shape.stroke");
pub const SHAPE_STROKE_WIDTH: PropertyId = PropertyId("object.shape.stroke_width");
pub const SHAPE_FILL_ENABLED: PropertyId = PropertyId("object.shape.fill.enabled");
pub const SHAPE_FILL_COLOR: PropertyId = PropertyId("object.shape.fill.color");
pub const LOCKED: PropertyId = PropertyId("object.locked");

pub const SUPERIMPOSED: &str = "superimposed";
pub const OFFSET: &str = "offset";
pub const COLOR_OVERLAY: &str = "color_overlay";
pub const ALIGN_LEFT: &str = "left";
pub const ALIGN_CENTER: &str = "center";
pub const ALIGN_RIGHT: &str = "right";
pub const SHAPE_RECT: &str = "rect";
pub const SHAPE_ELLIPSE: &str = "ellipse";
pub const SHAPE_LINE: &str = "line";
pub const SHAPE_ARROW: &str = "arrow";

pub(super) const STACK_MODES: &[EnumVariant] = &[
    EnumVariant::new(SUPERIMPOSED, "Superimposed"),
    EnumVariant::new(OFFSET, "Offset"),
    EnumVariant::new(COLOR_OVERLAY, "Color overlay"),
];
const ALIGNMENTS: &[EnumVariant] = &[
    EnumVariant::new(ALIGN_LEFT, "Left"),
    EnumVariant::new(ALIGN_CENTER, "Center"),
    EnumVariant::new(ALIGN_RIGHT, "Right"),
];
const SHAPES: &[EnumVariant] = &[
    EnumVariant::new(SHAPE_RECT, "Rectangle"),
    EnumVariant::new(SHAPE_ELLIPSE, "Ellipse"),
    EnumVariant::new(SHAPE_LINE, "Line"),
    EnumVariant::new(SHAPE_ARROW, "Arrow"),
];
const COLORMAPS: &[EnumVariant] = &[
    EnumVariant::new("viridis", "Viridis"),
    EnumVariant::new("plasma", "Plasma"),
    EnumVariant::new("inferno", "Inferno"),
    EnumVariant::new("magma", "Magma"),
    EnumVariant::new("turbo", "Turbo"),
    EnumVariant::new("coolwarm", "Coolwarm"),
    EnumVariant::new("grays", "Grays"),
];
const CHART_TYPES: &[EnumVariant] = &[
    EnumVariant::new("afm_map", "AFM Map").requiring(&[
        crate::automation::CAP_FIELD_SCALAR_GRID_2D_REGULAR,
        crate::automation::CAP_FIELD_AFM_MAP,
    ]),
    EnumVariant::new("afm_force_curve", "Force Curve").requiring(&[
        crate::automation::CAP_FIELD_CURVE_1D,
        crate::automation::CAP_FIELD_FORCE_CURVE,
    ]),
    EnumVariant::new("electrophysiology_sweeps", "Sweeps").requiring(&[
        crate::automation::CAP_FIELD_CURVE_1D,
        crate::automation::CAP_FIELD_SWEEP_COLLECTION,
    ]),
    EnumVariant::new("nmr_spectrum", "Spectrum").requiring(&[
        crate::automation::CAP_FIELD_CURVE_1D,
        crate::automation::CAP_FIELD_NMR_SPECTRUM,
    ]),
    EnumVariant::new("nmr_contour", "Contour").requiring(&[
        crate::automation::CAP_FIELD_SCALAR_GRID_2D_REGULAR,
        crate::automation::CAP_FIELD_NMR_CONTOUR,
    ]),
    EnumVariant::new("nmr_pseudo", "Stack / analysis").requiring(&[
        crate::automation::CAP_FIELD_CURVE_1D,
        crate::automation::CAP_FIELD_NMR_STACK,
    ]),
    EnumVariant::new("table_line", "Line").requiring(&[
        crate::automation::CAP_FIELD_CURVE_1D,
        crate::automation::CAP_FIELD_TABLE,
    ]),
    EnumVariant::new("table_bar", "Bar").requiring(&[crate::automation::CAP_FIELD_TABLE]),
    EnumVariant::new("table_bar_grouped", "Grouped bars")
        .requiring(&[crate::automation::CAP_FIELD_TABLE]),
    EnumVariant::new("table_histogram", "Histogram")
        .requiring(&[crate::automation::CAP_FIELD_TABLE]),
    EnumVariant::new("table_box", "Box").requiring(&[crate::automation::CAP_FIELD_TABLE]),
    EnumVariant::new("table_violin", "Violin").requiring(&[crate::automation::CAP_FIELD_TABLE]),
    EnumVariant::new("table_heatmap", "Heatmap").requiring(&[crate::automation::CAP_FIELD_TABLE]),
    EnumVariant::new("table_pie", "Pie").requiring(&[crate::automation::CAP_FIELD_TABLE]),
    EnumVariant::new("table_surface", "Surface 3D")
        .requiring(&[crate::automation::CAP_FIELD_TABLE]),
];

pub(super) const FILL_FALLBACK: Color = Color::rgb(200, 200, 200);
const OBJECT: Applicability = Applicability::component(ComponentKind::None);
const SERIES: Applicability = Applicability::component(ComponentKind::Series);

const fn definition(
    id: PropertyId,
    schema: ValueSchema,
    default_policy: DefaultPolicy,
    label: &'static str,
    aliases: &'static [&'static str],
) -> PropertyDefinition {
    PropertyDefinition {
        id,
        scope_kind: ScopeKind::Object,
        value_schema: schema,
        access: PropertyAccess::ReadWrite,
        applicability: OBJECT,
        default_policy,
        tier: Tier::Essential,
        copies: ValueCopies::PerTarget,
        canonical_label: label,
        canonical_aliases: aliases,
    }
}

#[rustfmt::skip]
pub(crate) const DEFINITIONS: &[PropertyDefinition] = &[
    definition(STACK_MODE, ValueSchema::Enum { variants: STACK_MODES }, DefaultPolicy::Fixed(PropertyValue::Enum(SUPERIMPOSED)), "Stack mode", &["overlay mode"]),
    definition(STACK_SPACING_Y, ValueSchema::Float { bounds: FloatBounds::inclusive(0.0, 1.0), display: FloatDisplay::Linear(""), drag_step: Some(0.01) }, DefaultPolicy::Fixed(PropertyValue::Float(0.12)), "Vertical stack spacing", &["vertical spacing"]),
    definition(STACK_SHEAR_X, ValueSchema::Float { bounds: FloatBounds::inclusive(-0.5, 0.5), display: FloatDisplay::Linear(""), drag_step: Some(0.01) }, DefaultPolicy::Fixed(PropertyValue::Float(0.0)), "Horizontal stack shear", &["3D shear"]),
    definition(STACK_NORMALIZE, ValueSchema::Bool, DefaultPolicy::Fixed(PropertyValue::Bool(false)), "Normalize stacked traces", &["normalize stack"]),
    definition(CHART_TYPE_ID, ValueSchema::Enum { variants: CHART_TYPES }, DefaultPolicy::Derived, "Chart type", &["plot type"]),
    definition(CHART_BINS_AUTO, ValueSchema::Bool, DefaultPolicy::Fixed(PropertyValue::Bool(true)), "Automatic histogram bins", &["auto bins"]),
    definition(CHART_BINS_COUNT, ValueSchema::IntWithDrag { min: 1, max: 512, drag_step: 1.0 }, DefaultPolicy::Fixed(PropertyValue::Int(20)), "Histogram bin count", &["bins", "bucket count"]),
    definition(CHART_STACKED, ValueSchema::Bool, DefaultPolicy::Fixed(PropertyValue::Bool(false)), "Stack grouped bars", &["stacked bars"]),
    definition(CHART_COLORMAP, ValueSchema::Enum { variants: COLORMAPS }, DefaultPolicy::Fixed(PropertyValue::Enum("viridis")), "Chart colormap", &["colour map"]),
    definition(CHART_VIEW_AZIMUTH, ValueSchema::Float { bounds: FloatBounds::inclusive(-180.0_f64.to_radians(), 180.0_f64.to_radians()), display: FloatDisplay::Degrees, drag_step: Some(1.0) }, DefaultPolicy::Fixed(PropertyValue::Float(-50.0_f64.to_radians())), "Surface azimuth", &["view azimuth"]),
    definition(CHART_VIEW_ELEVATION, ValueSchema::Float { bounds: FloatBounds::inclusive(5.0_f64.to_radians(), 90.0_f64.to_radians()), display: FloatDisplay::Degrees, drag_step: Some(1.0) }, DefaultPolicy::Fixed(PropertyValue::Float(30.0_f64.to_radians())), "Surface elevation", &["view elevation"]),
    definition(PANEL_USER_NOTE, ValueSchema::Text, DefaultPolicy::Fixed(PropertyValue::Text(String::new())), "Panel note", &["figure note"]),
    definition(PANEL_VISIBLE, ValueSchema::Bool, DefaultPolicy::Fixed(PropertyValue::Bool(true)), "Show panel letter", &["panel label visible"]),
    PropertyDefinition { id: SERIES_VISIBLE, scope_kind: ScopeKind::Object, value_schema: ValueSchema::Bool, access: PropertyAccess::ReadWrite, applicability: SERIES, default_policy: DefaultPolicy::Fixed(PropertyValue::Bool(true)), tier: Tier::Essential, copies: ValueCopies::PerTarget, canonical_label: "Series visibility", canonical_aliases: &["show series"] },
    definition(TEXT, ValueSchema::Text, DefaultPolicy::Derived, "Text", &["label text"]),
    definition(TEXT_FONT_SIZE, ValueSchema::Float { bounds: FloatBounds::inclusive(4.0, 200.0), display: FloatDisplay::Linear("pt"), drag_step: Some(0.5) }, DefaultPolicy::Derived, "Text size", &["font size"]),
    definition(TEXT_BOLD, ValueSchema::Bool, DefaultPolicy::Derived, "Bold text", &["font weight"]),
    definition(TEXT_ALIGN, ValueSchema::Enum { variants: ALIGNMENTS }, DefaultPolicy::Derived, "Text alignment", &["align"]),
    definition(TEXT_COLOR, ValueSchema::Color, DefaultPolicy::Derived, "Text color", &["text colour"]),
    definition(SHAPE_KIND, ValueSchema::Enum { variants: SHAPES }, DefaultPolicy::Fixed(PropertyValue::Enum(SHAPE_RECT)), "Shape kind", &["shape primitive"]),
    definition(SHAPE_STROKE, ValueSchema::Color, DefaultPolicy::Fixed(PropertyValue::Color(Color::BLACK)), "Shape stroke color", &["outline colour"]),
    definition(SHAPE_STROKE_WIDTH, ValueSchema::Float { bounds: FloatBounds::inclusive(0.1, 40.0), display: FloatDisplay::Linear("pt"), drag_step: Some(0.1) }, DefaultPolicy::Fixed(PropertyValue::Float(1.5)), "Shape stroke width", &["outline width"]),
    definition(SHAPE_FILL_ENABLED, ValueSchema::Bool, DefaultPolicy::Fixed(PropertyValue::Bool(false)), "Fill shape", &["fill enabled"]),
    definition(SHAPE_FILL_COLOR, ValueSchema::Color, DefaultPolicy::Fixed(PropertyValue::Color(FILL_FALLBACK)), "Shape fill color", &["fill colour"]),
    definition(LOCKED, ValueSchema::Bool, DefaultPolicy::Fixed(PropertyValue::Bool(false)), "Lock object", &["locked"]),
];
