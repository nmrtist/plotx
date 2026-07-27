//! Canvas-owned page, grid, caption, and size properties.

use super::provider::PropertyProvider;
use super::target::{require_canvas_target, resolved_schema};
use super::{
    AggregateValue, Applicability, Availability, ComponentKind, DefaultPolicy, EditOp, EnumVariant,
    FloatBounds, FloatDisplay, PropertyAccess, PropertyAddress, PropertyDefinition, PropertyError,
    PropertyId, PropertyTransaction, PropertyValue, ResolvedProperty, ScopeKind, Tier, ValueCopies,
    ValueSchema, definition,
};
use crate::layout::SpacingMode;
use crate::state::{CanvasDocument, FieldCapabilities, PanelLabelStyle, PlotxApp};

pub const MARGIN_TOP_MM: PropertyId = PropertyId("canvas.layout.margin_top_mm");
pub const MARGIN_RIGHT_MM: PropertyId = PropertyId("canvas.layout.margin_right_mm");
pub const MARGIN_BOTTOM_MM: PropertyId = PropertyId("canvas.layout.margin_bottom_mm");
pub const MARGIN_LEFT_MM: PropertyId = PropertyId("canvas.layout.margin_left_mm");
pub const GUTTER_MM: PropertyId = PropertyId("canvas.layout.gutter_mm");
pub const ROWS: PropertyId = PropertyId("canvas.layout.rows");
pub const COLS: PropertyId = PropertyId("canvas.layout.cols");
pub const SHOW_GRID: PropertyId = PropertyId("canvas.layout.show_grid");
pub const SPACING_MODE: PropertyId = PropertyId("canvas.layout.spacing_mode");
pub const WIDTH_MM: PropertyId = PropertyId("canvas.size.width_mm");
pub const HEIGHT_MM: PropertyId = PropertyId("canvas.size.height_mm");
pub const AUTO_HEIGHT: PropertyId = PropertyId("canvas.size.auto_height");
pub const CAPTION_VISIBLE: PropertyId = PropertyId("canvas.caption_visible");
pub const PANEL_LABEL_STYLE: PropertyId = PropertyId("canvas.panel_label_style");

const LENGTH_BOUNDS: FloatBounds = FloatBounds::inclusive(0.0, 100.0);
const SIZE_BOUNDS: FloatBounds = FloatBounds::inclusive(10.0, 1000.0);
const LENGTH_STEP_MM: f64 = 1.0;
const GRID_MIN: i64 = 1;
const GRID_MAX: i64 = 12;
const GRID_DRAG_STEP: f64 = 0.1;

pub const SPACING_FRAME: &str = "frame";
pub const SPACING_VISUAL: &str = "visual";
pub const LABEL_LOWER_ALPHA: &str = "lower_alpha";
pub const LABEL_UPPER_ALPHA: &str = "upper_alpha";
pub const LABEL_LOWER_ROMAN: &str = "lower_roman";
pub const LABEL_ARABIC: &str = "arabic";

const SPACING_VARIANTS: &[EnumVariant] = &[
    EnumVariant::new(SPACING_FRAME, "Frame"),
    EnumVariant::new(SPACING_VISUAL, "Visual"),
];
const PANEL_LABEL_VARIANTS: &[EnumVariant] = &[
    EnumVariant::new(LABEL_LOWER_ALPHA, "a, b, c"),
    EnumVariant::new(LABEL_UPPER_ALPHA, "A, B, C"),
    EnumVariant::new(LABEL_LOWER_ROMAN, "i, ii, iii"),
    EnumVariant::new(LABEL_ARABIC, "1, 2, 3"),
];

const CANVAS_VALUE: Applicability = Applicability::component(ComponentKind::None);

const fn float_definition(
    id: PropertyId,
    bounds: FloatBounds,
    default: f64,
    label: &'static str,
    aliases: &'static [&'static str],
) -> PropertyDefinition {
    PropertyDefinition {
        id,
        scope_kind: ScopeKind::Canvas,
        value_schema: ValueSchema::Float {
            bounds,
            display: FloatDisplay::Linear("mm"),
            drag_step: Some(LENGTH_STEP_MM),
        },
        access: PropertyAccess::ReadWrite,
        applicability: CANVAS_VALUE,
        default_policy: DefaultPolicy::Fixed(PropertyValue::Float(default)),
        tier: Tier::Essential,
        copies: ValueCopies::PerTarget,
        canonical_label: label,
        canonical_aliases: aliases,
    }
}

pub(crate) const DEFINITIONS: &[PropertyDefinition] = &[
    float_definition(
        MARGIN_TOP_MM,
        LENGTH_BOUNDS,
        0.0,
        "Top page margin",
        &["top margin", "page margin"],
    ),
    float_definition(
        MARGIN_RIGHT_MM,
        LENGTH_BOUNDS,
        0.0,
        "Right page margin",
        &["right margin", "page margin"],
    ),
    float_definition(
        MARGIN_BOTTOM_MM,
        LENGTH_BOUNDS,
        0.0,
        "Bottom page margin",
        &["bottom margin", "page margin"],
    ),
    float_definition(
        MARGIN_LEFT_MM,
        LENGTH_BOUNDS,
        0.0,
        "Left page margin",
        &["left margin", "page margin"],
    ),
    float_definition(
        GUTTER_MM,
        LENGTH_BOUNDS,
        5.0,
        "Minimum panel spacing",
        &["gutter", "panel spacing"],
    ),
    PropertyDefinition {
        id: ROWS,
        scope_kind: ScopeKind::Canvas,
        value_schema: ValueSchema::IntWithDrag {
            min: GRID_MIN,
            max: GRID_MAX,
            drag_step: GRID_DRAG_STEP,
        },
        access: PropertyAccess::ReadWrite,
        applicability: CANVAS_VALUE,
        default_policy: DefaultPolicy::Fixed(PropertyValue::Int(1)),
        tier: Tier::Essential,
        copies: ValueCopies::PerTarget,
        canonical_label: "Layout grid rows",
        canonical_aliases: &["grid rows", "page rows"],
    },
    PropertyDefinition {
        id: COLS,
        scope_kind: ScopeKind::Canvas,
        value_schema: ValueSchema::IntWithDrag {
            min: GRID_MIN,
            max: GRID_MAX,
            drag_step: GRID_DRAG_STEP,
        },
        access: PropertyAccess::ReadWrite,
        applicability: CANVAS_VALUE,
        default_policy: DefaultPolicy::Fixed(PropertyValue::Int(1)),
        tier: Tier::Essential,
        copies: ValueCopies::PerTarget,
        canonical_label: "Layout grid columns",
        canonical_aliases: &["grid columns", "page columns"],
    },
    PropertyDefinition {
        id: SHOW_GRID,
        scope_kind: ScopeKind::Canvas,
        value_schema: ValueSchema::Bool,
        access: PropertyAccess::ReadWrite,
        applicability: CANVAS_VALUE,
        default_policy: DefaultPolicy::Fixed(PropertyValue::Bool(false)),
        tier: Tier::Essential,
        copies: ValueCopies::PerTarget,
        canonical_label: "Show layout grid",
        canonical_aliases: &["grid overlay", "show grid"],
    },
    PropertyDefinition {
        id: SPACING_MODE,
        scope_kind: ScopeKind::Canvas,
        value_schema: ValueSchema::Enum {
            variants: SPACING_VARIANTS,
        },
        access: PropertyAccess::ReadWrite,
        applicability: CANVAS_VALUE,
        default_policy: DefaultPolicy::Fixed(PropertyValue::Enum(SPACING_VISUAL)),
        tier: Tier::Essential,
        copies: ValueCopies::PerTarget,
        canonical_label: "Panel spacing basis",
        canonical_aliases: &["spacing mode", "visual spacing", "frame spacing"],
    },
    float_definition(
        WIDTH_MM,
        SIZE_BOUNDS,
        crate::state::DEFAULT_CANVAS_SIZE_MM[0] as f64,
        "Canvas width",
        &["page width", "figure width"],
    ),
    float_definition(
        HEIGHT_MM,
        SIZE_BOUNDS,
        crate::state::DEFAULT_CANVAS_SIZE_MM[1] as f64,
        "Canvas height",
        &["page height", "figure height"],
    ),
    PropertyDefinition {
        id: AUTO_HEIGHT,
        scope_kind: ScopeKind::Canvas,
        value_schema: ValueSchema::Bool,
        access: PropertyAccess::ReadWrite,
        applicability: CANVAS_VALUE,
        default_policy: DefaultPolicy::Fixed(PropertyValue::Bool(false)),
        tier: Tier::Essential,
        copies: ValueCopies::PerTarget,
        canonical_label: "Automatic canvas height",
        canonical_aliases: &["auto height", "content height"],
    },
    PropertyDefinition {
        id: CAPTION_VISIBLE,
        scope_kind: ScopeKind::Canvas,
        value_schema: ValueSchema::Bool,
        access: PropertyAccess::ReadWrite,
        applicability: CANVAS_VALUE,
        default_policy: DefaultPolicy::Fixed(PropertyValue::Bool(true)),
        tier: Tier::Essential,
        copies: ValueCopies::PerTarget,
        canonical_label: "Show canvas caption",
        canonical_aliases: &["caption visibility", "show caption"],
    },
    PropertyDefinition {
        id: PANEL_LABEL_STYLE,
        scope_kind: ScopeKind::Canvas,
        value_schema: ValueSchema::Enum {
            variants: PANEL_LABEL_VARIANTS,
        },
        access: PropertyAccess::ReadWrite,
        applicability: CANVAS_VALUE,
        default_policy: DefaultPolicy::Fixed(PropertyValue::Enum(LABEL_LOWER_ALPHA)),
        tier: Tier::Essential,
        copies: ValueCopies::PerTarget,
        canonical_label: "Panel label style",
        canonical_aliases: &["panel letters", "panel numbering"],
    },
];

pub(crate) struct CanvasProvider;

pub(crate) static PROVIDER: CanvasProvider = CanvasProvider;

impl PropertyProvider for CanvasProvider {
    fn definitions(&self) -> &'static [PropertyDefinition] {
        DEFINITIONS
    }

    fn read(
        &self,
        app: &PlotxApp,
        address: &PropertyAddress,
    ) -> Result<ResolvedProperty, PropertyError> {
        let definition = property_definition(address.definition)?;
        let id = require_canvas_target(app, &address.target, definition)?;
        let index = app
            .doc
            .canvas_index(id)
            .ok_or_else(|| PropertyError::UnknownTarget(address.target.resource.id.clone()))?;
        let canvas = &app.doc.canvases[index];
        let availability = if definition.id == HEIGHT_MM && canvas.auto_height {
            Availability::Disabled("Turn off Auto height to set the height manually.")
        } else {
            Availability::Editable
        };
        Ok(ResolvedProperty {
            address: address.clone(),
            modified: None,
            value: AggregateValue::Uniform(value_of(definition.id, canvas)?),
            default_value: fixed_default(definition),
            availability,
            schema: resolved_schema(definition, &FieldCapabilities::default()),
        })
    }

    fn edit(
        &self,
        app: &PlotxApp,
        transaction: &mut PropertyTransaction,
        address: &PropertyAddress,
        operation: &EditOp<'_>,
    ) -> Result<(), PropertyError> {
        let definition = property_definition(address.definition)?;
        let id = require_canvas_target(app, &address.target, definition)?;
        let value = match operation {
            EditOp::Set(value) => checked_value(definition, value)?,
            EditOp::Reset => {
                fixed_default(definition).ok_or_else(|| PropertyError::InvalidValue {
                    property: definition.id,
                    message: "this canvas property has no fixed default".to_owned(),
                })?
            }
            EditOp::Step(_) => {
                return Err(PropertyError::InvalidValue {
                    property: definition.id,
                    message: "this canvas setting has no step gesture".to_owned(),
                });
            }
        };
        match (definition.id, value) {
            (SHOW_GRID, PropertyValue::Bool(show)) => {
                transaction.set_canvas_show_grid(app, id, show)
            }
            (SPACING_MODE, PropertyValue::Enum(value)) => transaction.set_canvas_spacing_mode(
                app,
                id,
                spacing_mode(value).expect("validated spacing mode"),
            ),
            (_, value) => write(definition.id, transaction.canvas(app, id)?, value),
        }
    }
}

fn property_definition(id: PropertyId) -> Result<&'static PropertyDefinition, PropertyError> {
    definition(id).ok_or_else(|| PropertyError::UnknownProperty(id.as_str().to_owned()))
}

fn fixed_default(definition: &'static PropertyDefinition) -> Option<PropertyValue> {
    match &definition.default_policy {
        DefaultPolicy::Fixed(value) => Some(value.clone()),
        DefaultPolicy::EncodingFactory
        | DefaultPolicy::ProcessingFactory
        | DefaultPolicy::Derived
        | DefaultPolicy::None => None,
    }
}

fn margin_index(id: PropertyId) -> Option<usize> {
    match id {
        MARGIN_TOP_MM => Some(0),
        MARGIN_RIGHT_MM => Some(1),
        MARGIN_BOTTOM_MM => Some(2),
        MARGIN_LEFT_MM => Some(3),
        _ => None,
    }
}

fn value_of(id: PropertyId, canvas: &CanvasDocument) -> Result<PropertyValue, PropertyError> {
    if let Some(index) = margin_index(id) {
        return Ok(PropertyValue::Float(f64::from(
            canvas.layout.margin_mm[index],
        )));
    }
    match id {
        GUTTER_MM => Ok(PropertyValue::Float(f64::from(canvas.layout.gutter_mm))),
        ROWS => Ok(PropertyValue::Int(i64::from(canvas.layout.rows))),
        COLS => Ok(PropertyValue::Int(i64::from(canvas.layout.cols))),
        SHOW_GRID => Ok(PropertyValue::Bool(canvas.layout.show_grid)),
        SPACING_MODE => Ok(PropertyValue::Enum(spacing_key(canvas.layout.spacing_mode))),
        WIDTH_MM => Ok(PropertyValue::Float(f64::from(canvas.size_mm[0]))),
        HEIGHT_MM => Ok(PropertyValue::Float(f64::from(canvas.size_mm[1]))),
        AUTO_HEIGHT => Ok(PropertyValue::Bool(canvas.auto_height)),
        CAPTION_VISIBLE => Ok(PropertyValue::Bool(canvas.caption_visible)),
        PANEL_LABEL_STYLE => Ok(PropertyValue::Enum(canvas.panel_label_style.as_key())),
        _ => Err(PropertyError::UnknownProperty(id.as_str().to_owned())),
    }
}

fn checked_value(
    definition: &'static PropertyDefinition,
    value: &PropertyValue,
) -> Result<PropertyValue, PropertyError> {
    match (definition.value_schema, value) {
        (ValueSchema::Float { bounds, .. }, PropertyValue::Float(value)) => {
            bounds.check(definition.id, definition.canonical_label, *value)?;
            Ok(PropertyValue::Float(*value))
        }
        (
            ValueSchema::Int { min, max } | ValueSchema::IntWithDrag { min, max, .. },
            PropertyValue::Int(value),
        ) if (min..=max).contains(value) => Ok(PropertyValue::Int(*value)),
        (ValueSchema::Bool, PropertyValue::Bool(value)) => Ok(PropertyValue::Bool(*value)),
        (ValueSchema::Enum { variants }, PropertyValue::Enum(value))
            if variants.iter().any(|variant| variant.id == *value) =>
        {
            Ok(PropertyValue::Enum(value))
        }
        (_, value) => Err(PropertyError::InvalidValue {
            property: definition.id,
            message: format!(
                "{} does not accept a value of kind {}",
                definition.canonical_label,
                value.kind()
            ),
        }),
    }
}

fn write(
    id: PropertyId,
    canvas: &mut super::transaction::CanvasPropertyState,
    value: PropertyValue,
) -> Result<(), PropertyError> {
    match (id, value) {
        (id, PropertyValue::Float(value)) if margin_index(id).is_some() => {
            canvas.layout.margin_mm[margin_index(id).expect("matched margin")] = value as f32
        }
        (GUTTER_MM, PropertyValue::Float(value)) => canvas.layout.gutter_mm = value as f32,
        (ROWS, PropertyValue::Int(value)) => canvas.layout.rows = value as u32,
        (COLS, PropertyValue::Int(value)) => canvas.layout.cols = value as u32,
        (WIDTH_MM | HEIGHT_MM, PropertyValue::Float(value)) => {
            let mut size_mm = canvas.page_size.size_mm;
            size_mm[usize::from(id == HEIGHT_MM)] = value as f32;
            canvas.page_size = canvas.page_size.after_manual_resize(size_mm);
        }
        (AUTO_HEIGHT, PropertyValue::Bool(value)) => canvas.auto_height = value,
        (CAPTION_VISIBLE, PropertyValue::Bool(value)) => canvas.caption.1 = value,
        (PANEL_LABEL_STYLE, PropertyValue::Enum(value)) => {
            canvas.panel_label_style = panel_label_style(value).expect("validated panel style")
        }
        (_, value) => {
            return Err(PropertyError::InvalidValue {
                property: id,
                message: format!("the canvas property cannot store {}", value.kind()),
            });
        }
    }
    Ok(())
}

fn spacing_key(mode: SpacingMode) -> &'static str {
    match mode {
        SpacingMode::Frame => SPACING_FRAME,
        SpacingMode::Visual => SPACING_VISUAL,
    }
}

fn spacing_mode(value: &str) -> Option<SpacingMode> {
    match value {
        SPACING_FRAME => Some(SpacingMode::Frame),
        SPACING_VISUAL => Some(SpacingMode::Visual),
        _ => None,
    }
}

fn panel_label_style(value: &str) -> Option<PanelLabelStyle> {
    match value {
        LABEL_LOWER_ALPHA => Some(PanelLabelStyle::LowerAlpha),
        LABEL_UPPER_ALPHA => Some(PanelLabelStyle::UpperAlpha),
        LABEL_LOWER_ROMAN => Some(PanelLabelStyle::LowerRoman),
        LABEL_ARABIC => Some(PanelLabelStyle::Arabic),
        _ => None,
    }
}
