use super::*;
use num_complex::Complex64;
use plotx_io::{Dim, Domain, NmrData2D, QuadMode};
use plotx_processing::{ProcessingStep, ReferenceParams, StepKind, StepSource};
use std::time::{Duration, Instant};

fn data_2d(source: &str) -> NmrData2D {
    let dim = Dim {
        spectral_width_hz: 1000.0,
        observe_freq_mhz: 100.0,
        carrier_ppm: 5.0,
        nucleus: "X".into(),
        group_delay: 0.0,
    };
    NmrData2D {
        data: (0..16)
            .map(|value| Complex64::new((value + 1) as f64, 0.0))
            .collect(),
        rows: 4,
        cols: 4,
        domain: Domain::Time,
        direct: dim.clone(),
        indirect: dim,
        quad: QuadMode::Complex,
        indirect_conjugate: false,
        experiment: None,
        pseudo_axis: None,
        diffusion: None,
        nus: None,
        source: source.into(),
    }
}

fn craft_data() -> plotx_io::NmrData {
    let spectral_width_hz = 2_000.0;
    let points = (0..256)
        .map(|index| {
            let time = index as f64 / spectral_width_hz;
            Complex64::from_polar(
                2.0 * (-4.0 * time).exp(),
                std::f64::consts::TAU * 140.0 * time + 0.25,
            )
        })
        .collect();
    plotx_io::NmrData {
        points,
        domain: Domain::Time,
        spectral_width_hz,
        observe_freq_mhz: 400.0,
        carrier_ppm: 4.7,
        nucleus: "1H".into(),
        source: "CRAFT synthetic".into(),
        group_delay: 0.0,
    }
}

#[test]
fn craft_result_is_installed_with_provenance_by_dataset_identity() {
    let mut app = PlotxApp::new();
    app.doc
        .datasets
        .push(Dataset::Nmr(Box::new(NmrDataset::load(craft_data()))));
    let nmr = app.doc.datasets[0].as_nmr_mut().unwrap();
    let reference_id = nmr.allocate_step_id();
    nmr.pipeline.steps.push(ProcessingStep::new(
        reference_id,
        StepKind::Reference(ReferenceParams {
            at_ppm: 5.05,
            target_ppm: 5.25,
        }),
        StepSource::User,
    ));
    nmr.rebuild();
    let target = app.doc.datasets[0].resource_id();
    app.session.ui.craft_task_dataset = Some(target);
    let mut params = plotx_processing::craft::CraftParams::conventional();
    params.filter_taps = 31;
    params.max_fit_window_width_hz = 2_000.0;
    params.max_downsampled_points = 512;
    params.max_components_per_fit_window = 2;

    assert!(app.request_craft_analysis(
        0,
        plotx_processing::craft::CraftParamOverrides::from_params(params.clone()),
        None,
    ));
    let deadline = Instant::now() + Duration::from_secs(8);
    while app.compute_busy() && Instant::now() < deadline {
        app.poll_compute();
        std::thread::sleep(Duration::from_millis(5));
    }
    app.poll_compute();

    assert!(!app.compute_busy(), "CRAFT worker completed before timeout");
    assert_eq!(app.doc.datasets[0].resource_id(), target);
    let nmr = app.doc.datasets[0].as_nmr().unwrap();
    assert_eq!(nmr.craft_runs.len(), 1);
    assert_eq!(nmr.craft_runs[0].provenance.invocation.params, params);
    assert_eq!(
        nmr.craft_runs[0]
            .provenance
            .invocation
            .reference
            .acquisition_carrier_ppm,
        4.7
    );
    assert!((nmr.craft_runs[0].provenance.invocation.reference.offset_ppm - 0.2).abs() < 1e-12);
    assert!(!nmr.craft_runs[0].is_stale_for(&nmr.data, nmr.craft_reference()));
    assert!(!nmr.craft_runs[0].components.is_empty());
    assert!(
        nmr.craft_runs
            .iter()
            .flat_map(|run| &run.components)
            .any(|component| (component.chemical_shift_ppm - 5.25).abs() < 0.01)
    );
    assert_eq!(app.session.ui.craft_selected_run, Some(CraftRunId(0)));
    assert_eq!(app.session.ui.craft_task_page, CraftTaskPage::Results);

    let active_before = app.active_dataset();
    let view_before = app.session.view;
    let sheet_before = app.session.ui.sheet_open;
    let first = app
        .materialize_craft_component_table(0, CraftRunId(0))
        .unwrap();
    let second = app
        .materialize_craft_component_table(0, CraftRunId(0))
        .unwrap();
    assert_eq!(first, second, "one run reuses one component table");
    assert_eq!(app.doc.datasets.len(), 2);
    assert_eq!(app.active_dataset(), active_before);
    assert!(app.session.view == view_before);
    assert_eq!(app.session.ui.sheet_open, sheet_before);
    let table_id = app.doc.datasets[first].resource_id();
    assert!(
        !app.doc.datasets[first]
            .as_table()
            .unwrap()
            .board_sheet_visible()
    );
    assert_eq!(
        app.doc.datasets[0].as_nmr().unwrap().craft_runs[0].component_table,
        Some(table_id)
    );
    app.show_craft_component_table_on_board(first).unwrap();
    assert!(
        app.doc.datasets[first]
            .as_table()
            .unwrap()
            .board_sheet_visible()
    );

    let nmr = app.doc.datasets[0].as_nmr_mut().unwrap();
    let reference = nmr
        .pipeline
        .steps
        .iter_mut()
        .find_map(|step| match &mut step.kind {
            StepKind::Reference(reference) => Some(reference),
            _ => None,
        })
        .unwrap();
    reference.target_ppm += 0.1;
    nmr.rebuild();
    assert!(nmr.craft_runs[0].is_stale_for(&nmr.data, nmr.craft_reference()));
}

#[test]
fn craft_rerun_keeps_requested_parent_without_hijacking_another_task() {
    let mut app = PlotxApp::new();
    app.doc
        .datasets
        .push(Dataset::Nmr(Box::new(NmrDataset::load(craft_data()))));
    let target = app.doc.datasets[0].resource_id();
    app.session.ui.craft_task_dataset = Some(target);
    let mut params = plotx_processing::craft::CraftParams::conventional();
    params.filter_taps = 31;
    params.max_fit_window_width_hz = 2_000.0;
    params.max_downsampled_points = 512;
    params.max_components_per_fit_window = 2;
    assert!(app.request_craft_analysis(
        0,
        plotx_processing::craft::CraftParamOverrides::from_params(params),
        None,
    ));
    let deadline = Instant::now() + Duration::from_secs(8);
    while app.compute_busy() && Instant::now() < deadline {
        app.poll_compute();
        std::thread::sleep(Duration::from_millis(5));
    }
    app.poll_compute();
    assert_eq!(app.doc.datasets[0].as_nmr().unwrap().craft_runs.len(), 1);

    assert!(app.request_craft_analysis(
        0,
        plotx_processing::craft::CraftParamOverrides::default(),
        Some(CraftRunId(0)),
    ));
    app.doc
        .datasets
        .push(Dataset::Nmr(Box::new(NmrDataset::load(craft_data()))));
    let other = app.doc.datasets[1].resource_id();
    app.session.ui.craft_task_dataset = Some(other);
    app.session.ui.craft_base_run = None;
    app.session.ui.craft_selected_run = Some(CraftRunId(99));
    app.session.ui.craft_task_page = CraftTaskPage::Setup;

    let deadline = Instant::now() + Duration::from_secs(8);
    while app.compute_busy() && Instant::now() < deadline {
        app.poll_compute();
        std::thread::sleep(Duration::from_millis(5));
    }
    app.poll_compute();

    let runs = &app.doc.datasets[0].as_nmr().unwrap().craft_runs;
    assert_eq!(runs.len(), 2);
    assert_eq!(runs[1].provenance.parent_run, Some(CraftRunId(0)));
    assert_eq!(app.session.ui.craft_task_dataset, Some(other));
    assert_eq!(app.session.ui.craft_selected_run, Some(CraftRunId(99)));
    assert_eq!(app.session.ui.craft_base_run, None);
    assert_eq!(app.session.ui.craft_task_page, CraftTaskPage::Setup);
    assert_eq!(
        app.session.ui.craft_feedback.get(&target),
        Some(&CraftRunFeedback::Completed(CraftRunId(1)))
    );
}

#[test]
fn process_2d_result_follows_dataset_identity_after_earlier_deletion() {
    let mut app = PlotxApp::new();
    app.doc
        .datasets
        .push(Dataset::Nmr2D(Box::new(Nmr2DDataset::load(data_2d(
            "unrelated",
        )))));
    app.doc
        .datasets
        .push(Dataset::Nmr2D(Box::new(Nmr2DDataset::load(data_2d(
            "target",
        )))));
    let target_id = app.doc.datasets[1].resource_id();
    let before = app.doc.datasets[1].as_nmr2d().unwrap().processed.clone();
    let target = app.doc.datasets[1].as_nmr2d_mut().unwrap();
    let id = target.allocate_step_id();
    target.params.f2.steps.push(ProcessingStep {
        id,
        kind: StepKind::Invert,
        enabled: true,
        source: StepSource::User,
    });
    assert!(app.schedule_2d_processing(1, false));

    app.doc.datasets.remove(0);
    let deadline = Instant::now() + Duration::from_secs(3);
    while app.compute_busy() && Instant::now() < deadline {
        app.poll_compute();
        std::thread::sleep(Duration::from_millis(5));
    }
    app.poll_compute();

    assert_eq!(app.doc.datasets[0].resource_id(), target_id);
    let after = &app.doc.datasets[0].as_nmr2d().unwrap().processed;
    let same_allocation = match (&before, after) {
        (Processed2D::Ft(before), Processed2D::Ft(after)) => std::sync::Arc::ptr_eq(before, after),
        (Processed2D::Stack(before), Processed2D::Stack(after)) => {
            std::sync::Arc::ptr_eq(before, after)
        }
        _ => false,
    };
    assert!(
        !same_allocation,
        "the completed result must land on the same DatasetId after index shift"
    );
}

#[test]
fn successful_processing_promotes_fresh_runtime_versions_for_each_scalar_field() {
    let mut app = PlotxApp::new();
    app.doc
        .datasets
        .push(Dataset::Nmr2D(Box::new(Nmr2DDataset::load(data_2d(
            "versioned target",
        )))));
    let resource = app.doc.datasets[0].resource_id();
    let fields = app.doc.datasets[0]
        .field_descriptors()
        .into_iter()
        .filter(|field| matches!(field.local_id.as_str(), "nmr.real" | "nmr.magnitude"))
        .map(|field| FieldRef {
            resource,
            field: field.id,
        })
        .collect::<Vec<_>>();
    assert_eq!(fields.len(), 2);
    assert!(
        fields
            .iter()
            .all(|field| { app.session.compute.current_field_version(*field).is_none() })
    );

    assert!(app.schedule_2d_processing(0, true));
    let deadline = Instant::now() + Duration::from_secs(3);
    while app.compute_busy() && Instant::now() < deadline {
        app.poll_compute();
        std::thread::sleep(Duration::from_millis(5));
    }
    app.poll_compute();
    let first = fields
        .iter()
        .map(|field| app.session.compute.current_field_version(*field).unwrap())
        .collect::<Vec<_>>();
    assert_ne!(first[0], first[1], "each FieldId has its own version token");

    let dataset = app.doc.datasets[0].as_nmr2d_mut().unwrap();
    let id = dataset.allocate_step_id();
    dataset.params.f2.steps.push(ProcessingStep {
        id,
        kind: StepKind::Invert,
        enabled: true,
        source: StepSource::User,
    });
    assert!(app.schedule_2d_processing(0, false));
    while app.compute_busy() && Instant::now() < deadline {
        app.poll_compute();
        std::thread::sleep(Duration::from_millis(5));
    }
    app.poll_compute();
    for (field, previous) in fields.iter().zip(first) {
        assert!(
            app.session.compute.current_field_version(*field).unwrap() > previous,
            "a successfully installed processing artifact advances its field version"
        );
    }
}
