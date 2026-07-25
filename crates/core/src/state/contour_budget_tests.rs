//! The renderer's segment budget: what a field too dense to draw produces, and
//! what the user is told about it.
//!
//! A contour ladder is not bounded by its level count. A single level over a
//! large grid can cross millions of cells, and every crossing becomes a drawn
//! segment; a 2048×8192 spectrum whose lowest level sat in the noise produced
//! 8.4 million of them and asked the GPU for a 1.0 GB index buffer, which is a
//! device validation error rather than a slow frame. These tests fix the two
//! properties that make that impossible: the geometry a build hands back is
//! bounded, and a bounded-down build says so.

use super::compute_field::run_build_contour;
use crate::state::{
    AxisSampling, ChartSpec, ContourGeometryCacheKey, DataBinding, DataDomain, Dataset, DatasetId,
    FieldId, FieldRef, FieldVersion, FiniteF64, Nmr2DDataset, PlotxApp, ResolvedContourLevels,
    ScalarGrid2D, StackSpec, VersionedFieldRef,
};
use num_complex::Complex64;
use plotx_figure::{
    ContourBasePolicy, ContourLevelSpec, ContourSpec, ContourStyle, PositiveFiniteF64,
    SeriesEncoding,
};
use plotx_io::{Dim, Domain, NmrData2D, QuadMode};
use plotx_render::contour::MAX_CONTOUR_SEGMENTS;
use std::sync::Arc;
use std::time::{Duration, Instant};

const SIDE: usize = 256;

/// A grid every contour level crosses in every cell: neighbouring samples
/// alternate between `amplitude` and zero, so any level strictly between them
/// has a crossing on all four edges of all `(SIDE-1)²` cells. It is the densest
/// field a regular grid can hold, and stands in for the real failure — a
/// spectrum whose lowest levels sit inside its noise — without needing 16
/// million samples to reproduce it.
fn dense_values(amplitude: f32) -> Vec<f32> {
    (0..SIDE)
        .flat_map(|row| {
            (0..SIDE).map(move |col| {
                if (row + col).is_multiple_of(2) {
                    amplitude
                } else {
                    0.0
                }
            })
        })
        .collect()
}

/// The same field with both signs, for the half-symmetry check: alternating
/// samples run `+amplitude` / `-amplitude`, so a positive level and its negative
/// mirror each cross every cell.
fn dense_signed_values(amplitude: f32) -> Vec<f32> {
    (0..SIDE)
        .flat_map(|row| {
            (0..SIDE).map(move |col| {
                if (row + col).is_multiple_of(2) {
                    amplitude
                } else {
                    -amplitude
                }
            })
        })
        .collect()
}

fn grid(values: Vec<f32>) -> Arc<ScalarGrid2D> {
    Arc::new(ScalarGrid2D {
        values: Arc::from(values),
        rows: SIDE,
        cols: SIDE,
        x: AxisSampling::Linear {
            start: 0.0,
            end: 1.0,
        },
        y: AxisSampling::Linear {
            start: 0.0,
            end: 1.0,
        },
    })
}

fn key(levels: ResolvedContourLevels) -> ContourGeometryCacheKey {
    ContourGeometryCacheKey {
        source: VersionedFieldRef {
            field: FieldRef {
                resource: DatasetId::from_uuid(uuid::Uuid::from_u128(4242)),
                field: FieldId::new(0),
            },
            version: FieldVersion(1),
        },
        levels,
    }
}

fn finite(value: f64) -> FiniteF64 {
    FiniteF64::new(value).expect("test literals are finite")
}

fn resolved(positive: &[f64], negative: &[f64]) -> ResolvedContourLevels {
    ResolvedContourLevels {
        positive: Arc::from(positive.iter().copied().map(finite).collect::<Vec<_>>()),
        negative: Arc::from(negative.iter().copied().map(finite).collect::<Vec<_>>()),
    }
}

#[test]
fn a_field_too_dense_to_draw_yields_bounded_geometry_instead_of_an_unbounded_buffer() {
    let geometry = run_build_contour(
        key(resolved(&[0.25, 0.5, 1.0], &[])),
        grid(dense_values(1.0)),
    )
    .expect("a well-formed grid builds geometry");

    let drawn = geometry.positive.len() + geometry.negative.len();
    assert!(
        drawn <= MAX_CONTOUR_SEGMENTS,
        "a build must never hand the renderer more segments than it can draw, \
         got {drawn}"
    );
    let omitted = geometry
        .omitted
        .expect("this field cannot draw its whole ladder, so it must report what it dropped");
    assert_eq!(
        usize::from(geometry.positive_levels) + usize::from(omitted.positive),
        3,
        "every requested level is either drawn or reported as omitted"
    );
    assert!(omitted.positive > 0);
    let lowest = omitted
        .lowest_drawn
        .expect("the outermost level alone fits the budget");
    assert!(
        omitted.highest_omitted.get() < lowest.get(),
        "levels are dropped from the bottom up: {} is not below {}",
        omitted.highest_omitted.get(),
        lowest.get()
    );
}

#[test]
fn the_budget_cuts_both_halves_at_the_same_magnitude() {
    let geometry = run_build_contour(
        key(resolved(&[0.25, 0.5, 1.0], &[-0.25, -0.5, -1.0])),
        grid(dense_signed_values(1.0)),
    )
    .expect("a well-formed grid builds geometry");

    let omitted = geometry
        .omitted
        .expect("a signed field this dense cannot draw its whole ladder");
    assert_eq!(
        geometry.positive_levels, geometry.negative_levels,
        "a magnitude is drawn in both halves or in neither, so a signed plot \
         never loses its negative lobes to the budget alone"
    );
    assert_eq!(omitted.positive, omitted.negative);
    assert!(
        geometry.positive.len() + geometry.negative.len() <= MAX_CONTOUR_SEGMENTS,
        "the budget bounds the geometry, not one half of it"
    );
}

fn settle(app: &mut PlotxApp) {
    let deadline = Instant::now() + Duration::from_secs(30);
    while app.compute_busy() && Instant::now() < deadline {
        app.poll_compute();
        std::thread::sleep(Duration::from_millis(5));
    }
    app.poll_compute();
    assert!(!app.compute_busy(), "contour build did not settle in time");
}

/// A 256×256 true-2D dataset whose real plane is the dense grid above.
fn dense_dataset(label: &str, values: &[f32]) -> Dataset {
    let dimension = |nucleus: &str| Dim {
        spectral_width_hz: 4_000.0,
        observe_freq_mhz: 400.0,
        carrier_ppm: 0.0,
        nucleus: nucleus.to_owned(),
        group_delay: 0.0,
    };
    Dataset::Nmr2D(Box::new(Nmr2DDataset::load(NmrData2D {
        data: values
            .iter()
            .map(|value| Complex64::new(f64::from(*value), 0.0))
            .collect(),
        rows: SIDE,
        cols: SIDE,
        domain: Domain::Frequency,
        direct: dimension("1H"),
        indirect: dimension("13C"),
        quad: QuadMode::Complex,
        indirect_conjugate: false,
        experiment: None,
        pseudo_axis: None,
        diffusion: None,
        nus: None,
        source: label.to_owned(),
    })))
}

#[test]
fn a_capped_contour_build_tells_the_user_which_levels_were_not_drawn() {
    let mut app = PlotxApp::new_with_settings(crate::settings::Settings::default());
    app.doc
        .datasets
        .push(dense_dataset("dense", &dense_values(1.0)));
    let mut binding = DataBinding::single(&app.doc.datasets[0]);
    let level = ContourLevelSpec {
        base: ContourBasePolicy::Absolute(PositiveFiniteF64::new(0.25).unwrap()),
        count: 3,
        ratio: PositiveFiniteF64::new(2.0).unwrap(),
    };
    binding.series[0].encoding = SeriesEncoding::Contour(ContourSpec {
        positive: level,
        negative: None,
        style: ContourStyle::default(),
    });
    let chart = ChartSpec::default_for(DataDomain::Nmr2d);

    app.build_binding_figure(&binding, &chart, &StackSpec::default(), [120.0, 80.0]);
    settle(&mut app);
    let figure = app.build_binding_figure(&binding, &chart, &StackSpec::default(), [120.0, 80.0]);

    let drawn: usize = figure
        .contours
        .iter()
        .map(|contour| contour.segments.len())
        .sum();
    assert!(drawn > 0, "the levels that do fit are still drawn");
    assert!(
        drawn <= MAX_CONTOUR_SEGMENTS,
        "the figure carries only what the renderer can draw, got {drawn}"
    );
    let status = app.session.status.clone();
    assert!(
        status.contains("were not drawn"),
        "a plot that is not the ladder the panel lists must say so: {status:?}"
    );
    assert!(
        status.contains("Raise the lowest level"),
        "the explanation must end on the edit that recovers a full ladder: {status:?}"
    );
}
