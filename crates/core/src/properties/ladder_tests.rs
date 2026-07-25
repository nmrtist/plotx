//! The shared contour ladder read across the halves that carry it.
//!
//! `ContourSpec` keeps the positive and negative halves as two independent
//! `ContourLevelSpec`s, so a document can hold a ladder whose halves differ.
//! Writes deliberately keep the two in step, which is only safe as long as a
//! read never presents one half as the whole setting: otherwise an asymmetric
//! ladder would be flattened by the next edit without ever being shown.

use super::tests::{contour_app, contour_spec};
use super::*;
use crate::automation::{ComponentRef, TargetRef};
use crate::state::{CONTOUR_BASE_NOISE_FLOOR, ObjectId, PlotxApp, contour_base_kind};
use plotx_figure::{
    ContourBasePolicy, ContourSpec, PositiveFiniteF64, SeriesEncoding, UnitInterval,
};

/// Edit the contour spec of the series a target names, in place, without going
/// through the catalog — the point of these tests is what the catalog does with
/// a spec it did not write.
fn with_spec(app: &mut PlotxApp, target: &TargetRef, edit: impl FnOnce(&mut ContourSpec)) {
    let Some(ComponentRef::Series(series)) = target.component else {
        panic!("the fixture addresses a series");
    };
    let object: ObjectId = target
        .resource
        .local_id
        .as_deref()
        .expect("the fixture names an object")
        .parse()
        .expect("the object id parses");
    let plot = app.doc.canvases[0]
        .object_mut(object)
        .and_then(|object| object.plot_mut())
        .expect("plot");
    let binding = plot
        .binding
        .series
        .iter_mut()
        .find(|candidate| candidate.id == series)
        .expect("series");
    let SeriesEncoding::Contour(spec) = &mut binding.encoding else {
        panic!("the fixture draws a contour");
    };
    edit(spec);
}

/// A second contour series on the same object, cloned from the first and then
/// edited, so a selection can hold two targets.
fn add_series(
    app: &mut PlotxApp,
    target: &TargetRef,
    edit: impl FnOnce(&mut ContourSpec),
) -> TargetRef {
    let object: ObjectId = target
        .resource
        .local_id
        .as_deref()
        .expect("the fixture names an object")
        .parse()
        .expect("the object id parses");
    let id = {
        let plot = app.doc.canvases[0]
            .object_mut(object)
            .and_then(|object| object.plot_mut())
            .expect("plot");
        let id = plot.allocate_series_id();
        let mut extra = plot.binding.series[0].clone();
        extra.id = id;
        if let SeriesEncoding::Contour(spec) = &mut extra.encoding {
            edit(spec);
        }
        plot.binding.series.push(extra);
        id
    };
    app.series_target(0, object, id).expect("target resolves")
}

fn read(app: &PlotxApp, target: &TargetRef, property: PropertyId) -> AggregateValue<PropertyValue> {
    app.resolve_property(&PropertyAddress::new(target.clone(), property))
        .expect("the fixture resolves")
        .value
}

/// Change a base's number while keeping its policy kind, whatever kind that is.
fn scale_magnitude(base: &mut ContourBasePolicy) {
    match base {
        ContourBasePolicy::Absolute(value) => {
            *value = PositiveFiniteF64::new(value.get() * 2.0).expect("a doubled base is positive");
        }
        ContourBasePolicy::NoiseFloor { multiplier, .. }
        | ContourBasePolicy::BackgroundScale { multiplier, .. } => {
            *multiplier = PositiveFiniteF64::new(multiplier.get() * 2.0)
                .expect("a doubled multiplier is fine")
        }
        ContourBasePolicy::FractionOfRange(fraction) => {
            *fraction = UnitInterval::new(fraction.get() / 2.0).expect("a halved fraction is unit");
        }
    }
}

/// Halves that disagree on the count must not be summarized by the positive
/// one. Writing the shared rung is what converges them, and it is the user's own
/// gesture rather than something the read did behind their back.
#[test]
fn an_asymmetric_count_reads_as_mixed_until_a_write_converges_it() {
    let (mut app, target) = contour_app();
    with_spec(&mut app, &target, |spec| {
        spec.negative
            .as_mut()
            .expect("a signed field has one")
            .count = 3;
    });

    let resolved = app
        .resolve_property(&PropertyAddress::new(target.clone(), contour::COUNT))
        .expect("resolves");
    assert_eq!(
        resolved.value,
        AggregateValue::Mixed,
        "the positive half's 14 does not speak for a ladder whose other half is 3"
    );
    assert!(
        resolved.is_modified(),
        "a ladder no factory could have produced is a modified one"
    );
    // A single-target selection carries the same fact through to the panel.
    let set = app.resolve_property_set(contour::COUNT, std::slice::from_ref(&target));
    assert_eq!(set.applicable_targets.len(), 1);
    assert_eq!(set.value, AggregateValue::Mixed);

    let commit = app
        .plan_property_write(
            contour::COUNT,
            std::slice::from_ref(&target),
            &PropertyValue::Int(7),
        )
        .expect("count is writable");
    app.commit_property(commit);

    let spec = contour_spec(&app, &target);
    assert_eq!(spec.positive.count, 7);
    assert_eq!(spec.negative.as_ref().map(|half| half.count), Some(7));
    assert_eq!(
        read(&app, &target, contour::COUNT),
        AggregateValue::Uniform(PropertyValue::Int(7)),
        "the halves agree again, so there is a value to report"
    );
}

/// Halves anchored to different *kinds* of base disagree even when their numbers
/// coincide: five times the noise σ and an absolute level of five are not the
/// same threshold, and the magnitude read under one says nothing about the other.
#[test]
fn halves_anchored_to_different_policies_read_as_mixed() {
    let (mut app, target) = contour_app();
    with_spec(&mut app, &target, |spec| {
        assert_eq!(
            contour_base_kind(&spec.positive.base),
            CONTOUR_BASE_NOISE_FLOOR,
            "the signed NMR fixture anchors to noise σ"
        );
        let magnitude = match &spec.positive.base {
            ContourBasePolicy::NoiseFloor { multiplier, .. } => *multiplier,
            other => panic!("expected a σ anchor, got {other:?}"),
        };
        // Same number, different kind: only the kind may make this Mixed.
        spec.negative.as_mut().expect("a signed field has one").base =
            ContourBasePolicy::Absolute(magnitude);
    });

    assert_eq!(
        read(&app, &target, contour::BASE_POLICY),
        AggregateValue::Mixed
    );
    assert_eq!(
        read(&app, &target, contour::BASE_MAGNITUDE),
        AggregateValue::Mixed,
        "a number whose meaning differs between the halves is not a shared value"
    );
    // The rungs that do still agree keep reporting their value.
    assert_eq!(
        read(&app, &target, contour::COUNT),
        AggregateValue::Uniform(PropertyValue::Int(14))
    );
}

/// The other face of the same base: the halves agree on the kind of anchor and
/// disagree only on how far from it the ladder starts.
#[test]
fn halves_with_one_anchor_but_different_magnitudes_read_as_mixed() {
    let (mut app, target) = contour_app();
    with_spec(&mut app, &target, |spec| {
        scale_magnitude(&mut spec.negative.as_mut().expect("a signed field has one").base);
    });

    assert_eq!(
        read(&app, &target, contour::BASE_MAGNITUDE),
        AggregateValue::Mixed
    );
    assert_eq!(
        read(&app, &target, contour::BASE_POLICY),
        AggregateValue::Uniform(PropertyValue::Enum(CONTOUR_BASE_NOISE_FLOOR)),
        "the anchor itself is still the same choice in both halves"
    );
}

/// Switching the negative contours off is a setting, not a disagreement. One
/// half is one source, and one source always agrees with itself.
#[test]
fn a_ladder_with_no_negative_half_reads_as_uniform() {
    let (mut app, target) = contour_app();
    let commit = app
        .plan_property_write(
            contour::NEGATIVE_ENABLED,
            std::slice::from_ref(&target),
            &PropertyValue::Bool(false),
        )
        .expect("the negative half is writable on a signed field");
    app.commit_property(commit);
    assert!(contour_spec(&app, &target).negative.is_none());

    for (property, expected) in [
        (contour::COUNT, PropertyValue::Int(14)),
        (contour::BASE_MAGNITUDE, PropertyValue::Float(5.0)),
        (
            contour::BASE_POLICY,
            PropertyValue::Enum(CONTOUR_BASE_NOISE_FLOOR),
        ),
    ] {
        assert_eq!(
            read(&app, &target, property),
            AggregateValue::Uniform(expected),
            "{property} has one half to read, so it has a value"
        );
    }
    assert_eq!(
        app.resolve_property_set(contour::COUNT, std::slice::from_ref(&target))
            .value,
        AggregateValue::Uniform(PropertyValue::Int(14))
    );
}

/// Two targets that each agree with themselves, and disagree with each other.
#[test]
fn targets_that_are_each_uniform_but_differ_read_as_mixed() {
    let (mut app, first) = contour_app();
    let second = add_series(&mut app, &first, |spec| {
        spec.positive.count = 5;
        spec.negative
            .as_mut()
            .expect("a signed field has one")
            .count = 5;
    });
    for target in [&first, &second] {
        assert!(
            matches!(
                read(&app, target, contour::COUNT),
                AggregateValue::Uniform(_)
            ),
            "each target must be uniform on its own for this to prove anything"
        );
    }

    let set = app.resolve_property_set(contour::COUNT, &[first, second]);
    assert_eq!(set.applicable_targets.len(), 2);
    assert_eq!(set.value, AggregateValue::Mixed);
}

/// The superposition that must not collapse: two targets whose positive halves
/// agree, one of which is internally asymmetric. Aggregating on the positive
/// half alone would report a confident `Uniform` and hide the odd ladder.
#[test]
fn one_targets_asymmetric_ladder_makes_the_whole_selection_mixed() {
    let (mut app, first) = contour_app();
    let second = add_series(&mut app, &first, |spec| {
        spec.negative
            .as_mut()
            .expect("a signed field has one")
            .count = 2;
    });
    assert_eq!(
        contour_spec(&app, &first).positive.count,
        contour_spec(&app, &second).positive.count,
        "the positive halves must agree for this to prove anything"
    );
    assert_eq!(
        read(&app, &first, contour::COUNT),
        AggregateValue::Uniform(PropertyValue::Int(14))
    );
    assert_eq!(read(&app, &second, contour::COUNT), AggregateValue::Mixed);

    let set = app.resolve_property_set(contour::COUNT, &[first, second]);
    assert_eq!(set.applicable_targets.len(), 2);
    assert_eq!(
        set.value,
        AggregateValue::Mixed,
        "agreement between the targets must not swallow disagreement inside one"
    );
}
