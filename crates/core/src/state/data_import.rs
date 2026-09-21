//! Bounded, document-scoped background data preparation.
use super::{Dataset, PlotxApp};
use std::{collections::VecDeque, path::PathBuf, sync::mpsc};

pub(super) struct PreparedImport {
    pub dataset: Dataset,
    pub source: String,
    pub format: plotx_io::DataFormat,
    pub warnings: Vec<plotx_io::LoadWarning>,
}

#[cfg(test)]
#[path = "data_import_tests.rs"]
mod tests;

impl PreparedImport {
    pub(super) fn new(loaded: plotx_io::LoadResult, equal_scale: bool) -> Result<Self, String> {
        let (dataset, source) = crate::workflow::dataset_from_loaded_acquisition(
            loaded.acquisition,
            loaded.acquisition_identity,
            equal_scale,
        )
        .map_err(|error| error.to_string())?;
        Ok(Self {
            dataset,
            source,
            format: loaded.format,
            warnings: loaded.warnings,
        })
    }
}

type Discover = Box<dyn FnOnce() -> Result<Vec<PathBuf>, String> + Send>;
struct Request {
    recent: PathBuf,
    discover: Discover,
    equal_scale: bool,
}

enum Event {
    Started(PathBuf, usize),
    Item(PathBuf, Result<PreparedImport, String>),
    Finished,
}

struct Job {
    receiver: mpsc::Receiver<Event>,
    recent: PathBuf,
    loaded: usize,
    failed: usize,
}

/// Dropping a session disconnects the bounded channel. The worker then exits
/// without joining the UI thread or publishing results into the next document.
#[derive(Default)]
pub struct DataImports {
    pending: VecDeque<Request>,
    active: Option<Job>,
}

impl DataImports {
    pub fn is_pending(&self) -> bool {
        self.active.is_some() || !self.pending.is_empty()
    }
}

impl PlotxApp {
    /// Discovery runs off-thread; preparation uses at most four CPU workers.
    /// Additional gestures queue behind the active batch to bound memory use.
    pub fn queue_data_import(
        &mut self,
        recent: PathBuf,
        discover: impl FnOnce() -> Result<Vec<PathBuf>, String> + Send + 'static,
    ) {
        self.session.data_imports.pending.push_back(Request {
            recent,
            discover: Box::new(discover),
            equal_scale: self.settings.general.equal_scale_homonuclear_2d_imports,
        });
        self.session.status = "Data import queued; you can continue working.".into();
    }

    /// Call once per frame. Never drain the channel: each insertion gets its own
    /// frame even when the worker produces many small acquisitions immediately.
    pub fn poll_data_import(&mut self) -> bool {
        let mut imports = std::mem::take(&mut self.session.data_imports);
        if imports.active.is_none()
            && let Some(request) = imports.pending.pop_front()
        {
            let (sender, receiver) = mpsc::sync_channel(1);
            let recent = request.recent.clone();
            match std::thread::Builder::new()
                .name("data-import".into())
                .spawn(move || {
                    let paths = match (request.discover)() {
                        Ok(paths) => paths,
                        Err(error) => {
                            if sender
                                .send(Event::Item(request.recent, Err(error)))
                                .is_err()
                            {
                                return;
                            }
                            if sender.send(Event::Finished).is_err() {
                                return; // The owning document was closed.
                            }
                            return;
                        }
                    };
                    let workers = std::thread::available_parallelism()
                        .map_or(1, usize::from)
                        .min(4);
                    if !prepare_paths(&paths, &sender, workers, &|path| {
                        plotx_io::load_path(path)
                            .map_err(|error| error.to_string())
                            .and_then(|loaded| PreparedImport::new(loaded, request.equal_scale))
                    }) {
                        return;
                    }
                    // A disconnected receiver means the document was closed.
                    if sender.send(Event::Finished).is_err() {
                        // Document closure is normal cancellation, not an import failure.
                    }
                }) {
                Ok(_) => {
                    imports.active = Some(Job {
                        receiver,
                        recent,
                        loaded: 0,
                        failed: 0,
                    })
                }
                Err(error) => {
                    self.install_prepared_import(
                        &recent,
                        Err(format!("Could not start import worker: {error}")),
                    );
                }
            }
        }
        if let Some(job) = imports.active.as_mut() {
            match job.receiver.try_recv() {
                Ok(Event::Started(path, total)) => {
                    self.session.status = format!(
                        "Importing {}/{}: {} ({} loaded, {} failed)",
                        job.loaded + job.failed + 1,
                        total,
                        path.display(),
                        job.loaded,
                        job.failed
                    );
                }
                Ok(Event::Item(path, result)) => {
                    // Completion must not steal the user's current page or Data
                    // selection while they edit something else during the batch.
                    let active_canvas = self.session.active_canvas;
                    let selection = self.session.ui.data_selection.clone();
                    let view = self.session.view;
                    if self.install_prepared_import(&path, result) {
                        job.loaded += 1;
                    } else {
                        job.failed += 1;
                    }
                    self.session.active_canvas = active_canvas;
                    self.session.ui.data_selection = selection;
                    self.session.view = view;
                    self.session.status = format!(
                        "Importing: {} loaded, {} failed. {}",
                        job.loaded,
                        job.failed,
                        path.display()
                    );
                }
                Ok(Event::Finished) => {
                    self.session.status = format!(
                        "Import complete: {} loaded, {} failed.",
                        job.loaded, job.failed
                    );
                    if job.loaded > 0 {
                        self.note_recent_file(&job.recent);
                    }
                    imports.active = None;
                }
                Err(mpsc::TryRecvError::Empty) => {}
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.install_prepared_import(
                        &job.recent,
                        Err("The import worker stopped unexpectedly; retry the import.".into()),
                    );
                    imports.active = None;
                }
            }
        }
        let busy = imports.is_pending();
        self.session.data_imports = imports;
        busy
    }
}

/// Small windows bound both active processing and completed results awaiting the
/// UI. Publish in discovery order so scheduling cannot change board/undo order.
fn prepare_paths(
    paths: &[PathBuf],
    sender: &mpsc::SyncSender<Event>,
    workers: usize,
    prepare: &(impl Fn(&std::path::Path) -> Result<PreparedImport, String> + Sync),
) -> bool {
    for window in paths.chunks(workers.max(1)) {
        if sender
            .send(Event::Started(window[0].clone(), paths.len()))
            .is_err()
        {
            return false;
        }
        let connected = std::thread::scope(|scope| {
            let handles: Vec<_> = window
                .iter()
                .map(|path| {
                    let handle = std::thread::Builder::new()
                        .name("data-import-prepare".into())
                        .spawn_scoped(scope, move || prepare(path));
                    (path, handle)
                })
                .collect();
            let mut connected = true;
            for (path, handle) in handles {
                let result = match handle {
                    Ok(handle) => handle.join().unwrap_or_else(|_| {
                        Err("The import worker stopped unexpectedly; retry the import.".into())
                    }),
                    Err(error) => Err(format!("Could not start import worker: {error}")),
                };
                if connected && sender.send(Event::Item(path.clone(), result)).is_err() {
                    connected = false;
                }
            }
            connected
        });
        if !connected {
            return false;
        }
    }
    true
}
