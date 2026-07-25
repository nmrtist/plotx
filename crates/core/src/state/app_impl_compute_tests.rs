use super::*;
use num_complex::Complex64;
use plotx_io::{Dim, Domain, NmrData2D, QuadMode};
use plotx_processing::{ProcessingStep, StepKind, StepSource};
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
