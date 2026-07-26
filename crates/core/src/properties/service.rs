//! Selection-wide property planning and commit execution.
//!
//! Providers own one addressed target. This service is intentionally the only
//! place that loops over targets, reports skips, and produces an atomic action;
//! duplicating those responsibilities in each provider would create a second
//! planner for every property family.

use super::target::{
    app_target, canvas_object, document_target, processing_step_targets, series_context_unchecked,
    series_targets,
};
use super::{
    AggregateValue, ComponentKind, EditOp, EncodingKind, PropertyAccess, PropertyAddress,
    PropertyCommit, PropertyDefinition, PropertyError, PropertyId, PropertySkip, PropertyStep,
    PropertyTransaction, PropertyValue, ResolvedProperty, ResolvedPropertySet, SkipReason,
    definition, provider_for,
};
use crate::actions::{Action, DatasetProcessingState, PendingPropertyGesture};
use crate::automation::{ComponentRef, ResourceRef, TargetRef, canvas_object_ref};
use crate::state::{
    DatasetId, ObjectId, PlotxApp, PresentationProfile, RequestedChart, default_encoding,
    field_peak_magnitude,
};
use plotx_figure::SeriesEncoding;

impl PlotxApp {
    /// The target of a document-owned property. It deliberately has no canvas
    /// or object component: figure typography belongs to the document even
    /// when the document currently contains no pages.
    pub fn document_target(&self) -> TargetRef {
        document_target()
    }

    /// The singleton target for application-owned persistent preferences.
    pub fn app_target(&self) -> TargetRef {
        app_target()
    }

    /// The address of one series-owned property on one plot object.
    pub fn series_target(
        &self,
        canvas: usize,
        object: ObjectId,
        series: crate::state::SeriesId,
    ) -> Option<TargetRef> {
        let canvas_id = self.doc.canvases.get(canvas)?.resource_id;
        Some(TargetRef {
            resource: canvas_object_ref(canvas_id, object),
            component: Some(ComponentRef::Series(series)),
        })
    }

    /// Every series of one plot object, in binding order.
    pub fn series_targets(&self, canvas: usize, object: ObjectId) -> Vec<TargetRef> {
        series_targets(self, canvas, object)
    }

    /// Every series component of one plot-object resource.
    ///
    /// Automation selectors choose resources. A provider definition then
    /// determines the component shape the resource expands to; series are the
    /// only shape implemented at this point.
    pub fn resource_series_targets(&self, resource: &ResourceRef) -> Vec<TargetRef> {
        canvas_object(self, resource)
            .map(|(canvas, object)| self.series_targets(canvas, object))
            .unwrap_or_default()
    }

    /// Every processing-step component of one dataset resource.
    pub fn resource_processing_step_targets(&self, resource: &ResourceRef) -> Vec<TargetRef> {
        processing_step_targets(self, resource)
    }

    /// Expand one resource according to a definition's component shape.
    ///
    /// This is target-shape dispatch only. Providers still choose typed storage
    /// through [`PropertyTransaction`]; the service never chooses storage by
    /// scope kind.
    pub fn resource_property_targets(
        &self,
        resource: &ResourceRef,
        definition: &'static PropertyDefinition,
    ) -> Vec<TargetRef> {
        match definition.applicability.component {
            ComponentKind::None => vec![TargetRef::resource(resource.clone())],
            ComponentKind::Series => self.resource_series_targets(resource),
            ComponentKind::ProcessingStep => self.resource_processing_step_targets(resource),
        }
    }

    /// Read one property against one target through its registered provider.
    pub fn resolve_property(
        &self,
        address: &PropertyAddress,
    ) -> Result<ResolvedProperty, PropertyError> {
        self.definition_for(address.definition)?;
        let provider = provider_for(address.definition).ok_or_else(|| {
            PropertyError::UnknownProperty(address.definition.as_str().to_owned())
        })?;
        provider.read(self, address)
    }

    /// Read one property across a selection, reporting both the aggregate value
    /// and every target that cannot supply the property.
    pub fn resolve_property_set(
        &self,
        property: PropertyId,
        targets: &[TargetRef],
    ) -> ResolvedPropertySet {
        let mut applicable_targets = Vec::new();
        let mut skipped_targets = Vec::new();
        let mut value = AggregateValue::Unavailable;
        for target in targets {
            let address = PropertyAddress::new(target.clone(), property);
            match self.resolve_property(&address) {
                Ok(resolved) => {
                    value = value.merge(resolved.value);
                    applicable_targets.push(address);
                }
                Err(error) => {
                    skipped_targets.push(PropertySkip::from_error(target.clone(), &error))
                }
            }
        }
        ResolvedPropertySet {
            applicable_targets,
            skipped_targets,
            value,
        }
    }

    /// Read one property's display value through its owning provider.
    ///
    /// This is deliberately address-based rather than encoding-based: a new
    /// steppable encoding gets a readout by registering its provider, not by
    /// adding another `SeriesEncoding` branch to UI callers.
    pub fn property_readout(
        &self,
        address: &PropertyAddress,
    ) -> Result<super::PropertyReadout, PropertyError> {
        self.definition_for(address.definition)?;
        let provider = provider_for(address.definition).ok_or_else(|| {
            PropertyError::UnknownProperty(address.definition.as_str().to_owned())
        })?;
        provider.readout(self, address)
    }

    /// Validate a set operation across every applicable target and fold the
    /// result into one atomic action.
    pub fn plan_property_write(
        &self,
        property: PropertyId,
        targets: &[TargetRef],
        value: &PropertyValue,
    ) -> Result<PropertyCommit, PropertyError> {
        self.plan_edit(property, targets, EditOp::Set(*value))
    }

    /// Move a property along the scale its provider owns.
    pub fn plan_property_step(
        &self,
        property: PropertyId,
        targets: &[TargetRef],
        step: PropertyStep,
    ) -> Result<PropertyCommit, PropertyError> {
        self.plan_edit(property, targets, EditOp::Step(step))
    }

    /// Reset one property in each target's current context.
    pub fn plan_property_reset(
        &self,
        property: PropertyId,
        targets: &[TargetRef],
    ) -> Result<PropertyCommit, PropertyError> {
        self.plan_edit(property, targets, EditOp::Reset)
    }

    /// Reset a complete encoding through its existing default factory.
    ///
    /// This remains an encoding operation rather than a catalog provider
    /// operation: it deliberately names an encoding kind so resetting a contour
    /// cannot silently rebuild a heatmap stacked below it.
    pub fn plan_encoding_reset(
        &self,
        encoding: EncodingKind,
        targets: &[TargetRef],
    ) -> Result<PropertyCommit, PropertyError> {
        let mut transaction = PropertyTransaction::default();
        let mut skipped = Vec::new();
        let mut applied = Vec::new();
        for target in targets {
            let context = match series_context_unchecked(self, target) {
                Ok(context) => context,
                Err(error) => {
                    skipped.push(PropertySkip::from_error(target.clone(), &error));
                    continue;
                }
            };
            let actual = EncodingKind::of(context.encoding);
            if actual != encoding {
                skipped.push(PropertySkip::new(
                    target.clone(),
                    SkipReason::NotApplicable,
                    format!(
                        "this reset rebuilds a {} and this series is drawn as a {}",
                        encoding.as_str(),
                        actual.as_str()
                    ),
                ));
                continue;
            }
            let Some(descriptor) = context.dataset.field_descriptor(context.field) else {
                skipped.push(PropertySkip::new(
                    target.clone(),
                    SkipReason::TargetMissing,
                    "the source field is gone".to_owned(),
                ));
                continue;
            };
            let requested = match context.encoding {
                SeriesEncoding::Line(_) => RequestedChart::Line,
                SeriesEncoding::Contour(_) => RequestedChart::Contour,
                SeriesEncoding::Heatmap(_) => RequestedChart::Heatmap,
                SeriesEncoding::Image(_) => RequestedChart::Image,
            };
            let replacement = default_encoding(
                &descriptor.capabilities,
                &descriptor.metadata,
                requested,
                &PresentationProfile::default(),
                &|| field_peak_magnitude(context.dataset, context.field),
            );
            // Measured the same way the individual controls in this section
            // measure themselves, so a reset that finds the series already at
            // its factory encoding reports a skip instead of an update nobody
            // can see.
            transaction.begin_target();
            let binding = transaction.data_binding(self, context.canvas, context.object)?;
            let Some(series) = binding
                .series
                .iter_mut()
                .find(|series| series.id == context.series)
            else {
                skipped.push(PropertySkip::new(
                    target.clone(),
                    SkipReason::TargetMissing,
                    "the series is gone".to_owned(),
                ));
                continue;
            };
            series.encoding = replacement;
            if transaction.target_changed() {
                applied.push(PropertyAddress::new(target.clone(), PropertyId("encoding")));
            } else {
                skipped.push(PropertySkip::new(
                    target.clone(),
                    SkipReason::AlreadyAtValue,
                    format!(
                        "this {} is already what its factory produces",
                        encoding.as_str()
                    ),
                ));
            }
        }
        transaction.ensure_single_storage()?;
        Ok(transaction.into_commit(applied, skipped))
    }

    /// Execute a validated commit and report the number of targets that were
    /// actually changed.
    pub fn commit_property(&mut self, commit: PropertyCommit) -> usize {
        self.commit_property_with_persistence(commit, |app| app.persist_settings())
    }

    fn commit_property_with_persistence(
        &mut self,
        commit: PropertyCommit,
        persist: impl FnOnce(&mut PlotxApp) -> bool,
    ) -> usize {
        let applied = commit.applied.len();
        // A same-value write changes nothing, so its composite is empty and
        // `try_execute_action` would drop it anyway. Stopping here is about the
        // *report*: the caller is told the target was skipped and why, instead
        // of being told an update succeeded that it cannot tell apart from one
        // that moved a value.
        if applied != 0 {
            if let Some(action) = commit.document_action {
                self.execute_property_action(action);
            }
            if let Some(settings) = commit.app_preferences {
                self.apply_settings(settings);
                persist(self);
            }
        }
        applied
    }

    #[cfg(test)]
    pub(crate) fn commit_property_with_settings_writer(
        &mut self,
        commit: PropertyCommit,
        writer: impl FnOnce(&crate::settings::Settings) -> std::io::Result<()>,
    ) -> usize {
        self.commit_property_with_persistence(commit, |app| app.persist_settings_with(writer))
    }

    /// Open a continuous gesture on one catalog control.
    ///
    /// Between this call and [`Self::end_property_gesture`], commits are applied
    /// live but kept out of undo history; the gesture is recorded as one step
    /// when it closes. A drag that recorded a step per frame would fill the
    /// bounded history and leave the user unable to undo past it.
    pub fn begin_property_gesture(&mut self, property: PropertyId) {
        if self
            .session
            .ui
            .property_gesture
            .as_ref()
            .is_some_and(|gesture| gesture.property == property)
        {
            return;
        }
        self.end_property_gesture();
        self.session.ui.property_gesture = Some(PendingPropertyGesture {
            property,
            first: None,
            last: None,
            owns_processing_session: false,
        });
    }

    /// Close the open gesture, recording everything it did as one undo step.
    pub fn end_property_gesture(&mut self) {
        let Some(gesture) = self.session.ui.property_gesture.take() else {
            return;
        };
        if gesture.owns_processing_session {
            self.finish_processing_session();
        }
        let Some(first) = gesture.first else {
            return;
        };
        // `first` alone would undo the gesture but redo only its first frame.
        // Both bounds carry absolute snapshots, so the pair is the whole
        // gesture: reverting it restores the state before, applying it
        // reproduces the state after.
        let action = match gesture.last {
            Some(last) => Action::Composite(vec![first, last]),
            None => first,
        };
        if let Err(error) = self.record_applied_action(action) {
            self.session.status = error.to_string();
        }
    }

    /// Execute a catalog action, letting every store it touches keep its own
    /// commit rules.
    ///
    /// A processing recipe is why this exists. "Pause auto-recompute" is a
    /// judgement about when a recipe change becomes visible, and
    /// [`Self::commit_processing_edit`] is the one place that makes it. Running
    /// the composite through the generic executor would recompute immediately
    /// and never stage the pending edit, so the switch would silently do
    /// nothing for anything the catalog writes — and the catalog must not carry
    /// a second copy of the decision to avoid that.
    fn execute_property_action(&mut self, action: Action) {
        let mut recipes = Vec::new();
        let mut rest = Vec::new();
        split_property_action(action, &mut recipes, &mut rest);
        for (dataset, before, after) in recipes {
            let Some(index) = self.doc.dataset_index(dataset) else {
                continue;
            };
            // A gesture borrows the processing session, whose live edits are
            // already recorded once when it ends. Opening it here rather than
            // in the panel keeps the panel from having to know which store a
            // property writes to.
            if let Some(gesture) = self.session.ui.property_gesture.as_mut()
                && !gesture.owns_processing_session
                && self.session.ui.processing_session.is_none()
            {
                gesture.owns_processing_session = true;
                self.begin_processing_session(index);
            }
            self.commit_processing_edit(index, before, after);
        }
        if rest.is_empty() {
            return;
        }
        let action = Action::Composite(rest);
        let Some(gesture) = self.session.ui.property_gesture.as_mut() else {
            self.execute_action(action);
            return;
        };
        if gesture.first.is_none() {
            gesture.first = Some(action.clone());
        } else {
            gesture.last = Some(action.clone());
        }
        self.apply_action(&action);
        self.doc.dirty = true;
    }

    fn plan_edit(
        &self,
        property: PropertyId,
        targets: &[TargetRef],
        operation: EditOp,
    ) -> Result<PropertyCommit, PropertyError> {
        let definition = self.definition_for(property)?;
        if definition.access == PropertyAccess::ReadOnly {
            return Err(PropertyError::ReadOnly(property));
        }
        let provider = provider_for(property)
            .ok_or_else(|| PropertyError::UnknownProperty(property.as_str().to_owned()))?;
        let mut transaction = PropertyTransaction::default();
        let mut skipped = Vec::new();
        let mut applied = Vec::new();
        for target in targets {
            let address = PropertyAddress::new(target.clone(), property);
            transaction.begin_target();
            match provider.edit(self, &mut transaction, &address, operation) {
                Ok(()) if transaction.target_changed() => applied.push(address),
                Ok(()) => skipped.push(PropertySkip::new(
                    target.clone(),
                    SkipReason::AlreadyAtValue,
                    format!("{} already has that value", definition.canonical_label),
                )),
                Err(error) if skipped_target(&error) => {
                    transaction.rollback_target();
                    skipped.push(PropertySkip::from_error(target.clone(), &error));
                }
                // The transaction only contains working copies. Returning here
                // drops all of them, so a bad value can never land on a prefix
                // of a multi-target selection.
                Err(error) => return Err(error),
            }
        }
        transaction.ensure_single_storage()?;
        Ok(transaction.into_commit(applied, skipped))
    }

    fn definition_for(
        &self,
        property: PropertyId,
    ) -> Result<&'static super::PropertyDefinition, PropertyError> {
        definition(property)
            .ok_or_else(|| PropertyError::UnknownProperty(property.as_str().to_owned()))
    }
}

/// Separate the recipe edits of a catalog action from everything else, so each
/// half reaches the surface that owns its commit rules.
fn split_property_action(
    action: Action,
    recipes: &mut Vec<(DatasetId, DatasetProcessingState, DatasetProcessingState)>,
    rest: &mut Vec<Action>,
) {
    match action {
        Action::Composite(actions) => {
            for action in actions {
                split_property_action(action, recipes, rest);
            }
        }
        Action::UpdateDatasetProcessing {
            dataset,
            before,
            after,
        } => recipes.push((dataset, before, after)),
        other => rest.push(other),
    }
}

fn skipped_target(error: &PropertyError) -> bool {
    matches!(
        error,
        PropertyError::ComponentKind { .. }
            | PropertyError::UnknownTarget(_)
            | PropertyError::NotApplicable(_)
    )
}
