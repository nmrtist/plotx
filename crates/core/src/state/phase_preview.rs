//! Gesture-only contour ladders. The processing result always retains full precision.
use super::*;
use plotx_figure::{ContourBasePolicy, ContourSpec, PositiveFiniteF64, SeriesEncoding};
use std::collections::HashMap;

#[derive(Default)]
pub(crate) struct PhasePreview {
    datasets: HashMap<DatasetId, Vec<FrozenLevels>>,
}

struct FrozenLevels {
    field: FieldRef,
    spec: ContourSpec,
    fixed: ContourSpec,
}

impl PhasePreview {
    pub(crate) fn discard(&mut self, resource: DatasetId) {
        self.datasets.remove(&resource);
    }

    pub(crate) fn peek(
        &self,
        source: VersionedFieldRef,
        spec: &ContourSpec,
        summary: FieldSummary,
    ) -> Option<ContourResolution> {
        let entry = self
            .datasets
            .get(&source.field.resource)?
            .iter()
            .find(|entry| {
                entry.field == source.field
                    && entry.spec.positive == spec.positive
                    && entry.spec.negative == spec.negative
            })?;
        Some(resolve_fixed(source, spec, &entry.fixed, summary))
    }

    pub(crate) fn resolve(
        &mut self,
        compute: &mut ComputeService,
        source: VersionedFieldRef,
        spec: &ContourSpec,
        summary: FieldSummary,
    ) -> ContourResolution {
        let frozen = self.datasets.get_mut(&source.field.resource);
        if let Some(levels) = frozen.as_ref().and_then(|entries| {
            entries.iter().find(|entry| {
                entry.field == source.field
                    && entry.spec.positive == spec.positive
                    && entry.spec.negative == spec.negative
            })
        }) {
            return resolve_fixed(source, spec, &levels.fixed, summary);
        }
        if let Some(entries) = frozen
            && let Some(fixed) = fixed_spec(compute, source, spec, summary)
        {
            let resolution = resolve_fixed(source, spec, &fixed, summary);
            entries.push(FrozenLevels {
                field: source.field,
                spec: spec.clone(),
                fixed,
            });
            return resolution;
        }
        resolve_contour_levels(source, spec, summary, |key| {
            compute.estimate_for(key).cloned()
        })
    }
}

fn resolve_fixed(
    source: VersionedFieldRef,
    authored: &ContourSpec,
    fixed: &ContourSpec,
    summary: FieldSummary,
) -> ContourResolution {
    let mut resolution = resolve_contour_levels(source, fixed, summary, |_| None);
    if let ContourResolution::Ready { unreachable, .. } = &mut resolution {
        unreachable.retain(|threshold| {
            let half = if threshold.negative {
                authored.negative.as_ref()
            } else {
                Some(&authored.positive)
            };
            half.is_some_and(|half| matches!(half.base, ContourBasePolicy::Absolute(_)))
        });
    }
    resolution
}

fn fixed_spec(
    compute: &mut ComputeService,
    source: VersionedFieldRef,
    spec: &ContourSpec,
    summary: FieldSummary,
) -> Option<ContourSpec> {
    let mut fixed = spec.clone();
    for (negative, level) in [
        (false, Some(&mut fixed.positive)),
        (true, fixed.negative.as_mut()),
    ] {
        let Some(level) = level else {
            continue;
        };
        let base = resolve_contour_base(
            source,
            level,
            summary,
            negative,
            &mut |key| compute.estimate_for(key).cloned(),
            &mut Vec::new(),
        )?;
        let base = PositiveFiniteF64::new(base).or_else(|| {
            // Zero measured scale uses the same fallback ladder as final
            // rendering. An initially absent sign borrows the field's peak so
            // rotating through zero can reveal its lobes during the gesture.
            let peak = if negative {
                -summary.min.get()
            } else {
                summary.max.get()
            };
            let peak = if peak > 0.0 {
                peak
            } else {
                summary.min.get().abs().max(summary.max.get().abs())
            };
            crate::contour_ladder::contour_level_ladder(base, peak, level)
                .levels
                .first()
                .and_then(|value| PositiveFiniteF64::new(*value))
        })?;
        level.base = ContourBasePolicy::Absolute(base);
    }
    Some(fixed)
}

impl PlotxApp {
    pub(super) fn thaw_phase_preview(&mut self, resource: DatasetId) -> bool {
        if self.phase_gesture_for(resource)
            || self.session.compute.blocking_work_for(resource) == Some(ComputeKind::Processing2D)
        {
            return false;
        }
        self.session
            .phase_preview
            .datasets
            .remove(&resource)
            .is_some()
    }

    fn phase_gesture_for(&self, dataset: DatasetId) -> bool {
        matches!(self.interaction(), Interaction::Phase(drag)
            if drag.dataset == dataset && drag.kind != PhaseDragKind::Pivot)
            || self
                .session
                .ui
                .property_gesture
                .as_ref()
                .is_some_and(|gesture| {
                    matches!(
                        gesture.property,
                        crate::properties::phase::PHASE0
                            | crate::properties::phase::PHASE1
                            | crate::properties::phase::PIVOT
                    )
                })
    }

    pub(super) fn prepare_phase_preview(&mut self, dataset: usize) {
        let Some(data) = self
            .doc
            .datasets
            .get(dataset)
            .filter(|data| data.as_nmr2d().is_some_and(Nmr2DDataset::is_true_2d))
        else {
            return;
        };
        let resource = data.resource_id();
        if !self.phase_gesture_for(resource)
            || self.session.phase_preview.datasets.contains_key(&resource)
        {
            return;
        }
        self.session
            .phase_preview
            .datasets
            .insert(resource, Vec::new());
        let series: Vec<_> = self
            .doc
            .canvases
            .iter()
            .flat_map(|canvas| &canvas.objects)
            .filter_map(|object| object.plot())
            .flat_map(|plot| {
                self.display_binding(plot.display_owner, &plot.binding)
                    .series
            })
            .filter(|series| series.visible && series.source.resource == resource)
            .collect();
        for series in series {
            let SeriesEncoding::Contour(spec) = series.encoding else {
                continue;
            };
            let field = FieldRef {
                resource,
                field: series.source.field,
            };
            let Some(version) = self.session.compute.current_field_version(field) else {
                continue;
            };
            let source = VersionedFieldRef { field, version };
            if let Some(summary) = self.session.compute.cached_field_summary(source) {
                self.session.phase_preview.resolve(
                    &mut self.session.compute,
                    source,
                    &spec,
                    summary,
                );
            }
        }
    }

    /// Releasing, cancelling or switching tools ends the frozen display policy.
    /// The last recipe is already in the single deferred slot; do not reprocess
    /// it merely to restore the normal final thresholds.
    pub(super) fn finish_phase_previews(&mut self) {
        let finished: Vec<_> = self
            .session
            .phase_preview
            .datasets
            .keys()
            .copied()
            .filter(|dataset| !self.phase_gesture_for(*dataset))
            .collect();
        for resource in finished {
            if !self.thaw_phase_preview(resource) {
                continue;
            }
            if let Some(dataset) = self.doc.dataset_index(resource) {
                self.rebuild_canvases_for(dataset);
            }
        }
    }
}
