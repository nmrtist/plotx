//! Plot-object legends and continuous colour scales.

use super::provider::PropertyProvider;
use super::target::{require_plot_object_target, resolved_schema};
use super::{
    AggregateValue, Applicability, Availability, ComponentKind, DefaultPolicy, EditOp, EnumVariant,
    PropertyAccess, PropertyAddress, PropertyDefinition, PropertyError, PropertyId,
    PropertyTransaction, PropertyValue, ResolvedProperty, ScopeKind, Tier, ValueCopies,
    ValueSchema,
};
use crate::state::{FieldCapabilities, PlotObject, PlotxApp};
use plotx_figure::{GuideLayout, GuidePlacement, GuideVisibility};

pub const VISIBILITY: PropertyId = PropertyId("object.guides.visibility");
pub const PLACEMENT: PropertyId = PropertyId("object.guides.placement");
pub const LAYOUT: PropertyId = PropertyId("object.guides.layout");
pub const TITLE: PropertyId = PropertyId("object.guides.title");

pub const AUTO: &str = "auto";
pub const SHOW: &str = "show";
pub const HIDE: &str = "hide";
pub const INSIDE: &str = "inside";
pub const OUTSIDE_RIGHT: &str = "outside_right";
pub const OUTSIDE_BOTTOM: &str = "outside_bottom";
pub const CUSTOM: &str = "custom";

const VISIBILITY_MODES: &[EnumVariant] = &[
    EnumVariant::new(AUTO, "Auto"),
    EnumVariant::new(SHOW, "Show"),
    EnumVariant::new(HIDE, "Hide"),
];
const PLACEMENTS: &[EnumVariant] = &[
    EnumVariant::new(AUTO, "Auto"),
    EnumVariant::new(INSIDE, "Inside"),
    EnumVariant::new(OUTSIDE_RIGHT, "Outside right"),
    EnumVariant::new(OUTSIDE_BOTTOM, "Outside bottom"),
    EnumVariant::new(CUSTOM, "Custom"),
];
const LAYOUTS: &[EnumVariant] = &[
    EnumVariant::new(AUTO, "Auto"),
    EnumVariant::new("vertical", "Vertical"),
    EnumVariant::new("horizontal", "Horizontal"),
];

const OBJECT: Applicability = Applicability::component(ComponentKind::None);

pub(crate) const DEFINITIONS: &[PropertyDefinition] = &[
    definition(
        VISIBILITY,
        ValueSchema::Enum {
            variants: VISIBILITY_MODES,
        },
        "Guide visibility",
        &["legend visibility", "color scale visibility", "show key"],
    ),
    definition(
        PLACEMENT,
        ValueSchema::Enum {
            variants: PLACEMENTS,
        },
        "Guide placement",
        &["legend position", "color scale position"],
    ),
    definition(
        LAYOUT,
        ValueSchema::Enum { variants: LAYOUTS },
        "Legend layout",
        &["legend direction", "horizontal legend", "vertical legend"],
    ),
    PropertyDefinition {
        id: TITLE,
        scope_kind: ScopeKind::Object,
        value_schema: ValueSchema::Text,
        access: PropertyAccess::ReadWrite,
        applicability: OBJECT,
        default_policy: DefaultPolicy::Derived,
        tier: Tier::Advanced,
        copies: ValueCopies::PerTarget,
        canonical_label: "Legend title",
        canonical_aliases: &["guide title", "key title"],
    },
];

const fn definition(
    id: PropertyId,
    value_schema: ValueSchema,
    canonical_label: &'static str,
    canonical_aliases: &'static [&'static str],
) -> PropertyDefinition {
    PropertyDefinition {
        id,
        scope_kind: ScopeKind::Object,
        value_schema,
        access: PropertyAccess::ReadWrite,
        applicability: OBJECT,
        default_policy: DefaultPolicy::Fixed(PropertyValue::Enum(AUTO)),
        tier: Tier::Essential,
        copies: ValueCopies::PerTarget,
        canonical_label,
        canonical_aliases,
    }
}

pub(crate) struct GuideProvider;
pub(crate) static PROVIDER: GuideProvider = GuideProvider;

impl PropertyProvider for GuideProvider {
    fn definitions(&self) -> &'static [PropertyDefinition] {
        DEFINITIONS
    }

    fn read(
        &self,
        app: &PlotxApp,
        address: &PropertyAddress,
    ) -> Result<ResolvedProperty, PropertyError> {
        let definition = property_definition(address.definition)?;
        let (_, plot) = plot_for(app, address, definition)?;
        Ok(ResolvedProperty {
            address: address.clone(),
            value: AggregateValue::Uniform(value_of(definition.id, plot)?),
            default_value: Some(default_value(definition.id)?),
            modified: Some(has_override(definition.id, plot)?),
            availability: Availability::Editable,
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
        let (canvas, object) = require_plot_object_target(app, &address.target, definition)?;
        let overrides = transaction.axis_overrides(app, canvas, object)?;
        match (definition.id, operation) {
            (VISIBILITY, EditOp::Set(PropertyValue::Enum(value))) => {
                overrides.guide_visibility = match *value {
                    AUTO => None,
                    SHOW => Some(GuideVisibility::Show),
                    HIDE => Some(GuideVisibility::Hide),
                    _ => return Err(invalid_enum(definition, value)),
                };
            }
            (PLACEMENT, EditOp::Set(PropertyValue::Enum(value))) => match *value {
                AUTO => {
                    overrides.guide_placement = None;
                    overrides.legend_position = None;
                }
                INSIDE => {
                    overrides.guide_placement = Some(GuidePlacement::Inside);
                    overrides.legend_position = None;
                }
                OUTSIDE_RIGHT => {
                    overrides.guide_placement = Some(GuidePlacement::OutsideRight);
                    overrides.legend_position = None;
                }
                OUTSIDE_BOTTOM => {
                    overrides.guide_placement = Some(GuidePlacement::OutsideBottom);
                    overrides.legend_position = None;
                }
                CUSTOM => {
                    overrides.guide_placement = Some(GuidePlacement::Inside);
                    overrides.legend_position.get_or_insert([1.0, 0.0]);
                }
                _ => return Err(invalid_enum(definition, value)),
            },
            (LAYOUT, EditOp::Set(PropertyValue::Enum(value))) => {
                overrides.guide_layout = match *value {
                    AUTO => None,
                    "vertical" => Some(GuideLayout::Vertical),
                    "horizontal" => Some(GuideLayout::Horizontal),
                    _ => return Err(invalid_enum(definition, value)),
                };
            }
            (TITLE, EditOp::Set(PropertyValue::Text(value))) => {
                overrides.guide_title = Some(value.clone());
            }
            (VISIBILITY, EditOp::Reset) => overrides.guide_visibility = None,
            (PLACEMENT, EditOp::Reset) => {
                overrides.guide_placement = None;
                overrides.legend_position = None;
            }
            (LAYOUT, EditOp::Reset) => overrides.guide_layout = None,
            (TITLE, EditOp::Reset) => overrides.guide_title = None,
            (_, EditOp::Step(_)) => {
                return Err(PropertyError::InvalidValue {
                    property: definition.id,
                    message: "guides have no step gesture".to_owned(),
                });
            }
            (_, EditOp::Set(value)) => {
                return Err(PropertyError::InvalidValue {
                    property: definition.id,
                    message: format!(
                        "{} does not accept a value of kind {}",
                        definition.canonical_label,
                        value.kind()
                    ),
                });
            }
            (_, EditOp::Reset) => {
                return Err(PropertyError::UnknownProperty(
                    definition.id.as_str().to_owned(),
                ));
            }
        }
        Ok(())
    }
}

fn plot_for<'a>(
    app: &'a PlotxApp,
    address: &PropertyAddress,
    definition: &'static PropertyDefinition,
) -> Result<(usize, &'a PlotObject), PropertyError> {
    let (canvas, object) = require_plot_object_target(app, &address.target, definition)?;
    let plot = app.doc.canvases[canvas]
        .object(object)
        .and_then(|object| object.plot())
        .ok_or_else(|| PropertyError::UnknownTarget(address.target.describe()))?;
    Ok((canvas, plot))
}

fn value_of(id: PropertyId, plot: &PlotObject) -> Result<PropertyValue, PropertyError> {
    match id {
        VISIBILITY => Ok(PropertyValue::Enum(match plot.figure().guide_visibility {
            GuideVisibility::Auto => AUTO,
            GuideVisibility::Show => SHOW,
            GuideVisibility::Hide => HIDE,
        })),
        PLACEMENT if plot.figure().legend_position.is_some() => Ok(PropertyValue::Enum(CUSTOM)),
        PLACEMENT => Ok(PropertyValue::Enum(match plot.figure().guide_placement {
            GuidePlacement::Auto => AUTO,
            GuidePlacement::Inside => INSIDE,
            GuidePlacement::OutsideRight => OUTSIDE_RIGHT,
            GuidePlacement::OutsideBottom => OUTSIDE_BOTTOM,
        })),
        LAYOUT => Ok(PropertyValue::Enum(match plot.figure().guide_layout {
            GuideLayout::Auto => AUTO,
            GuideLayout::Vertical => "vertical",
            GuideLayout::Horizontal => "horizontal",
        })),
        TITLE => Ok(PropertyValue::Text(plot.figure().guide_title.clone())),
        _ => Err(PropertyError::UnknownProperty(id.as_str().to_owned())),
    }
}

fn has_override(id: PropertyId, plot: &PlotObject) -> Result<bool, PropertyError> {
    match id {
        VISIBILITY => Ok(plot.axis_overrides.guide_visibility.is_some()),
        PLACEMENT => Ok(plot.axis_overrides.guide_placement.is_some()
            || plot.axis_overrides.legend_position.is_some()),
        LAYOUT => Ok(plot.axis_overrides.guide_layout.is_some()),
        TITLE => Ok(plot.axis_overrides.guide_title.is_some()),
        _ => Err(PropertyError::UnknownProperty(id.as_str().to_owned())),
    }
}

fn default_value(id: PropertyId) -> Result<PropertyValue, PropertyError> {
    match id {
        VISIBILITY | PLACEMENT | LAYOUT => Ok(PropertyValue::Enum(AUTO)),
        TITLE => Ok(PropertyValue::Text(String::new())),
        _ => Err(PropertyError::UnknownProperty(id.as_str().to_owned())),
    }
}

fn property_definition(id: PropertyId) -> Result<&'static PropertyDefinition, PropertyError> {
    super::definition(id).ok_or_else(|| PropertyError::UnknownProperty(id.as_str().to_owned()))
}

fn invalid_enum(definition: &'static PropertyDefinition, value: &str) -> PropertyError {
    PropertyError::InvalidValue {
        property: definition.id,
        message: format!("{} does not support {value}", definition.canonical_label),
    }
}
