//! Dataset-owned binning-step properties.

use super::processing_common::{
    no_step_gesture, property_definition, spectrum_before_step, step_context, step_mut, wrong_kind,
};
use super::provider::PropertyProvider;
use super::{
    AggregateValue, Applicability, Availability, ComponentKind, DefaultPolicy, EditOp, EnumVariant,
    FloatBounds, FloatDisplay, PropertyAccess, PropertyAddress, PropertyDefinition, PropertyError,
    PropertyId, PropertyTransaction, PropertyValue, ResolvedProperty, ResolvedSchema, ScopeKind,
    Tier, ValueCopies, ValueSchema,
};
use crate::state::PlotxApp;
use plotx_processing::{BinMethod, BinParams, StepKind};

pub const WIDTH: PropertyId = PropertyId("dataset.processing.bin.width");
pub const METHOD: PropertyId = PropertyId("dataset.processing.bin.method");

pub const SUM: &str = "sum";
pub const MEAN: &str = "mean";

const METHODS: &[EnumVariant] = &[EnumVariant::new(SUM, "Sum"), EnumVariant::new(MEAN, "Mean")];
const WIDTH_BOUNDS: FloatBounds = FloatBounds::above(0.0, f64::MAX);
const STEP: Applicability = Applicability::component(ComponentKind::ProcessingStep);

pub(crate) const DEFINITIONS: &[PropertyDefinition] = &[
    PropertyDefinition {
        id: WIDTH,
        scope_kind: ScopeKind::Dataset,
        value_schema: ValueSchema::Float {
            bounds: WIDTH_BOUNDS,
            display: FloatDisplay::Linear("ppm"),
            drag_step: Some(0.005),
        },
        access: PropertyAccess::ReadWrite,
        applicability: STEP,
        default_policy: DefaultPolicy::None,
        tier: Tier::Essential,
        copies: ValueCopies::PerTarget,
        canonical_label: "Bin width",
        canonical_aliases: &["binning width", "bucket width", "ppm bins"],
    },
    PropertyDefinition {
        id: METHOD,
        scope_kind: ScopeKind::Dataset,
        value_schema: ValueSchema::Enum { variants: METHODS },
        access: PropertyAccess::ReadWrite,
        applicability: STEP,
        default_policy: DefaultPolicy::None,
        tier: Tier::Essential,
        copies: ValueCopies::PerTarget,
        canonical_label: "Bin aggregation",
        canonical_aliases: &["bin method", "sum bins", "mean bins"],
    },
];

pub(crate) struct BinProvider;

pub(crate) static PROVIDER: BinProvider = BinProvider;

impl PropertyProvider for BinProvider {
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
            matches!(kind, StepKind::Bin(_))
        })?;
        let StepKind::Bin(current) = context.step.kind else {
            unreachable!("the shared context checked the step kind");
        };
        let bounds = resolved_width_bounds(&context)?;
        Ok(ResolvedProperty {
            address: address.clone(),
            modified: None,
            value: AggregateValue::Uniform(value_of(definition, current)?),
            default_value: None,
            availability: Availability::Editable,
            schema: schema_for(definition, bounds),
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
            matches!(kind, StepKind::Bin(_))
        })?;
        let bounds = resolved_width_bounds(&context)?;
        let value = match operation {
            EditOp::Set(value) => checked_value(definition, bounds, value)?,
            EditOp::Reset => {
                return Err(PropertyError::NotApplicable(
                    "User-added binning steps have no factory setting to reset to.".to_owned(),
                ));
            }
            EditOp::Step(_) => return Err(no_step_gesture(definition)),
        };
        let state = transaction.processing_state(app, context.dataset_id)?;
        let step = step_mut(state, context.step.id, &address.target)?;
        let StepKind::Bin(current) = &mut step.kind else {
            return Err(PropertyError::NotApplicable(
                "the addressed step is no longer a binning step".to_owned(),
            ));
        };
        match (definition.id, value) {
            (WIDTH, PropertyValue::Float(value)) => current.width = value,
            (METHOD, PropertyValue::Enum(SUM)) => current.method = BinMethod::Sum,
            (METHOD, PropertyValue::Enum(MEAN)) => current.method = BinMethod::Mean,
            (_, value) => {
                return Err(wrong_kind(definition, &value, "the declared binning value"));
            }
        }
        Ok(())
    }
}

fn value_of(
    definition: &'static PropertyDefinition,
    current: BinParams,
) -> Result<PropertyValue, PropertyError> {
    match definition.id {
        WIDTH => Ok(PropertyValue::Float(current.width)),
        METHOD => Ok(PropertyValue::Enum(match current.method {
            BinMethod::Sum => SUM,
            BinMethod::Mean => MEAN,
        })),
        _ => Err(PropertyError::UnknownProperty(definition.id.to_string())),
    }
}

fn schema_for(definition: &'static PropertyDefinition, bounds: FloatBounds) -> ResolvedSchema {
    if definition.id == WIDTH {
        ResolvedSchema::Float {
            bounds,
            display: FloatDisplay::Linear("ppm"),
        }
    } else {
        ResolvedSchema::Enum {
            variants: METHODS.iter().collect(),
        }
    }
}

fn checked_value(
    definition: &'static PropertyDefinition,
    bounds: FloatBounds,
    value: &PropertyValue,
) -> Result<PropertyValue, PropertyError> {
    match (definition.id, value) {
        (WIDTH, PropertyValue::Float(value)) => Ok(PropertyValue::Float(bounds.check(
            definition.id,
            definition.canonical_label,
            *value,
        )?)),
        (WIDTH, value) => Err(wrong_kind(definition, value, "a positive number")),
        (METHOD, PropertyValue::Enum(value)) if METHODS.iter().any(|item| item.id == *value) => {
            Ok(PropertyValue::Enum(value))
        }
        (METHOD, PropertyValue::Enum(value)) => Err(PropertyError::InvalidValue {
            property: definition.id,
            message: format!("'{value}' is not a bin aggregation"),
        }),
        (METHOD, value) => Err(wrong_kind(definition, value, "a bin aggregation")),
        (_, _) => Err(PropertyError::UnknownProperty(definition.id.to_string())),
    }
}

fn resolved_width_bounds(
    context: &super::processing_common::StepContext<'_>,
) -> Result<FloatBounds, PropertyError> {
    let spectrum = spectrum_before_step(context).ok_or_else(|| {
        PropertyError::NotApplicable(
            "Binning needs a one-dimensional input spectrum with an axis.".to_owned(),
        )
    })?;
    let axis_step = plotx_processing::cleanup::axis_step(&spectrum.ppm);
    Ok(FloatBounds::above(1.5 * axis_step, f64::MAX))
}
