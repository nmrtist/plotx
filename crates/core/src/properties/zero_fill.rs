//! Dataset-owned zero-fill-step properties.

use super::processing_common::{
    no_factory_default, no_step_gesture, property_definition, raw_point_count, step_context,
    step_mut, wrong_kind,
};
use super::provider::PropertyProvider;
use super::{
    AggregateValue, Applicability, Availability, ComponentKind, DefaultPolicy, EditOp, EnumVariant,
    PropertyAccess, PropertyAddress, PropertyDefinition, PropertyError, PropertyId,
    PropertyReadout, PropertyTransaction, PropertyValue, ResolvedProperty, ResolvedSchema,
    ScopeKind, Tier, ValueCopies, ValueSchema, ZeroFillTargetReadout,
};
use crate::state::PlotxApp;
use plotx_processing::{StepKind, ZeroFill};

pub const MODE: PropertyId = PropertyId("dataset.processing.zero_fill.mode");
pub const POINTS: PropertyId = PropertyId("dataset.processing.zero_fill.points");

pub const NONE: &str = "none";
pub const X2: &str = "x2";
pub const X4: &str = "x4";
pub const X8: &str = "x8";
pub const CUSTOM: &str = "custom";

const MODES: &[EnumVariant] = &[
    EnumVariant::new(NONE, "None"),
    EnumVariant::new(X2, "×2"),
    EnumVariant::new(X4, "×4"),
    EnumVariant::new(X8, "×8"),
    EnumVariant::new(CUSTOM, "Custom"),
];
const STEP: Applicability = Applicability::component(ComponentKind::ProcessingStep);

pub(crate) const DEFINITIONS: &[PropertyDefinition] = &[
    PropertyDefinition {
        id: MODE,
        scope_kind: ScopeKind::Dataset,
        value_schema: ValueSchema::Enum { variants: MODES },
        access: PropertyAccess::ReadWrite,
        applicability: STEP,
        default_policy: DefaultPolicy::ProcessingFactory,
        tier: Tier::Essential,
        copies: ValueCopies::PerTarget,
        canonical_label: "Zero-fill mode",
        canonical_aliases: &["zero fill", "FFT size", "padding factor"],
    },
    PropertyDefinition {
        id: POINTS,
        scope_kind: ScopeKind::Dataset,
        value_schema: ValueSchema::IntWithDrag {
            min: 1,
            max: i64::MAX,
            drag_step: 256.0,
        },
        access: PropertyAccess::ReadWrite,
        applicability: STEP,
        default_policy: DefaultPolicy::ProcessingFactory,
        tier: Tier::Essential,
        copies: ValueCopies::PerTarget,
        canonical_label: "Zero-fill points",
        canonical_aliases: &["FFT points", "padded points", "zero-fill size"],
    },
];

pub(crate) struct ZeroFillProvider;

pub(crate) static PROVIDER: ZeroFillProvider = ZeroFillProvider;

impl PropertyProvider for ZeroFillProvider {
    fn definitions(&self) -> &'static [PropertyDefinition] {
        DEFINITIONS
    }

    fn read(
        &self,
        app: &PlotxApp,
        address: &PropertyAddress,
    ) -> Result<ResolvedProperty, PropertyError> {
        let definition = property_definition(address.definition)?;
        let context = step_context(app, address, definition, |kind| {
            matches!(kind, StepKind::ZeroFill(_))
        })?;
        let StepKind::ZeroFill(current) = context.step.kind else {
            unreachable!("the shared context checked the step kind");
        };
        let raw = raw_point_count(context.dataset, context.axis);
        let factory = context.factory.as_ref().and_then(|step| match step.kind {
            StepKind::ZeroFill(value) => Some(value),
            _ => None,
        });
        Ok(ResolvedProperty {
            address: address.clone(),
            modified: None,
            value: AggregateValue::Uniform(value_of(definition, current, raw)?),
            default_value: default_value(definition, factory, raw)?,
            availability: Availability::Editable,
            schema: schema_for(definition, current, raw)?,
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
        let context = step_context(app, address, definition, |kind| {
            matches!(kind, StepKind::ZeroFill(_))
        })?;
        let StepKind::ZeroFill(current) = context.step.kind else {
            unreachable!("the shared context checked the step kind");
        };
        let raw = raw_point_count(context.dataset, context.axis);
        let factory = context.factory.as_ref().and_then(|step| match step.kind {
            StepKind::ZeroFill(value) => Some(value),
            _ => None,
        });
        let value = match operation {
            EditOp::Set(value) => checked_value(definition, current, raw, value)?,
            EditOp::Reset => default_value(definition, factory, raw)?
                .ok_or_else(|| no_factory_default(definition))?,
            EditOp::Step(_) => return Err(no_step_gesture(definition)),
        };
        let state = transaction.processing_state(app, context.dataset_id)?;
        let step = step_mut(state, context.step.id, &address.target)?;
        let StepKind::ZeroFill(current) = &mut step.kind else {
            return Err(PropertyError::NotApplicable(
                "the addressed step is no longer a zero-fill step".to_owned(),
            ));
        };
        write(definition, current, raw, value)
    }

    fn readout(
        &self,
        app: &PlotxApp,
        address: &PropertyAddress,
    ) -> Result<PropertyReadout, PropertyError> {
        let definition = property_definition(address.definition)?;
        let context = step_context(app, address, definition, |kind| {
            matches!(kind, StepKind::ZeroFill(_))
        })?;
        let StepKind::ZeroFill(current) = context.step.kind else {
            unreachable!("the shared context checked the step kind");
        };
        Ok(PropertyReadout::ZeroFillTarget(ZeroFillTargetReadout {
            points: current.target(raw_point_count(context.dataset, context.axis)),
        }))
    }
}

fn value_of(
    definition: &'static PropertyDefinition,
    current: ZeroFill,
    raw: usize,
) -> Result<PropertyValue, PropertyError> {
    match definition.id {
        MODE => Ok(PropertyValue::Enum(mode_of(current))),
        POINTS if mode_of(current) == CUSTOM => {
            Ok(PropertyValue::Int(as_i64(current.target(raw), definition)?))
        }
        POINTS => Err(points_unavailable(current)),
        _ => Err(PropertyError::UnknownProperty(definition.id.to_string())),
    }
}

fn default_value(
    definition: &'static PropertyDefinition,
    factory: Option<ZeroFill>,
    raw: usize,
) -> Result<Option<PropertyValue>, PropertyError> {
    let Some(factory) = factory else {
        return Ok(None);
    };
    match definition.id {
        MODE => Ok(Some(PropertyValue::Enum(mode_of(factory)))),
        POINTS => Ok(Some(PropertyValue::Int(as_i64(
            factory.target(raw),
            definition,
        )?))),
        _ => Err(PropertyError::UnknownProperty(definition.id.to_string())),
    }
}

fn schema_for(
    definition: &'static PropertyDefinition,
    current: ZeroFill,
    raw: usize,
) -> Result<ResolvedSchema, PropertyError> {
    match definition.id {
        MODE => Ok(ResolvedSchema::Enum {
            variants: MODES.iter().collect(),
        }),
        POINTS if mode_of(current) == CUSTOM => Ok(ResolvedSchema::IntWithDrag {
            min: as_i64(raw, definition)?,
            max: i64::MAX,
            drag_step: 256.0,
            unit: "points",
        }),
        POINTS => Err(points_unavailable(current)),
        _ => Err(PropertyError::UnknownProperty(definition.id.to_string())),
    }
}

fn checked_value(
    definition: &'static PropertyDefinition,
    current: ZeroFill,
    raw: usize,
    value: &PropertyValue,
) -> Result<PropertyValue, PropertyError> {
    match (definition.id, value) {
        (MODE, PropertyValue::Enum(value)) if MODES.iter().any(|variant| variant.id == *value) => {
            Ok(PropertyValue::Enum(value))
        }
        (MODE, PropertyValue::Enum(value)) => Err(PropertyError::InvalidValue {
            property: definition.id,
            message: format!("'{value}' is not a zero-fill mode"),
        }),
        (MODE, value) => Err(wrong_kind(definition, value, "a zero-fill mode")),
        (POINTS, PropertyValue::Int(value)) if mode_of(current) == CUSTOM => {
            let raw = as_i64(raw, definition)?;
            if *value < raw {
                return Err(PropertyError::InvalidValue {
                    property: definition.id,
                    message: format!(
                        "zero-fill points {value} is out of range: it must be at least the dataset's {raw} original points"
                    ),
                });
            }
            usize::try_from(*value).map_err(|_| PropertyError::InvalidValue {
                property: definition.id,
                message: format!(
                    "zero-fill points {value} is out of range: this platform supports at most {} points",
                    usize::MAX
                ),
            })?;
            Ok(PropertyValue::Int(*value))
        }
        (POINTS, PropertyValue::Int(_)) => Err(points_unavailable(current)),
        (POINTS, value) => Err(wrong_kind(definition, value, "an integer point count")),
        (_, _) => Err(PropertyError::UnknownProperty(definition.id.to_string())),
    }
}

fn write(
    definition: &'static PropertyDefinition,
    current: &mut ZeroFill,
    raw: usize,
    value: PropertyValue,
) -> Result<(), PropertyError> {
    match (definition.id, value) {
        (MODE, PropertyValue::Enum(mode)) => {
            if mode == mode_of(*current) {
                return Ok(());
            }
            *current = match mode {
                NONE => ZeroFill::None,
                X2 => ZeroFill::Factor(2),
                X4 => ZeroFill::Factor(3),
                X8 => ZeroFill::Factor(4),
                // The current effective size is always at least `raw`, so the
                // seed is admitted by the dependent points schema.
                CUSTOM => ZeroFill::Size(current.target(raw)),
                _ => {
                    return Err(PropertyError::InvalidValue {
                        property: definition.id,
                        message: format!("'{mode}' is not a zero-fill mode"),
                    });
                }
            };
            Ok(())
        }
        (POINTS, PropertyValue::Int(points)) => {
            *current = ZeroFill::Size(usize::try_from(points).map_err(|_| {
                PropertyError::InvalidValue {
                    property: definition.id,
                    message: format!(
                        "zero-fill points {points} is out of range: this platform supports at most {} points",
                        usize::MAX
                    ),
                }
            })?);
            Ok(())
        }
        (_, value) => Err(wrong_kind(
            definition,
            &value,
            "the declared zero-fill value",
        )),
    }
}

/// Only the three factors the UI names get a factor label. Every other stored
/// factor is represented as Custom and retains its exact effective point count
/// until the user deliberately replaces it.
fn mode_of(value: ZeroFill) -> &'static str {
    match value {
        ZeroFill::None => NONE,
        ZeroFill::Factor(2) => X2,
        ZeroFill::Factor(3) => X4,
        ZeroFill::Factor(4) => X8,
        ZeroFill::Factor(_) | ZeroFill::Size(_) => CUSTOM,
    }
}

fn points_unavailable(current: ZeroFill) -> PropertyError {
    PropertyError::NotApplicable(format!(
        "Zero-fill points is available only in Custom mode; this step uses {}",
        MODES
            .iter()
            .find(|variant| variant.id == mode_of(current))
            .map(|variant| variant.canonical_label)
            .unwrap_or("an unknown mode")
    ))
}

fn as_i64(points: usize, definition: &'static PropertyDefinition) -> Result<i64, PropertyError> {
    i64::try_from(points).map_err(|_| PropertyError::InvalidValue {
        property: definition.id,
        message: format!(
            "point count {points} is out of range: the property interface supports at most {}",
            i64::MAX
        ),
    })
}
