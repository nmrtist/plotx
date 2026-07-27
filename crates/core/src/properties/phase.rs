//! Dataset-owned phase-step properties.

use super::processing_common::{
    no_factory_default, no_step_gesture, property_definition, step_context, step_mut, wrong_kind,
};
use super::provider::PropertyProvider;
use super::{
    AggregateValue, Applicability, Availability, ComponentKind, DefaultPolicy, EditOp, EnumVariant,
    FloatBounds, FloatDisplay, PropertyAccess, PropertyAddress, PropertyDefinition, PropertyError,
    PropertyId, PropertyReadout, PropertyTransaction, PropertyValue, ResolvedProperty,
    ResolvedSchema, ScopeKind, Tier, ValueCopies, ValueSchema,
};
use crate::state::PlotxApp;
use plotx_processing::{AutoPhaseMethod, PhaseParams, StepKind};

pub const MODE: PropertyId = PropertyId("dataset.processing.phase.mode");
pub const PHASE0: PropertyId = PropertyId("dataset.processing.phase.phase0");
pub const PHASE1: PropertyId = PropertyId("dataset.processing.phase.phase1");
pub const PIVOT: PropertyId = PropertyId("dataset.processing.phase.pivot");

pub const MANUAL: &str = "manual";
pub const ROBUST_CONSENSUS: &str = "robust_consensus";
pub const ABSORPTIVE_PEAK: &str = "absorptive_peak";
pub const ENTROPY: &str = "entropy";
pub const NEGATIVE_MINIMIZATION: &str = "negative_minimization";
pub const PEAK_REGRESSION: &str = "peak_regression";

const MODES: &[EnumVariant] = &[
    EnumVariant::new(MANUAL, "Manual"),
    EnumVariant::new(ROBUST_CONSENSUS, "Auto: Robust consensus"),
    EnumVariant::new(ABSORPTIVE_PEAK, "Auto: Absorptive peak"),
    EnumVariant::new(ENTROPY, "Auto: Entropy (ACME)"),
    EnumVariant::new(NEGATIVE_MINIMIZATION, "Auto: Min. negative area"),
    EnumVariant::new(PEAK_REGRESSION, "Auto: Peak regression"),
];
const UNBOUNDED: FloatBounds = FloatBounds::inclusive(-f64::MAX, f64::MAX);
const PIVOT_BOUNDS: FloatBounds = FloatBounds::inclusive(0.0, 1.0);
/// The old editor moved half a degree per notch. Drag steps are declared in the
/// display space, while the stored property and automation value stay radians.
const PHASE_STEP_DEGREES: f64 = 0.5;
const PIVOT_STEP: f64 = 0.001;
const STEP: Applicability = Applicability::component(ComponentKind::ProcessingStep);

pub const MANUAL_PHASE0_REASON: &str = "Switch the phase mode to Manual before setting φ0.";
pub const MANUAL_PHASE1_REASON: &str = "Switch the phase mode to Manual before setting φ1.";
pub const MANUAL_PIVOT_REASON: &str = "Switch the phase mode to Manual before setting the pivot.";

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
        canonical_label: "Phase mode",
        canonical_aliases: &["manual phase", "automatic phase", "autophase"],
    },
    PropertyDefinition {
        id: PHASE0,
        scope_kind: ScopeKind::Dataset,
        value_schema: ValueSchema::Float {
            bounds: UNBOUNDED,
            display: FloatDisplay::Degrees,
            drag_step: Some(PHASE_STEP_DEGREES),
        },
        access: PropertyAccess::ReadWrite,
        applicability: STEP,
        default_policy: DefaultPolicy::ProcessingFactory,
        tier: Tier::Essential,
        copies: ValueCopies::PerTarget,
        canonical_label: "Zero-order phase",
        canonical_aliases: &["phase0", "phi0", "φ0"],
    },
    PropertyDefinition {
        id: PHASE1,
        scope_kind: ScopeKind::Dataset,
        value_schema: ValueSchema::Float {
            bounds: UNBOUNDED,
            display: FloatDisplay::Degrees,
            drag_step: Some(PHASE_STEP_DEGREES),
        },
        access: PropertyAccess::ReadWrite,
        applicability: STEP,
        default_policy: DefaultPolicy::ProcessingFactory,
        tier: Tier::Essential,
        copies: ValueCopies::PerTarget,
        canonical_label: "First-order phase",
        canonical_aliases: &["phase1", "phi1", "φ1"],
    },
    PropertyDefinition {
        id: PIVOT,
        scope_kind: ScopeKind::Dataset,
        value_schema: ValueSchema::Float {
            bounds: PIVOT_BOUNDS,
            display: FloatDisplay::Linear("fraction"),
            drag_step: Some(PIVOT_STEP),
        },
        access: PropertyAccess::ReadWrite,
        applicability: STEP,
        default_policy: DefaultPolicy::ProcessingFactory,
        tier: Tier::Essential,
        copies: ValueCopies::PerTarget,
        canonical_label: "Phase pivot",
        canonical_aliases: &["phase pivot fraction", "pivot_frac", "phase origin"],
    },
];

pub(crate) struct PhaseProvider;

pub(crate) static PROVIDER: PhaseProvider = PhaseProvider;

impl PropertyProvider for PhaseProvider {
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
            matches!(kind, StepKind::Phase(_))
        })?;
        let StepKind::Phase(current) = context.step.kind else {
            unreachable!("the shared context checked the step kind");
        };
        let factory = context.factory.as_ref().and_then(|step| match step.kind {
            StepKind::Phase(value) => Some(value),
            _ => None,
        });
        Ok(ResolvedProperty {
            address: address.clone(),
            modified: None,
            value: AggregateValue::Uniform(value_of(definition, current)?),
            default_value: default_value(definition, factory)?,
            availability: availability(definition, current),
            schema: schema_for(definition)?,
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
            matches!(kind, StepKind::Phase(_))
        })?;
        let StepKind::Phase(current) = context.step.kind else {
            unreachable!("the shared context checked the step kind");
        };
        let factory = context.factory.as_ref().and_then(|step| match step.kind {
            StepKind::Phase(value) => Some(value),
            _ => None,
        });
        let value = match operation {
            EditOp::Set(value) => checked_value(definition, current, value)?,
            EditOp::Reset => checked_reset_value(
                definition,
                current,
                default_value(definition, factory)?
                    .ok_or_else(|| no_factory_default(definition))?,
            )?,
            EditOp::Step(_) => return Err(no_step_gesture(definition)),
        };
        // This is the same live automatic result the old `set_phase_method`
        // seeded. It is read before selecting the working copy because the
        // cached base belongs to the live dataset, not the transaction.
        let automatic_seed = if definition.id == MODE
            && value == PropertyValue::Enum(MANUAL)
            && current.auto.is_some()
        {
            context.dataset.automatic_phase_params(context.axis)
        } else {
            None
        };
        let state = transaction.processing_state(app, context.dataset_id)?;
        let step = step_mut(state, context.step.id, &address.target)?;
        let StepKind::Phase(current) = &mut step.kind else {
            return Err(PropertyError::NotApplicable(
                "the addressed step is no longer a phase step".to_owned(),
            ));
        };
        write(definition, current, value, automatic_seed)
    }

    fn readout(
        &self,
        app: &PlotxApp,
        address: &PropertyAddress,
    ) -> Result<PropertyReadout, PropertyError> {
        let definition = property_definition(address.definition)?;
        let context = step_context(app, address, definition, |kind| {
            matches!(kind, StepKind::Phase(_))
        })?;
        if definition.id == PIVOT {
            return context
                .dataset
                .pivot_ppm(context.axis)
                .map(|ppm| PropertyReadout::PhasePivotPpm { ppm })
                .ok_or_else(|| {
                    PropertyError::NotApplicable(
                        "The addressed phase axis has no ppm ruler.".to_owned(),
                    )
                });
        }
        super::readout::uniform_readout(self.read(app, address)?)
    }
}

fn value_of(
    definition: &'static PropertyDefinition,
    current: PhaseParams,
) -> Result<PropertyValue, PropertyError> {
    match definition.id {
        MODE => Ok(PropertyValue::Enum(mode_of(current.auto))),
        PHASE0 => Ok(PropertyValue::Float(current.phase0)),
        PHASE1 => Ok(PropertyValue::Float(current.phase1)),
        PIVOT => Ok(PropertyValue::Float(current.pivot_frac)),
        _ => Err(PropertyError::UnknownProperty(definition.id.to_string())),
    }
}

fn default_value(
    definition: &'static PropertyDefinition,
    factory: Option<PhaseParams>,
) -> Result<Option<PropertyValue>, PropertyError> {
    let Some(factory) = factory else {
        return Ok(None);
    };
    Ok(Some(value_of(definition, factory)?))
}

fn availability(definition: &'static PropertyDefinition, current: PhaseParams) -> Availability {
    if current.auto.is_none() {
        return Availability::Editable;
    }
    match definition.id {
        PHASE0 => Availability::Disabled(MANUAL_PHASE0_REASON),
        PHASE1 => Availability::Disabled(MANUAL_PHASE1_REASON),
        PIVOT => Availability::Disabled(MANUAL_PIVOT_REASON),
        _ => Availability::Editable,
    }
}

fn schema_for(definition: &'static PropertyDefinition) -> Result<ResolvedSchema, PropertyError> {
    match definition.id {
        MODE => Ok(ResolvedSchema::Enum {
            variants: MODES.iter().collect(),
        }),
        PHASE0 | PHASE1 => Ok(ResolvedSchema::Float {
            bounds: UNBOUNDED,
            display: FloatDisplay::Degrees,
        }),
        PIVOT => Ok(ResolvedSchema::Float {
            bounds: PIVOT_BOUNDS,
            display: FloatDisplay::Linear("fraction"),
        }),
        _ => Err(PropertyError::UnknownProperty(definition.id.to_string())),
    }
}

fn checked_reset_value(
    definition: &'static PropertyDefinition,
    current: PhaseParams,
    value: PropertyValue,
) -> Result<PropertyValue, PropertyError> {
    if current.auto.is_some() {
        let subject = match definition.id {
            PHASE0 => "φ0",
            PHASE1 => "φ1",
            PIVOT => "the pivot",
            _ => return checked_value(definition, current, &value),
        };
        return Err(PropertyError::NotApplicable(format!(
            "Switch the phase mode to Manual before resetting {subject}."
        )));
    }
    checked_value(definition, current, &value)
}

fn checked_value(
    definition: &'static PropertyDefinition,
    current: PhaseParams,
    value: &PropertyValue,
) -> Result<PropertyValue, PropertyError> {
    if let Availability::Disabled(reason) = availability(definition, current) {
        return Err(PropertyError::NotApplicable(reason.to_owned()));
    }
    match (definition.id, value) {
        (MODE, PropertyValue::Enum(value)) if method_of(value).is_some() => {
            Ok(PropertyValue::Enum(value))
        }
        (MODE, PropertyValue::Enum(value)) => Err(PropertyError::InvalidValue {
            property: definition.id,
            message: format!("'{value}' is not a phase mode"),
        }),
        (MODE, value) => Err(wrong_kind(definition, value, "a phase mode")),
        (PHASE0 | PHASE1, PropertyValue::Float(value)) => {
            UNBOUNDED.check(definition.id, definition.canonical_label, *value)?;
            Ok(PropertyValue::Float(*value))
        }
        (PIVOT, PropertyValue::Float(value)) => {
            PIVOT_BOUNDS.check(definition.id, definition.canonical_label, *value)?;
            Ok(PropertyValue::Float(*value))
        }
        (PHASE0 | PHASE1 | PIVOT, value) => Err(wrong_kind(definition, value, "a number")),
        (_, _) => Err(PropertyError::UnknownProperty(definition.id.to_string())),
    }
}

fn write(
    definition: &'static PropertyDefinition,
    current: &mut PhaseParams,
    value: PropertyValue,
    automatic_seed: Option<(f64, f64, f64)>,
) -> Result<(), PropertyError> {
    match (definition.id, value) {
        (MODE, PropertyValue::Enum(value)) => {
            match method_of(value) {
                Some(None) => {
                    if let Some((phase0, phase1, pivot_frac)) = automatic_seed {
                        current.phase0 = phase0;
                        current.phase1 = phase1;
                        current.pivot_frac = pivot_frac;
                    }
                    current.auto = None;
                }
                Some(Some(method)) => current.auto = Some(method),
                None => {
                    return Err(PropertyError::InvalidValue {
                        property: definition.id,
                        message: format!("'{value}' is not a phase mode"),
                    });
                }
            }
            Ok(())
        }
        (PHASE0, PropertyValue::Float(value)) => {
            current.phase0 = value;
            Ok(())
        }
        (PHASE1, PropertyValue::Float(value)) => {
            current.phase1 = value;
            Ok(())
        }
        (PIVOT, PropertyValue::Float(value)) => {
            current.pivot_frac = value;
            Ok(())
        }
        (_, value) => Err(wrong_kind(definition, &value, "the declared phase value")),
    }
}

fn mode_of(method: Option<AutoPhaseMethod>) -> &'static str {
    match method {
        None => MANUAL,
        Some(AutoPhaseMethod::RobustConsensus) => ROBUST_CONSENSUS,
        Some(AutoPhaseMethod::AbsorptivePeak) => ABSORPTIVE_PEAK,
        Some(AutoPhaseMethod::Entropy) => ENTROPY,
        Some(AutoPhaseMethod::NegativeMinimization) => NEGATIVE_MINIMIZATION,
        Some(AutoPhaseMethod::PeakRegression) => PEAK_REGRESSION,
    }
}

fn method_of(value: &str) -> Option<Option<AutoPhaseMethod>> {
    match value {
        MANUAL => Some(None),
        ROBUST_CONSENSUS => Some(Some(AutoPhaseMethod::RobustConsensus)),
        ABSORPTIVE_PEAK => Some(Some(AutoPhaseMethod::AbsorptivePeak)),
        ENTROPY => Some(Some(AutoPhaseMethod::Entropy)),
        NEGATIVE_MINIMIZATION => Some(Some(AutoPhaseMethod::NegativeMinimization)),
        PEAK_REGRESSION => Some(Some(AutoPhaseMethod::PeakRegression)),
        _ => None,
    }
}
