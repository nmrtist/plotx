//! What happens on the frames between asking for derived field work and getting
//! it back.
//!
//! Two things used to happen, both wrong. The plot said nothing at all, so a
//! build that was merely slow looked exactly like one that had failed. And the
//! rebuild materialized the field's whole payload before asking whether the job
//! it was about to queue was already running, so a 2048 × 8192 plane was read
//! and cloned once per frame for the entire life of a job that had been accepted
//! on the first.

use super::tests::{grid_dataset, wait_for_app_compute};
use super::*;
use crate::contour_probe;
use crate::state::{ChartSpec, DataBinding, DataDomain, PlotxApp, StackSpec};
use plotx_figure::{
    ContourLevelSpec, ContourSpec, ContourStyle, PositiveFiniteF64, SeriesEncoding,
};

/// A 4×4 signed field with enough structure for a robust noise estimate.
fn signed_values() -> Vec<f32> {
    vec![
        -4.0, -1.0, 3.0, 1.0, 2.0, -3.0, 4.0, -2.0, 1.0, 5.0, -5.0, 3.0, -2.0, 4.0, 0.0, 6.0,
    ]
}

fn app_with_contour(encoding: Option<ContourSpec>) -> (PlotxApp, DataBinding) {
    let mut app = PlotxApp::new_with_settings(crate::settings::Settings::default());
    app.doc
        .datasets
        .push(grid_dataset("progress", &signed_values()));
    let mut binding = DataBinding::single(&app.doc.datasets[0]);
    if let Some(spec) = encoding {
        binding.series[0].encoding = SeriesEncoding::Contour(spec);
    }
    assert!(
        matches!(binding.series[0].encoding, SeriesEncoding::Contour(_)),
        "a true-2D NMR field defaults to a contour encoding"
    );
    (app, binding)
}

fn absolute_spec() -> ContourSpec {
    let level = ContourLevelSpec {
        base: ContourBasePolicy::Absolute(PositiveFiniteF64::new(1.0).expect("literal is valid")),
        count: 3,
        ratio: PositiveFiniteF64::new(1.5).expect("literal is valid"),
    };
    ContourSpec {
        positive: level.clone(),
        negative: Some(level),
        style: ContourStyle::default(),
    }
}

fn rebuild(app: &mut PlotxApp, binding: &DataBinding) -> plotx_figure::Figure {
    app.build_binding_figure(
        binding,
        &ChartSpec::default_for(DataDomain::Nmr2d),
        &StackSpec::default(),
        [120.0, 80.0],
    )
}

/// While a contour build is with the workers, repeating the rebuild must not
/// touch the field's values again.
///
/// Deduplication already happened inside the enqueue, but only after a payload
/// had been built to hand it — so the frame cost of an in-flight job was one
/// full copy of the grid. Results are installed by `poll_compute`, so declining
/// to call it is exactly "the job is still running".
#[test]
fn rebuilding_while_a_contour_build_is_in_flight_materializes_no_further_payload() {
    let (mut app, binding) = app_with_contour(Some(absolute_spec()));

    contour_probe::reset();
    let first = rebuild(&mut app, &binding);
    assert!(first.contours.is_empty(), "the geometry is still pending");
    assert_eq!(contour_probe::queued_contour_builds(), 1);
    let materialized = contour_probe::field_payload_materializations();
    assert!(materialized > 0, "the queued job was handed a real grid");

    for _ in 0..8 {
        let pending = rebuild(&mut app, &binding);
        assert!(pending.contours.is_empty());
    }
    assert_eq!(
        contour_probe::queued_contour_builds(),
        1,
        "the in-flight set already deduplicates the job itself"
    );
    assert_eq!(
        contour_probe::field_payload_materializations(),
        materialized,
        "an in-flight build must be recognized before its input is built, not \
         after: otherwise every frame of the wait copies the whole field"
    );

    // The warm path is unchanged: the finished geometry draws, still without
    // marching squares on this thread.
    wait_for_app_compute(&mut app);
    let drawn = rebuild(&mut app, &binding);
    assert!(!drawn.contours.is_empty());
    assert_eq!(contour_probe::marching_squares_on_this_thread(), 0);
}

/// The same, one stage earlier: a pending noise estimate.
#[test]
fn rebuilding_while_an_estimate_is_in_flight_materializes_no_further_payload() {
    let (mut app, binding) = app_with_contour(None);

    contour_probe::reset();
    let first = rebuild(&mut app, &binding);
    assert!(first.contours.is_empty());
    assert_eq!(contour_probe::queued_estimates(), 1);
    let materialized = contour_probe::field_payload_materializations();
    assert!(materialized > 0);

    for _ in 0..8 {
        rebuild(&mut app, &binding);
    }
    assert_eq!(contour_probe::queued_estimates(), 1);
    assert_eq!(
        contour_probe::field_payload_materializations(),
        materialized,
        "an in-flight estimate must be recognized before its input is built"
    );
}

/// A plot that is empty because its geometry is still being built says so.
///
/// An empty plot with no explanation is indistinguishable from a broken one,
/// which is half the reason the crash this work came from was undiagnosable.
#[test]
fn a_pending_contour_build_says_it_is_building() {
    let (mut app, binding) = app_with_contour(Some(absolute_spec()));
    app.session.status.clear();

    let figure = rebuild(&mut app, &binding);
    assert!(figure.contours.is_empty());
    assert_eq!(app.session.status, "Building contour geometry…");

    // And stops saying it once there is something to look at.
    wait_for_app_compute(&mut app);
    app.session.status.clear();
    let drawn = rebuild(&mut app, &binding);
    assert!(!drawn.contours.is_empty());
    assert!(
        app.session.status.is_empty(),
        "a finished plot has nothing to report: {}",
        app.session.status
    );
}

/// The wait is named, not generic: a noise estimate and a background fit take
/// visibly different times, and the anchor the user just chose decides which one
/// is running.
#[test]
fn a_pending_estimate_names_the_measurement_it_is_waiting_for() {
    let (mut app, binding) = app_with_contour(None);
    app.session.status.clear();

    let figure = rebuild(&mut app, &binding);
    assert!(figure.contours.is_empty());
    assert_eq!(app.session.status, "Measuring this field's noise scale…");
}
