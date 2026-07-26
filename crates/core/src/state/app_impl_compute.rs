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

fn field_enqueue_error_status(error: FieldEnqueueError) -> String {
    match error {
        FieldEnqueueError::WorkersUnavailable => {
            "Background contour computation is unavailable in this session.".into()
        }
        FieldEnqueueError::VersionExhausted => {
            "Field runtime versions are exhausted; reopen PlotX to continue.".into()
        }
    }
}

impl PlotxApp {
    /// Register the immutable fields that entered this session with runtime
    /// versions before an action makes the dataset visible to rendering.
    pub(crate) fn register_loaded_dataset_fields(&mut self, dataset: &Dataset) -> bool {
        match self.session.compute.register_loaded_dataset_fields(dataset) {
            Ok(()) => true,
            Err(error) => {
                self.session.status = field_enqueue_error_status(error);
                false
            }
        }
    }

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
        self.request_ilt_map_with_params(dataset, self.explicit_ilt_input_for(dataset));
    }

    pub fn request_ilt_map_with_params(&mut self, dataset: usize, explicit: Option<IltParams>) {
        let params = self.resolve_ilt_params_for(dataset, explicit);
        if let Err(message) = crate::state::validate_ilt_params(params) {
            self.session.status = message;
            return;
        }
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
            axis.values.clone(),
            *meta,
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
                    provenance,
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
                    d2.ilt_provenance = Some(provenance);
                    d2.ilt_figure = Some(figure);
                    d2.dosy_provenance_warning = None;
                    if any {
                        d2.display = PseudoDisplay::DosyMap;
                    }
                    // The method, map and provenance above are persisted state and
                    // land on both branches; dirtying only the populated one would
                    // drop an empty result silently on close.
                    self.doc.dirty = true;
                    if any {
                        self.rebuild_canvases_for(dataset);
                        self.session.status = "Built ILT DOSY map.".into();
                    } else {
                        self.session.status = format!(
                            "ILT DOSY map is empty with λ = {} (legal range {}–{}): no columns are \
                             above the noise threshold.",
                            params.lambda,
                            crate::settings::MIN_ILT_LAMBDA,
                            crate::settings::MAX_ILT_LAMBDA
                        );
                    }
                }
                Done::Dosy {
                    generation,
                    dataset,
                    epoch,
                    result,
                    provenance,
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
                    d2.dosy_provenance = Some(provenance);
                    d2.dosy_figure = Some(figure);
                    d2.dosy_provenance_warning = None;
                    if any {
                        d2.dosy_method = DosyMethod::MonoExp;
                        d2.display = PseudoDisplay::DosyMap;
                    }
                    // See the ILT arm: the map and provenance are persisted and
                    // land regardless of whether any column fitted.
                    self.doc.dirty = true;
                    if any {
                        self.rebuild_canvases_for(dataset);
                        self.session.status = "Built DOSY map.".into();
                    } else {
                        self.session.status =
                            "DOSY map is empty: no columns fit above the noise threshold.".into();
                    }
                }
                Done::Processing2D {
                    dataset,
                    base,
                    processed,
                    fields,
                    params,
                    ..
                } => {
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
                    // ComputeService emits only the active processing completion.
                    // A Reapply result has no base to overwrite and may be shown
                    // while a newer recipe is queued; single-flight execution
                    // prevents out-of-order rollback.
                    // `params` may also lag `d2.params` for a paused edit, which is
                    // the intended display-trails-recipe contract.
                    if let Some(base) = base {
                        d2.base = base;
                        d2.base_params = params;
                        d2.base_stale = false;
                    }
                    d2.processed = processed;
                    d2.processed_figure =
                        std::sync::Arc::new(build_processed_figure(&d2.processed, d2.preset));
                    d2.invalidate_dosy_results(
                        "Processing changed and invalidated the selected DOSY map",
                    );
                    for field in fields {
                        self.session
                            .compute
                            .promote_field_version(field.source, field.summary);
                    }
                    self.recompute_integrals_2d_after_processing(dataset);
                    self.rebuild_canvases_for(dataset);
                    self.doc.dirty = true;
                    self.session.status = "Updated 2D processing.".into();
                }
                Done::EstimateField { key, result } => {
                    let dataset =
                        self.doc
                            .dataset_index(key.source.field.resource)
                            .filter(|&index| {
                                self.doc.datasets.get(index).is_some_and(|dataset| {
                                    dataset.has_field(key.source.field.field)
                                })
                            });
                    let current = dataset
                        .and_then(|_| self.session.compute.current_field_version(key.source.field));
                    if self.session.compute.finish_estimate(key, result, current)
                        && let Some(dataset) = dataset
                    {
                        // The completed job only populated a content-addressed
                        // cache. Rebuilding resolves each binding's current key;
                        // it never writes a worker result into a plot directly.
                        self.rebuild_canvases_for(dataset);
                    }
                }
                Done::EstimateFieldFailed { key, message } => {
                    let current = self
                        .doc
                        .dataset_index(key.source.field.resource)
                        .filter(|&index| {
                            self.doc
                                .datasets
                                .get(index)
                                .is_some_and(|dataset| dataset.has_field(key.source.field.field))
                        })
                        .and_then(|_| self.session.compute.current_field_version(key.source.field));
                    if current == Some(key.source.version) {
                        self.session.status =
                            format!("Field estimate could not be computed: {message}");
                    }
                }
                Done::BuildContour { key, geometry } => {
                    let dataset =
                        self.doc
                            .dataset_index(key.source.field.resource)
                            .filter(|&index| {
                                self.doc.datasets.get(index).is_some_and(|dataset| {
                                    dataset.has_field(key.source.field.field)
                                })
                            });
                    let current = dataset
                        .and_then(|_| self.session.compute.current_field_version(key.source.field));
                    if self.session.compute.finish_contour(key, geometry, current)
                        && let Some(dataset) = dataset
                    {
                        self.rebuild_canvases_for(dataset);
                    }
                }
                Done::BuildContourFailed { key, message } => {
                    let current = self
                        .doc
                        .dataset_index(key.source.field.resource)
                        .filter(|&index| {
                            self.doc
                                .datasets
                                .get(index)
                                .is_some_and(|dataset| dataset.has_field(key.source.field.field))
                        })
                        .and_then(|_| self.session.compute.current_field_version(key.source.field));
                    if current == Some(key.source.version) {
                        self.session.status =
                            format!("Contour geometry could not be computed: {message}");
                    }
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
        let dataset_id = d2.resource_id;
        let fields = [
            d2.field_catalog
                .id_for_key("nmr.real")
                .map(|field| ProcessingField {
                    field,
                    component: ProcessedFieldComponent::Real,
                }),
            d2.field_catalog
                .id_for_key("nmr.magnitude")
                .map(|field| ProcessingField {
                    field,
                    component: ProcessedFieldComponent::Magnitude,
                }),
        ]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();
        let outcome = if full {
            self.session.compute.request_2d_full(
                dataset_id,
                &fields,
                std::sync::Arc::clone(&d2.data),
                params,
            )
        } else {
            self.session
                .compute
                .request_2d_reapply(dataset_id, &fields, d2.base.clone(), params)
        };
        let aborted = match outcome {
            Ok(aborted) => aborted,
            Err(error) => {
                self.session.status = field_enqueue_error_status(error);
                return false;
            }
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
