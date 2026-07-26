//! Dataset-owned apodization-step properties.
//!
//! A processing step is an owner-local component, not a path through a
//! pipeline. `StepId` is consequently the only component identity this module
//! accepts; resolving the containing axis is an implementation detail of the
//! dataset snapshot the typed action already owns.

use super::provider::PropertyProvider;
use super::target::dataset_steps;
use super::{
    AggregateValue, Applicability, Availability, ComponentKind, DefaultPolicy, EditOp, EnumVariant,
    FloatBounds, PropertyAccess, PropertyAddress, PropertyDefinition, PropertyError, PropertyId,
    PropertyTransaction, PropertyValue, ResolvedProperty, ResolvedSchema, ScopeKind, Tier,
    ValueCopies, ValueSchema, definition,
};
use crate::actions::DatasetProcessingState;
use crate::automation::ComponentRef;
use crate::state::{Dataset, DatasetId, PhaseAxis, PlotxApp};
use plotx_processing::{Apodization, ProcessingStep, StepId, StepKind, StepSource};

pub const KIND: PropertyId = PropertyId("dataset.processing.apodization.kind");
pub const LB_HZ: PropertyId = PropertyId("dataset.processing.apodization.lb_hz");
pub const GB_HZ: PropertyId = PropertyId("dataset.processing.apodization.gb_hz");

pub const APODIZATION_NONE: &str = "none";
pub const APODIZATION_COSINE_BELL: &str = "cosine_bell";
pub const APODIZATION_EXPONENTIAL: &str = "exponential";
pub const APODIZATION_GAUSSIAN: &str = "gaussian";

/// Line broadening admits both signs. In the window this crate applies,
/// `exp(+pi*lb*t - g*t^2)` for a Gaussian and `exp(-pi*lb*t)` for an
/// exponential, a positive LB narrows lines under a Gaussian — the
/// Lorentz-to-Gauss resolution enhancement — and a negative one broadens them
/// further. Both are wanted, so neither sign is excluded.
///
/// The range is deliberately wide, and therefore says nothing about how far one
/// drag notch should move the value. That is why the definitions declare the
/// notch separately.
const LB_BOUNDS: FloatBounds = FloatBounds::inclusive(-10_000.0, 10_000.0);
/// Gaussian broadening is open at zero, and the bound is not decoration. The
/// window's Gaussian term is `g = (pi*gb)^2 / (4 ln 2)`, so `gb = 0` leaves
/// `exp(+pi*lb*t)` — a pure exponential *growth* with no maximum and no decay,
/// which is not a window at all. `g` is also even in `gb`, so a negative value
/// behaves exactly as its magnitude: admitting one would let the panel read back
/// a number that does not describe what the transform did.
const GB_BOUNDS: FloatBounds = FloatBounds::above(0.0, 10_000.0);
/// Half a hertz per notch — the step the inline processing editor used before
/// these parameters moved into the catalog, and the resolution at which typical
/// line broadenings (0.3–5 Hz) are actually chosen.
const PARAMETER_STEP: f64 = 0.5;
/// The broadening a window that has none yet starts from. Switching a step to
/// exponential or Gaussian and resetting one both land here, so the two cannot
/// disagree about what this parameter's neutral value is.
pub const LB_DEFAULT_HZ: f64 = 1.0;
/// The Gaussian broadening a window that has none yet starts from. It may not be
/// zero: [`GB_BOUNDS`] excludes it, and a seed the schema rejects would put the
/// step in a state its own control refuses to accept back.
pub const GB_DEFAULT_HZ: f64 = 1.0;
const APODIZATION_KINDS: &[EnumVariant] = &[
    EnumVariant::new(APODIZATION_NONE, "None"),
    EnumVariant::new(APODIZATION_COSINE_BELL, "Cosine bell"),
    EnumVariant::new(APODIZATION_EXPONENTIAL, "Exponential"),
    EnumVariant::new(APODIZATION_GAUSSIAN, "Gaussian"),
];

const APODIZATION_STEP: Applicability = Applicability::component(ComponentKind::ProcessingStep);

pub(crate) const DEFINITIONS: &[PropertyDefinition] = &[
    PropertyDefinition {
        id: KIND,
        scope_kind: ScopeKind::Dataset,
        value_schema: ValueSchema::Enum {
            variants: APODIZATION_KINDS,
        },
        access: PropertyAccess::ReadWrite,
        applicability: APODIZATION_STEP,
        default_policy: DefaultPolicy::ProcessingFactory,
        tier: Tier::Essential,
        copies: ValueCopies::PerTarget,
        canonical_label: "Apodization window",
        canonical_aliases: &["apodization", "window function", "exponential", "gaussian"],
    },
    PropertyDefinition {
        id: LB_HZ,
        scope_kind: ScopeKind::Dataset,
        value_schema: ValueSchema::Float {
            bounds: LB_BOUNDS,
            log: false,
            drag_step: Some(PARAMETER_STEP),
        },
        access: PropertyAccess::ReadWrite,
        applicability: APODIZATION_STEP,
        default_policy: DefaultPolicy::ProcessingFactory,
        // Line broadening is the most-reached-for setting in NMR processing and
        // sat beside the window choice in the editor this replaced. Advanced is
        // for what a user rarely looks for, not for what merely has a number.
        tier: Tier::Essential,
        copies: ValueCopies::PerTarget,
        canonical_label: "Apodization line broadening",
        canonical_aliases: &["LB", "line broadening", "apodization lb"],
    },
    PropertyDefinition {
        id: GB_HZ,
        scope_kind: ScopeKind::Dataset,
        value_schema: ValueSchema::Float {
            bounds: GB_BOUNDS,
            log: false,
            drag_step: Some(PARAMETER_STEP),
        },
        access: PropertyAccess::ReadWrite,
        applicability: APODIZATION_STEP,
        default_policy: DefaultPolicy::ProcessingFactory,
        // Only ever shown alongside LB, and meaningless without it.
        tier: Tier::Essential,
        copies: ValueCopies::PerTarget,
        canonical_label: "Apodization Gaussian broadening",
        canonical_aliases: &["GB", "gaussian broadening", "apodization gb"],
    },
];

pub(crate) struct ApodizationProvider;

pub(crate) static PROVIDER: ApodizationProvider = ApodizationProvider;

impl PropertyProvider for ApodizationProvider {
    fn definitions(&self) -> &'static [PropertyDefinition] {
        DEFINITIONS
    }

    fn read(
        &self,
        app: &PlotxApp,
        address: &PropertyAddress,
    ) -> Result<ResolvedProperty, PropertyError> {
        let definition = property_definition(address.definition)?;
        let context = context(app, address, definition)?;
        let value = value_of(definition, context.current)?;
        Ok(ResolvedProperty {
            address: address.clone(),
            value: AggregateValue::Uniform(value),
            default_value: default_value(definition, context.factory)?,
            availability: Availability::Editable,
            schema: schema_for(definition, context.current)?,
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
        let context = context(app, address, definition)?;
        let value = match operation {
            EditOp::Set(value) => checked_value(definition, context.current, value)?,
            // A hand-added step has no factory setting behind it, so there is
            // nothing to reset *to*. Saying so skips this target and leaves the
            // rest of a multi-target reset to land.
            EditOp::Reset => default_value(definition, context.factory)?.ok_or_else(|| {
                PropertyError::NotApplicable(format!(
                    "{} was added by hand and has no factory setting to reset to",
                    definition.canonical_label
                ))
            })?,
            EditOp::Step(_) => {
                return Err(PropertyError::InvalidValue {
                    property: definition.id,
                    message: "this processing setting has no step gesture".to_owned(),
                });
            }
        };
        let state = transaction.processing_state(app, context.dataset)?;
        let apodization = apodization_mut(state, context.step, &address.target)?;
        write(definition, apodization, value)
    }
}

#[derive(Clone, Copy)]
struct ApodizationContext {
    dataset: DatasetId,
    step: StepId,
    current: Apodization,
    /// The factory's window for this step, absent for a step the user added.
    factory: Option<Apodization>,
}

fn property_definition(id: PropertyId) -> Result<&'static PropertyDefinition, PropertyError> {
    definition(id).ok_or_else(|| PropertyError::UnknownProperty(id.as_str().to_owned()))
}

fn context(
    app: &PlotxApp,
    address: &PropertyAddress,
    definition: &'static PropertyDefinition,
) -> Result<ApodizationContext, PropertyError> {
    let actual = ComponentKind::of(address.target.component.as_ref());
    if actual != definition.applicability.component {
        return Err(PropertyError::ComponentKind {
            property: definition.id,
            expected: definition.applicability.component.as_str(),
            actual: actual.as_str(),
        });
    }
    let Some(ComponentRef::ProcessingStep(step)) = address.target.component else {
        return Err(PropertyError::ComponentKind {
            property: definition.id,
            expected: ComponentKind::ProcessingStep.as_str(),
            actual: actual.as_str(),
        });
    };
    let dataset = DatasetId::try_from(&address.target.resource).map_err(|error| {
        PropertyError::NotApplicable(format!(
            "{} needs a dataset resource: {error}",
            definition.id
        ))
    })?;
    let dataset_value = app
        .doc
        .dataset_by_id(dataset)
        .ok_or_else(|| PropertyError::UnknownTarget(address.target.resource.id.clone()))?;
    let (axis, processing_step) = step_in_dataset(dataset_value, step, &address.target)?;
    let StepKind::Apodize(current) = processing_step.kind else {
        return Err(PropertyError::NotApplicable(format!(
            "{} addresses an apodization step, but step {} is {}",
            definition.canonical_label,
            step.get(),
            step_kind_name(&processing_step.kind)
        )));
    };
    Ok(ApodizationContext {
        dataset,
        step,
        current,
        factory: factory_default(dataset_value, axis, processing_step),
    })
}

/// Locate an addressed step among the ones the dataset actually exposes.
///
/// The search runs over [`dataset_steps`], so a step on an axis the rest of the
/// application hides is not addressable here either.
fn step_in_dataset<'a>(
    dataset: &'a Dataset,
    id: StepId,
    target: &crate::automation::TargetRef,
) -> Result<(PhaseAxis, &'a ProcessingStep), PropertyError> {
    dataset_steps(dataset)
        .find(|(_, step)| step.id == id)
        .ok_or_else(|| PropertyError::UnknownTarget(target.describe()))
}

/// The window the factory recipe puts in this step, if it puts one there at all.
///
/// The answer is read out of the factory itself rather than re-derived from the
/// dataset's shape: a second derivation agrees with the factory only until one
/// of them changes. A step the user added by hand has no counterpart in the
/// factory recipe and therefore no default — `None` here, not "no window".
/// Claiming one would mark a freshly added step as already modified, and would
/// let its reset button turn a window the user deliberately chose into a step
/// that sits in the pipeline doing nothing.
fn factory_default(
    dataset: &Dataset,
    axis: PhaseAxis,
    step: &ProcessingStep,
) -> Option<Apodization> {
    if step.source != StepSource::Default {
        return None;
    }
    dataset
        .factory_pipeline(axis)?
        .steps
        .iter()
        .find(|candidate| candidate.id == step.id)
        .and_then(|candidate| match candidate.kind {
            StepKind::Apodize(apodization) => Some(apodization),
            _ => None,
        })
}

fn value_of(
    definition: &'static PropertyDefinition,
    apodization: Apodization,
) -> Result<PropertyValue, PropertyError> {
    match (definition.id, apodization) {
        (KIND, value) => Ok(PropertyValue::Enum(kind_of(value))),
        (LB_HZ, Apodization::Exponential { lb_hz } | Apodization::Gaussian { lb_hz, .. }) => {
            Ok(PropertyValue::Float(lb_hz))
        }
        (GB_HZ, Apodization::Gaussian { gb_hz, .. }) => Ok(PropertyValue::Float(gb_hz)),
        _ => Err(unavailable_parameter(definition, apodization)),
    }
}

fn default_value(
    definition: &'static PropertyDefinition,
    factory: Option<Apodization>,
) -> Result<Option<PropertyValue>, PropertyError> {
    match definition.default_policy {
        DefaultPolicy::ProcessingFactory => {
            let Some(factory) = factory else {
                return Ok(None);
            };
            match definition.id {
                KIND => Ok(Some(PropertyValue::Enum(kind_of(factory)))),
                LB_HZ => Ok(Some(PropertyValue::Float(match factory {
                    Apodization::Exponential { lb_hz } | Apodization::Gaussian { lb_hz, .. } => {
                        lb_hz
                    }
                    Apodization::None | Apodization::CosineBell => LB_DEFAULT_HZ,
                }))),
                GB_HZ => Ok(Some(PropertyValue::Float(match factory {
                    Apodization::Gaussian { gb_hz, .. } => gb_hz,
                    Apodization::None
                    | Apodization::CosineBell
                    | Apodization::Exponential { .. } => GB_DEFAULT_HZ,
                }))),
                _ => Err(PropertyError::UnknownProperty(
                    definition.id.as_str().to_owned(),
                )),
            }
        }
        DefaultPolicy::Fixed(value) => Ok(Some(value)),
        DefaultPolicy::EncodingFactory | DefaultPolicy::None => Err(PropertyError::InvalidValue {
            property: definition.id,
            message: "this property has no processing default".to_owned(),
        }),
    }
}

fn schema_for(
    definition: &'static PropertyDefinition,
    apodization: Apodization,
) -> Result<ResolvedSchema, PropertyError> {
    match definition.id {
        KIND => Ok(ResolvedSchema::Enum {
            variants: APODIZATION_KINDS.iter().collect(),
        }),
        LB_HZ
            if matches!(
                apodization,
                Apodization::Exponential { .. } | Apodization::Gaussian { .. }
            ) =>
        {
            Ok(parameter_schema(definition))
        }
        GB_HZ if matches!(apodization, Apodization::Gaussian { .. }) => {
            Ok(parameter_schema(definition))
        }
        _ => Err(unavailable_parameter(definition, apodization)),
    }
}

/// The bounds one broadening parameter is admitted by. They differ: line
/// broadening is signed, Gaussian broadening is not.
fn parameter_bounds(definition: &'static PropertyDefinition) -> FloatBounds {
    if definition.id == GB_HZ {
        GB_BOUNDS
    } else {
        LB_BOUNDS
    }
}

fn parameter_schema(definition: &'static PropertyDefinition) -> ResolvedSchema {
    ResolvedSchema::Float {
        bounds: parameter_bounds(definition),
        log: false,
        unit: "Hz",
    }
}

fn checked_value(
    definition: &'static PropertyDefinition,
    apodization: Apodization,
    value: PropertyValue,
) -> Result<PropertyValue, PropertyError> {
    match definition.id {
        KIND => match value {
            PropertyValue::Enum(value) if variant(value).is_some() => {
                Ok(PropertyValue::Enum(value))
            }
            PropertyValue::Enum(value) => Err(PropertyError::InvalidValue {
                property: definition.id,
                message: format!("'{value}' is not an apodization window"),
            }),
            value => wrong_kind(definition, value, "an apodization window"),
        },
        LB_HZ | GB_HZ => {
            let _ = value_of(definition, apodization)?;
            match value {
                PropertyValue::Float(value) => {
                    parameter_bounds(definition).check(
                        definition.id,
                        definition.canonical_label,
                        value,
                    )?;
                    Ok(PropertyValue::Float(value))
                }
                value => wrong_kind(definition, value, "a number"),
            }
        }
        _ => Err(PropertyError::UnknownProperty(
            definition.id.as_str().to_owned(),
        )),
    }
}

fn wrong_kind(
    definition: &'static PropertyDefinition,
    value: PropertyValue,
    expected: &str,
) -> Result<PropertyValue, PropertyError> {
    Err(PropertyError::InvalidValue {
        property: definition.id,
        message: format!("expected {expected}, got {}", value.kind()),
    })
}

fn write(
    definition: &'static PropertyDefinition,
    apodization: &mut Apodization,
    value: PropertyValue,
) -> Result<(), PropertyError> {
    match (definition.id, value) {
        (KIND, PropertyValue::Enum(kind)) => {
            let Some(kind) = variant(kind) else {
                return Err(PropertyError::InvalidValue {
                    property: definition.id,
                    message: "the apodization window is not recognized".to_owned(),
                });
            };
            *apodization = kind.with_current(*apodization);
            Ok(())
        }
        (LB_HZ, PropertyValue::Float(value)) => match apodization {
            Apodization::Exponential { lb_hz } | Apodization::Gaussian { lb_hz, .. } => {
                *lb_hz = value;
                Ok(())
            }
            current => Err(unavailable_parameter(definition, *current)),
        },
        (GB_HZ, PropertyValue::Float(value)) => match apodization {
            Apodization::Gaussian { gb_hz, .. } => {
                *gb_hz = value;
                Ok(())
            }
            current => Err(unavailable_parameter(definition, *current)),
        },
        (_, value) => Err(PropertyError::InvalidValue {
            property: definition.id,
            message: format!(
                "expected the property's declared value, got {}",
                value.kind()
            ),
        }),
    }
}

fn apodization_mut<'a>(
    state: &'a mut DatasetProcessingState,
    id: StepId,
    target: &crate::automation::TargetRef,
) -> Result<&'a mut Apodization, PropertyError> {
    let step = state
        .steps_mut()
        .find(|step| step.id == id)
        .ok_or_else(|| PropertyError::UnknownTarget(target.describe()))?;
    match &mut step.kind {
        StepKind::Apodize(apodization) => Ok(apodization),
        kind => Err(PropertyError::NotApplicable(format!(
            "step {} is {}, not an apodization step",
            id.get(),
            step_kind_name(kind)
        ))),
    }
}

fn unavailable_parameter(
    definition: &'static PropertyDefinition,
    apodization: Apodization,
) -> PropertyError {
    let required = match definition.id {
        LB_HZ => "an exponential or Gaussian window",
        GB_HZ => "a Gaussian window",
        _ => "this apodization window",
    };
    PropertyError::NotApplicable(format!(
        "{} is available only with {required}; this step uses {}",
        definition.canonical_label,
        // The label, not the wire id: this sentence is read by a person, and
        // "cosine_bell" is the identifier the choice is stored under.
        window_label(apodization)
    ))
}

#[derive(Clone, Copy)]
enum ApodizationKind {
    None,
    CosineBell,
    Exponential,
    Gaussian,
}

impl ApodizationKind {
    /// Switch the window kind, carrying over whatever the current one already
    /// says. A parameter the current kind does not carry starts from this
    /// module's declared neutral value — the same one a reset lands on, and one
    /// the schema admits. Seeding `gb` with zero produced a Gaussian whose own
    /// control refuses the value it was given, and whose transform grows without
    /// bound.
    fn with_current(self, current: Apodization) -> Apodization {
        let (lb_hz, gb_hz) = match current {
            Apodization::Exponential { lb_hz } => (lb_hz, GB_DEFAULT_HZ),
            Apodization::Gaussian { lb_hz, gb_hz } => (lb_hz, gb_hz),
            Apodization::None | Apodization::CosineBell => (LB_DEFAULT_HZ, GB_DEFAULT_HZ),
        };
        match self {
            Self::None => Apodization::None,
            Self::CosineBell => Apodization::CosineBell,
            Self::Exponential => Apodization::Exponential { lb_hz },
            Self::Gaussian => Apodization::Gaussian { lb_hz, gb_hz },
        }
    }
}

/// The choice's own display label, taken from the variant list the control is
/// built from so the two cannot word it differently.
fn window_label(apodization: Apodization) -> &'static str {
    let id = kind_of(apodization);
    APODIZATION_KINDS
        .iter()
        .find(|variant| variant.id == id)
        .map(|variant| variant.canonical_label)
        .unwrap_or(id)
}

fn kind_of(apodization: Apodization) -> &'static str {
    match apodization {
        Apodization::None => APODIZATION_NONE,
        Apodization::CosineBell => APODIZATION_COSINE_BELL,
        Apodization::Exponential { .. } => APODIZATION_EXPONENTIAL,
        Apodization::Gaussian { .. } => APODIZATION_GAUSSIAN,
    }
}

fn variant(value: &str) -> Option<ApodizationKind> {
    match value {
        APODIZATION_NONE => Some(ApodizationKind::None),
        APODIZATION_COSINE_BELL => Some(ApodizationKind::CosineBell),
        APODIZATION_EXPONENTIAL => Some(ApodizationKind::Exponential),
        APODIZATION_GAUSSIAN => Some(ApodizationKind::Gaussian),
        _ => None,
    }
}

fn step_kind_name(kind: &StepKind) -> &'static str {
    match kind {
        StepKind::Apodize(_) => "apodization",
        StepKind::ZeroFill(_) => "zero fill",
        StepKind::Fft => "FFT",
        StepKind::Phase(_) => "phase",
        StepKind::Baseline(_) => "baseline",
        StepKind::Reference(_) => "reference",
        StepKind::Magnitude => "magnitude",
        StepKind::Smooth(_) => "smoothing",
        StepKind::Normalize(_) => "normalization",
        StepKind::Bin(_) => "binning",
        StepKind::Reverse => "reverse",
        StepKind::Invert => "invert",
    }
}
