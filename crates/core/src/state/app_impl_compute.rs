//! Async compute plumbing: handing heavy 2D processing and DOSY/ILT analysis to
//! the worker pool, and applying or reporting what comes back.

use super::*;

/// Name the work that actually blocks a requested analysis, rather than claiming
/// the analysis itself is already running whatever the dataset is busy with.
fn enqueue_error_status(error: EnqueueError) -> String {
    match error {
        EnqueueError::Busy(ComputeKind::Processing2D) => {
            "2D processing is still updating for this dataset; try again once it finishes.".into()
        }
        EnqueueError::Busy(ComputeKind::Ilt) => {
            "An ILT DOSY computation is already running for this dataset.".into()
        }
        EnqueueError::Busy(ComputeKind::Dosy) => {
            "A DOSY computation is already running for this dataset.".into()
        }
        EnqueueError::WorkersUnavailable => {
            "Background computation is unavailable in this session; the analysis was not started."
                .into()
        }
    }
}

impl PlotxApp {
    /// Async twin of `build_dosy_map_for`: same validation, but hand the heavy
    /// per-column diffusion fit to the compute worker instead of blocking the UI.
    pub fn request_dosy_map(&mut self, dataset: usize) {
        let Some(d2) = self.doc.datasets.get(dataset).and_then(Dataset::as_nmr2d) else {
            self.session.status = "DOSY maps need a diffusion dataset.".into();
            return;
        };
        if d2.data.diffusion.is_none() {
            self.session.status =
                "This dataset has no diffusion parameters (not a DOSY array).".into();
            return;
        }
        let (Processed2D::Stack(stack), Some(axis), Some(meta)) =
            (&d2.processed, &d2.data.pseudo_axis, &d2.data.diffusion)
        else {
            return;
        };
        let values = axis.values.clone();
        let meta = *meta;
        let nucleus = d2.data.direct.nucleus.clone();
        let source = stack.source.clone();
        let stack = stack.clone();
        let dataset_id = d2.resource_id;
        let outcome = self.session.compute.enqueue_dosy(
            dataset_id,
            self.session.dataset_epoch,
            stack,
            values,
            meta,
            nucleus,
            source,
        );
        self.session.status = match outcome {
            Ok(()) => "Computing DOSY map…".into(),
            Err(error) => enqueue_error_status(error),
        };
    }

    /// Async twin of `build_ilt_map_for`: same validation and input prep, but hand
    /// the heavy regularized inversion to the compute worker off the UI thread.
    pub fn request_ilt_map(&mut self, dataset: usize) {
        let params = self.session.ui.ilt_params;
        let Some(d2) = self.doc.datasets.get(dataset).and_then(Dataset::as_nmr2d) else {
            self.session.status = "ILT DOSY maps need a diffusion dataset.".into();
            return;
        };
        if d2.data.diffusion.is_none() {
            self.session.status =
                "This dataset has no diffusion parameters (not a DOSY array).".into();
            return;
        }
        let is_gradient = d2
            .data
            .pseudo_axis
            .as_ref()
            .map(|a| a.kind == plotx_io::PseudoKind::Gradient)
            .unwrap_or(false);
        if !is_gradient {
            self.session.status =
                "ILT DOSY needs a gradient-encoded ruler (this array is not gradient-encoded)."
                    .into();
            return;
        }
        let (Processed2D::Stack(stack), Some(axis), Some(meta)) =
            (&d2.processed, &d2.data.pseudo_axis, &d2.data.diffusion)
        else {
            return;
        };
        let b_factors: Vec<f64> = axis.values.iter().map(|&g| meta.b_factor(g)).collect();
        let d_grid = log_grid(params.d_min, params.d_max, params.n_grid);
        let nucleus = d2.data.direct.nucleus.clone();
        let source = stack.source.clone();
        let stack = stack.clone();
        let dataset_id = d2.resource_id;
        let outcome = self.session.compute.enqueue_ilt(
            dataset_id,
            self.session.dataset_epoch,
            stack,
            b_factors,
            d_grid,
            params.lambda,
            params,
            nucleus,
            source,
        );
        self.session.status = match outcome {
            Ok(()) => "Computing ILT DOSY map…".into(),
            Err(error) => enqueue_error_status(error),
        };
    }

    pub fn cancel_compute(&mut self, dataset: usize, kind: ComputeKind) -> bool {
        let Some(dataset_id) = self.doc.datasets.get(dataset).map(Dataset::resource_id) else {
            return false;
        };
        if !self.session.compute.cancel(dataset_id, kind) {
            return false;
        }
        self.session.status = match kind {
            ComputeKind::Ilt => "ILT DOSY computation cancelled.",
            ComputeKind::Dosy => "DOSY computation cancelled.",
            ComputeKind::Processing2D => "2D processing cancelled.",
        }
        .into();
        true
    }

    /// Drain finished compute jobs and apply the current ones. Stale results —
    /// superseded by a newer request for the same dataset+op — are dropped.
    /// Returns whether work is still outstanding (so the shell keeps repainting
    /// until it lands).
    pub fn poll_compute(&mut self) -> bool {
        for done in self.session.compute.try_drain() {
            match done {
                Done::Ilt {
                    generation,
                    dataset,
                    epoch,
                    result,
                    params,
                    figure,
                } => {
                    if epoch != self.session.dataset_epoch
                        || !self
                            .session
                            .compute
                            .is_current(dataset, ComputeKind::Ilt, generation)
                    {
                        continue;
                    }
                    let Some(dataset) = self.doc.dataset_index(dataset) else {
                        continue;
                    };
                    let any = result.amp.iter().flatten().any(|&a| a > 0.0);
                    let Some(d2) = self
                        .doc
                        .datasets
                        .get_mut(dataset)
                        .and_then(Dataset::as_nmr2d_mut)
                    else {
                        continue;
                    };
                    d2.dosy_method = DosyMethod::Ilt(params);
                    d2.ilt_map = Some(result);
                    d2.ilt_figure = Some(figure);
                    if any {
                        d2.display = PseudoDisplay::DosyMap;
                        self.rebuild_canvases_for(dataset);
                        self.doc.dirty = true;
                        self.session.status = "Built ILT DOSY map.".into();
                    } else {
                        self.session.status =
                            "ILT DOSY map is empty: no columns above the noise threshold.".into();
                    }
                }
                Done::Dosy {
                    generation,
                    dataset,
                    epoch,
                    result,
                    figure,
                } => {
                    if epoch != self.session.dataset_epoch
                        || !self
                            .session
                            .compute
                            .is_current(dataset, ComputeKind::Dosy, generation)
                    {
                        continue;
                    }
                    let Some(dataset) = self.doc.dataset_index(dataset) else {
                        continue;
                    };
                    let any = result.d.iter().any(|d| d.is_finite());
                    let Some(d2) = self
                        .doc
                        .datasets
                        .get_mut(dataset)
                        .and_then(Dataset::as_nmr2d_mut)
                    else {
                        continue;
                    };
                    d2.dosy_map = Some(result);
                    d2.dosy_figure = Some(figure);
                    if any {
                        d2.dosy_method = DosyMethod::MonoExp;
                        d2.display = PseudoDisplay::DosyMap;
                        self.rebuild_canvases_for(dataset);
                        self.doc.dirty = true;
                        self.session.status = "Built DOSY map.".into();
                    } else {
                        self.session.status =
                            "DOSY map is empty: no columns fit above the noise threshold.".into();
                    }
                }
                Done::Processing2D {
                    generation,
                    dataset,
                    epoch,
                    base,
                    processed,
                    figure,
                    params,
                } => {
                    if epoch != self.session.dataset_epoch
                        || (base.is_some()
                            && !self.session.compute.is_current(
                                dataset,
                                ComputeKind::Processing2D,
                                generation,
                            ))
                    {
                        continue;
                    }
                    let Some(dataset) = self.doc.dataset_index(dataset) else {
                        continue;
                    };
                    let Some(d2) = self
                        .doc
                        .datasets
                        .get_mut(dataset)
                        .and_then(Dataset::as_nmr2d_mut)
                    else {
                        continue;
                    };
                    // Full results replace the cached base and therefore pass the
                    // strict generation check above. A Reapply result has no base
                    // to overwrite and may be shown while a newer recipe is queued;
                    // single-flight execution prevents out-of-order rollback.
                    // `params` may also lag `d2.params` for a paused edit, which is
                    // the intended display-trails-recipe contract.
                    if let Some(base) = base {
                        d2.base = base;
                        d2.base_params = params;
                        d2.base_stale = false;
                    }
                    d2.processed = processed;
                    d2.processed_figure = figure;
                    d2.dosy_map = None;
                    d2.ilt_map = None;
                    d2.dosy_figure = None;
                    d2.ilt_figure = None;
                    self.recompute_integrals_2d_after_processing(dataset);
                    self.rebuild_canvases_for(dataset);
                    self.doc.dirty = true;
                    self.session.status = "Updated 2D processing.".into();
                }
                Done::Cancelled { .. } => {}
                Done::Failed { dataset, kind, .. } => {
                    let name = self
                        .doc
                        .dataset_index(dataset)
                        .and_then(|index| self.doc.datasets.get(index))
                        .map_or_else(|| "the dataset".to_owned(), Dataset::display_name);
                    self.session.status = format!(
                        "{} for {name} could not be started; background computation is \
                         unavailable in this session.",
                        kind.label(),
                    );
                }
            }
        }
        self.session.compute.is_busy()
    }

    /// Whether any async compute is still outstanding. Checked by the shell after
    /// rendering, so a job enqueued this frame keeps the repaint loop alive until
    /// its result lands (rather than relying on egui to repaint again on its own).
    pub fn compute_busy(&self) -> bool {
        self.session.compute.is_busy()
    }

    /// Queue the latest 2D recipe for off-thread processing. Repeated calls are
    /// coalesced by `ComputeService`; a time-domain change requests a new base,
    /// while a frequency-only change shares the immutable cached base.
    pub fn schedule_2d_processing(&mut self, dataset: usize, force_full: bool) -> bool {
        let Some(d2) = self.doc.datasets.get(dataset).and_then(Dataset::as_nmr2d) else {
            return false;
        };
        // `base_stale` covers a mutation of `data` itself, which the recipe
        // comparison cannot see. It stays set until a fresh base lands, so an
        // intervening frequency-only edit cannot downgrade the pending retransform
        // to a re-apply and strand the reconstruction.
        let full = force_full
            || d2.base_stale
            || plotx_processing::needs_retransform_2d(&d2.params, &d2.base_params);
        let params = d2.params.clone();
        let preset = d2.preset;
        let dataset_id = d2.resource_id;
        let aborted = if full {
            self.session.compute.request_2d_full(
                dataset_id,
                self.session.dataset_epoch,
                std::sync::Arc::clone(&d2.data),
                params,
                preset,
            )
        } else {
            self.session.compute.request_2d_reapply(
                dataset_id,
                self.session.dataset_epoch,
                d2.base.clone(),
                params,
                preset,
            )
        };
        self.session.status = match aborted.first() {
            Some(kind) => format!(
                "Updating 2D processing… the running {} was cancelled because its input changed.",
                kind.label(),
            ),
            None => "Updating 2D processing…".into(),
        };
        true
    }
}
