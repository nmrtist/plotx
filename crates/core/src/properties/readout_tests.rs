//! The live readout may only ever *read*.
//!
//! These tests are the guard on the seam: a readout must report a cache miss
//! rather than filling it, so drawing the interface can never queue an estimate,
//! run marching squares on the calling thread, or materialize a field payload.

use crate::contour_probe;
use crate::properties::tests::contour_app;
use crate::properties::{ContourAnchor, PropertyValue, contour};
use crate::state::{
    CONTOUR_BASE_ABSOLUTE, CONTOUR_BASE_NOISE_FLOOR, ChartSpec, DataBinding, DataDomain, PlotxApp,
    StackSpec,
};
use std::time::{Duration, Instant};

fn binding(app: &PlotxApp) -> DataBinding {
    app.doc.canvases[0]
        .objects
        .first()
        .and_then(|object| object.plot())
        .expect("the fixture holds one plot")
        .binding
        .clone()
}

/// Draw the fixture's plot once, the way a frame does.
fn rebuild(app: &mut PlotxApp) {
    let binding = binding(app);
    app.build_binding_figure(
        &binding,
        &ChartSpec::default_for(DataDomain::Nmr2d),
        &StackSpec::default(),
        [120.0, 80.0],
    );
}

fn settle(app: &mut PlotxApp) {
    let deadline = Instant::now() + Duration::from_secs(2);
    while app.compute_busy() && Instant::now() < deadline {
        app.poll_compute();
        std::thread::sleep(Duration::from_millis(5));
    }
    app.poll_compute();
    assert!(!app.compute_busy(), "field work did not settle in time");
}

/// Rebuild until both the estimate and the geometry have landed.
fn warm(app: &mut PlotxApp) {
    for _ in 0..4 {
        rebuild(app);
        settle(app);
    }
}

/// §4.3: before anything is measured, the readout says so instead of showing a
/// number nobody computed — and asking does not start the measurement.
#[test]
fn an_unmeasured_anchor_reads_as_measuring_without_queueing_anything() {
    let (app, target) = contour_app();

    contour_probe::reset();
    let readout = app
        .contour_base_readout(&target)
        .expect("the fixture draws a contour");

    assert_eq!(readout.kind, CONTOUR_BASE_NOISE_FLOOR);
    assert_eq!(readout.anchor, ContourAnchor::Measuring);
    assert_eq!(readout.lowest_level, None);
    assert_eq!(contour_probe::queued_estimates(), 0);
    assert_eq!(contour_probe::queued_contour_builds(), 0);
    assert_eq!(contour_probe::field_payload_materializations(), 0);
    assert_eq!(contour_probe::marching_squares_on_this_thread(), 0);
}

/// The warm path: a cached noise estimate resolves the multiple into a level,
/// and reading it repeatedly costs nothing at all — no payload, no job, no
/// marching squares on this thread.
#[test]
fn a_cached_estimate_resolves_the_level_and_reading_it_stays_free() {
    let (mut app, target) = contour_app();
    warm(&mut app);

    contour_probe::reset();
    let readout = app
        .contour_base_readout(&target)
        .expect("the fixture draws a contour");

    assert_eq!(readout.anchor, ContourAnchor::Measured);
    assert_eq!(readout.kind, CONTOUR_BASE_NOISE_FLOOR);
    let level = readout.lowest_level.expect("a measured anchor has a level");
    assert!(level > 0.0, "a positive half's lowest level is positive");
    assert!(
        (level - readout.magnitude).abs() > f64::EPSILON,
        "the readout must resolve the multiple, not repeat it: {readout:?}"
    );

    for _ in 0..8 {
        assert_eq!(app.contour_base_readout(&target), Some(readout));
    }
    assert_eq!(
        contour_probe::queued_estimates(),
        0,
        "a readout must never queue an estimate"
    );
    assert_eq!(contour_probe::queued_contour_builds(), 0);
    assert_eq!(
        contour_probe::field_payload_materializations(),
        0,
        "a readout must never materialize a field payload"
    );
    assert_eq!(
        contour_probe::marching_squares_on_this_thread(),
        0,
        "a readout must never run marching squares on the calling thread"
    );
}

/// §4.3, and the whole point of spelling the floor into the policy: when the
/// floor is what the level came from, the readout says so rather than passing
/// the level off as a multiple of an estimate that did not produce it.
///
/// The plane is flat noise around zero with one feature a million times larger —
/// the shape of every spectrum with a dominant diagonal or solvent peak — so its
/// robust σ is far below the anchor's floor.
#[test]
fn a_field_whose_sigma_falls_under_the_floor_reads_as_floored() {
    let mut plane = vec![0.0, 1.0, 0.0, 1.0, 1.0, 0.0, 1.0, 0.0];
    plane.extend([0.0, 1.0, 0.0, 1.0, 1.0, 0.0, 1.0]);
    plane.push(1.0e6);
    let (mut app, target) = crate::properties::tests::contour_app_with_plane(&plane);
    warm(&mut app);

    contour_probe::reset();
    let readout = app
        .contour_base_readout(&target)
        .expect("the fixture draws a contour");

    assert_eq!(readout.kind, CONTOUR_BASE_NOISE_FLOOR);
    assert_eq!(
        readout.anchor,
        ContourAnchor::Floored,
        "a field this far past the floor cannot be described as `5 × σ`: {readout:?}"
    );
    assert_eq!(readout.peak_fraction, Some(1.0e-4));
    let level = readout.lowest_level.expect("a floored anchor has a level");
    assert!(
        (level - readout.magnitude * 1.0e-4 * 1.0e6).abs() < 1.0e-6,
        "the level comes from the floor, not the estimate: {readout:?}"
    );
    assert_eq!(contour_probe::queued_estimates(), 0);
    assert_eq!(contour_probe::field_payload_materializations(), 0);
}

/// §4.3 on a policy that needs no measurement: an absolute level is its own
/// level, and the readout says so directly rather than reporting `Measuring`.
#[test]
fn a_directly_anchored_base_needs_no_estimate() {
    let (mut app, target) = contour_app();
    warm(&mut app);
    let commit = app
        .plan_property_write(
            contour::BASE_POLICY,
            std::slice::from_ref(&target),
            &PropertyValue::Enum(CONTOUR_BASE_ABSOLUTE),
        )
        .expect("an absolute base needs no capability");
    app.commit_property(commit);
    warm(&mut app);

    contour_probe::reset();
    let readout = app.contour_base_readout(&target).expect("still a contour");

    assert_eq!(readout.anchor, ContourAnchor::Direct);
    assert_eq!(contour_probe::field_payload_materializations(), 0);
    assert_eq!(contour_probe::queued_estimates(), 0);
}
