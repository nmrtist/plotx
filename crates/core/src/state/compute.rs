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
    Params2D, Preset2D, Processed2D, StackSpectrum, process_2d_cancellable, reapply_2d_cancellable,
};

use super::DatasetId;
use super::build_processed_figure_cancellable;
use crate::{IltParams, build_dosy_figure_cancellable, build_ilt_figure_cancellable};

/// Which heavy operation a job/result belongs to, keying the per-dataset
/// newest-generation map that rejects stale results.
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
        generation: u64,
        dataset: DatasetId,
        epoch: u64,
        token: Arc<AtomicBool>,
        input: ProcessingInput,
        params: Params2D,
        preset: Preset2D,
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
    generation: u64,
    dataset: DatasetId,
    epoch: u64,
    input: ProcessingInput,
    params: Params2D,
    preset: Preset2D,
}

struct ActiveJob {
    generation: u64,
    started_at: Instant,
    token: Arc<AtomicBool>,
    processing_input: Option<ProcessingInputKind>,
}

/// A finished computation handed back to the main thread. `generation` is
/// checked against the newest request before the result is installed.
pub enum Done {
    Ilt {
        generation: u64,
        dataset: DatasetId,
        epoch: u64,
        result: IltResult,
        params: IltParams,
        figure: Arc<Figure>,
    },
    Dosy {
        generation: u64,
        dataset: DatasetId,
        epoch: u64,
        result: DiffusionMap,
        figure: Arc<Figure>,
    },
    Processing2D {
        generation: u64,
        dataset: DatasetId,
        epoch: u64,
        base: Option<Processed2D>,
        processed: Processed2D,
        figure: Arc<Figure>,
        params: Params2D,
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
    pub fn request_2d_full(
        &mut self,
        dataset: DatasetId,
        epoch: u64,
        data: Arc<NmrData2D>,
        params: Params2D,
        preset: Preset2D,
    ) -> Vec<ComputeKind> {
        self.request_2d(dataset, epoch, ProcessingInput::Full(data), params, preset)
    }

    /// Queue a re-apply from the cached base. Returns the aborted analyses, as
    /// [`Self::request_2d_full`] does.
    pub fn request_2d_reapply(
        &mut self,
        dataset: DatasetId,
        epoch: u64,
        base: Processed2D,
        params: Params2D,
        preset: Preset2D,
    ) -> Vec<ComputeKind> {
        self.request_2d(
            dataset,
            epoch,
            ProcessingInput::Reapply(base),
            params,
            preset,
        )
    }

    fn request_2d(
        &mut self,
        dataset: DatasetId,
        epoch: u64,
        input: ProcessingInput,
        params: Params2D,
        preset: Preset2D,
    ) -> Vec<ComputeKind> {
        let input_kind = input.kind();
        let aborted = self.cancel_incompatible_for_processing(dataset, input_kind);
        let generation = self.next_generation(dataset, ComputeKind::Processing2D);
        self.deferred_processing.insert(
            dataset,
            DeferredProcessing {
                generation,
                dataset,
                epoch,
                input,
                params,
                preset,
            },
        );
        // Avoid waiting for the next UI poll when no processing job is active.
        self.dispatch_ready_processing();
        aborted
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
                    generation: request.generation,
                    started_at: Instant::now(),
                    token: Arc::clone(&token),
                    processing_input: Some(input_kind),
                },
            );
            if self
                .job_tx
                .send(Job::Process2D {
                    generation: request.generation,
                    dataset: request.dataset,
                    epoch: request.epoch,
                    token,
                    input: request.input,
                    params: request.params,
                    preset: request.preset,
                })
                .is_err()
            {
                self.cancel_failed_enqueue(dataset, ComputeKind::Processing2D, request.generation);
                self.failures.push(Done::Failed {
                    generation: request.generation,
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
            let (dataset, kind, generation) = done_identity(&done);
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
        !self.active.is_empty() || !self.deferred_processing.is_empty()
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
        for kind in [
            ComputeKind::Ilt,
            ComputeKind::Dosy,
            ComputeKind::Processing2D,
        ] {
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
        if cancelled {
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

fn run_job(job: Job) -> Done {
    match job {
        Job::Ilt {
            generation,
            dataset,
            epoch,
            token,
            stack,
            b_factors,
            d_grid,
            lambda,
            params,
            nucleus,
            source,
        } => {
            let cancelled = || token.load(Ordering::Relaxed);
            match ilt_map_cancellable(&*stack, &b_factors, &d_grid, lambda, &cancelled) {
                Some(result) if !cancelled() => {
                    let Some(figure) =
                        build_ilt_figure_cancellable(&result, &nucleus, &source, &cancelled)
                            .map(Arc::new)
                    else {
                        return Done::Cancelled {
                            generation,
                            dataset,
                            kind: ComputeKind::Ilt,
                        };
                    };
                    Done::Ilt {
                        generation,
                        dataset,
                        epoch,
                        result,
                        params,
                        figure,
                    }
                }
                None => Done::Cancelled {
                    generation,
                    dataset,
                    kind: ComputeKind::Ilt,
                },
                Some(_) => Done::Cancelled {
                    generation,
                    dataset,
                    kind: ComputeKind::Ilt,
                },
            }
        }
        Job::Dosy {
            generation,
            dataset,
            epoch,
            token,
            stack,
            values,
            meta,
            nucleus,
            source,
        } => {
            let cancelled = || token.load(Ordering::Relaxed);
            match diffusion_map_cancellable(&*stack, &values, &meta, 0.05, &cancelled) {
                Some(result) if !cancelled() => {
                    let Some(figure) =
                        build_dosy_figure_cancellable(&result, &nucleus, &source, &cancelled)
                            .map(Arc::new)
                    else {
                        return Done::Cancelled {
                            generation,
                            dataset,
                            kind: ComputeKind::Dosy,
                        };
                    };
                    Done::Dosy {
                        generation,
                        dataset,
                        epoch,
                        result,
                        figure,
                    }
                }
                None => Done::Cancelled {
                    generation,
                    dataset,
                    kind: ComputeKind::Dosy,
                },
                Some(_) => Done::Cancelled {
                    generation,
                    dataset,
                    kind: ComputeKind::Dosy,
                },
            }
        }
        Job::Process2D {
            generation,
            dataset,
            epoch,
            token,
            input,
            params,
            preset,
        } => {
            let cancelled = || token.load(Ordering::Relaxed);
            let (base, processed) = match input {
                ProcessingInput::Full(data) => {
                    let Some(base) = process_2d_cancellable(&data, &params, &cancelled) else {
                        return cancelled_done(generation, dataset);
                    };
                    let Some(processed) = reapply_2d_cancellable(&base, &params, &cancelled) else {
                        return cancelled_done(generation, dataset);
                    };
                    (Some(base), processed)
                }
                ProcessingInput::Reapply(base) => {
                    let Some(processed) = reapply_2d_cancellable(&base, &params, &cancelled) else {
                        return cancelled_done(generation, dataset);
                    };
                    (None, processed)
                }
            };
            if cancelled() {
                return cancelled_done(generation, dataset);
            }
            let Some(figure) =
                build_processed_figure_cancellable(&processed, preset, &cancelled).map(Arc::new)
            else {
                return cancelled_done(generation, dataset);
            };
            Done::Processing2D {
                generation,
                dataset,
                epoch,
                base,
                processed,
                figure,
                params,
            }
        }
    }
}

fn cancelled_done(generation: u64, dataset: DatasetId) -> Done {
    Done::Cancelled {
        generation,
        dataset,
        kind: ComputeKind::Processing2D,
    }
}

fn done_identity(done: &Done) -> (DatasetId, ComputeKind, u64) {
    match done {
        Done::Ilt {
            dataset,
            generation,
            ..
        } => (*dataset, ComputeKind::Ilt, *generation),
        Done::Dosy {
            dataset,
            generation,
            ..
        } => (*dataset, ComputeKind::Dosy, *generation),
        Done::Processing2D {
            dataset,
            generation,
            ..
        } => (*dataset, ComputeKind::Processing2D, *generation),
        Done::Cancelled {
            dataset,
            generation,
            kind,
        }
        | Done::Failed {
            dataset,
            generation,
            kind,
        } => (*dataset, *kind, *generation),
    }
}

#[cfg(test)]
mod tests;
