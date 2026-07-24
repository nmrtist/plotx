//! Multi-peak line-fitting orchestration: seed from peak marks (or detect),
//! fit through `plotx_analysis::lineshape`, store the result on the dataset,
//! and materialize its parameter table on request as a separate undoable step.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc};
use std::time::{Duration, Instant};

use super::*;
use plotx_analysis::lineshape::{LineFit, fit_lineshapes, fit_lineshapes_cancellable, seed_peaks};
use plotx_analysis::peaks::{DetectParams, detect_peaks, estimate_noise};

const MAX_AUTO_SEEDS: usize = 10;
const MAX_FIT_SEEDS: usize = 24;
const AUTO_SEED_SIGMA_MULT: f64 = 3.0;

/// An in-flight background line fit. Runtime-only: never serialized, not part
/// of the undo history; the undoable step is committed when the result lands.
pub struct LineFitJob {
    dataset: usize,
    epoch: u64,
    lo: f64,
    hi: f64,
    started_at: Instant,
    cancel: Arc<AtomicBool>,
    rx: mpsc::Receiver<Option<LineFit>>,
}

struct PreparedFit {
    lo: f64,
    hi: f64,
    xs: Vec<f64>,
    ys: Vec<f64>,
    positions: Vec<f64>,
}

impl PlotxApp {
    /// Worker behind `SetLineFits`: install the fits and rebuild so the figure
    /// overlays repaint.
    pub fn set_line_fits(&mut self, dataset: usize, fits: &[StoredLineFit]) {
        if let Some(stored) = self
            .doc
            .datasets
            .get_mut(dataset)
            .and_then(Dataset::line_fits_mut)
        {
            *stored = fits.to_vec();
        }
        self.rebuild_canvases_for(dataset);
    }

    /// Validate the window and resolve the trace points and seed positions —
    /// the cheap synchronous prelude shared by the blocking and background paths.
    fn prepare_line_fit(&self, dataset: usize, lo: f64, hi: f64) -> Result<PreparedFit, String> {
        let (lo, hi) = if lo <= hi { (lo, hi) } else { (hi, lo) };
        let Some(trace) = self
            .doc
            .datasets
            .get(dataset)
            .and_then(|d| d.displayed_trace(None))
        else {
            return Err("Peak fitting needs a dataset with a 1D trace.".to_owned());
        };
        let mut xs = Vec::new();
        let mut ys = Vec::new();
        for (&x, &y) in trace.xs.iter().zip(&trace.ys) {
            if x >= lo && x <= hi {
                xs.push(x);
                ys.push(y);
            }
        }
        if xs.len() < 4 {
            return Err("The selected range contains too few points to fit.".to_owned());
        }

        let mut positions: Vec<f64> = self.doc.datasets[dataset]
            .peaks()
            .map(|p| p.resolve())
            .unwrap_or_default()
            .iter()
            .map(|p| p.x)
            .filter(|&x| x >= lo && x <= hi)
            .collect();
        if positions.is_empty() {
            let sigma = estimate_noise(&ys);
            let params = DetectParams {
                min_height: None,
                min_prominence: (AUTO_SEED_SIGMA_MULT * sigma).max(f64::MIN_POSITIVE),
                min_spacing: None,
                max_count: Some(MAX_AUTO_SEEDS),
            };
            positions = detect_peaks(&xs, &ys, &params)
                .into_iter()
                .map(|p| p.x)
                .collect();
        }
        if positions.is_empty() {
            return Err(
                "No peaks found in the range; place peak marks to seed the fit.".to_owned(),
            );
        }
        if positions.len() > MAX_FIT_SEEDS {
            return Err(format!(
                "{} peaks in the range exceeds the per-fit limit of {MAX_FIT_SEEDS}; \
                 narrow the range or remove peak marks.",
                positions.len()
            ));
        }
        Ok(PreparedFit {
            lo,
            hi,
            xs,
            ys,
            positions,
        })
    }

    /// Kick the numeric fit onto a worker thread; `poll_line_fit` applies the
    /// result. Refuses to start while another fit is still running.
    pub fn start_line_fit(
        &mut self,
        dataset: usize,
        lo: f64,
        hi: f64,
        shape: LineShapeKind,
    ) -> Result<(), String> {
        if self.session.line_fit_job.is_some() {
            return Err("A peak fit is already running; wait for it to finish.".to_owned());
        }
        let prep = self.prepare_line_fit(dataset, lo, hi)?;
        let (lo, hi) = (prep.lo, prep.hi);
        let (tx, rx) = mpsc::channel();
        let cancel = Arc::new(AtomicBool::new(false));
        let worker_cancel = Arc::clone(&cancel);
        std::thread::spawn(move || {
            let seeds = seed_peaks(&prep.xs, &prep.ys, &prep.positions);
            let cancelled = || worker_cancel.load(Ordering::Relaxed);
            let result =
                fit_lineshapes_cancellable(&prep.xs, &prep.ys, shape.into(), &seeds, &cancelled);
            let _ = tx.send(result);
        });
        self.session.line_fit_job = Some(LineFitJob {
            dataset,
            epoch: self.session.dataset_epoch,
            lo,
            hi,
            started_at: Instant::now(),
            cancel,
            rx,
        });
        self.session.status = "Fitting…".to_owned();
        Ok(())
    }

    /// Dataset and elapsed time for the active fit, used by the local job UI.
    pub fn line_fit_progress(&self) -> Option<(usize, Duration)> {
        self.session
            .line_fit_job
            .as_ref()
            .map(|job| (job.dataset, job.started_at.elapsed()))
    }

    /// Cooperatively stop the active fit and drop its result receiver.
    pub fn cancel_line_fit(&mut self) -> bool {
        let Some(job) = self.session.line_fit_job.take() else {
            return false;
        };
        job.cancel.store(true, Ordering::Relaxed);
        self.session.status = "Peak fit cancelled.".to_owned();
        true
    }

    /// Drain a finished background fit without blocking. Returns whether a job
    /// is still pending or a result was just applied, so the shell keeps
    /// repainting until completion is picked up.
    pub fn poll_line_fit(&mut self) -> bool {
        let Some(job) = &self.session.line_fit_job else {
            return false;
        };
        let result = match job.rx.try_recv() {
            Err(mpsc::TryRecvError::Empty) => return true,
            Err(mpsc::TryRecvError::Disconnected) => None,
            Ok(result) => result,
        };
        let job = self.session.line_fit_job.take().expect("job checked above");
        match result {
            None => {
                self.session.status =
                    "The fit did not converge; adjust the range or the peak seeds.".to_owned();
            }
            Some(fit) => {
                let alive = job.epoch == self.session.dataset_epoch
                    && self
                        .doc
                        .datasets
                        .get(job.dataset)
                        .is_some_and(|d| d.has_displayed_trace(None));
                if alive {
                    self.apply_line_fit(job.dataset, job.lo, job.hi, &fit);
                } else {
                    self.session.status =
                        "The dataset changed while fitting; the result was discarded.".to_owned();
                }
            }
        }
        true
    }

    /// Deconvolve the window `[lo, hi]` of a dataset's 1D trace into `shape`
    /// components, blocking until done. Test/bench helper; the UI goes through
    /// `start_line_fit` + `poll_line_fit`.
    pub fn run_line_fit(
        &mut self,
        dataset: usize,
        lo: f64,
        hi: f64,
        shape: LineShapeKind,
    ) -> Result<(), String> {
        let prep = self.prepare_line_fit(dataset, lo, hi)?;
        let seeds = seed_peaks(&prep.xs, &prep.ys, &prep.positions);
        let Some(fit) = fit_lineshapes(&prep.xs, &prep.ys, shape.into(), &seeds) else {
            return Err("The fit did not converge; adjust the range or the peak seeds.".to_owned());
        };
        self.apply_line_fit(dataset, prep.lo, prep.hi, &fit);
        Ok(())
    }

    /// Commit a computed fit to its source dataset. The parameter grid remains
    /// in the analysis panel until the user explicitly adds it to the board.
    fn apply_line_fit(&mut self, dataset: usize, lo: f64, hi: f64, fit: &LineFit) {
        let Some(next_id) = self.doc.datasets[dataset].next_line_fit_id_mut() else {
            return;
        };
        let id = *next_id;
        *next_id = id + 1;
        let stored = StoredLineFit::from_fit(id, lo, hi, fit);
        let before = self.doc.datasets[dataset].line_fits().to_vec();
        let mut after = before.clone();
        after.push(stored.clone());

        self.execute_action(Action::set_line_fits(
            self.doc.datasets[dataset].resource_id(),
            before,
            after,
        ));
        self.session.status = format!(
            "Fitted {} peak(s), R² = {:.4}. Open the result in the Peak Fit panel.",
            stored.peaks.len(),
            stored.r2
        );
    }

    /// Materialize one stored fit as a derived table and canvas on request.
    pub fn add_line_fit_result_to_board(&mut self, dataset: usize, id: u64) -> Result<(), String> {
        let Some(source) = self.doc.datasets.get(dataset) else {
            return Err("The source dataset is no longer available.".to_owned());
        };
        let Some(fit) = source.line_fits().iter().find(|fit| fit.id == id).cloned() else {
            return Err("The peak fit result is no longer available.".to_owned());
        };
        let unit = source.trace_x_unit();
        let source_name = source.display_name();
        let mut table = line_fit_parameter_table(&fit, &unit);
        table.lineage = Some(DatasetLineage::new(
            DerivationKind::LineFitTable,
            [self.doc.datasets[dataset].resource_id()],
        ));
        table.name = Some(format!("{source_name} — peak fit"));
        table.board_pos = super::app_impl_analysis::next_sheet_pos_after_new_canvas(self);
        let action = Action::insert_dataset_with_default_canvas(
            self,
            Dataset::Table(Box::new(table)),
            format!("Canvas {} — Peak fit", self.doc.canvases.len() + 1),
            DEFAULT_CANVAS_SIZE_MM,
        );
        self.execute_action(action);
        self.session.status = "Added the peak fit result to the board.".to_owned();
        Ok(())
    }

    /// Delete one stored fit as an undoable step.
    pub fn remove_line_fit(&mut self, dataset: usize, id: u64) {
        let Some(before) = self
            .doc
            .datasets
            .get(dataset)
            .map(|d| d.line_fits().to_vec())
        else {
            return;
        };
        let after: Vec<StoredLineFit> = before.iter().filter(|f| f.id != id).cloned().collect();
        self.execute_action(Action::set_line_fits(
            self.doc.datasets[dataset].resource_id(),
            before,
            after,
        ));
    }
}

/// One row per fitted component. Uncertainty is an explicit typed relation and
/// is retained only when every fitted component reports a standard error.
fn line_fit_parameter_table(fit: &StoredLineFit, unit: &str) -> TableDataset {
    let n = fit.peaks.len();
    let with_unit = |base: &str| {
        if unit.is_empty() {
            base.to_owned()
        } else {
            format!("{base} ({unit})")
        }
    };
    let column = |name: String, unit: &str, y: Vec<f64>, sigma: Vec<Option<f64>>| FloatSeries {
        name,
        unit: unit.to_owned(),
        values: y.into_iter().map(Some).collect(),
        uncertainty: sigma.iter().all(Option::is_some).then_some(sigma),
        fit: None,
    };
    let mut series = vec![
        column(
            with_unit("position"),
            unit,
            fit.peaks.iter().map(|p| p.position).collect(),
            fit.peaks.iter().map(|p| p.position_sigma).collect(),
        ),
        column(
            "height".to_owned(),
            "",
            fit.peaks.iter().map(|p| p.height).collect(),
            fit.peaks.iter().map(|p| p.height_sigma).collect(),
        ),
        column(
            with_unit("fwhm"),
            unit,
            fit.peaks.iter().map(|p| p.fwhm).collect(),
            fit.peaks.iter().map(|p| p.fwhm_sigma).collect(),
        ),
        column(
            "area".to_owned(),
            "",
            fit.peaks.iter().map(|p| p.area).collect(),
            fit.peaks.iter().map(|p| p.area_sigma).collect(),
        ),
    ];
    if fit.shape == LineShapeKind::PseudoVoigt {
        series.push(column(
            "eta".to_owned(),
            "1",
            fit.peaks.iter().map(|p| p.eta.unwrap_or(0.0)).collect(),
            fit.peaks.iter().map(|p| p.eta_sigma).collect(),
        ));
    }
    materialized_float_series_table(
        (
            "peak".into(),
            "".into(),
            (1..=n).map(|i| Some(i as f64)).collect(),
        ),
        series,
        "plotx.analysis.line-fit-table.v1",
    )
    .expect("stored line-fit values form aligned typed columns")
}

#[cfg(test)]
mod bench {
    use super::*;

    #[test]
    #[ignore]
    fn bench_line_fit_real_spectrum() {
        let Ok(path) = std::env::var("PLOTX_BENCH_JDF") else {
            println!("PLOTX_BENCH_JDF not set; skipping benchmark");
            return;
        };
        let mut app = PlotxApp::new();
        app.load_from(std::path::Path::new(&path));
        let trace = app.doc.datasets[0]
            .displayed_trace(None)
            .expect("1D spectrum trace");
        let (lo, hi) = trace
            .xs
            .iter()
            .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), &x| {
                (lo.min(x), hi.max(x))
            });
        let start = std::time::Instant::now();
        app.run_line_fit(0, lo, hi, LineShapeKind::Lorentzian)
            .expect("fit");
        let elapsed = start.elapsed();
        let fit = app.doc.datasets[0]
            .as_nmr()
            .and_then(|n| n.line_fits.last())
            .expect("stored fit");
        println!(
            "peak fit over [{lo:.2}, {hi:.2}]: {elapsed:?}, {} peak(s), R² = {:.4}",
            fit.peaks.len(),
            fit.r2
        );
    }
}
