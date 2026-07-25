//! Explicit contour thresholds the field never reaches.
//!
//! An `Absolute` base is the user's own input, so it is obeyed literally: a
//! threshold above the peak draws nothing rather than being rewritten into a
//! ladder nobody asked for. Drawing nothing *silently* is the failure this
//! module exists to prevent — the resolution reports the threshold and the peak
//! together, so a mistyped magnitude is legible instead of looking like a bug.

use super::tests::{finite, grid_dataset, source, summary};
use crate::state::{
    ChartSpec, ContourResolution, DataBinding, DataDomain, EstimateProvenance, EstimateResult,
    EstimatedScale, FieldSummary, FieldVersion, PlotxApp, ScaleEstimate, StackSpec,
    resolve_contour_levels,
};
use plotx_figure::{
    ContourBasePolicy, ContourLevelSpec, ContourSpec, ContourStyle, EstimatorSelection,
    PositiveFiniteF64, SeriesEncoding,
};

fn absolute_spec(threshold: f64, signed: bool) -> ContourSpec {
    let level = ContourLevelSpec {
        base: ContourBasePolicy::Absolute(PositiveFiniteF64::new(threshold).unwrap()),
        count: 3,
        ratio: PositiveFiniteF64::new(1.5).unwrap(),
    };
    ContourSpec {
        positive: level.clone(),
        negative: signed.then_some(level),
        style: ContourStyle::default(),
    }
}

fn ready(spec: &ContourSpec, summary: FieldSummary) -> ContourResolution {
    resolve_contour_levels(source(31, 1, 1), spec, summary, |_| None)
}

#[test]
fn an_unreachable_absolute_threshold_reports_the_threshold_and_the_peak() {
    let ContourResolution::Ready {
        levels,
        unreachable,
    } = ready(&absolute_spec(20.0, true), summary())
    else {
        panic!("an absolute contour needs no estimate");
    };
    assert!(levels.positive.is_empty());
    assert!(levels.negative.is_empty());

    // Both halves of a signed field are covered, and each carries the pair of
    // numbers that makes an extra typed zero visible.
    assert_eq!(unreachable.len(), 2, "{unreachable:?}");
    let positive = unreachable
        .iter()
        .find(|report| !report.negative)
        .expect("the positive half is unreachable");
    assert_eq!(positive.threshold, finite(20.0));
    assert_eq!(positive.peak, finite(10.0));
    let negative = unreachable
        .iter()
        .find(|report| report.negative)
        .expect("the negative half is unreachable");
    // The negative half compares magnitudes: the field bottoms out at -10.
    assert_eq!(negative.threshold, finite(20.0));
    assert_eq!(negative.peak, finite(10.0));
}

#[test]
fn a_reachable_absolute_threshold_reports_nothing() {
    let ContourResolution::Ready {
        levels,
        unreachable,
    } = ready(&absolute_spec(2.0, true), summary())
    else {
        panic!("an absolute contour needs no estimate");
    };
    assert!(!levels.positive.is_empty());
    assert!(!levels.negative.is_empty());
    assert!(unreachable.is_empty(), "{unreachable:?}");
}

#[test]
fn a_half_with_no_signal_of_its_sign_is_not_a_threshold_problem() {
    // An all-positive field has no negative lobe at all. That is the shape of
    // the data, not a mistyped threshold, so the negative half stays empty and
    // silent.
    let all_positive = FieldSummary {
        min: finite(1.0),
        max: finite(10.0),
    };
    let ContourResolution::Ready {
        levels,
        unreachable,
    } = ready(&absolute_spec(2.0, true), all_positive)
    else {
        panic!("an absolute contour needs no estimate");
    };
    assert!(!levels.positive.is_empty());
    assert!(levels.negative.is_empty());
    assert!(unreachable.is_empty(), "{unreachable:?}");
}

#[test]
fn a_policy_base_above_the_peak_falls_back_to_the_selected_ladder_span() {
    // A degenerate (zero) noise estimate is not something the user typed, so it
    // still falls back to the ladder the spec selected — and that fallback is a
    // successful resolution, never a threshold report.
    let spec = ContourSpec {
        positive: ContourLevelSpec {
            base: ContourBasePolicy::NoiseFloor {
                multiplier: PositiveFiniteF64::new(5.0).unwrap(),
                peak_fraction: plotx_figure::UnitInterval::new(0.0).expect("a zero floor is valid"),
                estimator: EstimatorSelection::FollowLatest,
            },
            count: 3,
            ratio: PositiveFiniteF64::new(1.5).unwrap(),
        },
        negative: None,
        style: ContourStyle::default(),
    };
    let ContourResolution::Ready {
        levels,
        unreachable,
    } = resolve_contour_levels(source(32, 1, 1), &spec, summary(), |_| {
        Some(EstimateResult::Scale(ScaleEstimate {
            scale: EstimatedScale::Degenerate,
            provenance: EstimateProvenance {
                estimator: plotx_analysis::robust::ROBUST_DIFFERENCE_MAD_ID.to_owned(),
                version: plotx_analysis::robust::ROBUST_DIFFERENCE_MAD_VERSION,
            },
        }))
    })
    else {
        panic!("the estimate is supplied, so resolution is not pending");
    };
    assert_eq!(levels.positive.len(), 3);
    assert!((levels.positive[0].get() - 10.0 / 1.5f64.powi(2)).abs() < 1e-9);
    assert!(
        unreachable.is_empty(),
        "a policy that recovered drew levels; there is nothing to explain: {unreachable:?}"
    );
}

/// A 4×4 plane whose real values run 0..=10 and never go negative: the positive
/// peak is exactly 10, and there is no negative lobe to report on.
fn peaked_values() -> Vec<f32> {
    let mut values = vec![0.0f32; 16];
    values[5] = 10.0;
    values[6] = 4.0;
    values[9] = 6.0;
    values
}

#[test]
fn a_mistyped_threshold_reaches_the_status_line_with_both_numbers() {
    let mut app = PlotxApp::new_with_settings(crate::settings::Settings::default());
    app.doc
        .datasets
        .push(grid_dataset("typo", &peaked_values()));
    let mut binding = DataBinding::single(&app.doc.datasets[0]);
    let field = binding.series[0].source.field;
    let peak = app.doc.datasets[0]
        .field_snapshot(field, FieldVersion(1), None)
        .and_then(|snapshot| snapshot.summary)
        .expect("a scalar field carries a summary");
    assert_eq!(peak.max.get(), 10.0, "fixture's positive peak");
    assert_eq!(peak.min.get(), 0.0, "fixture has no negative lobe");

    // 2.0 typed with one zero too many.
    binding.series[0].encoding = SeriesEncoding::Contour(absolute_spec(20.0, true));
    let figure = app.build_binding_figure(
        &binding,
        &ChartSpec::default_for(DataDomain::Nmr2d),
        &StackSpec::default(),
        [120.0, 80.0],
    );

    assert!(figure.contours.is_empty());
    assert_eq!(
        app.session.status,
        "The positive contour threshold 20 is above this field's positive peak 10, \
         so no positive contours are drawn. Lower the threshold below 10.",
        "the threshold and the peak must sit side by side, and only the half \
         that actually has signal is reported"
    );
}
