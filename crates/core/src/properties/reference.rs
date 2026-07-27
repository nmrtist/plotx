//! Dataset-owned chemical-shift reference-step properties.

use super::processing_common::{
    no_step_gesture, property_definition, step_context, step_mut, wrong_kind,
};
use super::provider::PropertyProvider;
use super::{
    AggregateValue, Applicability, Availability, ComponentKind, DefaultPolicy, EditOp, FloatBounds,
    FloatDisplay, PropertyAccess, PropertyAddress, PropertyDefinition, PropertyError, PropertyId,
    PropertyTransaction, PropertyValue, ResolvedProperty, ResolvedSchema, ScopeKind, Tier,
    ValueCopies, ValueSchema,
};
use crate::state::PlotxApp;
use plotx_processing::{ReferenceParams, StepKind};

pub const AT_PPM: PropertyId = PropertyId("dataset.processing.reference.at_ppm");
pub const TARGET_PPM: PropertyId = PropertyId("dataset.processing.reference.target_ppm");

const PPM_BOUNDS: FloatBounds = FloatBounds::inclusive(-f64::MAX, f64::MAX);
const PPM_STEP: f64 = 0.01;
const STEP: Applicability = Applicability::component(ComponentKind::ProcessingStep);

pub(crate) const DEFINITIONS: &[PropertyDefinition] = &[
    PropertyDefinition {
        id: AT_PPM,
        scope_kind: ScopeKind::Dataset,
        value_schema: ValueSchema::Float {
            bounds: PPM_BOUNDS,
            display: FloatDisplay::Linear("ppm"),
            drag_step: Some(PPM_STEP),
        },
        access: PropertyAccess::ReadWrite,
        applicability: STEP,
        default_policy: DefaultPolicy::None,
        tier: Tier::Essential,
        copies: ValueCopies::PerTarget,
        canonical_label: "Reference source position",
        canonical_aliases: &["reference at ppm", "source chemical shift"],
    },
    PropertyDefinition {
        id: TARGET_PPM,
        scope_kind: ScopeKind::Dataset,
        value_schema: ValueSchema::Float {
            bounds: PPM_BOUNDS,
            display: FloatDisplay::Linear("ppm"),
            drag_step: Some(PPM_STEP),
        },
        access: PropertyAccess::ReadWrite,
        applicability: STEP,
        default_policy: DefaultPolicy::None,
        tier: Tier::Essential,
        copies: ValueCopies::PerTarget,
        canonical_label: "Reference target position",
        canonical_aliases: &["reference target ppm", "target chemical shift"],
    },
];

pub(crate) struct ReferenceProvider;

pub(crate) static PROVIDER: ReferenceProvider = ReferenceProvider;

impl PropertyProvider for ReferenceProvider {
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
            matches!(kind, StepKind::Reference(_))
        })?;
        let StepKind::Reference(current) = context.step.kind else {
            unreachable!("the shared context checked the step kind");
        };
        Ok(ResolvedProperty {
            address: address.clone(),
            modified: None,
            value: AggregateValue::Uniform(value_of(definition, current)?),
            default_value: None,
            availability: Availability::Editable,
            schema: ResolvedSchema::Float {
                bounds: PPM_BOUNDS,
                display: FloatDisplay::Linear("ppm"),
            },
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
            matches!(kind, StepKind::Reference(_))
        })?;
        let StepKind::Reference(_) = context.step.kind else {
            unreachable!("the shared context checked the step kind");
        };
        let value = match operation {
            EditOp::Set(value) => checked_value(definition, value)?,
            EditOp::Reset => {
                return Err(PropertyError::NotApplicable(
                    "User-added reference steps have no factory setting to reset to.".to_owned(),
                ));
            }
            EditOp::Step(_) => return Err(no_step_gesture(definition)),
        };
        let state = transaction.processing_state(app, context.dataset_id)?;
        let step = step_mut(state, context.step.id, &address.target)?;
        let StepKind::Reference(current) = &mut step.kind else {
            return Err(PropertyError::NotApplicable(
                "the addressed step is no longer a reference step".to_owned(),
            ));
        };
        match (definition.id, value) {
            (AT_PPM, PropertyValue::Float(value)) => current.at_ppm = value,
            (TARGET_PPM, PropertyValue::Float(value)) => current.target_ppm = value,
            (_, value) => {
                return Err(wrong_kind(
                    definition,
                    &value,
                    "the declared reference value",
                ));
            }
        }
        Ok(())
    }
}

fn value_of(
    definition: &'static PropertyDefinition,
    current: ReferenceParams,
) -> Result<PropertyValue, PropertyError> {
    match definition.id {
        AT_PPM => Ok(PropertyValue::Float(current.at_ppm)),
        TARGET_PPM => Ok(PropertyValue::Float(current.target_ppm)),
        _ => Err(PropertyError::UnknownProperty(definition.id.to_string())),
    }
}

fn checked_value(
    definition: &'static PropertyDefinition,
    value: &PropertyValue,
) -> Result<PropertyValue, PropertyError> {
    let PropertyValue::Float(value) = value else {
        return Err(wrong_kind(definition, value, "a number"));
    };
    PPM_BOUNDS.check(definition.id, definition.canonical_label, *value)?;
    Ok(PropertyValue::Float(*value))
}
