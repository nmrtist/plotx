use super::*;
use crate::state::{AlignOutcome, AlignTargetMode};
use plotx_io::{Domain, NmrData};
use std::f64::consts::TAU;

fn synthetic_at(peak_ppm: f64, nucleus: &str) -> NmrData {
    let npoints = 512;
    let sw = 4000.0;
    let obs = 400.0;
    let carrier = 5.0;
    let dt = 1.0 / sw;
    let freq_hz = (peak_ppm - carrier) * obs;
    let points = (0..npoints)
        .map(|k| {
            let t = k as f64 * dt;
            Complex64::from_polar((-t / 0.25).exp(), TAU * freq_hz * t)
        })
        .collect();
    NmrData {
        points,
        domain: Domain::Time,
        spectral_width_hz: sw,
        observe_freq_mhz: obs,
        carrier_ppm: carrier,
        nucleus: nucleus.to_owned(),
        source: "synthetic".to_owned(),
        group_delay: 0.0,
    }
}

fn app_with(peaks: &[f64]) -> PlotxApp {
    let mut app = PlotxApp::new();
    app.doc.save_include_view_snapshots = false;
    for &p in peaks {
        app.doc
            .datasets
            .push(Dataset::Nmr(Box::new(NmrDataset::load(synthetic_at(
                p, "1H",
            )))));
    }
    let all: Vec<usize> = (0..peaks.len()).collect();
    app.focus_datasets(&all, Some(0));
    app
}

fn peak_ppm(app: &PlotxApp, di: usize) -> f64 {
    let s = &app.doc.datasets[di].as_nmr().unwrap().spectrum;
    let i = s
        .values
        .iter()
        .enumerate()
        .max_by(|a, b| a.1.re.total_cmp(&b.1.re))
        .unwrap()
        .0;
    s.ppm[i]
}

#[test]
fn align_to_reference_peak_brings_all_peaks_together() {
    let mut app = app_with(&[2.0, 2.5, 1.6]);
    let plan = app.plan_spectrum_alignment(0.0, 5.0, AlignTargetMode::ReferencePeak);
    let target = plan.target_ppm.unwrap();
    assert!((target - 2.0).abs() < 0.05);
    assert_eq!(plan.shift_count(), 3);

    app.apply_spectrum_alignment(&plan);
    for di in 0..3 {
        assert!((peak_ppm(&app, di) - target).abs() < 1e-6);
    }
}

#[test]
fn align_to_custom_target_and_single_undo_restores_everything() {
    let mut app = app_with(&[2.0, 2.5]);
    let before: Vec<_> = (0..2)
        .map(|di| app.doc.datasets[di].as_nmr().unwrap().pipeline.clone())
        .collect();

    let plan = app.plan_spectrum_alignment(0.0, 5.0, AlignTargetMode::Custom(3.0));
    app.apply_spectrum_alignment(&plan);
    for di in 0..2 {
        assert!((peak_ppm(&app, di) - 3.0).abs() < 1e-6);
    }

    app.undo();
    for (di, pipe) in before.iter().enumerate() {
        assert_eq!(&app.doc.datasets[di].as_nmr().unwrap().pipeline, pipe);
    }
    assert!((peak_ppm(&app, 0) - 2.0).abs() < 0.05);
    assert!((peak_ppm(&app, 1) - 2.5).abs() < 0.05);
    assert!(!app.can_undo());
}

#[test]
fn window_without_peak_skips_every_spectrum() {
    let mut app = app_with(&[2.0, 2.5]);
    let plan = app.plan_spectrum_alignment(8.0, 9.0, AlignTargetMode::ReferencePeak);
    assert!(plan.target_ppm.is_none());
    for row in &plan.rows {
        match &row.outcome {
            AlignOutcome::Skip(reason) => assert!(reason.contains("No significant peak")),
            _ => panic!("expected skip"),
        }
    }

    app.apply_spectrum_alignment(&plan);
    assert!(!app.can_undo());
}

#[test]
fn other_nuclei_and_non_1d_datasets_are_skipped_with_reasons() {
    let mut app = app_with(&[2.0, 2.5]);
    app.doc
        .datasets
        .push(Dataset::Nmr(Box::new(NmrDataset::load(synthetic_at(
            2.2, "13C",
        )))));
    app.doc
        .datasets
        .push(Dataset::Nmr2D(Box::new(crate::state::Nmr2DDataset::load(
            synthetic_2d(),
        ))));
    app.focus_datasets(&[0, 1, 2, 3], Some(0));
    let carbon_before = peak_ppm(&app, 2);

    let plan = app.plan_spectrum_alignment(0.0, 5.0, AlignTargetMode::Custom(3.0));
    let reason = |di: usize| match &plan.rows.iter().find(|r| r.dataset == di).unwrap().outcome {
        AlignOutcome::Skip(reason) => reason.clone(),
        _ => panic!("expected skip for dataset {di}"),
    };
    assert!(reason(2).contains("Nucleus"));
    assert!(reason(3).contains("Not a 1D spectrum"));
    assert_eq!(plan.shift_count(), 2);

    app.apply_spectrum_alignment(&plan);
    assert!((peak_ppm(&app, 0) - 3.0).abs() < 1e-6);
    assert!((peak_ppm(&app, 1) - 3.0).abs() < 1e-6);
    assert_eq!(peak_ppm(&app, 2), carbon_before);
}

#[test]
fn realignment_folds_into_the_existing_reference_step() {
    let mut app = app_with(&[2.0, 2.5]);
    let ref_steps = |app: &PlotxApp, di: usize| {
        app.doc.datasets[di]
            .as_nmr()
            .unwrap()
            .pipeline
            .steps
            .iter()
            .filter(|s| matches!(s.kind, plotx_processing::StepKind::Reference(_)))
            .count()
    };

    let plan = app.plan_spectrum_alignment(0.0, 5.0, AlignTargetMode::Custom(3.0));
    app.apply_spectrum_alignment(&plan);
    let counts: Vec<usize> = (0..2).map(|di| ref_steps(&app, di)).collect();
    assert_eq!(counts, vec![1, 1]);

    let plan = app.plan_spectrum_alignment(0.0, 5.0, AlignTargetMode::Custom(4.0));
    app.apply_spectrum_alignment(&plan);
    for di in 0..2 {
        assert_eq!(ref_steps(&app, di), 1);
        assert!((peak_ppm(&app, di) - 4.0).abs() < 1e-6);
    }

    app.undo();
    for di in 0..2 {
        assert!((peak_ppm(&app, di) - 3.0).abs() < 1e-6);
    }
}

#[test]
fn pending_paused_processing_blocks_alignment() {
    let mut app = app_with(&[2.0, 2.5]);
    app.session.ui.proc_paused = true;
    app.session.ui.proc_pending = Some((
        dataset_id(&app, 0),
        crate::actions::DatasetProcessingState::from_dataset(&app.doc.datasets[0]),
    ));

    let plan = app.plan_spectrum_alignment(0.0, 5.0, AlignTargetMode::Custom(3.0));
    app.apply_spectrum_alignment(&plan);
    assert!(!app.can_undo());
    assert!((peak_ppm(&app, 0) - 2.0).abs() < 0.05);
    assert!((peak_ppm(&app, 1) - 2.5).abs() < 0.05);
}

#[test]
fn empty_selection_scopes_to_all_datasets() {
    let mut app = app_with(&[2.0, 2.5]);
    app.clear_selection();
    let plan = app.plan_spectrum_alignment(0.0, 5.0, AlignTargetMode::ReferencePeak);
    assert_eq!(plan.rows.len(), 2);
    assert_eq!(plan.shift_count(), 2);
    assert!(app.can_align_spectra());
}

/// P0-1 regression. `apply_reference_shift` appends a step to a *live* recipe,
/// so its identity has to come from the owning dataset's allocator. When the
/// step was minted with template-local numbering it landed on `StepId(0)`,
/// aliasing the pipeline's first row: the Processing panel resolves rows with
/// `position(|s| s.id == id)`, so deleting the new Reference row deleted
/// Apodize instead. Reverting the fix makes the uniqueness assertion fail.
#[test]
fn reference_alignment_gives_the_appended_step_a_unique_identity() {
    let mut app = app_with(&[2.0]);
    let before_ids: Vec<_> = app.doc.datasets[0]
        .axis_pipeline(crate::state::PhaseAxis::Direct)
        .unwrap()
        .steps
        .iter()
        .map(|step| step.id)
        .collect();
    assert!(
        !before_ids.is_empty(),
        "the fixture starts with a populated pipeline"
    );

    let plan = app.plan_spectrum_alignment(0.0, 5.0, AlignTargetMode::Custom(3.0));
    app.apply_spectrum_alignment(&plan);

    let steps = app.doc.datasets[0]
        .axis_pipeline(crate::state::PhaseAxis::Direct)
        .unwrap()
        .steps
        .clone();
    let appended = steps
        .iter()
        .find(|step| matches!(step.kind, plotx_processing::StepKind::Reference(_)))
        .expect("alignment appended a referencing step");
    assert!(
        !before_ids.contains(&appended.id),
        "the appended step reused an identity already in the live pipeline"
    );

    let mut ids: Vec<_> = steps.iter().map(|step| step.id).collect();
    ids.sort();
    let unique = ids.len();
    ids.dedup();
    assert_eq!(
        ids.len(),
        unique,
        "step ids must be unique within a dataset"
    );

    // The allocator moved past the id it handed out, so the next step cannot
    // collide either.
    let next = app.doc.datasets[0].as_nmr().unwrap().next_step_id;
    assert!(ids.iter().all(|id| id.get() < next));
}
