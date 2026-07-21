use super::*;
use num_complex::Complex64;
use plotx_io::{Dim, Domain, QuadMode};
use plotx_processing::{PhaseParams, ProcessingStep, StepKind, process_2d};
fn data_2d() -> Arc<NmrData2D> {
    let dim = Dim {
        spectral_width_hz: 1000.0,
        observe_freq_mhz: 100.0,
        carrier_ppm: 5.0,
        nucleus: "X".into(),
        group_delay: 0.0,
    };
    Arc::new(NmrData2D {
        data: (0..16)
            .map(|i| Complex64::new((i + 1) as f64, 0.0))
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
        source: "test".into(),
    })
}

fn stack_spectrum() -> Arc<StackSpectrum> {
    Arc::new(StackSpectrum {
        ppm: vec![0.0],
        traces: vec![vec![Complex64::new(1.0, 0.0); 1]; 3],
        direct: plotx_processing::AxisMeta {
            nucleus: "X".into(),
            observe_freq_mhz: 100.0,
        },
        source: "test".into(),
    })
}

fn diffusion_meta() -> DiffusionMeta {
    DiffusionMeta {
        gamma: 2.675e8,
        delta: 1e-3,
        big_delta: 0.1,
        tau: 0.0,
        shape_factor: 1.0 / 3.0,
    }
}

#[test]
fn repeated_processing_requests_coalesce_to_latest_recipe() {
    let mut service = ComputeService::new();
    let preset = Preset2D::Cosy;
    let first = Params2D::default_for(preset);
    let mut latest = first.clone();
    latest.f2.steps.push(ProcessingStep::new(
        StepKind::Phase(PhaseParams::MANUAL_ZERO),
        plotx_processing::StepSource::User,
    ));

    service.request_2d_full(0, 4, data_2d(), first, preset);
    service.request_2d_full(0, 4, data_2d(), latest.clone(), preset);
    assert_eq!(service.deferred_processing.len(), 1);

    let deadline = Instant::now() + Duration::from_secs(2);
    let mut completed = Vec::new();
    while service.is_busy() && Instant::now() < deadline {
        completed.extend(service.try_drain());
        thread::sleep(Duration::from_millis(5));
    }
    completed.extend(service.try_drain());
    assert!(!service.is_busy());
    assert_eq!(completed.len(), 1);
    let Done::Processing2D { params, epoch, .. } = &completed[0] else {
        panic!("expected processing result");
    };
    assert_eq!(params, &latest);
    assert_eq!(*epoch, 4);
}

#[test]
fn an_idle_processing_request_dispatches_immediately() {
    let mut service = ComputeService::new();
    let preset = Preset2D::Cosy;
    let params = Params2D::default_for(preset);

    service.request_2d_full(0, 0, data_2d(), params, preset);
    assert!(service.deferred_processing.is_empty());
    assert!(service.active.contains_key(&(0, ComputeKind::Processing2D)));
}

#[test]
fn reapply_to_reapply_keeps_the_active_job_and_replaces_the_deferred_recipe() {
    let mut service = ComputeService::new();
    let preset = Preset2D::Cosy;
    let mut first = Params2D::default_for(preset);
    let base = process_2d(&data_2d(), &first);

    let token = Arc::new(AtomicBool::new(false));
    service.active.insert(
        (0, ComputeKind::Processing2D),
        ActiveJob {
            generation: 10,
            started_at: Instant::now(),
            token: Arc::clone(&token),
            processing_input: Some(ProcessingInputKind::Reapply),
        },
    );

    first.f2.steps.push(ProcessingStep::new(
        StepKind::Phase(PhaseParams::MANUAL_ZERO),
        plotx_processing::StepSource::User,
    ));
    service.request_2d_reapply(0, 0, base.clone(), first, preset);
    assert!(!token.load(Ordering::Relaxed));
    let first_generation = service.deferred_processing[&0].generation;

    service.request_2d_reapply(0, 0, base, Params2D::default_for(preset), preset);
    assert!(!token.load(Ordering::Relaxed));
    assert!(service.deferred_processing[&0].generation > first_generation);
}

#[test]
fn any_full_retransform_cancels_an_active_reapply() {
    let mut service = ComputeService::new();
    let token = Arc::new(AtomicBool::new(false));
    service.active.insert(
        (0, ComputeKind::Processing2D),
        ActiveJob {
            generation: 10,
            started_at: Instant::now(),
            token: Arc::clone(&token),
            processing_input: Some(ProcessingInputKind::Reapply),
        },
    );

    let preset = Preset2D::Cosy;
    service.request_2d_full(0, 0, data_2d(), Params2D::default_for(preset), preset);
    assert!(token.load(Ordering::Relaxed));
    assert!(matches!(
        service.deferred_processing[&0].input,
        ProcessingInput::Full(_)
    ));
}

/// A processing edit invalidates a running analysis' input, so it is cancelled
/// — but a minutes-long user-initiated DOSY run must not vanish silently.
#[test]
fn a_processing_request_reports_the_analysis_it_cancels() {
    let mut service = ComputeService::new();
    let stack = stack_spectrum();
    service
        .enqueue_dosy(
            1,
            0,
            stack,
            vec![0.0, 1.0, 2.0],
            diffusion_meta(),
            "X".into(),
            "test".into(),
        )
        .expect("an idle dataset accepts a DOSY job");

    let preset = Preset2D::Cosy;
    let aborted = service.request_2d_full(1, 0, data_2d(), Params2D::default_for(preset), preset);
    assert_eq!(aborted, vec![ComputeKind::Dosy]);
}

/// `cancel` keeps the active entry until the worker acknowledges, but `progress`
/// already reports nothing running. The busy gate must agree, or the user is
/// told to wait for a computation the UI gives them no way to see.
#[test]
fn a_cancelled_analysis_stops_blocking_a_re_run() {
    let mut service = ComputeService::new();
    service
        .enqueue_dosy(
            2,
            0,
            stack_spectrum(),
            vec![0.0, 1.0, 2.0],
            diffusion_meta(),
            "X".into(),
            "test".into(),
        )
        .expect("an idle dataset accepts a DOSY job");
    assert_eq!(service.blocking_work_for(2), Some(ComputeKind::Dosy));

    assert!(service.cancel(2, ComputeKind::Dosy));
    assert_eq!(service.progress(2, ComputeKind::Dosy), None);
    assert_eq!(
        service.blocking_work_for(2),
        None,
        "a cancelled job the user cannot see must not block a re-run"
    );
}

/// Pending 2D processing blocks a DOSY run because it would replace the stack
/// the fit reads — but saying "a DOSY map is already being computed" would
/// describe work that does not exist.
#[test]
fn pending_processing_blocks_dosy_under_its_own_name() {
    let mut service = ComputeService::new();
    let preset = Preset2D::Cosy;
    service.request_2d_full(4, 0, data_2d(), Params2D::default_for(preset), preset);
    assert_eq!(
        service.blocking_work_for(4),
        Some(ComputeKind::Processing2D)
    );
}

#[test]
fn cancelling_processing_discards_its_result_and_releases_the_service() {
    let mut service = ComputeService::new();
    let preset = Preset2D::Cosy;
    service.request_2d_full(3, 2, data_2d(), Params2D::default_for(preset), preset);

    assert!(service.cancel(3, ComputeKind::Processing2D));
    assert_eq!(service.progress(3, ComputeKind::Processing2D), None);

    let deadline = Instant::now() + Duration::from_secs(2);
    let mut completed = Vec::new();
    while service.is_busy() && Instant::now() < deadline {
        completed.extend(service.try_drain());
        thread::sleep(Duration::from_millis(5));
    }
    completed.extend(service.try_drain());
    assert!(!service.is_busy());
    assert!(completed.is_empty());
}

#[test]
fn cancelled_ilt_job_reports_acknowledgement_without_a_result() {
    let token = Arc::new(AtomicBool::new(true));
    let stack = stack_spectrum();
    let done = run_job(Job::Ilt {
        generation: 7,
        dataset: 2,
        epoch: 0,
        token,
        stack,
        b_factors: vec![0.0, 1.0, 2.0],
        d_grid: vec![1e-10, 1e-9],
        lambda: 0.01,
        params: IltParams::default(),
        nucleus: "X".into(),
        source: "test".into(),
    });

    assert!(matches!(
        done,
        Done::Cancelled {
            generation: 7,
            dataset: 2,
            kind: ComputeKind::Ilt
        }
    ));
}
