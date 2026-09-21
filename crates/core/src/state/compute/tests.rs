use super::*;
use num_complex::Complex64;
use plotx_io::{Dim, Domain, NmrData2D, QuadMode};
use plotx_processing::{PhaseParams, Preset2D, ProcessingStep, StepKind};

fn dataset(value: u128) -> DatasetId {
    DatasetId::from_uuid(uuid::Uuid::from_u128(value))
}
fn data_2d() -> Full2DInput {
    let dim = Dim {
        spectral_width_hz: 1000.0,
        observe_freq_mhz: 100.0,
        carrier_ppm: 5.0,
        nucleus: "X".into(),
        group_delay: 0.0,
    };
    let data = NmrData2D {
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
    };
    let source = plotx_io::nmr_series::NmrSeriesSource::try_from(data).unwrap();
    Full2DInput {
        source: source.source_dataset().clone(),
        delay: DelayPolicy::AxisEvidence,
        nus: None,
    }
}

fn stack_spectrum() -> Arc<StackSpectrum> {
    Arc::new(StackSpectrum {
        direct_domain: Domain::Frequency,
        ppm: vec![0.0],
        traces: vec![vec![Complex64::new(1.0, 0.0); 1]; 3],
        direct: plotx_processing::AxisMeta {
            nucleus: "X".into(),
            observe_freq_mhz: Some(100.0),
            unit: Some(nmr::axis::AxisUnit::Ppm),
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

fn processing_fields() -> [ProcessingField; 2] {
    [
        ProcessingField {
            field: FieldId::new(0),
            component: ProcessedFieldComponent::Real,
        },
        ProcessingField {
            field: FieldId::new(1),
            component: ProcessedFieldComponent::Magnitude,
        },
    ]
}

#[test]
fn repeated_processing_requests_coalesce_to_latest_recipe() {
    let mut service = ComputeService::new();
    let preset = Preset2D::Cosy;
    let first = Params2D::default_for(preset);
    let mut latest = first.clone();
    latest.f2.steps.push(ProcessingStep::new(
        plotx_processing::StepId::new(99),
        StepKind::Phase(PhaseParams::MANUAL_ZERO),
        plotx_processing::StepSource::User,
    ));

    let fields = processing_fields();
    service
        .request_2d_full(dataset(0), &fields, data_2d(), first)
        .unwrap();
    service
        .request_2d_full(dataset(0), &fields, data_2d(), latest.clone())
        .unwrap();
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
    let Done::Processing2D {
        fields,
        params,
        version,
        ..
    } = &completed[0]
    else {
        panic!("expected processing result");
    };
    assert_eq!(params, &latest);
    assert!(version.0 > 0);
    assert_eq!(fields.len(), 2);
    assert!(
        fields.iter().all(|field| field.summary.is_some()),
        "successful scalar Process2D artifacts carry their cheap FieldSummary"
    );
}

#[test]
fn an_idle_processing_request_dispatches_immediately() {
    let mut service = ComputeService::new();
    let preset = Preset2D::Cosy;
    let params = Params2D::default_for(preset);

    let fields = processing_fields();
    service
        .request_2d_full(dataset(0), &fields, data_2d(), params)
        .unwrap();
    assert!(service.deferred_processing.is_empty());
    assert!(
        service
            .active
            .contains_key(&(dataset(0), ComputeKind::Processing2D))
    );
}

#[test]
fn reapply_to_reapply_keeps_the_active_job_and_replaces_the_deferred_recipe() {
    let mut service = ComputeService::new();
    let preset = Preset2D::Cosy;
    let mut first = Params2D::default_for(preset);
    let base = execute_2d(
        &data_2d().source,
        &first,
        DelayPolicy::AxisEvidence,
        RecipeRange::Base,
        None,
        &mut nmr::ExecutionContext::default(),
    )
    .unwrap()
    .source;

    let token = CancellationToken::new();
    service.active.insert(
        (dataset(0), ComputeKind::Processing2D),
        ActiveJob {
            generation: 10,
            started_at: Instant::now(),
            token: token.clone(),
            processing_input: Some(ProcessingInputKind::Reapply),
        },
    );

    first.f2.steps.push(ProcessingStep::new(
        plotx_processing::StepId::new(99),
        StepKind::Phase(PhaseParams::MANUAL_ZERO),
        plotx_processing::StepSource::User,
    ));
    let fields = processing_fields();
    service
        .request_2d_reapply(dataset(0), &fields, base.clone(), first)
        .unwrap();
    assert!(!token.is_cancelled());
    let first_version = service.deferred_processing[&dataset(0)].version;

    service
        .request_2d_reapply(dataset(0), &fields, base, Params2D::default_for(preset))
        .unwrap();
    assert!(!token.is_cancelled());
    assert!(service.deferred_processing[&dataset(0)].version > first_version);
}

#[test]
fn any_full_retransform_cancels_an_active_reapply() {
    let mut service = ComputeService::new();
    let token = CancellationToken::new();
    service.active.insert(
        (dataset(0), ComputeKind::Processing2D),
        ActiveJob {
            generation: 10,
            started_at: Instant::now(),
            token: token.clone(),
            processing_input: Some(ProcessingInputKind::Reapply),
        },
    );

    let preset = Preset2D::Cosy;
    let fields = processing_fields();
    service
        .request_2d_full(
            dataset(0),
            &fields,
            data_2d(),
            Params2D::default_for(preset),
        )
        .unwrap();
    assert!(token.is_cancelled());
    assert!(matches!(
        service.deferred_processing[&dataset(0)].input,
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
            dataset(1),
            0,
            stack,
            vec![0.0, 1.0, 2.0],
            diffusion_meta(),
            "X".into(),
            "test".into(),
        )
        .expect("an idle dataset accepts a DOSY job");

    let preset = Preset2D::Cosy;
    let fields = processing_fields();
    let aborted = service
        .request_2d_full(
            dataset(1),
            &fields,
            data_2d(),
            Params2D::default_for(preset),
        )
        .unwrap();
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
            dataset(2),
            0,
            stack_spectrum(),
            vec![0.0, 1.0, 2.0],
            diffusion_meta(),
            "X".into(),
            "test".into(),
        )
        .expect("an idle dataset accepts a DOSY job");
    assert_eq!(
        service.blocking_work_for(dataset(2)),
        Some(ComputeKind::Dosy)
    );

    assert!(service.cancel(dataset(2), ComputeKind::Dosy));
    assert_eq!(service.progress(dataset(2), ComputeKind::Dosy), None);
    assert_eq!(
        service.blocking_work_for(dataset(2)),
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
    let fields = processing_fields();
    service
        .request_2d_full(
            dataset(4),
            &fields,
            data_2d(),
            Params2D::default_for(preset),
        )
        .unwrap();
    assert_eq!(
        service.blocking_work_for(dataset(4)),
        Some(ComputeKind::Processing2D)
    );
}

#[test]
fn cancelling_processing_discards_its_result_and_releases_the_service() {
    let mut service = ComputeService::new();
    let preset = Preset2D::Cosy;
    let fields = processing_fields();
    service
        .request_2d_full(
            dataset(3),
            &fields,
            data_2d(),
            Params2D::default_for(preset),
        )
        .unwrap();

    assert!(service.cancel(dataset(3), ComputeKind::Processing2D));
    assert_eq!(
        service.progress(dataset(3), ComputeKind::Processing2D),
        None
    );

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
    let token = CancellationToken::new();
    token.cancel();
    let stack = stack_spectrum();
    let done = run_job(Job::Ilt {
        generation: 7,
        dataset: dataset(2),
        epoch: 0,
        token,
        stack,
        b_factors: vec![0.0, 1.0, 2.0],
        d_grid: vec![1e-10, 1e-9],
        lambda: 0.01,
        params: IltParams::default(),
        values: vec![0.0, 1.0, 2.0],
        meta: diffusion_meta(),
        nucleus: "X".into(),
        source: "test".into(),
    });

    assert!(matches!(
        done,
        Done::Cancelled {
            generation: 7,
            dataset: id,
            kind: ComputeKind::Ilt
        } if id == dataset(2)
    ));
}

#[test]
fn reapply_overlaps_geometry_but_waits_to_deliver_then_dispatches_latest_recipe() {
    let mut service = ComputeService::new();
    let source = VersionedFieldRef {
        field: FieldRef {
            resource: dataset(0),
            field: FieldId::new(0),
        },
        version: FieldVersion(1),
    };
    let key = ContourGeometryCacheKey {
        source,
        levels: crate::state::ResolvedContourLevels {
            positive: Arc::from([]),
            negative: Arc::from([]),
        },
    };
    assert!(service.field_runtime.begin_geometry(key.clone()));
    let base = execute_2d(
        &data_2d().source,
        &Params2D::default_for(Preset2D::Cosy),
        DelayPolicy::Disabled,
        RecipeRange::Base,
        None,
        &mut nmr::ExecutionContext::default(),
    )
    .unwrap()
    .source;
    for _ in 0..20 {
        service
            .request_2d_reapply(
                dataset(0),
                &processing_fields(),
                base.clone(),
                Params2D::default_for(Preset2D::Cosy),
            )
            .unwrap();
        assert!(
            service
                .active
                .contains_key(&(dataset(0), ComputeKind::Processing2D))
        );
    }
    let latest = service.deferred_processing[&dataset(0)].version;
    // Another dataset is independent of this preview's downstream work.
    service
        .request_2d_reapply(
            dataset(1),
            &processing_fields(),
            base,
            Params2D::default_for(Preset2D::Cosy),
        )
        .unwrap();
    assert!(
        service
            .active
            .contains_key(&(dataset(1), ComputeKind::Processing2D))
    );
    let deadline = Instant::now() + Duration::from_secs(2);
    while service.completed_processing.is_empty() {
        assert!(Instant::now() < deadline);
        assert!(!service.try_drain().iter().any(
            |done| matches!(done, Done::Processing2D { dataset: id, .. } if *id == dataset(0))
        ));
        thread::sleep(Duration::from_millis(1));
    }
    service.field_runtime.finish_geometry_request(&key);
    let delivered = service.try_drain();
    assert!(
        delivered.iter().any(
            |done| matches!(done, Done::Processing2D { dataset: id, .. } if *id == dataset(0))
        )
    );
    assert_eq!(
        service.active[&(dataset(0), ComputeKind::Processing2D)].generation,
        latest.0
    );
    assert!(!service.deferred_processing.contains_key(&dataset(0)));
}

#[test]
fn held_processing_respects_estimate_to_geometry_boundary_and_cancellation() {
    for cancel in [false, true] {
        let mut service = ComputeService::new();
        let (done_tx, done_rx) = std::sync::mpsc::channel();
        service.done_rx = done_rx;
        let params = Params2D::default_for(Preset2D::Cosy);
        let processed = execute_2d(
            &data_2d().source,
            &params,
            DelayPolicy::Disabled,
            RecipeRange::All,
            None,
            &mut nmr::ExecutionContext::default(),
        )
        .unwrap();
        let source = VersionedFieldRef {
            field: FieldRef {
                resource: dataset(0),
                field: FieldId::new(0),
            },
            version: FieldVersion(1),
        };
        let estimate = EstimateKey {
            source,
            kind: crate::state::EstimateKind::Noise,
            estimator: plotx_figure::EstimatorSelection::FollowLatest,
        };
        service.field_runtime.begin_estimate(estimate.clone());
        service.active.insert(
            (dataset(0), ComputeKind::Processing2D),
            ActiveJob {
                generation: 2,
                started_at: Instant::now(),
                token: CancellationToken::new(),
                processing_input: Some(ProcessingInputKind::Reapply),
            },
        );
        done_tx
            .send(Done::EstimateField {
                key: estimate,
                result: EstimateResult::Scale(crate::state::ScaleEstimate {
                    scale: crate::state::EstimatedScale::Degenerate,
                    provenance: crate::state::EstimateProvenance {
                        estimator: "test".into(),
                        version: 1,
                    },
                }),
            })
            .unwrap();
        done_tx
            .send(Done::Processing2D {
                version: FieldVersion(2),
                dataset: dataset(0),
                base: None,
                processed,
                fields: vec![],
                params,
            })
            .unwrap();
        let delivered = service.try_drain();
        assert_eq!(delivered.len(), 1);
        assert!(matches!(delivered[0], Done::EstimateField { .. }));
        assert_eq!(service.completed_processing.len(), 1);
        // The app resolves the estimate and queues geometry between polls.
        let geometry = ContourGeometryCacheKey {
            source,
            levels: crate::state::ResolvedContourLevels {
                positive: Arc::from([]),
                negative: Arc::from([]),
            },
        };
        service.field_runtime.begin_geometry(geometry.clone());
        assert!(service.try_drain().is_empty());
        if cancel {
            assert!(service.cancel(dataset(0), ComputeKind::Processing2D));
        }
        service.field_runtime.finish_geometry_request(&geometry);
        let delivered = service.try_drain();
        assert_eq!(delivered.len(), usize::from(!cancel));
        assert!(!service.is_busy());
    }
}
