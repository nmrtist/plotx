//! Deliver complete processing frames without starving their derived geometry.
use super::*;

impl ComputeService {
    pub fn try_drain(&mut self) -> Vec<Done> {
        self.dispatch_ready_processing();
        let mut out = std::mem::take(&mut self.failures);
        let waiting = std::mem::take(&mut self.completed_processing);
        let done: Vec<_> = self.done_rx.try_iter().chain(waiting).collect();
        let mut field_completions = std::collections::HashSet::new();
        for done in done {
            match &done {
                Done::EstimateField { key, .. } | Done::EstimateFieldFailed { key, .. } => {
                    field_completions.insert(key.source.field.resource);
                    self.field_runtime.finish_estimate_request(key);
                    out.push(done);
                    continue;
                }
                Done::BuildContour { key, .. } | Done::BuildContourFailed { key, .. } => {
                    field_completions.insert(key.source.field.resource);
                    self.field_runtime.finish_geometry_request(key);
                    out.push(done);
                    continue;
                }
                Done::Ilt { .. }
                | Done::Dosy { .. }
                | Done::Craft { .. }
                | Done::CraftFailed { .. }
                | Done::Processing2D { .. }
                | Done::Processing2DFailed { .. }
                | Done::Cancelled { .. }
                | Done::Failed { .. } => {}
            }
            let Some((dataset, kind, generation)) = done_identity(&done) else {
                continue;
            };
            let matching_active = self
                .active
                .get(&(dataset, kind))
                .filter(|active| active.generation == generation);
            if kind == ComputeKind::Processing2D && matching_active.is_none() {
                continue;
            }
            // A worker can send success immediately before cancellation. Check
            // the shared token again on the receiving side so explicit cancel,
            // Full/Reapply replacement, and dataset invalidation cannot install
            // that already-queued success.
            let cancelled_after_send =
                matching_active.is_some_and(|active| active.token.is_cancelled());
            if !cancelled_after_send
                && matches!(
                    done,
                    Done::Processing2D { .. } | Done::Processing2DFailed { .. }
                )
                && (self.field_runtime.has_in_flight_for(dataset)
                    || field_completions.contains(&dataset))
            {
                // Keep the active slot until delivery, bounding the pipeline to
                // one completed result and one newest deferred recipe. The next
                // processing pass may overlap geometry, but must not obsolete it.
                self.completed_processing.push(done);
                continue;
            }
            if matching_active.is_some() {
                self.active.remove(&(dataset, kind));
            }
            if !cancelled_after_send && !matches!(done, Done::Cancelled { .. }) {
                out.push(done);
            }
        }
        // A field completion may enqueue the next derived stage in the app.
        // `field_completions` keeps held results behind that boundary as well.
        self.dispatch_ready_processing();
        out.append(&mut self.failures);
        out
    }
}

fn done_identity(done: &Done) -> Option<(DatasetId, ComputeKind, u64)> {
    match done {
        Done::Ilt {
            dataset,
            generation,
            ..
        } => Some((*dataset, ComputeKind::Ilt, *generation)),
        Done::Dosy {
            dataset,
            generation,
            ..
        } => Some((*dataset, ComputeKind::Dosy, *generation)),
        Done::Craft {
            dataset,
            generation,
            ..
        }
        | Done::CraftFailed {
            dataset,
            generation,
            ..
        } => Some((*dataset, ComputeKind::Craft, *generation)),
        Done::Processing2D {
            dataset, version, ..
        }
        | Done::Processing2DFailed {
            dataset, version, ..
        } => Some((*dataset, ComputeKind::Processing2D, version.0)),
        Done::Cancelled {
            dataset,
            generation,
            kind,
        }
        | Done::Failed {
            dataset,
            generation,
            kind,
        } => Some((*dataset, *kind, *generation)),
        Done::EstimateField { .. }
        | Done::EstimateFieldFailed { .. }
        | Done::BuildContour { .. }
        | Done::BuildContourFailed { .. } => None,
    }
}
