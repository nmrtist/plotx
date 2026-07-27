//! The enabled flag shared by every processing-step component.

use super::processing_common::{
    no_factory_default, no_step_gesture, property_definition, step_context, step_mut, wrong_kind,
};
use super::provider::PropertyProvider;
use super::{
    AggregateValue, Applicability, Availability, ComponentKind, DefaultPolicy, EditOp,
    PropertyAccess, PropertyAddress, PropertyDefinition, PropertyError, PropertyId,
    PropertyTransaction, PropertyValue, ResolvedProperty, ResolvedSchema, ScopeKind, Tier,
    ValueCopies, ValueSchema,
};
use crate::state::PlotxApp;

pub const ENABLED: PropertyId = PropertyId("dataset.processing.step.enabled");

const STEP: Applicability = Applicability::component(ComponentKind::ProcessingStep);

pub(crate) const DEFINITIONS: &[PropertyDefinition] = &[PropertyDefinition {
    id: ENABLED,
    scope_kind: ScopeKind::Dataset,
    value_schema: ValueSchema::Bool,
    access: PropertyAccess::ReadWrite,
    applicability: STEP,
    default_policy: DefaultPolicy::ProcessingFactory,
    tier: Tier::Essential,
    copies: ValueCopies::PerTarget,
    canonical_label: "Processing step enabled",
    canonical_aliases: &["enable step", "disable step", "processing toggle"],
}];

pub(crate) struct StepEnabledProvider;

pub(crate) static PROVIDER: StepEnabledProvider = StepEnabledProvider;

impl PropertyProvider for StepEnabledProvider {
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
            !matches!(kind, plotx_processing::StepKind::Fft)
        })?;
        Ok(ResolvedProperty {
            address: address.clone(),
            modified: None,
            value: AggregateValue::Uniform(PropertyValue::Bool(context.step.enabled)),
            default_value: context
                .factory
                .as_ref()
                .map(|factory| PropertyValue::Bool(factory.enabled)),
            availability: Availability::Editable,
            schema: ResolvedSchema::Bool,
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
            !matches!(kind, plotx_processing::StepKind::Fft)
        })?;
        let value = match operation {
            EditOp::Set(PropertyValue::Bool(value)) => *value,
            EditOp::Set(value) => return Err(wrong_kind(definition, value, "true or false")),
            EditOp::Reset => context
                .factory
                .as_ref()
                .map(|factory| factory.enabled)
                .ok_or_else(|| no_factory_default(definition))?,
            EditOp::Step(_) => return Err(no_step_gesture(definition)),
        };
        let state = transaction.processing_state(app, context.dataset_id)?;
        step_mut(state, context.step.id, &address.target)?.enabled = value;
        Ok(())
    }
}
