//! Dataset-owned digital-filter group-delay correction.

use super::processing_common::{no_step_gesture, property_definition, wrong_kind};
use super::provider::PropertyProvider;
use super::{
    AggregateValue, Applicability, Availability, ComponentKind, DefaultPolicy, EditOp,
    PropertyAccess, PropertyAddress, PropertyDefinition, PropertyError, PropertyId,
    PropertyTransaction, PropertyValue, ResolvedProperty, ResolvedSchema, ScopeKind, Tier,
    ValueCopies, ValueSchema,
};
use crate::state::{Dataset, DatasetId, Nmr2DDataset, NmrDataset, PlotxApp};

pub const CORRECT: PropertyId = PropertyId("dataset.processing.group_delay");

pub(crate) const DEFINITIONS: &[PropertyDefinition] = &[PropertyDefinition {
    id: CORRECT,
    scope_kind: ScopeKind::Dataset,
    value_schema: ValueSchema::Bool,
    access: PropertyAccess::ReadWrite,
    applicability: Applicability::component(ComponentKind::None),
    default_policy: DefaultPolicy::ProcessingFactory,
    tier: Tier::Advanced,
    copies: ValueCopies::PerTarget,
    canonical_label: "Group-delay correction",
    canonical_aliases: &["digital filter", "group delay", "GRPDLY"],
}];

pub(crate) struct GroupDelayProvider;

pub(crate) static PROVIDER: GroupDelayProvider = GroupDelayProvider;

impl PropertyProvider for GroupDelayProvider {
    fn definitions(&self) -> &'static [PropertyDefinition] {
        DEFINITIONS
    }

    fn read(
        &self,
        app: &PlotxApp,
        address: &PropertyAddress,
    ) -> Result<ResolvedProperty, PropertyError> {
        let definition = property_definition(address.definition)?;
        let (_, dataset) = dataset_context(app, address, definition)?;
        Ok(ResolvedProperty {
            address: address.clone(),
            modified: None,
            value: AggregateValue::Uniform(PropertyValue::Bool(dataset.current_value())),
            default_value: Some(PropertyValue::Bool(dataset.factory_value())),
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
        let (dataset_id, dataset) = dataset_context(app, address, definition)?;
        let value = match operation {
            EditOp::Set(PropertyValue::Bool(value)) => *value,
            EditOp::Set(value) => return Err(wrong_kind(definition, value, "true or false")),
            EditOp::Reset => dataset.factory_value(),
            EditOp::Step(_) => return Err(no_step_gesture(definition)),
        };
        let state = transaction.processing_state(app, dataset_id)?;
        let group_delay_correct = state.group_delay_correct_mut().ok_or_else(|| {
            PropertyError::NotApplicable(
                "Group-delay correction applies only to NMR datasets.".to_owned(),
            )
        })?;
        *group_delay_correct = value;
        Ok(())
    }
}

fn dataset_context<'a>(
    app: &'a PlotxApp,
    address: &PropertyAddress,
    definition: &'static PropertyDefinition,
) -> Result<(DatasetId, NmrDatasetContext<'a>), PropertyError> {
    let actual = ComponentKind::of(address.target.component.as_ref());
    if actual != ComponentKind::None {
        return Err(PropertyError::ComponentKind {
            property: definition.id,
            expected: ComponentKind::None.as_str(),
            actual: actual.as_str(),
        });
    }
    let dataset_id = DatasetId::try_from(&address.target.resource).map_err(|error| {
        PropertyError::NotApplicable(format!(
            "{} needs a dataset resource: {error}",
            definition.id
        ))
    })?;
    let dataset = app
        .doc
        .dataset_by_id(dataset_id)
        .ok_or_else(|| PropertyError::UnknownTarget(address.target.resource.id.clone()))?;
    match dataset {
        Dataset::Nmr(dataset) => Ok((dataset_id, NmrDatasetContext::One(dataset))),
        Dataset::Nmr2D(dataset) => Ok((dataset_id, NmrDatasetContext::Two(dataset))),
        Dataset::Table(_) | Dataset::Electrophysiology(_) | Dataset::Afm(_) => {
            Err(PropertyError::NotApplicable(
                "Group-delay correction applies only to NMR datasets.".to_owned(),
            ))
        }
    }
}

#[derive(Clone, Copy)]
enum NmrDatasetContext<'a> {
    One(&'a NmrDataset),
    Two(&'a Nmr2DDataset),
}

impl NmrDatasetContext<'_> {
    fn current_value(self) -> bool {
        match self {
            Self::One(dataset) => dataset.group_delay_correct,
            Self::Two(dataset) => dataset.group_delay_correct,
        }
    }

    fn factory_value(self) -> bool {
        match self {
            Self::One(dataset) => crate::state::default_group_delay_correct(dataset.data.domain),
            Self::Two(dataset) => crate::state::default_group_delay_correct(dataset.data.domain),
        }
    }
}
