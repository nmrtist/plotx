use super::*;
use crate::state::XrdDataset;

fn prepared() -> PreparedImport {
    PreparedImport {
        dataset: Dataset::Xrd(Box::new(XrdDataset::load(plotx_io::XrdData {
            two_theta_deg: vec![1.0, 2.0, 3.0],
            intensity: vec![2.0, 4.0, 2.0],
            attenuation: None,
            source: "sample.raw".into(),
            instrument: None,
            target: None,
            wavelength_angstrom: None,
            voltage_kv: None,
            current_ma: None,
            scan_step_deg: None,
            scan_speed_deg_min: None,
        }))),
        source: "sample.raw".into(),
        format: plotx_io::DataFormat::Xrd(plotx_io::XrdFormat::RigakuRaw),
        warnings: Vec::new(),
    }
}

fn app_with_channel() -> (PlotxApp, mpsc::Sender<Event>) {
    let mut app = PlotxApp::new_with_settings(Default::default());
    let (sender, receiver) = mpsc::channel();
    app.session.data_imports.active = Some(Job {
        receiver,
        recent: "batch".into(),
        loaded: 0,
        failed: 0,
    });
    (app, sender)
}

#[test]
fn commits_only_one_item_per_poll_and_preserves_selection_and_undo() {
    let (mut app, sender) = app_with_channel();
    assert!(app.install_prepared_import(std::path::Path::new("existing"), Ok(prepared())));
    let selected = app.session.ui.data_selection.clone();
    let active = app.session.active_canvas;
    for _ in 0..2 {
        sender
            .send(Event::Item("next".into(), Ok(prepared())))
            .unwrap();
    }
    assert!(app.poll_data_import());
    assert_eq!(app.doc.datasets.len(), 2);
    assert_eq!(app.session.active_canvas, active);
    assert_eq!(app.session.ui.data_selection, selected);
    assert!(app.poll_data_import());
    assert_eq!(app.doc.datasets.len(), 3);
    app.undo();
    assert_eq!(app.doc.datasets.len(), 2);
    app.redo();
    assert_eq!(app.doc.datasets.len(), 3);
}

#[test]
fn failed_item_is_reported_and_does_not_prevent_later_success() {
    let (mut app, sender) = app_with_channel();
    sender
        .send(Event::Item("bad".into(), Err("broken source".into())))
        .unwrap();
    sender
        .send(Event::Item("good".into(), Ok(prepared())))
        .unwrap();
    app.poll_data_import();
    assert!(app.doc.datasets.is_empty());
    assert_eq!(
        app.session
            .operation_history
            .operations()
            .next_back()
            .unwrap()
            .outcome,
        crate::operation::OperationOutcome::Failure
    );
    app.poll_data_import();
    assert_eq!(app.doc.datasets.len(), 1);
    assert!(app.session.status.contains("1 loaded, 1 failed"));
}

#[test]
fn disconnected_worker_reaches_user_feedback() {
    let (mut app, sender) = app_with_channel();
    drop(sender);
    assert!(!app.poll_data_import());
    assert!(app.session.status.contains("stopped unexpectedly"));
    assert_eq!(app.session.operation_history.operations().count(), 1);
}

#[test]
fn document_swap_discards_ready_results_and_queued_requests() {
    let (mut app, sender) = app_with_channel();
    sender
        .send(Event::Item("old".into(), Ok(prepared())))
        .unwrap();
    app.queue_data_import("old queued".into(), || {
        panic!("old request must be dropped")
    });
    app.start_new_project();
    assert!(sender.send(Event::Finished).is_err());
    assert!(!app.poll_data_import());
    assert!(app.doc.datasets.is_empty());
}

#[test]
fn discovery_runs_off_thread_and_poll_does_not_wait_for_it() {
    let mut app = PlotxApp::new_with_settings(Default::default());
    let main_thread = std::thread::current().id();
    let (entered, started) = mpsc::channel();
    let (release, wait) = mpsc::channel();
    app.queue_data_import("batch".into(), move || {
        entered.send(std::thread::current().id()).unwrap();
        wait.recv().unwrap();
        Ok(vec![])
    });
    assert!(app.poll_data_import());
    assert_ne!(
        started
            .recv_timeout(std::time::Duration::from_secs(5))
            .unwrap(),
        main_thread
    );
    assert!(app.poll_data_import());
    app.start_new_project();
    release.send(()).unwrap();
    assert!(!app.poll_data_import());
}

#[test]
fn preparation_is_parallel_bounded_ordered_and_keeps_item_errors() {
    use std::sync::{
        Arc, Barrier,
        atomic::{AtomicUsize, Ordering},
    };
    let (sender, receiver) = mpsc::sync_channel(1);
    let barrier = Arc::new(Barrier::new(2));
    let running = Arc::new(AtomicUsize::new(0));
    let peak = Arc::new(AtomicUsize::new(0));
    let peak_worker = peak.clone();
    let worker = std::thread::spawn(move || {
        let paths = (0..4)
            .map(|i| PathBuf::from(i.to_string()))
            .collect::<Vec<_>>();
        prepare_paths(&paths, &sender, 2, &|path| {
            let active = running.fetch_add(1, Ordering::SeqCst) + 1;
            peak_worker.fetch_max(active, Ordering::SeqCst);
            barrier.wait();
            running.fetch_sub(1, Ordering::SeqCst);
            if path == std::path::Path::new("1") {
                Err("bad item".into())
            } else {
                Ok(prepared())
            }
        })
    });
    let mut items = Vec::new();
    while let Ok(event) = receiver.recv_timeout(std::time::Duration::from_secs(10)) {
        if let Event::Item(path, result) = event {
            items.push((path, result.is_ok()));
        }
    }
    assert!(worker.join().unwrap());
    assert_eq!(peak.load(Ordering::SeqCst), 2);
    assert_eq!(
        items,
        vec![
            ("0".into(), true),
            ("1".into(), false),
            ("2".into(), true),
            ("3".into(), true)
        ]
    );
}

#[test]
fn disconnected_import_does_not_prepare_another_window() {
    let (sender, receiver) = mpsc::sync_channel(1);
    drop(receiver);
    assert!(!prepare_paths(&["unused".into()], &sender, 2, &|_| {
        panic!("closed document must not start preparation")
    }));
}
