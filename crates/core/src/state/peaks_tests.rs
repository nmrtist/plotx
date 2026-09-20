use super::*;

fn trace(ys: Vec<f64>) -> Trace1d {
    Trace1d {
        xs: (0..ys.len()).map(|i| i as f64).collect(),
        ys,
        x_reversed: false,
    }
}

/// A strong apex at x = 5 and a weak one at x = 10.
fn two_peaks() -> Trace1d {
    let mut ys = vec![0.0; 16];
    ys[4] = 40.0;
    ys[5] = 100.0;
    ys[6] = 40.0;
    ys[9] = 2.0;
    ys[10] = 5.0;
    ys[11] = 2.0;
    trace(ys)
}

#[test]
fn a_narrow_window_picks_the_weak_apex_beside_a_strong_one() {
    let trace = two_peaks();
    // Clicking at the weak line while zoomed in: the pixel-derived window no
    // longer reaches the strong apex.
    assert_eq!(trace.snap_within(10.4, 2.0), (10.0, 5.0));
}

#[test]
fn a_wide_window_still_lands_on_the_tallest_apex() {
    let trace = two_peaks();
    // Zoomed out, the same click may sit several samples off; the tallest
    // apex in reach is the intended target, not the nearest noise wiggle.
    assert_eq!(trace.snap_within(7.0, 6.0), (5.0, 100.0));
}

#[test]
fn free_placement_takes_the_nearest_sample_without_an_apex_search() {
    let trace = two_peaks();
    assert_eq!(trace.pick(9.4, ManualPeakSnap::NearestSample), (9.0, 2.0));
}

#[test]
fn an_empty_window_falls_back_to_the_nearest_sample() {
    let trace = two_peaks();
    // No local maximum within reach of x = 14.
    assert_eq!(trace.snap_within(14.2, 1.0), (14.0, 0.0));
}

#[test]
fn apex_snap_routes_through_pick() {
    let trace = two_peaks();
    assert_eq!(
        trace.pick(10.4, ManualPeakSnap::Apex { half_width: 2.0 }),
        (10.0, 5.0)
    );
}

/// A small frequency-domain spectrum with one clear line, loaded as a dataset.
fn frequency_app() -> crate::state::PlotxApp {
    let mut points = vec![num_complex::Complex64::new(0.0, 0.0); 64];
    points[32] = num_complex::Complex64::new(100.0, 0.0);
    points[31] = num_complex::Complex64::new(40.0, 0.0);
    points[33] = num_complex::Complex64::new(40.0, 0.0);
    let data = plotx_io::NmrData {
        points,
        domain: plotx_io::Domain::Frequency,
        spectral_width_hz: 640.0,
        observe_freq_mhz: 100.0,
        carrier_ppm: 5.0,
        nucleus: "1H".into(),
        source: "test".into(),
        group_delay: 0.0,
    };
    let mut app = crate::state::PlotxApp::new();
    app.doc.datasets.push(crate::state::Dataset::Nmr(Box::new(
        crate::state::NmrDataset::load(data).unwrap(),
    )));
    app
}

fn apply_reference(app: &mut crate::state::PlotxApp, at_ppm: f64, target_ppm: f64) {
    let nmr = app.doc.datasets[0].as_nmr_mut().expect("NMR dataset");
    let id = plotx_processing::StepId::new(nmr.next_step_id);
    nmr.next_step_id += 1;
    nmr.pipeline
        .steps
        .push(plotx_processing::ProcessingStep::new(
            id,
            plotx_processing::StepKind::Reference(plotx_processing::ReferenceParams {
                at_ppm,
                target_ppm,
            }),
            plotx_processing::StepSource::User,
        ));
    let nmr = app.doc.datasets[0].as_nmr_mut().expect("NMR dataset");
    nmr.rebuild().unwrap();
}

fn resolved_marks(app: &crate::state::PlotxApp) -> Vec<ResolvedPeak> {
    let dataset = &app.doc.datasets[0];
    dataset
        .peaks()
        .expect("peak set")
        .resolve(dataset.peak_reference_offset_ppm())
}

/// The reported defect: mark a peak, then edit the Reference step — the mark
/// must follow the recalibrated axis instead of pinning the old coordinates.
#[test]
fn marks_follow_a_later_reference_edit() {
    let mut app = frequency_app();
    let apex_x = app.doc.datasets[0]
        .displayed_trace(None)
        .expect("1D trace")
        .xs[32];
    app.add_manual_peak(0, apex_x, None, ManualPeakSnap::NearestSample);
    let before = resolved_marks(&app);
    assert_eq!(before.len(), 1);
    assert!((before[0].x - apex_x).abs() < 1e-12);

    apply_reference(&mut app, apex_x, apex_x + 0.5);

    let after = resolved_marks(&app);
    assert!((after[0].x - (apex_x + 0.5)).abs() < 1e-12);
    // The mark tracks the shifted trace: the same array position now reads
    // the mark's resolved x.
    let shifted = app.doc.datasets[0]
        .displayed_trace(None)
        .expect("1D trace")
        .xs[32];
    assert!((after[0].x - shifted).abs() < 1e-12);
    // The default label reads the calibrated position.
    assert_eq!(after[0].label, format!("{:.2}", after[0].x));
}

/// Picks made on an already-referenced spectrum resolve back to the clicked
/// finished coordinate (the stored value is uncalibrated).
#[test]
fn picks_on_a_referenced_spectrum_round_trip() {
    let mut app = frequency_app();
    apply_reference(&mut app, 0.0, 0.75);
    let apex_x = app.doc.datasets[0]
        .displayed_trace(None)
        .expect("1D trace")
        .xs[32];

    app.add_manual_peak(0, apex_x, None, ManualPeakSnap::NearestSample);

    let resolved = resolved_marks(&app);
    assert!((resolved[0].x - apex_x).abs() < 1e-12);
    let stored = &app.doc.datasets[0].peaks().expect("peak set").marks[0];
    assert!((stored.x - (apex_x - 0.75)).abs() < 1e-12);
}
