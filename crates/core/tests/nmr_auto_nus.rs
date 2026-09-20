//! Automatic import uses the library estimator and retains its execution evidence.
use nmr::axis::{AxisCoordinates, AxisDomain, AxisUnit};
use nmr::raw::*;
use nmr::{Complex64, Dataset as NativeDataset};
use plotx_core::state::{Dataset, Nmr2DDataset, PlotxApp};
use plotx_io::nmr_view::NmrSource;
use std::sync::Arc;

fn synthetic() -> NmrSource {
    let (grid, observations, points) = (256, 96, 128);
    let axis = |kind, points| {
        RawAxis::new(
            kind,
            AxisDomain::Time,
            Some(AxisUnit::Second),
            points,
            AxisCoordinates::Uniform {
                start: 0.0,
                step: 0.001,
            },
        )
        .unwrap()
        .with_group_delay(nmr::acquisition::GroupDelayState::NotApplicable)
        .unwrap()
    };
    let axes = vec![
        axis(
            RawAxisKind::Indirect(IndirectComponents::Cartesian(
                ComponentEvidence::user_constructed(),
            )),
            grid,
        ),
        axis(RawAxisKind::Direct(DirectSamples::Complex), points),
    ];
    let coordinates: Vec<_> = (0..observations)
        .map(|i| SamplingCoordinate::new(vec![i * 73 % grid]))
        .collect();
    let mut rng = 7193_u64;
    let mut uniform = || {
        rng = rng.wrapping_mul(6364136223846793005).wrapping_add(1);
        ((rng >> 11) as f64 + 0.5) / (1u64 << 53) as f64
    };
    let traces = coordinates
        .iter()
        .enumerate()
        .map(|(ordinal, coordinate)| {
            let mut samples = Vec::new();
            let angle =
                std::f64::consts::TAU * 17.0 * coordinate.as_slice()[0] as f64 / grid as f64;
            for lane in [angle.cos(), angle.sin()] {
                for j in 0..points {
                    let signal = Complex64::from_polar(
                        lane * (-6.0 * j as f64 / points as f64).exp(),
                        std::f64::consts::TAU * 21.0 * j as f64 / points as f64,
                    );
                    let radius = 0.1 * (-2.0 * uniform().ln()).sqrt();
                    samples.push(
                        signal + Complex64::from_polar(radius, std::f64::consts::TAU * uniform()),
                    );
                }
            }
            SparseTrace::new(
                ObservationOrdinal::new(ordinal),
                coordinate.clone(),
                samples,
            )
        })
        .collect();
    let raw = RawDatasetBuilder::new(axes, RawMetadata::default())
        .unwrap()
        .sparse(
            traces,
            SamplingSchedule::new(vec![grid], coordinates).unwrap(),
        )
        .unwrap();
    NmrSource::new(Arc::new(raw.into())).unwrap()
}

fn assert_frequency(dataset: &Nmr2DDataset) {
    assert!(dataset.is_true_2d());
    assert!(dataset.reconstruction_warning.is_none());
    assert!(
        dataset.nus_request.is_none(),
        "automatic sigma is result evidence, not an override"
    );
    let processed = dataset.native_processed.dataset().as_processed().unwrap();
    assert!(
        processed
            .descriptor()
            .axes()
            .iter()
            .all(|axis| axis.domain() == AxisDomain::Frequency)
    );
    assert!(
        dataset
            .native_processed
            .dataset()
            .as_dense_processed()
            .unwrap()
            .samples()
            .iter()
            .all(|x| x.is_finite())
    );
}

fn assert_auto_evidence(dataset: &NativeDataset) {
    let mut bytes = Vec::new();
    nmr::execution_report::write_json(
        dataset.as_processed().unwrap(),
        &[],
        &mut bytes,
        16 * 1024 * 1024,
    )
    .unwrap();
    let json = String::from_utf8(bytes).unwrap();
    assert!(
        [
            "split-observation-cartesian-rms.v1",
            "split-holdout-component-rms.v1",
            "jeol-interior-split-rms.v1",
            "jeol-interior-holdout-rms.v1",
        ]
        .iter()
        .any(|method| json.contains(method)),
        "{json}"
    );
}

#[test]
fn automatic_import_and_offline_project_reopen_produce_2d() {
    let dataset = Nmr2DDataset::load(synthetic()).unwrap();
    assert_frequency(&dataset);
    assert_auto_evidence(dataset.native_processed.dataset());
    let mut app = PlotxApp::new();
    app.doc.datasets.push(Dataset::Nmr2D(Box::new(dataset)));
    assert!(app.schedule_2d_processing(0, true));
    let started = std::time::Instant::now();
    while app.session.compute.is_busy() {
        assert!(started.elapsed() < std::time::Duration::from_secs(30));
        std::thread::sleep(std::time::Duration::from_millis(5));
        app.poll_compute();
    }
    app.poll_compute();
    assert_eq!(app.session.status, "Updated 2D processing.");
    assert_frequency(app.doc.datasets[0].as_nmr2d().unwrap());
    let path = std::env::temp_dir().join(format!("plotx-auto-nus-{}.plotx", uuid::Uuid::new_v4()));
    plotx_core::project::save_project(&app, &path, false).unwrap();
    let reopened = plotx_core::project::load_project(&path).unwrap();
    std::fs::remove_file(path).unwrap();
    let dataset = reopened.doc.datasets[0].as_nmr2d().unwrap();
    assert_frequency(dataset);
    assert_auto_evidence(dataset.native_processed.dataset());
}

#[test]
fn failed_auto_estimation_keeps_observations_and_reports_the_reason() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../io/tests/fixtures/nmr/bruker-nus");
    let mut app = PlotxApp::new();
    app.load_from(&path);
    assert_eq!(app.doc.datasets.len(), 1);
    let data = app.doc.datasets[0].as_nmr2d().unwrap();
    assert!(!data.is_true_2d());
    assert!(data.native_processed.dataset().as_raw().is_some());
    let warning = data.reconstruction_warning.as_ref().unwrap();
    assert!(
        warning.contains("automatic NUS noise requires"),
        "{warning}"
    );
    assert!(app.session.status.contains(warning));
    let loaded = plotx_core::workflow::load_dataset(&path).unwrap();
    assert!(
        loaded
            .inspection
            .warnings
            .iter()
            .any(|warning| warning.code == "nmr-reconstruction-failed"
                && warning.message.contains("automatic NUS noise requires"))
    );
}

#[test]
fn automatic_noise_analysis_honors_cancellation_and_work_limits() {
    use plotx_processing::nmr_bridge::{DelayPolicy, RecipeRange};
    use plotx_processing::nmr_execution::{execute_2d, processing_2d_work_ledger};
    let input = synthetic();
    let params = plotx_processing::Params2D::default_for(plotx_processing::Preset2D::Generic);
    let token = nmr::CancellationToken::new();
    let cancel = token.clone();
    let mut saw_noise = false;
    let mut progress = |event: nmr::execution::ProgressEvent| {
        if event.stage == nmr::execution::ExecutionStage::NoiseEstimation {
            saw_noise = true;
            cancel.cancel();
        }
    };
    let mut work = processing_2d_work_ledger();
    let mut context = nmr::ExecutionContext::new(&mut work)
        .with_cancellation(token)
        .with_progress(&mut progress);
    let error = execute_2d(
        &input,
        &params,
        DelayPolicy::AxisEvidence,
        RecipeRange::Base,
        None,
        &mut context,
    )
    .unwrap_err();
    assert!(error.is_cancelled(), "{error}");
    assert!(saw_noise);

    let mut work = nmr::resource::WorkLedger::new(1);
    let error = execute_2d(
        &input,
        &params,
        DelayPolicy::AxisEvidence,
        RecipeRange::Base,
        None,
        &mut nmr::ExecutionContext::new(&mut work),
    )
    .unwrap_err();
    assert!(!error.is_cancelled());
    assert!(error.to_string().contains("work"), "{error}");
}

#[test]
#[ignore = "requires a local NMR acquisition path in PLOTX_NUS_SAMPLE"]
fn local_nus_import_uses_the_default_plotx_recipe() {
    let path = std::env::var_os("PLOTX_NUS_SAMPLE").expect("set PLOTX_NUS_SAMPLE");
    let mut app = PlotxApp::new();
    app.load_from(std::path::Path::new(&path));
    assert_eq!(app.doc.datasets.len(), 1, "{}", app.session.status);
    let dataset = app.doc.datasets[0].as_nmr2d().unwrap();
    assert_frequency(dataset);
    assert_auto_evidence(dataset.native_processed.dataset());
    println!(
        "shape={:?}",
        dataset
            .native_processed
            .dataset()
            .as_processed()
            .unwrap()
            .descriptor()
            .logical_shape()
    );
}
