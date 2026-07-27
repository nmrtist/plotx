//! Shared target resolution for series-owned property providers.

use super::{
    ComponentKind, EncodingKind, PropertyDefinition, PropertyError, ResolvedSchema, ValueSchema,
    permitted_variants,
};
use crate::automation::{ComponentRef, ResourceRef, TargetRef, canvas_object_ref};
use crate::state::{
    CanvasId, Dataset, DatasetId, FieldCapabilities, FieldId, ObjectId, PlotxApp, SeriesId,
};
use plotx_figure::SeriesEncoding;
use plotx_processing::ProcessingStep;

/// The singleton project document is the implicit root above all persisted
/// resources. It has no separately persisted UUID because a `PlotxApp` owns
/// exactly one document; the stable root token is therefore sufficient inside
/// that document's target space.
pub(crate) fn document_target() -> TargetRef {
    TargetRef::resource(ResourceRef {
        id: crate::automation::DOCUMENT_RESOURCE_ID.to_owned(),
        kind: crate::automation::ResourceKindId::new(crate::automation::KIND_DOCUMENT),
        parent_id: None,
        local_id: None,
    })
}

pub(crate) fn canvas_target(id: CanvasId) -> TargetRef {
    TargetRef::resource(ResourceRef::from(id))
}

pub(crate) fn app_target() -> TargetRef {
    TargetRef::resource(ResourceRef {
        id: crate::automation::APP_RESOURCE_ID.to_owned(),
        kind: crate::automation::ResourceKindId::new(crate::automation::KIND_APP),
        parent_id: None,
        local_id: None,
    })
}

pub(crate) fn require_canvas_target(
    app: &PlotxApp,
    target: &TargetRef,
    definition: &'static PropertyDefinition,
) -> Result<CanvasId, PropertyError> {
    let actual = ComponentKind::of(target.component.as_ref());
    if actual != ComponentKind::None {
        return Err(PropertyError::ComponentKind {
            property: definition.id,
            expected: ComponentKind::None.as_str(),
            actual: actual.as_str(),
        });
    }
    if target.resource.kind.0 != crate::automation::KIND_CANVAS {
        return Err(PropertyError::NotApplicable(format!(
            "{} belongs to a canvas, not {}",
            definition.canonical_label, target.resource.id
        )));
    }
    let unknown = || PropertyError::UnknownTarget(target.resource.id.clone());
    let id = CanvasId::try_from(&target.resource).map_err(|_| unknown())?;
    app.doc.canvas_index(id).ok_or_else(unknown)?;
    Ok(id)
}

pub(crate) fn require_app_target(
    target: &TargetRef,
    definition: &'static PropertyDefinition,
) -> Result<(), PropertyError> {
    let actual = ComponentKind::of(target.component.as_ref());
    if actual != ComponentKind::None {
        return Err(PropertyError::ComponentKind {
            property: definition.id,
            expected: ComponentKind::None.as_str(),
            actual: actual.as_str(),
        });
    }
    if target.resource.kind.0 != crate::automation::KIND_APP
        || target.resource.id != crate::automation::APP_RESOURCE_ID
    {
        return Err(PropertyError::NotApplicable(format!(
            "{} belongs to app preferences, not {}",
            definition.canonical_label, target.resource.id
        )));
    }
    Ok(())
}

pub(crate) fn require_document_target(
    target: &TargetRef,
    definition: &'static PropertyDefinition,
) -> Result<(), PropertyError> {
    let actual = ComponentKind::of(target.component.as_ref());
    if actual != ComponentKind::None {
        return Err(PropertyError::ComponentKind {
            property: definition.id,
            expected: ComponentKind::None.as_str(),
            actual: actual.as_str(),
        });
    }
    if target.resource.kind.0 != crate::automation::KIND_DOCUMENT
        || target.resource.id != crate::automation::DOCUMENT_RESOURCE_ID
    {
        return Err(PropertyError::NotApplicable(format!(
            "{} belongs to the document, not {}",
            definition.canonical_label, target.resource.id
        )));
    }
    Ok(())
}

/// A target resolved to one series plus the field facts providers need.
/// Indices are one-shot lookup positions and never leave this module.
pub(crate) struct SeriesContext<'a> {
    pub(crate) canvas: usize,
    pub(crate) object: ObjectId,
    pub(crate) series: SeriesId,
    pub(crate) dataset: &'a Dataset,
    pub(crate) field: FieldId,
    pub(crate) capabilities: FieldCapabilities,
    pub(crate) encoding: &'a SeriesEncoding,
}

pub(crate) fn series_context<'a>(
    app: &'a PlotxApp,
    target: &TargetRef,
    definition: &'static PropertyDefinition,
) -> Result<SeriesContext<'a>, PropertyError> {
    let actual = ComponentKind::of(target.component.as_ref());
    if actual != definition.applicability.component {
        return Err(PropertyError::ComponentKind {
            property: definition.id,
            expected: definition.applicability.component.as_str(),
            actual: actual.as_str(),
        });
    }
    let context = series_context_unchecked(app, target)?;
    let missing = definition
        .applicability
        .required_capabilities
        .iter()
        .find(|capability| !context.capabilities.contains(capability));
    if let Some(missing) = missing {
        return Err(PropertyError::NotApplicable(format!(
            "{} needs a field that exposes {missing}",
            definition.canonical_label
        )));
    }
    if let Some(expected) = definition.applicability.encoding
        && EncodingKind::of(context.encoding) != expected
    {
        return Err(not_applicable_encoding(definition, context.encoding));
    }
    Ok(context)
}

pub(crate) fn series_context_unchecked<'a>(
    app: &'a PlotxApp,
    target: &TargetRef,
) -> Result<SeriesContext<'a>, PropertyError> {
    let Some(ComponentRef::Series(series)) = target.component else {
        return Err(PropertyError::ComponentKind {
            property: super::PropertyId("series"),
            expected: ComponentKind::Series.as_str(),
            actual: ComponentKind::of(target.component.as_ref()).as_str(),
        });
    };
    let (canvas, object) = canvas_object(app, &target.resource)?;
    let plot = app.doc.canvases[canvas]
        .object(object)
        .and_then(|object| object.plot())
        .ok_or_else(|| PropertyError::UnknownTarget(target.resource.id.clone()))?;
    let binding = plot
        .binding
        .series
        .iter()
        .find(|binding| binding.id == series)
        .ok_or_else(|| {
            PropertyError::UnknownTarget(format!("{}/series/{series}", target.resource.id))
        })?;
    let dataset = app
        .doc
        .dataset_by_id(binding.source.resource)
        .ok_or_else(|| PropertyError::UnknownTarget(binding.source.resource.to_string()))?;
    let capabilities = dataset
        .field_descriptor(binding.source.field)
        .map(|descriptor| descriptor.capabilities)
        .unwrap_or_default();
    Ok(SeriesContext {
        canvas,
        object,
        series,
        dataset,
        field: binding.source.field,
        capabilities,
        encoding: &binding.encoding,
    })
}

pub(crate) fn canvas_object(
    app: &PlotxApp,
    resource: &ResourceRef,
) -> Result<(usize, ObjectId), PropertyError> {
    let unknown = || PropertyError::UnknownTarget(resource.id.clone());
    if resource.kind.0 != crate::automation::KIND_CANVAS_OBJECT {
        return Err(PropertyError::NotApplicable(format!(
            "expected a plot object, got {}",
            resource.kind.0
        )));
    }
    let parent = resource.parent_id.as_deref().ok_or_else(unknown)?;
    let local = resource.local_id.as_deref().ok_or_else(unknown)?;
    let canvas_id: CanvasId = parent.parse().map_err(|_| unknown())?;
    let object: ObjectId = local.parse().map_err(|_| unknown())?;
    let canvas = app.doc.canvas_index(canvas_id).ok_or_else(unknown)?;
    Ok((canvas, object))
}

pub(crate) fn require_plot_object_target(
    app: &PlotxApp,
    target: &TargetRef,
    definition: &'static PropertyDefinition,
) -> Result<(usize, ObjectId), PropertyError> {
    let actual = ComponentKind::of(target.component.as_ref());
    if actual != ComponentKind::None {
        return Err(PropertyError::ComponentKind {
            property: definition.id,
            expected: ComponentKind::None.as_str(),
            actual: actual.as_str(),
        });
    }
    let (canvas, object) = canvas_object(app, &target.resource)?;
    app.doc.canvases[canvas]
        .object(object)
        .and_then(|object| object.plot())
        .ok_or_else(|| {
            PropertyError::NotApplicable(format!(
                "{} belongs to a plot object",
                definition.canonical_label
            ))
        })?;
    Ok((canvas, object))
}

pub(crate) fn require_object_target(
    app: &PlotxApp,
    target: &TargetRef,
    definition: &'static PropertyDefinition,
) -> Result<(usize, ObjectId), PropertyError> {
    let actual = ComponentKind::of(target.component.as_ref());
    if actual != ComponentKind::None {
        return Err(PropertyError::ComponentKind {
            property: definition.id,
            expected: ComponentKind::None.as_str(),
            actual: actual.as_str(),
        });
    }
    let (canvas, object) = canvas_object(app, &target.resource)?;
    app.doc.canvases[canvas]
        .object(object)
        .ok_or_else(|| PropertyError::UnknownTarget(target.resource.id.clone()))?;
    Ok((canvas, object))
}

pub(crate) fn series_targets(app: &PlotxApp, canvas: usize, object: ObjectId) -> Vec<TargetRef> {
    let Some(canvas_document) = app.doc.canvases.get(canvas) else {
        return Vec::new();
    };
    let resource = canvas_object_ref(canvas_document.resource_id, object);
    canvas_document
        .object(object)
        .and_then(|object| object.plot())
        .map(|plot| {
            plot.binding
                .series
                .iter()
                .map(|series| TargetRef {
                    resource: resource.clone(),
                    component: Some(ComponentRef::Series(series.id)),
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Every processing step one dataset exposes, with the axis that owns it.
///
/// Which axes exist, and which of them currently have a recipe at all, is asked
/// of the dataset's own neutral pipeline API rather than re-derived from its
/// variant. `phase_axes` already withholds F1 from a dataset that is not truly
/// two-dimensional, and `axis_pipeline` already withholds it until the indirect
/// dimension has been transformed; a second derivation here would hand the
/// catalog steps the processing panel never shows and the user cannot navigate
/// to, and report writing to them as a success.
pub(crate) fn dataset_steps(
    dataset: &Dataset,
) -> impl Iterator<Item = (crate::state::PhaseAxis, &ProcessingStep)> {
    dataset
        .phase_axes()
        .iter()
        .filter_map(move |&axis| Some((axis, dataset.axis_pipeline(axis)?)))
        .flat_map(|(axis, pipeline)| pipeline.steps.iter().map(move |step| (axis, step)))
}

/// Whether a dataset holds any component the property catalog can address.
///
/// This is the admission question behind `CAP_PROPERTY_CATALOG`, asked once so
/// the capability and the target expansion cannot disagree about what "has
/// addressable components" means.
pub(crate) fn dataset_has_property_components(dataset: &Dataset) -> bool {
    dataset_steps(dataset).next().is_some()
}

/// Every processing-step component owned by one dataset resource.
///
/// The provider sees only a stable `StepId`; axis placement is intentionally
/// resolved here as a one-shot lookup and never crosses an action or persistence
/// boundary as an index.
pub(crate) fn processing_step_targets(app: &PlotxApp, resource: &ResourceRef) -> Vec<TargetRef> {
    let Ok(dataset_id) = DatasetId::try_from(resource) else {
        return Vec::new();
    };
    let Some(dataset) = app.doc.dataset_by_id(dataset_id) else {
        return Vec::new();
    };
    dataset_steps(dataset)
        .map(|(_, step)| TargetRef {
            resource: resource.clone(),
            component: Some(ComponentRef::ProcessingStep(step.id)),
        })
        .collect()
}

pub(crate) fn not_applicable_encoding(
    definition: &'static PropertyDefinition,
    encoding: &SeriesEncoding,
) -> PropertyError {
    let expected = definition
        .applicability
        .encoding
        .map(EncodingKind::as_str)
        .unwrap_or("this target");
    PropertyError::NotApplicable(format!(
        "{} applies to a {expected}, and this series is drawn as a {}",
        definition.canonical_label,
        EncodingKind::of(encoding).as_str()
    ))
}

pub(crate) fn resolved_schema(
    definition: &'static PropertyDefinition,
    capabilities: &FieldCapabilities,
) -> ResolvedSchema {
    match definition.value_schema {
        ValueSchema::Bool => ResolvedSchema::Bool,
        ValueSchema::Text => ResolvedSchema::Text,
        ValueSchema::Int { min, max } => ResolvedSchema::Int { min, max, unit: "" },
        ValueSchema::IntWithDrag {
            min,
            max,
            drag_step,
        } => ResolvedSchema::IntWithDrag {
            min,
            max,
            drag_step,
            unit: "",
        },
        ValueSchema::SteppedInt {
            min,
            max,
            step,
            drag_step,
        } => ResolvedSchema::SteppedInt {
            min,
            max,
            step,
            drag_step,
            unit: "",
        },
        ValueSchema::Float {
            bounds, display, ..
        } => ResolvedSchema::Float { bounds, display },
        ValueSchema::Enum { .. } => ResolvedSchema::Enum {
            variants: permitted_variants(&definition.value_schema, capabilities),
        },
        ValueSchema::Color => ResolvedSchema::Color,
    }
}
