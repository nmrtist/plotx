//! Persistent ILT lambda defaults and read-only result provenance.

use super::provider::PropertyProvider;
use super::target::require_app_target;
use super::{
    AggregateValue, Applicability, Availability, ComponentKind, DefaultPolicy, EditOp, FloatBounds,
    PropertyAccess, PropertyAddress, PropertyDefinition, PropertyError, PropertyId,
    PropertyTransaction, PropertyValue, ResolvedProperty, ResolvedSchema, ScopeKind, Tier,
    ValueCopies, ValueSchema, definition,
};
use crate::DosyInvocation;
use crate::settings::{DEFAULT_ILT_LAMBDA, MAX_ILT_LAMBDA, MIN_ILT_LAMBDA};
use crate::state::{DatasetId, PlotxApp};

pub const DEFAULT_LAMBDA: PropertyId = PropertyId("settings.analysis.ilt.lambda");
pub const RESULT_LAMBDA: PropertyId = PropertyId("dataset.analysis.ilt.result_lambda");

const LAMBDA_BOUNDS: FloatBounds = FloatBounds::inclusive(MIN_ILT_LAMBDA, MAX_ILT_LAMBDA);

pub(crate) const DEFINITIONS: &[PropertyDefinition] = &[
    PropertyDefinition {
        id: DEFAULT_LAMBDA,
        scope_kind: ScopeKind::App,
        value_schema: ValueSchema::Float {
            bounds: LAMBDA_BOUNDS,
            log: true,
            drag_step: Some(0.001),
        },
        access: PropertyAccess::ReadWrite,
        applicability: Applicability::component(ComponentKind::None),
        default_policy: DefaultPolicy::Fixed(PropertyValue::Float(DEFAULT_ILT_LAMBDA)),
        tier: Tier::Essential,
        copies: ValueCopies::PerTarget,
        canonical_label: "Default ILT regularization",
        canonical_aliases: &["ILT lambda", "regularization lambda", "DOSY lambda"],
    },
    PropertyDefinition {
        id: RESULT_LAMBDA,
        scope_kind: ScopeKind::Dataset,
        value_schema: ValueSchema::Float {
            bounds: LAMBDA_BOUNDS,
            log: true,
            drag_step: None,
        },
        access: PropertyAccess::ReadOnly,
        applicability: Applicability::component(ComponentKind::None),
        default_policy: DefaultPolicy::None,
        tier: Tier::Expert,
        copies: ValueCopies::PerTarget,
        canonical_label: "Stored ILT result lambda",
        canonical_aliases: &["ILT provenance", "result lambda", "DOSY provenance"],
    },
];

pub(crate) struct IltProvider;

pub(crate) static PROVIDER: IltProvider = IltProvider;

impl PropertyProvider for IltProvider {
    fn definitions(&self) -> &'static [PropertyDefinition] {
        DEFINITIONS
    }

    fn read(
        &self,
        app: &PlotxApp,
        address: &PropertyAddress,
    ) -> Result<ResolvedProperty, PropertyError> {
        let definition = property_definition(address.definition)?;
        let (value, default_value, availability) = match definition.id {
            DEFAULT_LAMBDA => {
                require_app_target(&address.target, definition)?;
                (
                    app.settings.processing.ilt_lambda,
                    match definition.default_policy {
                        DefaultPolicy::Fixed(value) => Some(value),
                        DefaultPolicy::EncodingFactory
                        | DefaultPolicy::ProcessingFactory
                        | DefaultPolicy::None => None,
                    },
                    Availability::Editable,
                )
            }
            RESULT_LAMBDA => (
                result_lambda(app, address, definition)?,
                None,
                Availability::ReadOnly,
            ),
            _ => return Err(PropertyError::UnknownProperty(definition.id.to_string())),
        };
        Ok(ResolvedProperty {
            address: address.clone(),
            value: AggregateValue::Uniform(PropertyValue::Float(value)),
            default_value,
            availability,
            schema: ResolvedSchema::Float {
                bounds: LAMBDA_BOUNDS,
                log: true,
                unit: "",
            },
        })
    }

    fn edit(
        &self,
        app: &PlotxApp,
        transaction: &mut PropertyTransaction,
        address: &PropertyAddress,
        operation: EditOp,
    ) -> Result<(), PropertyError> {
        let definition = property_definition(address.definition)?;
        // `PropertyService::plan_edit` already refuses read-only definitions before
        // any provider is consulted, so this arm is unreachable today. It still
        // reports `ReadOnly` rather than `NotApplicable`, because the service
        // classifies `NotApplicable` as a skipped target: were the gate ever to
        // move, the wrong variant would turn a refusal into a reported success
        // that wrote nothing.
        if definition.access == PropertyAccess::ReadOnly {
            return Err(PropertyError::ReadOnly(definition.id));
        }
        require_app_target(&address.target, definition)?;
        let value = match operation {
            EditOp::Set(PropertyValue::Float(value)) => checked_lambda(definition.id, value)?,
            EditOp::Set(value) => {
                return Err(PropertyError::InvalidValue {
                    property: definition.id,
                    message: format!("expected a float, got {}", value.kind()),
                });
            }
            EditOp::Reset => match definition.default_policy {
                DefaultPolicy::Fixed(PropertyValue::Float(value)) => {
                    checked_lambda(definition.id, value)?
                }
                DefaultPolicy::Fixed(_)
                | DefaultPolicy::EncodingFactory
                | DefaultPolicy::ProcessingFactory
                | DefaultPolicy::None => {
                    return Err(PropertyError::InvalidValue {
                        property: definition.id,
                        message: "the default policy has no float value".to_owned(),
                    });
                }
            },
            EditOp::Step(_) => {
                return Err(PropertyError::InvalidValue {
                    property: definition.id,
                    message: "this setting has no step gesture".to_owned(),
                });
            }
        };
        transaction.app_preferences(app).processing.ilt_lambda = value;
        Ok(())
    }
}

fn result_lambda(
    app: &PlotxApp,
    address: &PropertyAddress,
    definition: &'static PropertyDefinition,
) -> Result<f64, PropertyError> {
    let actual = ComponentKind::of(address.target.component.as_ref());
    if actual != ComponentKind::None {
        return Err(PropertyError::ComponentKind {
            property: definition.id,
            expected: ComponentKind::None.as_str(),
            actual: actual.as_str(),
        });
    }
    let dataset_id = DatasetId::try_from(&address.target.resource).map_err(|_| {
        PropertyError::NotApplicable(format!(
            "{} belongs to a dataset resource, not {}",
            definition.canonical_label, address.target.resource.id
        ))
    })?;
    let dataset = app
        .doc
        .dataset_by_id(dataset_id)
        .ok_or_else(|| PropertyError::UnknownTarget(address.target.resource.id.clone()))?;
    let nmr2d = dataset.as_nmr2d().ok_or_else(|| {
        PropertyError::NotApplicable(
            "Stored ILT result lambda applies to a pseudo-2D DOSY dataset.".to_owned(),
        )
    })?;
    let provenance = nmr2d.ilt_provenance.as_ref().ok_or_else(|| {
        PropertyError::NotApplicable(
            "Build an ILT DOSY map before inspecting its stored lambda.".to_owned(),
        )
    })?;
    match provenance.input {
        DosyInvocation::Ilt { params } => Ok(params.lambda),
        DosyInvocation::MonoExp { .. } => Err(PropertyError::NotApplicable(
            "The stored ILT provenance does not contain an ILT invocation.".to_owned(),
        )),
    }
}

fn property_definition(id: PropertyId) -> Result<&'static PropertyDefinition, PropertyError> {
    definition(id).ok_or_else(|| PropertyError::UnknownProperty(id.as_str().to_owned()))
}

fn checked_lambda(property: PropertyId, value: f64) -> Result<f64, PropertyError> {
    if LAMBDA_BOUNDS.admits(value) {
        return Ok(value);
    }
    Err(PropertyError::InvalidValue {
        property,
        message: format!("ILT lambda {value} is outside {MIN_ILT_LAMBDA}–{MAX_ILT_LAMBDA}"),
    })
}
