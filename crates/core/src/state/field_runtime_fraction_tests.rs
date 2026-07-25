//! `FractionOfRange` resolution.
//!
//! The policy names a fraction of a field's dynamic range. Because a base policy
//! never produces a signed magnitude — each half owns its sign — the fraction is
//! measured across that half's own magnitude range. These tests pin both halves
//! of that rule: the value stays unsigned even on a field that straddles zero,
//! and it is unchanged on the single-signed fields the `Bounded` capability
//! admits, which are the only ones a user may select the policy on.

use super::*;
use plotx_figure::{ContourBasePolicy, ContourLevelSpec, PositiveFiniteF64, UnitInterval};

fn finite(value: f64) -> FiniteF64 {
    FiniteF64::new(value).expect("test literals are finite")
}

fn source() -> VersionedFieldRef {
    VersionedFieldRef {
        field: FieldRef {
            resource: DatasetId::from_uuid(uuid::Uuid::from_u128(7)),
            field: FieldId::new(0),
        },
        version: FieldVersion(1),
    }
}

fn fraction_spec(fraction: f64, negative: bool) -> ContourSpec {
    let level = ContourLevelSpec {
        base: ContourBasePolicy::FractionOfRange(
            UnitInterval::new(fraction).expect("test literal is in range"),
        ),
        count: 1,
        ratio: PositiveFiniteF64::new(2.0).expect("test literal is valid"),
    };
    ContourSpec {
        positive: level.clone(),
        negative: negative.then_some(level),
        style: plotx_figure::ContourStyle::default(),
    }
}

fn resolved(spec: &ContourSpec, min: f64, max: f64) -> ResolvedContourLevels {
    let summary = FieldSummary {
        min: finite(min),
        max: finite(max),
    };
    match resolve_contour_levels(source(), spec, summary, |_| None) {
        ContourResolution::Ready { levels, .. } => levels,
        other => panic!("a fraction base needs no estimate, got {other:?}"),
    }
}

/// The bug this rule closes: across a raw `-P..P` span the positive half's
/// "four percent" evaluated to `-0.92·P`, a *negative* base for the positive
/// half. Measuring from the half's own floor keeps it a magnitude.
#[test]
fn a_signed_field_never_yields_a_signed_base() {
    let levels = resolved(&fraction_spec(0.04, true), -10.0, 10.0);
    assert_eq!(levels.positive.len(), 1);
    assert!(
        levels.positive[0].get() > 0.0,
        "the positive half draws a positive level, got {}",
        levels.positive[0].get()
    );
    assert!((levels.positive[0].get() - 0.4).abs() < 1e-12);
    assert_eq!(levels.negative.len(), 1);
    assert!((levels.negative[0].get() + 0.4).abs() < 1e-12);
}

/// A single-signed field with an offset floor — an AFM height map running
/// 100..200 nm — still measures the fraction across its own span, so the lowest
/// level sits just above the surface rather than far below it.
#[test]
fn a_bounded_field_measures_across_its_own_span() {
    let levels = resolved(&fraction_spec(0.04, false), 100.0, 200.0);
    assert_eq!(levels.positive.len(), 1);
    assert!((levels.positive[0].get() - 104.0).abs() < 1e-12);
}

/// A field whose values start at zero — a magnitude plane — is the case the
/// legacy peak-fraction ladder was written for, and must be unchanged.
#[test]
fn a_magnitude_plane_keeps_the_conventional_peak_fraction() {
    let levels = resolved(&fraction_spec(0.04, false), 0.0, 250.0);
    assert_eq!(levels.positive.len(), 1);
    assert!((levels.positive[0].get() - 10.0).abs() < 1e-12);
}

/// An entirely negative field: its magnitudes run from `|max|` up to `|min|`,
/// and the negative half's base must land inside that band with the sign applied
/// afterwards.
#[test]
fn an_entirely_negative_field_resolves_inside_its_own_band() {
    let levels = resolved(&fraction_spec(0.5, true), -200.0, -100.0);
    assert!(levels.positive.is_empty(), "no positive samples exist");
    assert_eq!(levels.negative.len(), 1);
    assert!((levels.negative[0].get() + 150.0).abs() < 1e-12);
}
