use super::{
    DatasetId, TypedTableState, execute_typed_plan, execute_typed_plan_cancellable,
    refresh_typed_plan, refresh_typed_plan_cancellable,
};
use plotx_data::TableId;
use std::collections::BTreeSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc};
use std::time::{Duration, Instant};

pub struct TableTransformJob {
    input_datasets: Vec<DatasetId>,
    epoch: u64,
    name: String,
    started_at: Instant,
    cancel: Arc<AtomicBool>,
    rx: mpsc::Receiver<Result<TypedTableState, String>>,
}

pub struct TableRefreshJob {
    dataset: DatasetId,
    epoch: u64,
    started_at: Instant,
    cancel: Arc<AtomicBool>,
    before: TypedTableState,
    rx: mpsc::Receiver<Result<TypedTableState, String>>,
}

impl crate::state::PlotxApp {
    pub fn start_table_transform(
        &mut self,
        plan: plotx_data::RelPlanV1,
        input_datasets: Vec<usize>,
        name: String,
        memory_limit_bytes: u64,
    ) -> Result<(), String> {
        if self.session.table_transform_job.is_some() || self.session.table_refresh_job.is_some() {
            return Err("A table transform is already running.".into());
        }
        let input_datasets: Vec<DatasetId> = input_datasets
            .into_iter()
            .map(|index| self.doc.datasets[index].resource_id())
            .collect();
        let inputs = self.typed_inputs(&input_datasets)?;
        let cancel = Arc::new(AtomicBool::new(false));
        let worker_cancel = Arc::clone(&cancel);
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let refs = inputs.iter().collect::<Vec<_>>();
            let result = execute_typed_plan_cancellable(
                plan,
                &refs,
                TableId::new(),
                memory_limit_bytes,
                &BTreeSet::new(),
                worker_cancel.as_ref(),
            )
            .map_err(|error| error.to_string());
            let _ = tx.send(result);
        });
        self.session.table_transform_job = Some(TableTransformJob {
            input_datasets,
            epoch: self.session.dataset_epoch,
            name,
            started_at: Instant::now(),
            cancel,
            rx,
        });
        self.session.status = "Running table transform…".into();
        Ok(())
    }

    fn typed_inputs(&self, datasets: &[DatasetId]) -> Result<Vec<TypedTableState>, String> {
        datasets
            .iter()
            .map(|id| {
                self.doc
                    .dataset_by_id(*id)
                    .and_then(crate::state::Dataset::as_table)
                    .map(|table| table.typed_state.clone())
                    .ok_or_else(|| {
                        "A source data table for this refresh is missing or is no longer a table."
                            .to_owned()
                    })
            })
            .collect()
    }

    pub fn table_transform_progress(&self) -> Option<Duration> {
        self.session
            .table_transform_job
            .as_ref()
            .map(|job| job.started_at.elapsed())
            .or_else(|| {
                self.session
                    .table_refresh_job
                    .as_ref()
                    .map(|job| job.started_at.elapsed())
            })
    }

    pub fn cancel_table_transform(&mut self) -> bool {
        let cancel = self
            .session
            .table_transform_job
            .as_ref()
            .map(|job| &job.cancel)
            .or_else(|| {
                self.session
                    .table_refresh_job
                    .as_ref()
                    .map(|job| &job.cancel)
            });
        let Some(cancel) = cancel else { return false };
        cancel.store(true, Ordering::Relaxed);
        self.session.status = "Cancelling table transform…".into();
        true
    }

    pub fn poll_table_transform(&mut self) -> bool {
        let Some(job) = &self.session.table_transform_job else {
            return self.poll_table_refresh();
        };
        let result = match job.rx.try_recv() {
            Err(mpsc::TryRecvError::Empty) => return true,
            Err(mpsc::TryRecvError::Disconnected) => {
                Err("The table transform worker stopped unexpectedly.".into())
            }
            Ok(result) => result,
        };
        let job = self
            .session
            .table_transform_job
            .take()
            .expect("job checked above");
        if job.epoch != self.session.dataset_epoch {
            self.session.status =
                "Datasets changed while transforming; the result was discarded.".into();
            return true;
        }
        match result {
            Ok(typed) => self.insert_transformed_table(typed, &job, false),
            Err(_) if job.cancel.load(Ordering::Relaxed) => {
                self.session.status = "Table transform cancelled.".into();
            }
            Err(error) => self.session.status = format!("Table transform failed: {error}"),
        }
        true
    }

    pub fn start_table_refresh(
        &mut self,
        dataset: usize,
        input_datasets: Vec<DatasetId>,
        memory_limit_bytes: u64,
    ) -> Result<(), String> {
        if self.session.table_transform_job.is_some() || self.session.table_refresh_job.is_some() {
            return Err("A table operation is already running.".into());
        }
        let (dataset_id, before) = self
            .doc
            .datasets
            .get(dataset)
            .and_then(|value| {
                let table = value.as_table()?;
                Some((value.resource_id(), table.typed_state.clone()))
            })
            .ok_or_else(|| "Select a derived data table to refresh.".to_owned())?;
        let inputs = self.typed_inputs(&input_datasets)?;
        let worker_derived = before.clone();
        let cancel = Arc::new(AtomicBool::new(false));
        let worker_cancel = Arc::clone(&cancel);
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let refs = inputs.iter().collect::<Vec<_>>();
            let result = refresh_typed_plan_cancellable(
                &worker_derived,
                &refs,
                memory_limit_bytes,
                &BTreeSet::new(),
                worker_cancel.as_ref(),
            )
            .map_err(|error| error.to_string());
            let _ = tx.send(result);
        });
        self.session.table_refresh_job = Some(TableRefreshJob {
            dataset: dataset_id,
            epoch: self.session.dataset_epoch,
            started_at: Instant::now(),
            cancel,
            before,
            rx,
        });
        self.session.status = "Refreshing table…".into();
        Ok(())
    }

    fn poll_table_refresh(&mut self) -> bool {
        let Some(job) = &self.session.table_refresh_job else {
            return false;
        };
        let result = match job.rx.try_recv() {
            Err(mpsc::TryRecvError::Empty) => return true,
            Err(mpsc::TryRecvError::Disconnected) => {
                Err("The table refresh worker stopped unexpectedly.".into())
            }
            Ok(result) => result,
        };
        let job = self
            .session
            .table_refresh_job
            .take()
            .expect("job checked above");
        if job.epoch != self.session.dataset_epoch {
            self.session.status =
                "Datasets changed while refreshing; the result was discarded.".into();
            return true;
        }
        match result {
            Ok(after) => {
                let revision = after.envelope.revision.id;
                self.execute_action(crate::actions::Action::SetTypedTableState {
                    dataset: job.dataset,
                    before: Box::new(job.before),
                    after: Box::new(after),
                });
                self.session.status = format!("Refreshed table revision {revision}.");
            }
            Err(_) if job.cancel.load(Ordering::Relaxed) => {
                self.session.status = "Table refresh cancelled.".into();
            }
            Err(error) => self.session.status = format!("Table refresh blocked: {error}"),
        }
        true
    }

    fn insert_transformed_table(
        &mut self,
        typed: TypedTableState,
        job: &TableTransformJob,
        reveal: bool,
    ) {
        let mut table = crate::state::TableDataset::from_typed(typed);
        table.name = Some(job.name.clone());
        table.lineage = Some(crate::state::DatasetLineage::new(
            crate::state::DerivationKind::RelationalTransform,
            job.input_datasets.iter().copied(),
        ));
        table.board_pos = crate::state::next_sheet_board_pos(self);
        let index = self.doc.datasets.len();
        let action = crate::actions::Action::insert_dataset_with_default_canvas(
            self,
            crate::state::Dataset::Table(Box::new(table)),
            format!("Canvas {} - {}", self.doc.canvases.len() + 1, job.name),
            crate::state::DEFAULT_CANVAS_SIZE_MM,
        );
        self.execute_action(action);
        if reveal {
            self.focus_single(index);
            self.session.view = crate::state::PrimaryView::Data;
            self.session.ui.frame_selection = vec![crate::state::FrameRef::Sheet(index)];
            self.session.ui.sheet_open = Some(index);
        }
        self.session.status = "Table transform completed.".into();
    }

    pub fn derive_table_from_plan(
        &mut self,
        plan: plotx_data::RelPlanV1,
        input_datasets: &[usize],
        name: String,
        memory_limit_bytes: u64,
    ) -> Result<usize, String> {
        let input_ids: Vec<DatasetId> = input_datasets
            .iter()
            .map(|&index| self.doc.datasets[index].resource_id())
            .collect();
        let inputs = self.typed_inputs(&input_ids)?;
        let refs = inputs.iter().collect::<Vec<_>>();
        let typed = execute_typed_plan(
            plan,
            &refs,
            TableId::new(),
            memory_limit_bytes,
            &BTreeSet::new(),
        )
        .map_err(|error| error.to_string())?;
        let index = self.doc.datasets.len();
        let job = TableTransformJob {
            input_datasets: input_ids,
            epoch: self.session.dataset_epoch,
            name,
            started_at: Instant::now(),
            cancel: Arc::new(AtomicBool::new(false)),
            rx: mpsc::channel().1,
        };
        self.insert_transformed_table(typed, &job, true);
        Ok(index)
    }

    pub fn refresh_derived_table(
        &mut self,
        dataset: usize,
        input_datasets: &[usize],
        memory_limit_bytes: u64,
    ) -> Result<(), String> {
        let (dataset_id, derived) = self
            .doc
            .datasets
            .get(dataset)
            .and_then(|value| {
                let table = value.as_table()?;
                Some((value.resource_id(), table.typed_state.clone()))
            })
            .ok_or_else(|| "Select a derived data table to refresh.".to_owned())?;
        let input_ids: Vec<DatasetId> = input_datasets
            .iter()
            .map(|&index| {
                self.doc
                    .datasets
                    .get(index)
                    .map(crate::state::Dataset::resource_id)
                    .ok_or_else(|| "An input table is no longer available.".to_owned())
            })
            .collect::<Result<_, String>>()?;
        let inputs = self.typed_inputs(&input_ids)?;
        let refs = inputs.iter().collect::<Vec<_>>();
        let refreshed = refresh_typed_plan(&derived, &refs, memory_limit_bytes, &BTreeSet::new())
            .map_err(|error| error.to_string())?;
        let revision = refreshed.envelope.revision.id;
        self.execute_action(crate::actions::Action::SetTypedTableState {
            dataset: dataset_id,
            before: Box::new(derived),
            after: Box::new(refreshed),
        });
        self.session.status = format!("Refreshed table revision {revision}.");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::{Dataset, FloatSeries, materialized_float_series_table};
    use plotx_data::{Relation, SnapshotRead};

    #[test]
    fn typed_inputs_reports_a_missing_source_dataset() {
        let app = crate::state::PlotxApp::new();
        let missing = DatasetId::new();

        let error = match app.typed_inputs(&[missing]) {
            Ok(_) => panic!("a missing source dataset must be rejected"),
            Err(error) => error,
        };

        assert!(error.contains("source data table"), "{error}");
        assert!(error.contains("missing"), "{error}");
    }

    #[test]
    fn background_transform_commits_only_after_polling_completion() {
        let source = materialized_float_series_table(
            ("time".into(), "s".into(), vec![Some(0.0), Some(1.0)]),
            vec![FloatSeries {
                name: "signal".into(),
                unit: String::new(),
                values: vec![Some(2.0), Some(3.0)],
                uncertainty: None,
                fit: None,
            }],
            "plotx.test.background-transform.v1",
        )
        .unwrap();
        let revision = &source.typed_state.envelope.revision;
        let signal = source.series_bindings[0].value_column;
        let plan = plotx_data::RelPlanV1::new(Relation::Project {
            input: Box::new(Relation::SnapshotRead(SnapshotRead {
                table: revision.table_id,
                revision: revision.id,
                fingerprint: revision.snapshot.fingerprint,
            })),
            columns: vec![signal],
        });
        let mut app = crate::state::PlotxApp::new();
        app.doc.datasets.push(Dataset::Table(Box::new(source)));
        app.session.view = crate::state::PrimaryView::Canvas;
        app.session.ui.sheet_open = Some(0);
        app.session.ui.frame_selection = vec![crate::state::FrameRef::Sheet(0)];
        app.start_table_transform(plan, vec![0], "Projected".into(), 16 * 1024 * 1024)
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(2);
        while Instant::now() < deadline && app.session.table_transform_job.is_some() {
            app.poll_table_transform();
            std::thread::sleep(Duration::from_millis(1));
        }
        assert!(app.session.table_transform_job.is_none());
        assert_eq!(app.doc.datasets.len(), 2);
        assert!(app.session.view == crate::state::PrimaryView::Canvas);
        assert_eq!(app.session.ui.sheet_open, Some(0));
        assert_eq!(
            app.session.ui.frame_selection,
            vec![crate::state::FrameRef::Sheet(0)]
        );
        assert_eq!(
            app.doc.datasets[1]
                .as_table()
                .unwrap()
                .typed_state
                .envelope
                .revision
                .snapshot
                .schema
                .columns[0]
                .id,
            signal
        );
        app.undo();
        assert_eq!(app.doc.datasets.len(), 1);
        app.redo();
        assert_eq!(app.doc.datasets.len(), 2);
    }

    #[test]
    fn background_transform_discards_results_after_dataset_epoch_changes() {
        let source = materialized_float_series_table(
            ("time".into(), "s".into(), vec![Some(0.0), Some(1.0)]),
            vec![FloatSeries {
                name: "signal".into(),
                unit: String::new(),
                values: vec![Some(2.0), Some(3.0)],
                uncertainty: None,
                fit: None,
            }],
            "plotx.test.stale-background-transform.v1",
        )
        .unwrap();
        let revision = &source.typed_state.envelope.revision;
        let plan = plotx_data::RelPlanV1::new(Relation::Project {
            input: Box::new(Relation::SnapshotRead(SnapshotRead {
                table: revision.table_id,
                revision: revision.id,
                fingerprint: revision.snapshot.fingerprint,
            })),
            columns: vec![source.series_bindings[0].value_column],
        });
        let mut app = crate::state::PlotxApp::new();
        app.doc.datasets.push(Dataset::Table(Box::new(source)));
        app.start_table_transform(plan, vec![0], "Stale".into(), 16 * 1024 * 1024)
            .unwrap();
        app.session.dataset_epoch += 1;
        let deadline = Instant::now() + Duration::from_secs(2);
        while Instant::now() < deadline && app.session.table_transform_job.is_some() {
            app.poll_table_transform();
            std::thread::sleep(Duration::from_millis(1));
        }
        assert!(app.session.table_transform_job.is_none());
        assert_eq!(app.doc.datasets.len(), 1);
        assert!(app.session.status.contains("discarded"));
    }
}
