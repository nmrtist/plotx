//! Dataset-owned smoothing-step properties.

use super::processing_common::{
    no_step_gesture, property_definition, spectrum_before_step, step_context, step_mut, wrong_kind,
};
use super::provider::PropertyProvider;
use super::{
    AggregateValue, Applicability, Availability, ComponentKind, DefaultPolicy, EditOp, EnumVariant,
    PropertyAccess, PropertyAddress, PropertyDefinition, PropertyError, PropertyId,
    PropertyTransaction, PropertyValue, ResolvedProperty, ResolvedSchema, ScopeKind, Tier,
    ValueCopies, ValueSchema,
};
use crate::state::PlotxApp;
use plotx_processing::{SmoothMethod, StepKind};

pub const METHOD: PropertyId = PropertyId("dataset.processing.smooth.method");
pub const WINDOW: PropertyId = PropertyId("dataset.processing.smooth.window");
pub const POLYNOMIAL_ORDER: PropertyId = PropertyId("dataset.processing.smooth.polynomial_order");

pub const MOVING_AVERAGE: &str = "moving_average";
pub const SAVITZKY_GOLAY: &str = "savitzky_golay";

const METHODS: &[EnumVariant] = &[
    EnumVariant::new(MOVING_AVERAGE, "Moving average"),
    EnumVariant::new(SAVITZKY_GOLAY, "Polynomial (Savitzky-Golay)"),
];
const WINDOW_MIN: i64 = 3;
const WINDOW_MAX: i64 = 201;
const ORDER_MIN: i64 = 1;
const ORDER_MAX: i64 = 8;
/// The old editor seeded order three. A small carried window lowers this seed
/// so `poly_order < window` is true immediately after switching methods.
pub const POLYNOMIAL_ORDER_SEED: u8 = 3;
/// Nine is the processing domain's smoothing default and is odd, inside
/// [`WINDOW_MIN`]–[`WINDOW_MAX`], and larger than the order seed.
pub const WINDOW_SEED: u16 = 9;
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
        canonical_label: "Smoothing method",
        canonical_aliases: &["moving average", "Savitzky-Golay", "smoothing"],
    },
    PropertyDefinition {
        id: WINDOW,
        scope_kind: ScopeKind::Dataset,
        value_schema: ValueSchema::SteppedInt {
            min: WINDOW_MIN,
            max: WINDOW_MAX,
            step: 2,
            drag_step: 0.2,
        },
        access: PropertyAccess::ReadWrite,
        applicability: STEP,
        default_policy: DefaultPolicy::None,
        tier: Tier::Essential,
        copies: ValueCopies::PerTarget,
        canonical_label: "Smoothing window",
        canonical_aliases: &["window points", "smoothing points"],
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
        tier: Tier::Essential,
        copies: ValueCopies::PerTarget,
        canonical_label: "Smoothing polynomial order",
        canonical_aliases: &["Savitzky-Golay order", "polynomial degree"],
    },
];

pub(crate) struct SmoothProvider;

pub(crate) static PROVIDER: SmoothProvider = SmoothProvider;

impl PropertyProvider for SmoothProvider {
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
            matches!(kind, StepKind::Smooth(_))
        })?;
        let StepKind::Smooth(current) = context.step.kind else {
            unreachable!("the shared context checked the step kind");
        };
        let point_count = spectrum_before_step(&context)
            .map(|spectrum| spectrum.values.len())
            .ok_or_else(|| smoothing_unavailable("its input spectrum is unavailable"))?;
        Ok(ResolvedProperty {
            address: address.clone(),
            modified: None,
            value: AggregateValue::Uniform(value_of(definition, current)?),
            default_value: None,
            availability: Availability::Editable,
            schema: schema_for(definition, current, point_count)?,
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
            matches!(kind, StepKind::Smooth(_))
        })?;
        let StepKind::Smooth(current) = context.step.kind else {
            unreachable!("the shared context checked the step kind");
        };
        let point_count = spectrum_before_step(&context)
            .map(|spectrum| spectrum.values.len())
            .ok_or_else(|| smoothing_unavailable("its input spectrum is unavailable"))?;
        let value = match operation {
            EditOp::Set(value) => checked_value(definition, current, point_count, value)?,
            EditOp::Reset => {
                return Err(PropertyError::NotApplicable(
                    "User-added smoothing steps have no factory setting to reset to.".to_owned(),
                ));
            }
            EditOp::Step(_) => return Err(no_step_gesture(definition)),
        };
        let state = transaction.processing_state(app, context.dataset_id)?;
        let step = step_mut(state, context.step.id, &address.target)?;
        let StepKind::Smooth(current) = &mut step.kind else {
            return Err(PropertyError::NotApplicable(
                "the addressed step is no longer a smoothing step".to_owned(),
            ));
        };
        write(definition, current, value)
    }
}

fn value_of(
    definition: &'static PropertyDefinition,
    current: SmoothMethod,
) -> Result<PropertyValue, PropertyError> {
    match (definition.id, current) {
        (METHOD, value) => Ok(PropertyValue::Enum(method_of(value))),
        (WINDOW, SmoothMethod::MovingAverage { window })
        | (WINDOW, SmoothMethod::SavitzkyGolay { window, .. }) => {
            Ok(PropertyValue::Int(i64::from(window)))
        }
        (POLYNOMIAL_ORDER, SmoothMethod::SavitzkyGolay { poly_order, .. }) => {
            Ok(PropertyValue::Int(i64::from(poly_order)))
        }
        _ => Err(polynomial_unavailable(current)),
    }
}

fn schema_for(
    definition: &'static PropertyDefinition,
    current: SmoothMethod,
    point_count: usize,
) -> Result<ResolvedSchema, PropertyError> {
    let window_max = maximum_odd_window(point_count)?;
    match definition.id {
        METHOD => Ok(ResolvedSchema::Enum {
            variants: METHODS.iter().collect(),
        }),
        WINDOW => {
            let min = match current {
                SmoothMethod::SavitzkyGolay { poly_order, .. } => minimum_odd_window(poly_order),
                SmoothMethod::MovingAverage { .. } => WINDOW_MIN,
            };
            Ok(ResolvedSchema::SteppedInt {
                min,
                max: window_max,
                step: 2,
                drag_step: 0.2,
                unit: "points",
            })
        }
        POLYNOMIAL_ORDER => {
            let SmoothMethod::SavitzkyGolay { window, .. } = current else {
                return Err(polynomial_unavailable(current));
            };
            Ok(ResolvedSchema::IntWithDrag {
                min: ORDER_MIN,
                max: ORDER_MAX.min(i64::from(window.min(window_max as u16)) - 1),
                drag_step: 0.1,
                unit: "",
            })
        }
        _ => Err(PropertyError::UnknownProperty(definition.id.to_string())),
    }
}

fn checked_value(
    definition: &'static PropertyDefinition,
    current: SmoothMethod,
    point_count: usize,
    value: &PropertyValue,
) -> Result<PropertyValue, PropertyError> {
    let window_max = maximum_odd_window(point_count)?;
    match (definition.id, value) {
        (METHOD, PropertyValue::Enum(value)) if variant(value).is_some() => {
            validate_current_window(current, window_max)?;
            Ok(PropertyValue::Enum(value))
        }
        (METHOD, PropertyValue::Enum(value)) => Err(PropertyError::InvalidValue {
            property: definition.id,
            message: format!("'{value}' is not a smoothing method"),
        }),
        (METHOD, value) => Err(wrong_kind(definition, value, "a smoothing method")),
        (WINDOW, PropertyValue::Int(value)) => {
            let min = match current {
                SmoothMethod::SavitzkyGolay { poly_order, .. } => minimum_odd_window(poly_order),
                SmoothMethod::MovingAverage { .. } => WINDOW_MIN,
            };
            if *value < min || *value > window_max || value % 2 == 0 {
                return Err(PropertyError::InvalidValue {
                    property: definition.id,
                    message: format!(
                        "smoothing window {value} is out of range: it must be an odd value between {min} and {window_max} for this {point_count}-point spectrum"
                    ),
                });
            }
            Ok(PropertyValue::Int(*value))
        }
        (POLYNOMIAL_ORDER, PropertyValue::Int(value)) => {
            let SmoothMethod::SavitzkyGolay { window, .. } = current else {
                return Err(polynomial_unavailable(current));
            };
            let effective_window = i64::from(window).min(window_max);
            let max = ORDER_MAX.min(effective_window - 1);
            if *value < ORDER_MIN || *value > max {
                return Err(PropertyError::InvalidValue {
                    property: definition.id,
                    message: format!(
                        "smoothing polynomial order {value} is out of range for window {window}: it must be between {ORDER_MIN} and {max}, and strictly less than the window"
                    ),
                });
            }
            Ok(PropertyValue::Int(*value))
        }
        (WINDOW | POLYNOMIAL_ORDER, value) => Err(wrong_kind(definition, value, "an integer")),
        (_, _) => Err(PropertyError::UnknownProperty(definition.id.to_string())),
    }
}

fn write(
    definition: &'static PropertyDefinition,
    current: &mut SmoothMethod,
    value: PropertyValue,
) -> Result<(), PropertyError> {
    match (definition.id, value) {
        (METHOD, PropertyValue::Enum(value)) => {
            let window = window_of(*current);
            *current = match variant(value) {
                Some(SmoothVariant::MovingAverage) => SmoothMethod::MovingAverage { window },
                Some(SmoothVariant::SavitzkyGolay) => SmoothMethod::SavitzkyGolay {
                    window,
                    poly_order: match *current {
                        SmoothMethod::SavitzkyGolay { poly_order, .. } => {
                            poly_order.min((window - 1) as u8)
                        }
                        _ => seed_order(window),
                    },
                },
                None => {
                    return Err(PropertyError::InvalidValue {
                        property: definition.id,
                        message: format!("'{value}' is not a smoothing method"),
                    });
                }
            };
            Ok(())
        }
        (WINDOW, PropertyValue::Int(value)) => {
            let window = u16::try_from(value).map_err(|_| PropertyError::InvalidValue {
                property: definition.id,
                message: format!(
                    "smoothing window {value} is out of range: it must be between {WINDOW_MIN} and {WINDOW_MAX}"
                ),
            })?;
            match current {
                SmoothMethod::MovingAverage {
                    window: current_window,
                }
                | SmoothMethod::SavitzkyGolay {
                    window: current_window,
                    ..
                } => *current_window = window,
            }
            Ok(())
        }
        (POLYNOMIAL_ORDER, PropertyValue::Int(value)) => {
            let SmoothMethod::SavitzkyGolay { poly_order, .. } = current else {
                return Err(polynomial_unavailable(*current));
            };
            *poly_order = u8::try_from(value).map_err(|_| PropertyError::InvalidValue {
                property: definition.id,
                message: format!(
                    "smoothing polynomial order {value} is out of range: it must be between {ORDER_MIN} and {ORDER_MAX}"
                ),
            })?;
            Ok(())
        }
        (_, value) => Err(wrong_kind(
            definition,
            &value,
            "the declared smoothing value",
        )),
    }
}

fn minimum_odd_window(poly_order: u8) -> i64 {
    let minimum = (i64::from(poly_order) + 1).max(WINDOW_MIN);
    if minimum % 2 == 0 {
        minimum + 1
    } else {
        minimum
    }
}

fn seed_order(window: u16) -> u8 {
    POLYNOMIAL_ORDER_SEED
        .min((window.saturating_sub(1)) as u8)
        .max(1)
}

fn maximum_odd_window(point_count: usize) -> Result<i64, PropertyError> {
    let capped = point_count.min(WINDOW_MAX as usize);
    let maximum = if capped.is_multiple_of(2) {
        capped.saturating_sub(1)
    } else {
        capped
    };
    if maximum < WINDOW_MIN as usize {
        return Err(smoothing_unavailable(
            "it needs an input spectrum with at least 3 points",
        ));
    }
    Ok(maximum as i64)
}

fn validate_current_window(current: SmoothMethod, window_max: i64) -> Result<(), PropertyError> {
    let window = i64::from(window_of(current));
    let min = match current {
        SmoothMethod::SavitzkyGolay { poly_order, .. } => minimum_odd_window(poly_order),
        SmoothMethod::MovingAverage { .. } => WINDOW_MIN,
    };
    if window < min || window > window_max || window % 2 == 0 {
        return Err(PropertyError::InvalidValue {
            property: METHOD,
            message: format!(
                "stored smoothing window {window} is out of range: it must be an odd value between {min} and {window_max}; correct the window before switching methods"
            ),
        });
    }
    Ok(())
}

fn smoothing_unavailable(reason: &str) -> PropertyError {
    PropertyError::NotApplicable(format!("Smoothing is unavailable because {reason}."))
}

fn window_of(method: SmoothMethod) -> u16 {
    match method {
        SmoothMethod::MovingAverage { window } | SmoothMethod::SavitzkyGolay { window, .. } => {
            window
        }
    }
}

#[derive(Clone, Copy)]
enum SmoothVariant {
    MovingAverage,
    SavitzkyGolay,
}

fn variant(value: &str) -> Option<SmoothVariant> {
    match value {
        MOVING_AVERAGE => Some(SmoothVariant::MovingAverage),
        SAVITZKY_GOLAY => Some(SmoothVariant::SavitzkyGolay),
        _ => None,
    }
}

fn method_of(method: SmoothMethod) -> &'static str {
    match method {
        SmoothMethod::MovingAverage { .. } => MOVING_AVERAGE,
        SmoothMethod::SavitzkyGolay { .. } => SAVITZKY_GOLAY,
    }
}

fn polynomial_unavailable(current: SmoothMethod) -> PropertyError {
    PropertyError::NotApplicable(format!(
        "Smoothing polynomial order is available only with Polynomial (Savitzky-Golay); this step uses {}",
        METHODS
            .iter()
            .find(|variant| variant.id == method_of(current))
            .map(|variant| variant.canonical_label)
            .unwrap_or("an unknown method")
    ))
}
