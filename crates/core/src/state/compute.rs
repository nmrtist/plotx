use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use plotx_analysis::diffusion::{DiffusionMap, diffusion_map_cancellable};
use plotx_analysis::ilt::{IltResult, ilt_map_cancellable};
use plotx_figure::Figure;
use plotx_io::{DiffusionMeta, NmrData2D};
use plotx_processing::{
    Params2D, Processed2D, StackSpectrum, process_2d_cancellable, reapply_2d_cancellable,
};

use super::{
    ContourGeometry, ContourGeometryCacheKey, EstimateKey, EstimateResult, FieldId, FieldRef,
    FieldRuntime, FieldSummary, FieldVersion, ScalarGrid2D, VersionedFieldRef, nmr_scalar_grid,
};
use super::{DatasetId, DosyResultProvenance};
use crate::{IltParams, build_dosy_figure_cancellable, build_ilt_figure_cancellable};

#[path = "compute_field.rs"]
mod compute_field;
pub(crate) use compute_field::FieldEnqueueError;
#[path = "compute_worker.rs"]
mod compute_worker;
use compute_worker::run_job;

/// Which user-visible heavy operation is running. ILT/DOSY retain their own
/// generation guard; scalar field artifacts use `FieldVersion` and
/// content-addressed caches instead.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ComputeKind {
    Ilt,
    Dosy,
    Processing2D,
}

impl ComputeKind {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Ilt => "ILT DOSY computation",
            Self::Dosy => "DOSY computation",
            Self::Processing2D => "2D processing",
        }
    }
}

/// Why a user-initiated analysis could not be started.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EnqueueError {
    /// Another computation for this dataset must finish first; its input would be
    /// invalidated by the one already running.
    Busy(ComputeKind),
    /// The worker pool is gone, so no background work can run this session.
    WorkersUnavailable,
}

/// Which scalar payload a processing result populates. The field identity is
/// carried beside it, so real and magnitude never accidentally share a version
/// or geometry cache entry.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ProcessedFieldComponent {
    Real,
    Magnitude,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ProcessingField {
    pub field: FieldId,
    pub component: ProcessedFieldComponent,
}

#[derive(Clone, Copy, Debug)]
struct VersionedProcessingField {
    source: VersionedFieldRef,
    component: ProcessedFieldComponent,
}

#[derive(Clone, Copy, Debug)]
pub struct ProcessedFieldArtifact {
    pub source: VersionedFieldRef,
    pub summary: Option<FieldSummary>,
}

enum Job {
    Ilt {
        generation: u64,
        dataset: DatasetId,
        epoch: u64,
        token: Arc<AtomicBool>,
        stack: Arc<StackSpectrum>,
        b_factors: Vec<f64>,
        d_grid: Vec<f64>,
        lambda: f64,
        params: IltParams,
        /// Raw ruler and metadata kept beside the derived b-factors so the worker
        /// can fingerprint the same inputs the per-column path does.
        values: Vec<f64>,
        meta: DiffusionMeta,
        nucleus: String,
        source: String,
    },
    Dosy {
        generation: u64,
        dataset: DatasetId,
        epoch: u64,
        token: Arc<AtomicBool>,
        stack: Arc<StackSpectrum>,
        values: Vec<f64>,
        meta: DiffusionMeta,
        nucleus: String,
        source: String,
    },
    Process2D {
        version: FieldVersion,
        dataset: DatasetId,
        token: Arc<AtomicBool>,
        input: ProcessingInput,
        params: Params2D,
        fields: Vec<VersionedProcessingField>,
    },
    EstimateField {
        key: EstimateKey,
        grid: Arc<ScalarGrid2D>,
    },
    BuildContour {
        key: ContourGeometryCacheKey,
        grid: Arc<ScalarGrid2D>,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ProcessingInputKind {
    Full,
    Reapply,
}

enum ProcessingInput {
    Full(Arc<NmrData2D>),
    Reapply(Processed2D),
}

impl ProcessingInput {
    const fn kind(&self) -> ProcessingInputKind {
        match self {
            Self::Full(_) => ProcessingInputKind::Full,
            Self::Reapply(_) => ProcessingInputKind::Reapply,
        }
    }
}

struct DeferredProcessing {
    version: FieldVersion,
    dataset: DatasetId,
    input: ProcessingInput,
    params: Params2D,
    fields: Vec<VersionedProcessingField>,
}

struct ActiveJob {
    generation: u64,
    started_at: Instant,
    token: Arc<AtomicBool>,
    processing_input: Option<ProcessingInputKind>,
}

/// A finished computation handed back to the main thread. ILT/DOSY generations
/// are checked against their newest request; field-derived results validate the
/// `FieldVersion` embedded in their content-addressed key.
pub enum Done {
    Ilt {
        generation: u64,
        dataset: DatasetId,
        epoch: u64,
        result: IltResult,
        params: IltParams,
        provenance: DosyResultProvenance,
        figure: Arc<Figure>,
    },
    Dosy {
        generation: u64,
        dataset: DatasetId,
        epoch: u64,
        result: DiffusionMap,
        provenance: DosyResultProvenance,
        figure: Arc<Figure>,
    },
    Processing2D {
        version: FieldVersion,
        dataset: DatasetId,
        base: Option<Processed2D>,
        processed: Processed2D,
        fields: Vec<ProcessedFieldArtifact>,
        params: Params2D,
    },
    EstimateField {
        key: EstimateKey,
        result: EstimateResult,
    },
    EstimateFieldFailed {
        key: EstimateKey,
        message: String,
    },
    BuildContour {
        key: ContourGeometryCacheKey,
        geometry: ContourGeometry,
    },
    BuildContourFailed {
        key: ContourGeometryCacheKey,
        message: String,
    },
    Cancelled {
        generation: u64,
        dataset: DatasetId,
        kind: ComputeKind,
    },
    /// A request could not be handed to a worker. Reported through the same queue
    /// as real results so the failure reaches application state rather than
    /// leaving the caller waiting on work that will never run.
    Failed {
        generation: u64,
        dataset: DatasetId,
        kind: ComputeKind,
    },
}

/// Off-thread runner for heavy pseudo-2D analysis and 2D processing. Every job
/// carries a cooperative cancellation token. Cached-base reapplications run
/// single-flight: the active result may be displayed while one deferred slot is
/// overwritten with the newest recipe. Full retransforms retain strict
/// cancellation and generation checks because they replace the cached base.
pub struct ComputeService {
    job_tx: Sender<Job>,
    done_rx: Receiver<Done>,
    next_gen: u64,
    latest: HashMap<(DatasetId, ComputeKind), u64>,
    active: HashMap<(DatasetId, ComputeKind), ActiveJob>,
    deferred_processing: HashMap<DatasetId, DeferredProcessing>,
    field_runtime: FieldRuntime,
    /// Dispatch failures awaiting collection by `try_drain`.
    failures: Vec<Done>,
}

impl ComputeService {
    #[allow(clippy::new_without_default)]
    pub fn new() -> Self {
        let (job_tx, job_rx) = std::sync::mpsc::channel::<Job>();
        let (done_tx, done_rx) = std::sync::mpsc::channel::<Done>();
        let job_rx = Arc::new(Mutex::new(job_rx));
        let worker_count = thread::available_parallelism()
            .map(|count| count.get())
            .unwrap_or(2)
            .clamp(2, 4);
        for _ in 0..worker_count {
            let job_rx = Arc::clone(&job_rx);
            let done_tx = done_tx.clone();
            thread::spawn(move || worker_loop(job_rx, done_tx));
        }
        Self {
            job_tx,
            done_rx,
            next_gen: 0,
            latest: HashMap::new(),
            active: HashMap::new(),
            deferred_processing: HashMap::new(),
            field_runtime: FieldRuntime::default(),
            failures: Vec::new(),
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn enqueue_ilt(
        &mut self,
        dataset: DatasetId,
        epoch: u64,
        stack: Arc<StackSpectrum>,
        b_factors: Vec<f64>,
        d_grid: Vec<f64>,
        lambda: f64,
        params: IltParams,
        values: Vec<f64>,
        meta: DiffusionMeta,
        nucleus: String,
        source: String,
    ) -> Result<(), EnqueueError> {
        if let Some(kind) = self.blocking_work_for(dataset) {
            return Err(EnqueueError::Busy(kind));
        }
        let generation = self.next_generation(dataset, ComputeKind::Ilt);
        let token = Arc::new(AtomicBool::new(false));
        self.active.insert(
            (dataset, ComputeKind::Ilt),
            ActiveJob {
                generation,
                started_at: Instant::now(),
                token: Arc::clone(&token),
                processing_input: None,
            },
        );
        let sent = self
            .job_tx
            .send(Job::Ilt {
                generation,
                dataset,
                epoch,
                token,
                stack,
                b_factors,
                d_grid,
                lambda,
                params,
                values,
                meta,
                nucleus,
                source,
            })
            .is_ok();
        if !sent {
            self.cancel_failed_enqueue(dataset, ComputeKind::Ilt, generation);
            return Err(EnqueueError::WorkersUnavailable);
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub fn enqueue_dosy(
        &mut self,
        dataset: DatasetId,
        epoch: u64,
        stack: Arc<StackSpectrum>,
        values: Vec<f64>,
        meta: DiffusionMeta,
        nucleus: String,
        source: String,
    ) -> Result<(), EnqueueError> {
        if let Some(kind) = self.blocking_work_for(dataset) {
            return Err(EnqueueError::Busy(kind));
        }
        let generation = self.next_generation(dataset, ComputeKind::Dosy);
        let token = Arc::new(AtomicBool::new(false));
        self.active.insert(
            (dataset, ComputeKind::Dosy),
            ActiveJob {
                generation,
                started_at: Instant::now(),
                token: Arc::clone(&token),
                processing_input: None,
            },
        );
        let sent = self
            .job_tx
            .send(Job::Dosy {
                generation,
                dataset,
                epoch,
                token,
                stack,
                values,
                meta,
                nucleus,
                source,
            })
            .is_ok();
        if !sent {
            self.cancel_failed_enqueue(dataset, ComputeKind::Dosy, generation);
            return Err(EnqueueError::WorkersUnavailable);
        }
        Ok(())
    }

    /// Queue a retransform-from-FID. Returns the user-initiated analyses this
    /// request aborted, so the caller can say so.
    pub(crate) fn request_2d_full(
        &mut self,
        dataset: DatasetId,
        fields: &[ProcessingField],
        data: Arc<NmrData2D>,
        params: Params2D,
    ) -> Result<Vec<ComputeKind>, FieldEnqueueError> {
        self.request_2d(dataset, fields, ProcessingInput::Full(data), params)
    }

    /// Queue a re-apply from the cached base. Returns the aborted analyses, as
    /// [`Self::request_2d_full`] does.
    pub(crate) fn request_2d_reapply(
        &mut self,
        dataset: DatasetId,
        fields: &[ProcessingField],
        base: Processed2D,
        params: Params2D,
    ) -> Result<Vec<ComputeKind>, FieldEnqueueError> {
        self.request_2d(dataset, fields, ProcessingInput::Reapply(base), params)
    }

    fn request_2d(
        &mut self,
        dataset: DatasetId,
        fields: &[ProcessingField],
        input: ProcessingInput,
        params: Params2D,
    ) -> Result<Vec<ComputeKind>, FieldEnqueueError> {
        let input_kind = input.kind();
        let aborted = self.cancel_incompatible_for_processing(dataset, input_kind);
        let mut versioned_fields = Vec::with_capacity(fields.len());
        for field in fields {
            let version = self.reserve_field_version()?;
            versioned_fields.push(VersionedProcessingField {
                source: VersionedFieldRef {
                    field: FieldRef {
                        resource: dataset,
                        field: field.field,
                    },
                    version,
                },
                component: field.component,
            });
        }
        let version = versioned_fields
            .first()
            .map(|field| field.source.version)
            .ok_or(FieldEnqueueError::VersionExhausted)?;
        self.deferred_processing.insert(
            dataset,
            DeferredProcessing {
                version,
                dataset,
                input,
                params,
                fields: versioned_fields,
            },
        );
        // Avoid waiting for the next UI poll when no processing job is active.
        self.dispatch_ready_processing();
        Ok(aborted)
    }

    fn next_generation(&mut self, dataset: DatasetId, kind: ComputeKind) -> u64 {
        let generation = self.next_gen;
        self.next_gen = self.next_gen.wrapping_add(1);
        self.latest.insert((dataset, kind), generation);
        generation
    }

    fn cancel_failed_enqueue(&mut self, dataset: DatasetId, kind: ComputeKind, generation: u64) {
        self.active.remove(&(dataset, kind));
        if self.latest.get(&(dataset, kind)) == Some(&generation) {
            self.latest.remove(&(dataset, kind));
        }
    }

    fn dispatch_ready_processing(&mut self) {
        let ready: Vec<DatasetId> = self
            .deferred_processing
            .keys()
            .filter(|dataset| {
                !self
                    .active
                    .contains_key(&(**dataset, ComputeKind::Processing2D))
            })
            .copied()
            .collect();
        for dataset in ready {
            let Some(request) = self.deferred_processing.remove(&dataset) else {
                continue;
            };
            let token = Arc::new(AtomicBool::new(false));
            let input_kind = request.input.kind();
            self.active.insert(
                (dataset, ComputeKind::Processing2D),
                ActiveJob {
                    generation: request.version.0,
                    started_at: Instant::now(),
                    token: Arc::clone(&token),
                    processing_input: Some(input_kind),
                },
            );
            if self
                .job_tx
                .send(Job::Process2D {
                    version: request.version,
                    dataset: request.dataset,
                    token,
                    input: request.input,
                    params: request.params,
                    fields: request.fields,
                })
                .is_err()
            {
                self.cancel_failed_enqueue(dataset, ComputeKind::Processing2D, request.version.0);
                self.failures.push(Done::Failed {
                    generation: request.version.0,
                    dataset,
                    kind: ComputeKind::Processing2D,
                });
            }
        }
    }

    pub fn try_drain(&mut self) -> Vec<Done> {
        self.dispatch_ready_processing();
        let mut out = std::mem::take(&mut self.failures);
        while let Ok(done) = self.done_rx.try_recv() {
            match &done {
                Done::EstimateField { key, .. } | Done::EstimateFieldFailed { key, .. } => {
                    self.field_runtime.finish_estimate_request(key);
                    out.push(done);
                    continue;
                }
                Done::BuildContour { key, .. } | Done::BuildContourFailed { key, .. } => {
                    self.field_runtime.finish_geometry_request(key);
                    out.push(done);
                    continue;
                }
                Done::Ilt { .. }
                | Done::Dosy { .. }
                | Done::Processing2D { .. }
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
            // A worker can send success immediately before cancellation. Check
            // the shared token again on the receiving side so explicit cancel,
            // Full/Reapply replacement, and dataset invalidation cannot install
            // that already-queued success.
            let cancelled_after_send =
                matching_active.is_some_and(|active| active.token.load(Ordering::Relaxed));
            if matching_active.is_some() {
                self.active.remove(&(dataset, kind));
            }
            if !cancelled_after_send && !matches!(done, Done::Cancelled { .. }) {
                out.push(done);
            }
        }
        self.dispatch_ready_processing();
        out.append(&mut self.failures);
        out
    }

    pub fn is_busy(&self) -> bool {
        !self.active.is_empty()
            || !self.deferred_processing.is_empty()
            || self.field_runtime.has_in_flight()
    }

    pub fn progress(&self, dataset: DatasetId, kind: ComputeKind) -> Option<Duration> {
        self.active.get(&(dataset, kind)).and_then(|active| {
            (!active.token.load(Ordering::Relaxed)).then(|| active.started_at.elapsed())
        })
    }

    /// Return the active DOSY computation regardless of which method the UI is
    /// currently displaying. Only one method may run per dataset at a time.
    pub fn dosy_progress(&self, dataset: DatasetId) -> Option<(ComputeKind, Duration)> {
        [ComputeKind::Dosy, ComputeKind::Ilt]
            .into_iter()
            .find_map(|kind| self.progress(dataset, kind).map(|elapsed| (kind, elapsed)))
    }

    /// The work that would invalidate a new analysis for `dataset`, if any. A job
    /// already cancelled does not count even though its entry lives until the
    /// worker acknowledges: `progress` reports it as gone, so blocking on it would
    /// reject a re-run against a computation the user cannot see or wait for.
    pub fn blocking_work_for(&self, dataset: DatasetId) -> Option<ComputeKind> {
        if self.deferred_processing.contains_key(&dataset) {
            return Some(ComputeKind::Processing2D);
        }
        self.active
            .iter()
            .find(|((active_dataset, _), active)| {
                *active_dataset == dataset && !active.token.load(Ordering::Relaxed)
            })
            .map(|((_, kind), _)| *kind)
    }

    /// Cancel work whose input is invalidated by a new processing request.
    /// Reapply-to-Reapply is the sole compatible pair: it uses an immutable
    /// cached base, so the active preview may finish while the deferred slot is
    /// replaced with the newest recipe. Any pair involving Full remains strict
    /// because Full may replace that cached base.
    fn cancel_incompatible_for_processing(
        &mut self,
        dataset: DatasetId,
        requested_input: ProcessingInputKind,
    ) -> Vec<ComputeKind> {
        let mut aborted = Vec::new();
        for ((active_dataset, kind), active) in &self.active {
            if *active_dataset != dataset {
                continue;
            }
            let compatible_reapply = *kind == ComputeKind::Processing2D
                && active.processing_input == Some(ProcessingInputKind::Reapply)
                && requested_input == ProcessingInputKind::Reapply;
            if compatible_reapply {
                continue;
            }
            let running = !active.token.swap(true, Ordering::Relaxed);
            if running && *kind != ComputeKind::Processing2D {
                aborted.push(*kind);
            }
        }
        for kind in [ComputeKind::Ilt, ComputeKind::Dosy] {
            self.latest.remove(&(dataset, kind));
        }
        aborted
    }

    pub fn cancel(&mut self, dataset: DatasetId, kind: ComputeKind) -> bool {
        let mut cancelled = false;
        if let Some(active) = self.active.get(&(dataset, kind)) {
            active.token.store(true, Ordering::Relaxed);
            cancelled = true;
        }
        if kind == ComputeKind::Processing2D && self.deferred_processing.remove(&dataset).is_some()
        {
            cancelled = true;
        }
        if cancelled && kind != ComputeKind::Processing2D {
            self.latest.remove(&(dataset, kind));
        }
        cancelled
    }

    pub fn is_current(&self, dataset: DatasetId, kind: ComputeKind, generation: u64) -> bool {
        self.latest.get(&(dataset, kind)) == Some(&generation)
    }
}

fn worker_loop(job_rx: Arc<Mutex<Receiver<Job>>>, done_tx: Sender<Done>) {
    loop {
        let job = {
            let Ok(receiver) = job_rx.lock() else {
                break;
            };
            let Ok(job) = receiver.recv() else {
                break;
            };
            job
        };
        let done = run_job(job);
        if done_tx.send(done).is_err() {
            break;
        }
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
        Done::Processing2D {
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

#[cfg(test)]
mod tests;

#[cfg(test)]
#[path = "contour_budget_tests.rs"]
mod contour_budget_tests;
