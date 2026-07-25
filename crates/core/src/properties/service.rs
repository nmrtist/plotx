//! Reading and writing catalog properties against the live document.
//!
//! One planner serves every entry point. A UI control, and later an automation
//! tool, both hand a typed [`PropertyValue`] to [`PlotxApp::plan_property_write`]
//! and receive a single atomic [`crate::actions::Action`] plus an explicit list
//! of the targets that were skipped and why.

use super::model::*;
use super::{contour, definition, permitted_variants, variant_list};
use crate::actions::Action;
use crate::automation::{ComponentRef, ResourceRef, TargetRef, canvas_object_ref};
use crate::state::{
    CanvasId, DataBinding, Dataset, FieldCapabilities, FieldId, ObjectId, PlotxApp,
    PresentationProfile, RequestedChart, SeriesId, default_contour_spec, default_encoding,
    field_peak_magnitude,
};
use plotx_figure::SeriesEncoding;

/// A target resolved down to the one series it names, plus everything
/// applicability and defaults need. Indices here are one-shot lookup positions
/// and never leave this struct.
pub(super) struct SeriesContext<'a> {
    canvas: usize,
    object: ObjectId,
    series: SeriesId,
    pub(super) dataset: &'a Dataset,
    pub(super) field: FieldId,
    capabilities: FieldCapabilities,
    pub(super) encoding: &'a SeriesEncoding,
}

impl PlotxApp {
    /// The address of one contour-style property on one series of a plot.
    pub fn series_target(
        &self,
        canvas: usize,
        object: ObjectId,
        series: SeriesId,
    ) -> Option<TargetRef> {
        let canvas_id = self.doc.canvases.get(canvas)?.resource_id;
        Some(TargetRef {
            resource: canvas_object_ref(canvas_id, object),
            component: Some(ComponentRef::Series(series)),
        })
    }

    /// Every series of one plot object, in binding order.
    pub fn series_targets(&self, canvas: usize, object: ObjectId) -> Vec<TargetRef> {
        let Some(canvas_document) = self.doc.canvases.get(canvas) else {
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

    /// Every series component of one plot-object *resource*.
    ///
    /// This is the expansion an automation plan performs: a selector names
    /// resources, and a property whose scope is a series has to become one
    /// target per series before anything can be said about applicability. It
    /// resolves the reference through the same lookup a property write does, so
    /// a plan and the commit that follows it cannot disagree about which
    /// components exist.
    pub fn resource_series_targets(&self, resource: &ResourceRef) -> Vec<TargetRef> {
        self.canvas_object(resource)
            .map(|(canvas, object)| self.series_targets(canvas, object))
            .unwrap_or_default()
    }

    /// Read one property against one target.
    pub fn resolve_property(
        &self,
        address: &PropertyAddress,
    ) -> Result<ResolvedProperty, PropertyError> {
        let definition = self.definition_for(address.definition)?;
        let context = self.series_context(&address.target, definition)?;
        let SeriesEncoding::Contour(spec) = context.encoding else {
            return Err(not_applicable_encoding(definition, context.encoding));
        };
        let value = contour::read(definition.id, spec)
            .ok_or_else(|| PropertyError::UnknownProperty(definition.id.as_str().to_owned()))?;
        let default_value = match definition.default_policy {
            DefaultPolicy::None => None,
            DefaultPolicy::Fixed(value) => Some(value),
            DefaultPolicy::EncodingFactory => self.factory_default(definition.id, &context),
        };
        let availability = match definition.access {
            PropertyAccess::ReadOnly => Availability::ReadOnly,
            PropertyAccess::ReadWrite => Availability::Editable,
        };
        Ok(ResolvedProperty {
            address: address.clone(),
            value,
            default_value,
            availability,
            schema: contour::resolved_schema(definition.id, spec)
                .unwrap_or_else(|| resolved_schema(definition, &context.capabilities)),
        })
    }

    /// Read one property across a selection, reporting both the aggregate value
    /// and every target the property does not apply to.
    pub fn resolve_property_set(
        &self,
        property: PropertyId,
        targets: &[TargetRef],
    ) -> ResolvedPropertySet {
        let mut applicable_targets = Vec::new();
        let mut skipped_targets = Vec::new();
        // Each target already answers with an aggregate over its own copies of
        // the setting, so folding the selection in with the same operation makes
        // an asymmetric ladder inside one target and a disagreement between two
        // targets compose rather than mask each other.
        let mut value = AggregateValue::Unavailable;
        for target in targets {
            let address = PropertyAddress::new(target.clone(), property);
            match self.resolve_property(&address) {
                Ok(resolved) => {
                    value = value.merge(resolved.value);
                    applicable_targets.push(address);
                }
                Err(error) => skipped_targets.push((target.clone(), error.to_string())),
            }
        }
        ResolvedPropertySet {
            applicable_targets,
            skipped_targets,
            value,
        }
    }

    /// Validate a write against every applicable target and fold it into one
    /// atomic action. Nothing is executed here, and a single validation failure
    /// aborts the whole commit rather than landing on part of the selection.
    pub fn plan_property_write(
        &self,
        property: PropertyId,
        targets: &[TargetRef],
        value: &PropertyValue,
    ) -> Result<PropertyCommit, PropertyError> {
        let definition = self.definition_for(property)?;
        if definition.access == PropertyAccess::ReadOnly {
            return Err(PropertyError::ReadOnly(property));
        }
        self.plan(definition, targets, &mut |spec, context| {
            let permitted = permitted_variants(&definition.value_schema, &context.capabilities);
            if let PropertyValue::Enum(choice) = value
                && !permitted.iter().any(|variant| variant.id == *choice)
            {
                // Name what this field does allow. A caller told only that its
                // choice is unavailable has to guess the next one, and the
                // permitted set is a fact about the target's capabilities that
                // no caller can derive from the static schema.
                return Err(PropertyError::InvalidValue {
                    property,
                    message: format!(
                        "'{choice}' needs a capability this field does not expose; this field allows {}",
                        variant_list(&permitted)
                    ),
                });
            }
            contour::write(property, spec, value, &|| {
                field_peak_magnitude(context.dataset, context.field)
            })
        })
    }

    /// Move one property one step along its own scale, from whatever each
    /// target currently holds.
    ///
    /// This is the entry point for direct-manipulation gestures. It does not
    /// compute a value somewhere else and hand it over: the gesture names a
    /// direction, and the step is taken inside the same planner, against the
    /// same working copy, and validated by the same rules as a value typed into
    /// the panel. A gesture therefore cannot become a second source of state,
    /// and cannot reach a value the panel would have rejected.
    pub fn plan_property_step(
        &self,
        property: PropertyId,
        targets: &[TargetRef],
        step: PropertyStep,
    ) -> Result<PropertyCommit, PropertyError> {
        let definition = self.definition_for(property)?;
        if definition.access == PropertyAccess::ReadOnly {
            return Err(PropertyError::ReadOnly(property));
        }
        self.plan(definition, targets, &mut |spec, _context| {
            contour::step(property, spec, step)
        })
    }

    /// Reset one property by re-deriving it from the default policy in each
    /// target's current context.
    pub fn plan_property_reset(
        &self,
        property: PropertyId,
        targets: &[TargetRef],
    ) -> Result<PropertyCommit, PropertyError> {
        let definition = self.definition_for(property)?;
        if definition.access == PropertyAccess::ReadOnly {
            return Err(PropertyError::ReadOnly(property));
        }
        self.plan(definition, targets, &mut |spec, context| {
            let value = match definition.default_policy {
                DefaultPolicy::Fixed(value) => value,
                DefaultPolicy::None => return Err(PropertyError::ReadOnly(property)),
                DefaultPolicy::EncodingFactory => {
                    self.factory_default(property, context)
                        .ok_or(PropertyError::InvalidValue {
                            property,
                            message: "the default factory has no single value for this setting"
                                .to_owned(),
                        })?
                }
            };
            contour::write(property, spec, &value, &|| {
                field_peak_magnitude(context.dataset, context.field)
            })
        })
    }

    /// Reset a whole encoding by calling the default factory again, replacing it
    /// with a freshly materialized concrete encoding.
    ///
    /// The caller names *which* encoding it is resetting, and a target drawn as
    /// anything else is skipped with a reason rather than rebuilt. A property
    /// write gets that filter for free — a control belongs to a definition, and
    /// the definition's applicability skips whatever it does not describe — but
    /// an encoding reset has no property to compare against, so the scope has to
    /// be part of the request. Without it, a reset offered inside one encoding's
    /// section reaches every series of the object: the heatmap underneath a
    /// contour would be silently rebuilt from defaults by a button that names
    /// only the contour, and reported back to the user as a contour.
    pub fn plan_encoding_reset(
        &self,
        encoding: EncodingKind,
        targets: &[TargetRef],
    ) -> Result<PropertyCommit, PropertyError> {
        let mut plan = BindingPlan::default();
        let mut skipped = Vec::new();
        let mut applied = Vec::new();
        for target in targets {
            let context = match self.series_context_unchecked(target) {
                Ok(context) => context,
                Err(error) => {
                    skipped.push((target.clone(), error.to_string()));
                    continue;
                }
            };
            let actual = EncodingKind::of(context.encoding);
            if actual != encoding {
                skipped.push((
                    target.clone(),
                    format!(
                        "this reset rebuilds a {} and this series is drawn as a {}",
                        encoding.as_str(),
                        actual.as_str()
                    ),
                ));
                continue;
            }
            let Some(descriptor) = context.dataset.field_descriptor(context.field) else {
                skipped.push((target.clone(), "the source field is gone".to_owned()));
                continue;
            };
            let requested = match context.encoding {
                SeriesEncoding::Line(_) => RequestedChart::Line,
                SeriesEncoding::Contour(_) => RequestedChart::Contour,
                SeriesEncoding::Heatmap(_) => RequestedChart::Heatmap,
                SeriesEncoding::Image(_) => RequestedChart::Image,
            };
            let encoding = default_encoding(
                &descriptor.capabilities,
                &descriptor.metadata,
                requested,
                &PresentationProfile::default(),
                &|| field_peak_magnitude(context.dataset, context.field),
            );
            let binding = plan.entry(self, context.canvas, context.object)?;
            let Some(series) = binding
                .series
                .iter_mut()
                .find(|series| series.id == context.series)
            else {
                skipped.push((target.clone(), "the series is gone".to_owned()));
                continue;
            };
            series.encoding = encoding;
            applied.push(PropertyAddress::new(target.clone(), PropertyId("encoding")));
        }
        Ok(PropertyCommit {
            action: plan.into_action(),
            applied,
            skipped,
        })
    }

    /// Execute a validated commit and report what was skipped. Returns the
    /// number of targets actually changed.
    pub fn commit_property(&mut self, commit: PropertyCommit) -> usize {
        let applied = commit.applied.len();
        self.execute_action(commit.action);
        applied
    }

    fn definition_for(
        &self,
        property: PropertyId,
    ) -> Result<&'static PropertyDefinition, PropertyError> {
        definition(property)
            .ok_or_else(|| PropertyError::UnknownProperty(property.as_str().to_owned()))
    }

    /// Shared planning body: resolve, validate, mutate a working copy, and fold
    /// every touched object into one action.
    fn plan(
        &self,
        definition: &'static PropertyDefinition,
        targets: &[TargetRef],
        edit: &mut dyn FnMut(
            &mut plotx_figure::ContourSpec,
            &SeriesContext<'_>,
        ) -> Result<(), PropertyError>,
    ) -> Result<PropertyCommit, PropertyError> {
        let mut plan = BindingPlan::default();
        let mut skipped = Vec::new();
        let mut applied = Vec::new();
        for target in targets {
            let context = match self.series_context(target, definition) {
                Ok(context) => context,
                Err(error) => {
                    skipped.push((target.clone(), error.to_string()));
                    continue;
                }
            };
            if !matches!(context.encoding, SeriesEncoding::Contour(_)) {
                skipped.push((
                    target.clone(),
                    not_applicable_encoding(definition, context.encoding).to_string(),
                ));
                continue;
            }
            let binding = plan.entry(self, context.canvas, context.object)?;
            let Some(series) = binding
                .series
                .iter_mut()
                .find(|series| series.id == context.series)
            else {
                skipped.push((target.clone(), "the series is gone".to_owned()));
                continue;
            };
            let SeriesEncoding::Contour(spec) = &mut series.encoding else {
                skipped.push((
                    target.clone(),
                    "the series is no longer a contour".to_owned(),
                ));
                continue;
            };
            // A failure here aborts the whole commit: the working copies are
            // dropped and no action is ever built, so nothing lands partially.
            edit(spec, &context)?;
            applied.push(PropertyAddress::new(target.clone(), definition.id));
        }
        Ok(PropertyCommit {
            action: plan.into_action(),
            applied,
            skipped,
        })
    }

    /// What the default policy resolves to for this property in the target's
    /// current context.
    ///
    /// The context is the target as it stands now, not only the field it draws:
    /// a setting whose meaning depends on another setting the user has changed
    /// is re-derived under that change (§8.1). The factory writes one ladder to
    /// both halves, so its answer is always `Uniform`; `None` reports the ways
    /// that can fail to hold — a target that is no longer a contour, an id the
    /// contour reader does not know, or a factory that grew an asymmetric
    /// default — instead of inventing a value to reset to.
    fn factory_default(
        &self,
        property: PropertyId,
        context: &SeriesContext<'_>,
    ) -> Option<PropertyValue> {
        let SeriesEncoding::Contour(current) = context.encoding else {
            return None;
        };
        let defaults = self.default_contour_spec_for(context);
        contour::default_for(property, &defaults, current, &|| {
            field_peak_magnitude(context.dataset, context.field)
        })?
        .uniform()
        .copied()
    }

    fn default_contour_spec_for(&self, context: &SeriesContext<'_>) -> plotx_figure::ContourSpec {
        default_contour_spec(&context.capabilities, &|| {
            field_peak_magnitude(context.dataset, context.field)
        })
    }

    /// Resolve a target and enforce the definition's applicability. The
    /// component shape is checked before any domain lookup happens, so a
    /// misaddressed property is rejected rather than reaching plot code.
    fn series_context(
        &self,
        target: &TargetRef,
        definition: &'static PropertyDefinition,
    ) -> Result<SeriesContext<'_>, PropertyError> {
        let actual = ComponentKind::of(target.component.as_ref());
        if actual != definition.applicability.component {
            return Err(PropertyError::ComponentKind {
                property: definition.id,
                expected: definition.applicability.component.as_str(),
                actual: actual.as_str(),
            });
        }
        let context = self.series_context_unchecked(target)?;
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

    /// Address resolution only: object, series, source field and capabilities.
    pub(super) fn series_context_unchecked(
        &self,
        target: &TargetRef,
    ) -> Result<SeriesContext<'_>, PropertyError> {
        let Some(ComponentRef::Series(series)) = target.component else {
            return Err(PropertyError::ComponentKind {
                property: PropertyId("series"),
                expected: ComponentKind::Series.as_str(),
                actual: ComponentKind::of(target.component.as_ref()).as_str(),
            });
        };
        let (canvas, object) = self.canvas_object(&target.resource)?;
        let plot = self.doc.canvases[canvas]
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
        let dataset = self
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

    /// Turn a canvas-object resource reference into a one-shot lookup position.
    fn canvas_object(&self, resource: &ResourceRef) -> Result<(usize, ObjectId), PropertyError> {
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
        let canvas = self.doc.canvas_index(canvas_id).ok_or_else(unknown)?;
        Ok((canvas, object))
    }
}

/// Working copies of every plot binding a commit touches, so two series of the
/// same object collapse into one action rather than two whose snapshots would
/// overwrite each other.
#[derive(Default)]
struct BindingPlan {
    entries: Vec<(usize, ObjectId, DataBinding, DataBinding)>,
}

impl BindingPlan {
    fn entry(
        &mut self,
        app: &PlotxApp,
        canvas: usize,
        object: ObjectId,
    ) -> Result<&mut DataBinding, PropertyError> {
        if let Some(index) = self
            .entries
            .iter()
            .position(|entry| entry.0 == canvas && entry.1 == object)
        {
            return Ok(&mut self.entries[index].3);
        }
        let binding = app
            .doc
            .canvases
            .get(canvas)
            .and_then(|canvas| canvas.object(object))
            .and_then(|object| object.plot())
            .map(|plot| plot.binding.clone())
            .ok_or_else(|| PropertyError::UnknownTarget(object.to_string()))?;
        self.entries
            .push((canvas, object, binding.clone(), binding));
        let last = self.entries.len() - 1;
        Ok(&mut self.entries[last].3)
    }

    fn into_action(self) -> Action {
        Action::Composite(
            self.entries
                .into_iter()
                .filter(|(_, _, before, after)| before != after)
                .map(|(canvas, object, before, after)| {
                    Action::set_data_binding(canvas, object, before, after)
                })
                .collect(),
        )
    }
}

fn not_applicable_encoding(
    definition: &'static PropertyDefinition,
    encoding: &SeriesEncoding,
) -> PropertyError {
    PropertyError::NotApplicable(format!(
        "{} applies to a contour, and this series is drawn as a {}",
        definition.canonical_label,
        EncodingKind::of(encoding).as_str()
    ))
}

fn resolved_schema(
    definition: &'static PropertyDefinition,
    capabilities: &FieldCapabilities,
) -> ResolvedSchema {
    match definition.value_schema {
        ValueSchema::Bool => ResolvedSchema::Bool,
        ValueSchema::Int { min, max } => ResolvedSchema::Int { min, max },
        ValueSchema::Float { bounds, log } => ResolvedSchema::Float {
            bounds,
            log,
            unit: "",
        },
        ValueSchema::Enum { .. } => ResolvedSchema::Enum {
            variants: permitted_variants(&definition.value_schema, capabilities),
        },
        ValueSchema::Color => ResolvedSchema::Color,
    }
}
