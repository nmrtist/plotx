//! Dataset-owned normalization-step properties.

use super::processing_common::{
    no_step_gesture, property_definition, step_context, step_mut, wrong_kind,
};
use super::provider::PropertyProvider;
use super::{
    AggregateValue, Applicability, Availability, ComponentKind, DefaultPolicy, EditOp, EnumVariant,
    FloatBounds, FloatDisplay, PropertyAccess, PropertyAddress, PropertyDefinition, PropertyError,
    PropertyId, PropertyTransaction, PropertyValue, ResolvedProperty, ResolvedSchema, ScopeKind,
    Tier, ValueCopies, ValueSchema,
};
use crate::state::PlotxApp;
use plotx_processing::{NormalizeMethod, StepKind};

pub const METHOD: PropertyId = PropertyId("dataset.processing.normalize.method");
pub const DIVISOR: PropertyId = PropertyId("dataset.processing.normalize.divisor");

pub const MAX_PEAK: &str = "max_peak";
pub const TOTAL_AREA: &str = "total_area";
pub const CONSTANT: &str = "constant";

const METHODS: &[EnumVariant] = &[
    EnumVariant::new(MAX_PEAK, "Largest peak = 1"),
    EnumVariant::new(TOTAL_AREA, "Total area = 1"),
    EnumVariant::new(CONSTANT, "Divide by constant"),
];
const DIVISOR_BOUNDS: FloatBounds =
    FloatBounds::excluding_magnitude(-f64::MAX, f64::MAX, f64::MIN_POSITIVE);
/// One is the multiplicative identity and is admitted by the non-zero schema,
/// so switching to Constant never changes the spectrum before the user chooses
/// a divisor.
pub const DIVISOR_SEED: f64 = 1.0;
const STEP: Applicability = Applicability::component(ComponentKind::ProcessingStep);

pub(crate) const DEFINITIONS: &[PropertyDefinition] = &[
    PropertyDefinition {
        id: METHOD,
        scope_kind: ScopeKind::Dataset,
        value_schema: ValueSchema::Enum { variants: METHODS },
        access: PropertyAccess::ReadWrite,
        applicability: STEP,
        default_policy: DefaultPolicy::None,
        tier: Tier::Essential,
        copies: ValueCopies::PerTarget,
        canonical_label: "Normalization method",
        canonical_aliases: &["normalize", "scale spectrum", "normalization"],
    },
    PropertyDefinition {
        id: DIVISOR,
        scope_kind: ScopeKind::Dataset,
        value_schema: ValueSchema::Float {
            bounds: DIVISOR_BOUNDS,
            display: FloatDisplay::Linear(""),
            drag_step: Some(0.1),
        },
        access: PropertyAccess::ReadWrite,
        applicability: STEP,
        default_policy: DefaultPolicy::None,
        tier: Tier::Essential,
        copies: ValueCopies::PerTarget,
        canonical_label: "Normalization divisor",
        canonical_aliases: &["constant divisor", "divide by"],
    },
];

pub(crate) struct NormalizeProvider;

pub(crate) static PROVIDER: NormalizeProvider = NormalizeProvider;

impl PropertyProvider for NormalizeProvider {
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
            matches!(kind, StepKind::Normalize(_))
        })?;
        let StepKind::Normalize(current) = context.step.kind else {
            unreachable!("the shared context checked the step kind");
        };
        Ok(ResolvedProperty {
            address: address.clone(),
            modified: None,
            value: AggregateValue::Uniform(value_of(definition, current)?),
            default_value: None,
            availability: Availability::Editable,
            schema: schema_for(definition, current)?,
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
            matches!(kind, StepKind::Normalize(_))
        })?;
        let StepKind::Normalize(current) = context.step.kind else {
            unreachable!("the shared context checked the step kind");
        };
        let value = match operation {
            EditOp::Set(value) => checked_value(definition, current, value)?,
            EditOp::Reset => {
                return Err(PropertyError::NotApplicable(
                    "User-added normalization steps have no factory setting to reset to."
                        .to_owned(),
                ));
            }
            EditOp::Step(_) => return Err(no_step_gesture(definition)),
        };
        let state = transaction.processing_state(app, context.dataset_id)?;
        let step = step_mut(state, context.step.id, &address.target)?;
        let StepKind::Normalize(current) = &mut step.kind else {
            return Err(PropertyError::NotApplicable(
                "the addressed step is no longer a normalization step".to_owned(),
            ));
        };
        write(definition, current, value)
    }
}

fn value_of(
    definition: &'static PropertyDefinition,
    current: NormalizeMethod,
) -> Result<PropertyValue, PropertyError> {
    match (definition.id, current) {
        (METHOD, value) => Ok(PropertyValue::Enum(method_of(value))),
        (DIVISOR, NormalizeMethod::Constant { divisor }) => Ok(PropertyValue::Float(divisor)),
        _ => Err(divisor_unavailable(current)),
    }
}

fn schema_for(
    definition: &'static PropertyDefinition,
    current: NormalizeMethod,
) -> Result<ResolvedSchema, PropertyError> {
    match definition.id {
        METHOD => Ok(ResolvedSchema::Enum {
            variants: METHODS.iter().collect(),
        }),
        DIVISOR if matches!(current, NormalizeMethod::Constant { .. }) => {
            Ok(ResolvedSchema::Float {
                bounds: DIVISOR_BOUNDS,
                display: FloatDisplay::Linear(""),
            })
        }
        DIVISOR => Err(divisor_unavailable(current)),
        _ => Err(PropertyError::UnknownProperty(definition.id.to_string())),
    }
}

fn checked_value(
    definition: &'static PropertyDefinition,
    current: NormalizeMethod,
    value: &PropertyValue,
) -> Result<PropertyValue, PropertyError> {
    match (definition.id, value) {
        (METHOD, PropertyValue::Enum(value)) if variant(value).is_some() => {
            Ok(PropertyValue::Enum(value))
        }
        (METHOD, PropertyValue::Enum(value)) => Err(PropertyError::InvalidValue {
            property: definition.id,
            message: format!("'{value}' is not a normalization method"),
        }),
        (METHOD, value) => Err(wrong_kind(definition, value, "a normalization method")),
        (DIVISOR, PropertyValue::Float(value))
            if matches!(current, NormalizeMethod::Constant { .. }) =>
        {
            Ok(PropertyValue::Float(DIVISOR_BOUNDS.check(
                definition.id,
                definition.canonical_label,
                *value,
            )?))
        }
        (DIVISOR, PropertyValue::Float(_)) => Err(divisor_unavailable(current)),
        (DIVISOR, value) => Err(wrong_kind(definition, value, "a non-zero number")),
        (_, _) => Err(PropertyError::UnknownProperty(definition.id.to_string())),
    }
}

fn write(
    definition: &'static PropertyDefinition,
    current: &mut NormalizeMethod,
    value: PropertyValue,
) -> Result<(), PropertyError> {
    match (definition.id, value) {
        (METHOD, PropertyValue::Enum(value)) => {
            *current = match variant(value) {
                Some(NormalizeVariant::MaxPeak) => NormalizeMethod::MaxPeak,
                Some(NormalizeVariant::TotalArea) => NormalizeMethod::TotalArea,
                Some(NormalizeVariant::Constant) => NormalizeMethod::Constant {
                    divisor: match *current {
                        NormalizeMethod::Constant { divisor } => divisor,
                        _ => DIVISOR_SEED,
                    },
                },
                None => {
                    return Err(PropertyError::InvalidValue {
                        property: definition.id,
                        message: format!("'{value}' is not a normalization method"),
                    });
                }
            };
            Ok(())
        }
        (DIVISOR, PropertyValue::Float(value)) => {
            let NormalizeMethod::Constant { divisor } = current else {
                return Err(divisor_unavailable(*current));
            };
            *divisor = value;
            Ok(())
        }
        (_, value) => Err(wrong_kind(
            definition,
            &value,
            "the declared normalization value",
        )),
    }
}

#[derive(Clone, Copy)]
enum NormalizeVariant {
    MaxPeak,
    TotalArea,
    Constant,
}

fn variant(value: &str) -> Option<NormalizeVariant> {
    match value {
        MAX_PEAK => Some(NormalizeVariant::MaxPeak),
        TOTAL_AREA => Some(NormalizeVariant::TotalArea),
        CONSTANT => Some(NormalizeVariant::Constant),
        _ => None,
    }
}

fn method_of(method: NormalizeMethod) -> &'static str {
    match method {
        NormalizeMethod::MaxPeak => MAX_PEAK,
        NormalizeMethod::TotalArea => TOTAL_AREA,
        NormalizeMethod::Constant { .. } => CONSTANT,
    }
}

fn divisor_unavailable(current: NormalizeMethod) -> PropertyError {
    PropertyError::NotApplicable(format!(
        "Normalization divisor is available only with Divide by constant; this step uses {}",
        METHODS
            .iter()
            .find(|variant| variant.id == method_of(current))
            .map(|variant| variant.canonical_label)
            .unwrap_or("an unknown method")
    ))
}
