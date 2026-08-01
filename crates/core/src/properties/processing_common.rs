//! Shared addressing helpers for processing-property providers.

use super::target::dataset_steps;
use super::{
    ComponentKind, PropertyAddress, PropertyDefinition, PropertyError, PropertyId, PropertyValue,
    definition,
};
use crate::actions::DatasetProcessingState;
use crate::automation::{ComponentRef, TargetRef};
use crate::state::{Dataset, DatasetId, PhaseAxis, PlotxApp};
use plotx_processing::{ProcessingStep, Spectrum, StepId, StepKind, StepSource};

pub(super) struct StepContext<'a> {
    pub dataset_id: DatasetId,
    pub dataset: &'a Dataset,
    pub axis: PhaseAxis,
    pub step: &'a ProcessingStep,
    pub factory: Option<ProcessingStep>,
}

pub(super) fn property_definition(
    id: PropertyId,
) -> Result<&'static PropertyDefinition, PropertyError> {
    definition(id).ok_or_else(|| PropertyError::UnknownProperty(id.as_str().to_owned()))
}

pub(super) fn step_context<'a>(
    app: &'a PlotxApp,
    address: &PropertyAddress,
    definition: &'static PropertyDefinition,
    accepts: impl FnOnce(&StepKind) -> bool,
) -> Result<StepContext<'a>, PropertyError> {
    let actual = ComponentKind::of(address.target.component.as_ref());
    if actual != definition.applicability.component {
        return Err(PropertyError::ComponentKind {
            property: definition.id,
            expected: definition.applicability.component.as_str(),
            actual: actual.as_str(),
        });
    }
    let Some(ComponentRef::ProcessingStep(id)) = address.target.component else {
        return Err(PropertyError::ComponentKind {
            property: definition.id,
            expected: ComponentKind::ProcessingStep.as_str(),
            actual: actual.as_str(),
        });
    };
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
    let (axis, step) = dataset_steps(dataset)
        .find(|(_, step)| step.id == id)
        .ok_or_else(|| PropertyError::UnknownTarget(address.target.describe()))?;
    if !accepts(&step.kind) {
        return Err(PropertyError::NotApplicable(format!(
            "{} does not apply because step {} is {}, not the required processing step",
            definition.canonical_label,
            id.get(),
            step_kind_name(&step.kind)
        )));
    }
    let factory = factory_step(dataset, axis, step);
    Ok(StepContext {
        dataset_id,
        dataset,
        axis,
        step,
        factory,
    })
}

pub(super) fn factory_step(
    dataset: &Dataset,
    axis: PhaseAxis,
    step: &ProcessingStep,
) -> Option<ProcessingStep> {
    if step.source != StepSource::Default {
        return None;
    }
    dataset
        .factory_pipeline(axis)?
        .steps
        .into_iter()
        // Live 2D ids are owner-global while each detached axis template starts
        // at zero, so provenance matches the factory slot by typed step kind.
        .find(|candidate| same_step_kind(&candidate.kind, &step.kind))
}

fn same_step_kind(left: &StepKind, right: &StepKind) -> bool {
    std::mem::discriminant(left) == std::mem::discriminant(right)
}

pub(super) fn step_mut<'a>(
    state: &'a mut DatasetProcessingState,
    id: StepId,
    target: &TargetRef,
) -> Result<&'a mut ProcessingStep, PropertyError> {
    state
        .steps_mut()
        .find(|step| step.id == id)
        .ok_or_else(|| PropertyError::UnknownTarget(target.describe()))
}

pub(super) fn wrong_kind(
    definition: &'static PropertyDefinition,
    value: &PropertyValue,
    expected: &str,
) -> PropertyError {
    PropertyError::InvalidValue {
        property: definition.id,
        message: format!("expected {expected}, got {}", value.kind()),
    }
}

pub(super) fn no_step_gesture(definition: &'static PropertyDefinition) -> PropertyError {
    PropertyError::InvalidValue {
        property: definition.id,
        message: "this processing setting has no step gesture".to_owned(),
    }
}

pub(super) fn no_factory_default(definition: &'static PropertyDefinition) -> PropertyError {
    PropertyError::NotApplicable(format!(
        "{} was added by hand and has no factory setting to reset to",
        definition.canonical_label
    ))
}

pub(super) fn step_kind_name(kind: &StepKind) -> &'static str {
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

pub(super) fn raw_point_count(dataset: &Dataset, axis: PhaseAxis) -> usize {
    match dataset {
        Dataset::Nmr(n) => n.data.len(),
        Dataset::Nmr2D(n) => match axis {
            PhaseAxis::F1 => n.data.nus.as_ref().map_or_else(
                || plotx_processing::fft2::f1_increments(n.data.rows, n.data.quad),
                |nus| nus.grid,
            ),
            PhaseAxis::F2 | PhaseAxis::Direct => n.data.cols,
        },
        Dataset::Table(_)
        | Dataset::Electrophysiology(_)
        | Dataset::Afm(_)
        | Dataset::MassSpec(_) => 0,
    }
}

/// The real spectrum presented to one frequency-domain step. This reuses the
/// cached FFT result and the processing kernel's own step dispatcher, so schema
/// bounds follow prior binning and cleanup exactly without duplicating them.
pub(super) fn spectrum_before_step(context: &StepContext<'_>) -> Option<Spectrum> {
    let Dataset::Nmr(dataset) = context.dataset else {
        return None;
    };
    let mut spectrum = dataset.base.as_frequency()?.clone();
    for step in dataset
        .pipeline
        .steps
        .iter()
        .skip_while(|step| step.kind.at_or_before_fft())
    {
        if step.id == context.step.id {
            return Some(spectrum);
        }
        if step.enabled {
            plotx_processing::apply_freq_step(&mut spectrum, &step.kind);
        }
    }
    None
}
