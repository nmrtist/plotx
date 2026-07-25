//! Degenerate estimate results versus genuinely unavailable estimators.
//!
//! A flat field has no noise to measure, and saying so is a *result*: it is
//! cached, keeps its provenance, and still draws contours. An estimator that
//! cannot run at all is an error: it reaches the status line and is never
//! cached, so the two can never be confused for one another.

use super::tests::{grid_dataset, wait_for_app_compute};
use crate::contour_probe;
use crate::state::{ChartSpec, DataBinding, DataDomain, PlotxApp, StackSpec};
use plotx_figure::{
    ContourBasePolicy, ContourSpec, EstimatorSelection, PositiveFiniteF64, SeriesEncoding,
};

/// A tilted plane: every first difference is identical, so the robust MAD of
/// those differences is exactly zero — the same degenerate case an ideal
/// noiseless synthetic grid produces — while the field itself still spans a
/// range that marching squares can cut.
fn planar_values() -> Vec<f32> {
    (0..4u8)
        .flat_map(|row| (0..4u8).map(move |col| 1.0 + f32::from(row) + f32::from(col)))
        .collect()
}

fn contour_binding(app: &PlotxApp) -> DataBinding {
    let binding = DataBinding::single(&app.doc.datasets[0]);
    let SeriesEncoding::Contour(contour) = &binding.series[0].encoding else {
        panic!("a true-2D NMR field defaults to a contour encoding");
    };
    assert!(
        matches!(contour.positive.base, ContourBasePolicy::NoiseSigma { .. }),
        "a noise-scale field defaults to a NoiseSigma base"
    );
    binding
}

fn rebuild(app: &mut PlotxApp, binding: &DataBinding) -> plotx_figure::Figure {
    app.build_binding_figure(
        binding,
        &ChartSpec::default_for(DataDomain::Nmr2d),
        &StackSpec::default(),
        [120.0, 80.0],
    )
}

#[test]
fn a_degenerate_scale_estimate_draws_contours_and_is_never_requeued() {
    let mut app = PlotxApp::new_with_settings(crate::settings::Settings::default());
    app.doc
        .datasets
        .push(grid_dataset("flat", &planar_values()));
    let binding = contour_binding(&app);

    contour_probe::reset();
    let pending = rebuild(&mut app, &binding);
    assert!(pending.contours.is_empty(), "the estimate is still pending");
    assert_eq!(contour_probe::queued_estimates(), 1);

    // The estimate settles, then the geometry it unblocked.
    wait_for_app_compute(&mut app);
    let resolved = rebuild(&mut app, &binding);
    assert_eq!(
        contour_probe::queued_estimates(),
        1,
        "a zero scale is a cached result, so the resolver never re-queues it"
    );
    assert!(resolved.contours.is_empty(), "geometry is still pending");
    wait_for_app_compute(&mut app);

    let drawn = rebuild(&mut app, &binding);
    assert!(
        !drawn.contours.is_empty(),
        "a field with no measurable noise still gets a visible ladder \
         instead of a permanently blank plot"
    );
    assert_eq!(
        contour_probe::queued_estimates(),
        1,
        "repeated rebuilds must not re-queue an estimate that already answered"
    );
    assert!(
        !app.session.status.contains("Field estimate could not"),
        "a degenerate measurement is not an error: {}",
        app.session.status
    );
}

#[test]
fn an_unavailable_estimator_reports_an_error_and_is_not_cached_as_degenerate() {
    let mut app = PlotxApp::new_with_settings(crate::settings::Settings::default());
    app.doc
        .datasets
        .push(grid_dataset("flat", &planar_values()));
    let mut binding = contour_binding(&app);
    let SeriesEncoding::Contour(contour) = &mut binding.series[0].encoding else {
        unreachable!("checked while building the binding");
    };
    *contour = ContourSpec {
        positive: plotx_figure::ContourLevelSpec {
            base: ContourBasePolicy::NoiseSigma {
                multiplier: PositiveFiniteF64::new(5.0).unwrap(),
                estimator: EstimatorSelection::Frozen {
                    estimator: "retired_estimator".to_owned(),
                    version: 99,
                },
            },
            count: 3,
            ratio: PositiveFiniteF64::new(1.5).unwrap(),
        },
        negative: None,
        style: contour.style.clone(),
    };

    contour_probe::reset();
    let figure = rebuild(&mut app, &binding);
    assert!(figure.contours.is_empty());
    assert_eq!(contour_probe::queued_estimates(), 1);

    wait_for_app_compute(&mut app);
    assert!(
        app.session
            .status
            .contains("Field estimate could not be computed"),
        "a frozen estimator that no longer exists must reach the user: {}",
        app.session.status
    );

    let still_blank = rebuild(&mut app, &binding);
    assert!(still_blank.contours.is_empty());
    assert_eq!(
        contour_probe::queued_estimates(),
        2,
        "an unavailable estimator produced no cacheable result, degenerate or otherwise"
    );
}
