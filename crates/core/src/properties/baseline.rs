//! Dataset-owned baseline-step properties.

use super::processing_common::{
    no_factory_default, no_step_gesture, property_definition, step_context, step_mut, wrong_kind,
};
use super::provider::PropertyProvider;
use super::{
    AggregateValue, Applicability, Availability, ComponentKind, DefaultPolicy, EditOp, EnumVariant,
    FloatBounds, FloatDisplay, PropertyAccess, PropertyAddress, PropertyDefinition, PropertyError,
    PropertyId, PropertyTransaction, PropertyValue, ResolvedProperty, ResolvedSchema, ScopeKind,
    Tier, ValueCopies, ValueSchema,
};
use crate::state::PlotxApp;
use plotx_processing::{BaselineMethod, StepKind};

pub const METHOD: PropertyId = PropertyId("dataset.processing.baseline.method");
pub const POLYNOMIAL_ORDER: PropertyId = PropertyId("dataset.processing.baseline.polynomial_order");
pub const SMOOTHNESS: PropertyId = PropertyId("dataset.processing.baseline.smoothness");
pub const ASYMMETRY: PropertyId = PropertyId("dataset.processing.baseline.asymmetry");
pub const ITERATIONS: PropertyId = PropertyId("dataset.processing.baseline.iterations");

pub const OFFSET: &str = "offset";
pub const POLYNOMIAL: &str = "polynomial";
pub const ASYMMETRIC_LEAST_SQUARES: &str = "asymmetric_least_squares";

const METHODS: &[EnumVariant] = &[
    EnumVariant::new(OFFSET, "Offset"),
    EnumVariant::new(POLYNOMIAL, "Polynomial"),
    EnumVariant::new(ASYMMETRIC_LEAST_SQUARES, "Automatic (AsLS)"),
];
const ORDER_MIN: i64 = 1;
const ORDER_MAX: i64 = 8;
const SMOOTHNESS_BOUNDS: FloatBounds = FloatBounds::inclusive(1.0, 1.0e12);
// The kernel clamps to this effective scientific range. Admitting a wider
// stored value would make the catalog report a number the algorithm did not use.
const ASYMMETRY_BOUNDS: FloatBounds = FloatBounds::inclusive(1.0e-6, 0.5);
const ITERATIONS_MIN: i64 = 1;
const ITERATIONS_MAX: i64 = 100;
/// A polynomial step switched in without a carried order starts at the old
/// editor's order, which is inside [`ORDER_MIN`]–[`ORDER_MAX`].
pub const POLYNOMIAL_ORDER_SEED: u8 = 2;
/// These are the processing kernel's own AsLS defaults. Each is inside the
/// schema declared for its parameter, so switching method cannot create a step
/// its controls immediately reject.
pub const SMOOTHNESS_SEED: f64 = 5.0e4;
pub const ASYMMETRY_SEED: f64 = 0.001;
pub const ITERATIONS_SEED: u16 = 20;
const STEP: Applicability = Applicability::component(ComponentKind::ProcessingStep);

pub(crate) const DEFINITIONS: &[PropertyDefinition] = &[
    PropertyDefinition {
        id: METHOD,
        scope_kind: ScopeKind::Dataset,
        value_schema: ValueSchema::Enum { variants: METHODS },
        access: PropertyAccess::ReadWrite,
        applicability: STEP,
        default_policy: DefaultPolicy::ProcessingFactory,
        tier: Tier::Essential,
        copies: ValueCopies::PerTarget,
        canonical_label: "Baseline method",
        canonical_aliases: &["baseline correction", "AsLS", "polynomial baseline"],
    },
    PropertyDefinition {
        id: POLYNOMIAL_ORDER,
        scope_kind: ScopeKind::Dataset,
        value_schema: ValueSchema::IntWithDrag {
            min: ORDER_MIN,
            max: ORDER_MAX,
            drag_step: 0.1,
        },
        access: PropertyAccess::ReadWrite,
        applicability: STEP,
        default_policy: DefaultPolicy::None,
        // Migration preserves the old editor's directly visible order row.
        tier: Tier::Essential,
        copies: ValueCopies::PerTarget,
        canonical_label: "Baseline polynomial order",
        canonical_aliases: &["baseline order", "polynomial degree"],
    },
    PropertyDefinition {
        id: SMOOTHNESS,
        scope_kind: ScopeKind::Dataset,
        value_schema: ValueSchema::Float {
            bounds: SMOOTHNESS_BOUNDS,
            display: FloatDisplay::Log10("λ"),
            drag_step: None,
        },
        access: PropertyAccess::ReadWrite,
        applicability: STEP,
        default_policy: DefaultPolicy::ProcessingFactory,
        tier: Tier::Essential,
        copies: ValueCopies::PerTarget,
        canonical_label: "Baseline smoothness",
        canonical_aliases: &["AsLS lambda", "baseline lambda", "smoothness"],
    },
    PropertyDefinition {
        id: ASYMMETRY,
        scope_kind: ScopeKind::Dataset,
        value_schema: ValueSchema::Float {
            bounds: ASYMMETRY_BOUNDS,
            display: FloatDisplay::Linear(""),
            drag_step: Some(0.0005),
        },
        access: PropertyAccess::ReadWrite,
        applicability: STEP,
        default_policy: DefaultPolicy::ProcessingFactory,
        tier: Tier::Essential,
        copies: ValueCopies::PerTarget,
        canonical_label: "Baseline peak weight",
        canonical_aliases: &["AsLS asymmetry", "peak weight", "asymmetry"],
    },
    PropertyDefinition {
        id: ITERATIONS,
        scope_kind: ScopeKind::Dataset,
        value_schema: ValueSchema::IntWithDrag {
            min: ITERATIONS_MIN,
            max: ITERATIONS_MAX,
            drag_step: 0.2,
        },
        access: PropertyAccess::ReadWrite,
        applicability: STEP,
        default_policy: DefaultPolicy::ProcessingFactory,
        tier: Tier::Essential,
        copies: ValueCopies::PerTarget,
        canonical_label: "Baseline iterations",
        canonical_aliases: &["AsLS iterations", "baseline passes"],
    },
];

pub(crate) struct BaselineProvider;

pub(crate) static PROVIDER: BaselineProvider = BaselineProvider;

impl PropertyProvider for BaselineProvider {
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
            matches!(kind, StepKind::Baseline(_))
        })?;
        let StepKind::Baseline(current) = context.step.kind else {
            unreachable!("the shared context checked the step kind");
        };
        let factory = context.factory.as_ref().and_then(|step| match step.kind {
            StepKind::Baseline(value) => Some(value),
            _ => None,
        });
        Ok(ResolvedProperty {
            address: address.clone(),
            modified: None,
            value: AggregateValue::Uniform(value_of(definition, current)?),
            default_value: default_value(definition, factory)?,
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
            matches!(kind, StepKind::Baseline(_))
        })?;
        let StepKind::Baseline(current) = context.step.kind else {
            unreachable!("the shared context checked the step kind");
        };
        let factory = context.factory.as_ref().and_then(|step| match step.kind {
            StepKind::Baseline(value) => Some(value),
            _ => None,
        });
        let value = match operation {
            EditOp::Set(value) => checked_value(definition, current, value)?,
            EditOp::Reset => {
                default_value(definition, factory)?.ok_or_else(|| no_factory_default(definition))?
            }
            EditOp::Step(_) => return Err(no_step_gesture(definition)),
        };
        let state = transaction.processing_state(app, context.dataset_id)?;
        let step = step_mut(state, context.step.id, &address.target)?;
        let StepKind::Baseline(current) = &mut step.kind else {
            return Err(PropertyError::NotApplicable(
                "the addressed step is no longer a baseline step".to_owned(),
            ));
        };
        write(definition, current, value)
    }
}

fn value_of(
    definition: &'static PropertyDefinition,
    current: BaselineMethod,
) -> Result<PropertyValue, PropertyError> {
    if !property_applies_to_method(definition.id, method_of(current)) {
        return Err(unavailable_parameter(definition, current));
    }
    match (definition.id, current) {
        (METHOD, value) => Ok(PropertyValue::Enum(method_of(value))),
        (POLYNOMIAL_ORDER, BaselineMethod::Polynomial { order }) => {
            Ok(PropertyValue::Int(i64::from(order)))
        }
        (SMOOTHNESS, BaselineMethod::AsymmetricLeastSquares { smoothness, .. }) => {
            Ok(PropertyValue::Float(smoothness))
        }
        (ASYMMETRY, BaselineMethod::AsymmetricLeastSquares { asymmetry, .. }) => {
            Ok(PropertyValue::Float(asymmetry))
        }
        (ITERATIONS, BaselineMethod::AsymmetricLeastSquares { iterations, .. }) => {
            Ok(PropertyValue::Int(i64::from(iterations)))
        }
        _ => Err(unavailable_parameter(definition, current)),
    }
}

/// Whether a Baseline-section property is rendered for the selected method.
///
/// Panel-density calibration enumerates the method discriminator through this
/// same predicate, so its mutually exclusive parameter rows cannot be counted
/// as though they appeared together.
#[doc(hidden)]
pub fn property_applies_to_method(property: PropertyId, method: &str) -> bool {
    match property {
        METHOD => true,
        POLYNOMIAL_ORDER => method == POLYNOMIAL,
        SMOOTHNESS | ASYMMETRY | ITERATIONS => method == ASYMMETRIC_LEAST_SQUARES,
        _ => false,
    }
}

fn default_value(
    definition: &'static PropertyDefinition,
    factory: Option<BaselineMethod>,
) -> Result<Option<PropertyValue>, PropertyError> {
    let Some(factory) = factory else {
        return Ok(None);
    };
    let value = match definition.id {
        METHOD => PropertyValue::Enum(method_of(factory)),
        POLYNOMIAL_ORDER => match factory {
            BaselineMethod::Polynomial { order } => PropertyValue::Int(i64::from(order)),
            _ => return Ok(None),
        },
        SMOOTHNESS => match factory {
            BaselineMethod::AsymmetricLeastSquares { smoothness, .. } => {
                PropertyValue::Float(smoothness)
            }
            _ => return Ok(None),
        },
        ASYMMETRY => match factory {
            BaselineMethod::AsymmetricLeastSquares { asymmetry, .. } => {
                PropertyValue::Float(asymmetry)
            }
            _ => return Ok(None),
        },
        ITERATIONS => match factory {
            BaselineMethod::AsymmetricLeastSquares { iterations, .. } => {
                PropertyValue::Int(i64::from(iterations))
            }
            _ => return Ok(None),
        },
        _ => return Err(PropertyError::UnknownProperty(definition.id.to_string())),
    };
    Ok(Some(value))
}

fn schema_for(
    definition: &'static PropertyDefinition,
    current: BaselineMethod,
) -> Result<ResolvedSchema, PropertyError> {
    value_of(definition, current)?;
    match definition.id {
        METHOD => Ok(ResolvedSchema::Enum {
            variants: METHODS.iter().collect(),
        }),
        POLYNOMIAL_ORDER => Ok(ResolvedSchema::IntWithDrag {
            min: ORDER_MIN,
            max: ORDER_MAX,
            drag_step: 0.1,
            unit: "",
        }),
        SMOOTHNESS => Ok(ResolvedSchema::Float {
            bounds: SMOOTHNESS_BOUNDS,
            display: FloatDisplay::Log10("λ"),
        }),
        ASYMMETRY => Ok(ResolvedSchema::Float {
            bounds: ASYMMETRY_BOUNDS,
            display: FloatDisplay::Linear(""),
        }),
        ITERATIONS => Ok(ResolvedSchema::IntWithDrag {
            min: ITERATIONS_MIN,
            max: ITERATIONS_MAX,
            drag_step: 0.2,
            unit: "",
        }),
        _ => Err(PropertyError::UnknownProperty(definition.id.to_string())),
    }
}

fn checked_value(
    definition: &'static PropertyDefinition,
    current: BaselineMethod,
    value: &PropertyValue,
) -> Result<PropertyValue, PropertyError> {
    if definition.id != METHOD {
        value_of(definition, current)?;
    }
    match (definition.id, value) {
        (METHOD, PropertyValue::Enum(value)) if variant(value).is_some() => {
            Ok(PropertyValue::Enum(value))
        }
        (METHOD, PropertyValue::Enum(value)) => Err(PropertyError::InvalidValue {
            property: definition.id,
            message: format!("'{value}' is not a baseline method"),
        }),
        (METHOD, value) => Err(wrong_kind(definition, value, "a baseline method")),
        (POLYNOMIAL_ORDER, PropertyValue::Int(value)) | (ITERATIONS, PropertyValue::Int(value)) => {
            let (min, max) = if definition.id == POLYNOMIAL_ORDER {
                (ORDER_MIN, ORDER_MAX)
            } else {
                (ITERATIONS_MIN, ITERATIONS_MAX)
            };
            if !(min..=max).contains(value) {
                return Err(PropertyError::InvalidValue {
                    property: definition.id,
                    message: format!(
                        "{} {value} is out of range: it must be between {min} and {max}",
                        definition.canonical_label
                    ),
                });
            }
            Ok(PropertyValue::Int(*value))
        }
        (SMOOTHNESS, PropertyValue::Float(value)) => Ok(PropertyValue::Float(
            SMOOTHNESS_BOUNDS.check(definition.id, definition.canonical_label, *value)?,
        )),
        (ASYMMETRY, PropertyValue::Float(value)) => Ok(PropertyValue::Float(
            ASYMMETRY_BOUNDS.check(definition.id, definition.canonical_label, *value)?,
        )),
        (POLYNOMIAL_ORDER | ITERATIONS, value) => Err(wrong_kind(definition, value, "an integer")),
        (SMOOTHNESS | ASYMMETRY, value) => Err(wrong_kind(definition, value, "a number")),
        (_, _) => Err(PropertyError::UnknownProperty(definition.id.to_string())),
    }
}

fn write(
    definition: &'static PropertyDefinition,
    current: &mut BaselineMethod,
    value: PropertyValue,
) -> Result<(), PropertyError> {
    match (definition.id, value) {
        (METHOD, PropertyValue::Enum(value)) => {
            *current = match variant(value) {
                Some(BaselineVariant::Offset) => BaselineMethod::Offset,
                Some(BaselineVariant::Polynomial) => BaselineMethod::Polynomial {
                    order: carried_order(*current),
                },
                Some(BaselineVariant::Asls) => carried_asls(*current),
                None => {
                    return Err(PropertyError::InvalidValue {
                        property: definition.id,
                        message: format!("'{value}' is not a baseline method"),
                    });
                }
            };
            Ok(())
        }
        (POLYNOMIAL_ORDER, PropertyValue::Int(value)) => {
            let BaselineMethod::Polynomial { order } = current else {
                return Err(unavailable_parameter(definition, *current));
            };
            *order = u8::try_from(value).map_err(|_| {
                wrong_kind(
                    definition,
                    &PropertyValue::Int(value),
                    "an order from 1 to 8",
                )
            })?;
            Ok(())
        }
        (SMOOTHNESS, PropertyValue::Float(value)) => {
            let BaselineMethod::AsymmetricLeastSquares { smoothness, .. } = current else {
                return Err(unavailable_parameter(definition, *current));
            };
            *smoothness = value;
            Ok(())
        }
        (ASYMMETRY, PropertyValue::Float(value)) => {
            let BaselineMethod::AsymmetricLeastSquares { asymmetry, .. } = current else {
                return Err(unavailable_parameter(definition, *current));
            };
            *asymmetry = value;
            Ok(())
        }
        (ITERATIONS, PropertyValue::Int(value)) => {
            let BaselineMethod::AsymmetricLeastSquares { iterations, .. } = current else {
                return Err(unavailable_parameter(definition, *current));
            };
            *iterations = u16::try_from(value).map_err(|_| {
                wrong_kind(
                    definition,
                    &PropertyValue::Int(value),
                    "an iteration count from 1 to 100",
                )
            })?;
            Ok(())
        }
        (_, value) => Err(wrong_kind(
            definition,
            &value,
            "the declared baseline value",
        )),
    }
}

fn carried_order(current: BaselineMethod) -> u8 {
    match current {
        BaselineMethod::Polynomial { order } => order,
        _ => POLYNOMIAL_ORDER_SEED,
    }
}

fn carried_asls(current: BaselineMethod) -> BaselineMethod {
    match current {
        value @ BaselineMethod::AsymmetricLeastSquares { .. } => value,
        _ => BaselineMethod::AsymmetricLeastSquares {
            smoothness: SMOOTHNESS_SEED,
            asymmetry: ASYMMETRY_SEED,
            iterations: ITERATIONS_SEED,
        },
    }
}

#[derive(Clone, Copy)]
enum BaselineVariant {
    Offset,
    Polynomial,
    Asls,
}

fn variant(value: &str) -> Option<BaselineVariant> {
    match value {
        OFFSET => Some(BaselineVariant::Offset),
        POLYNOMIAL => Some(BaselineVariant::Polynomial),
        ASYMMETRIC_LEAST_SQUARES => Some(BaselineVariant::Asls),
        _ => None,
    }
}

fn method_of(method: BaselineMethod) -> &'static str {
    match method {
        BaselineMethod::Offset => OFFSET,
        BaselineMethod::Polynomial { .. } => POLYNOMIAL,
        BaselineMethod::AsymmetricLeastSquares { .. } => ASYMMETRIC_LEAST_SQUARES,
    }
}

fn unavailable_parameter(
    definition: &'static PropertyDefinition,
    current: BaselineMethod,
) -> PropertyError {
    let required = if definition.id == POLYNOMIAL_ORDER {
        "Polynomial"
    } else {
        "Automatic (AsLS)"
    };
    PropertyError::NotApplicable(format!(
        "{} is available only with {required}; this step uses {}",
        definition.canonical_label,
        METHODS
            .iter()
            .find(|variant| variant.id == method_of(current))
            .map(|variant| variant.canonical_label)
            .unwrap_or("an unknown method")
    ))
}
