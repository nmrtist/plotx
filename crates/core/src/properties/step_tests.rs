//! The canvas gesture (§8.5 channel 3) and the shared-ladder marker.
//!
//! The gesture is the case §1 principle 1 exists for: a second *entry point*
//! is fine, a second *source of state* is not. These tests pin that the step
//! reaches the domain model only through the planner, obeys the same validation
//! a typed value meets, and produces exactly the action a typed value would.

use super::tests::{contour_app, contour_spec};
use super::*;
use crate::automation::{ComponentRef, TargetRef};
use crate::state::{
    CONTOUR_BASE_ABSOLUTE, CONTOUR_BASE_NOISE_FLOOR, ObjectId, PlotxApp, contour_base_kind,
};
use plotx_figure::{ContourBasePolicy, PositiveFiniteF64, SeriesEncoding};

fn object_of(target: &TargetRef) -> ObjectId {
    target
        .resource
        .local_id
        .as_deref()
        .expect("a canvas-object target names its object")
        .parse()
        .expect("object ids round-trip through their string form")
}

/// Reach into the document and give the negative half a ladder of its own, the
/// state a symmetric write can never produce but a project file can hold.
fn desynchronize_halves(app: &mut PlotxApp, target: &TargetRef) {
    let Some(ComponentRef::Series(series)) = target.component else {
        panic!("the fixture addresses a series");
    };
    let binding = &mut app.doc.canvases[0]
        .object_mut(object_of(target))
        .and_then(|object| object.plot_mut())
        .expect("the fixture holds a plot")
        .binding;
    let encoding = &mut binding
        .series
        .iter_mut()
        .find(|candidate| candidate.id == series)
        .expect("the series exists")
        .encoding;
    let SeriesEncoding::Contour(spec) = encoding else {
        panic!("the fixture draws a contour");
    };
    let negative = spec
        .negative
        .as_mut()
        .expect("a signed field has both halves");
    negative.base = ContourBasePolicy::Absolute(PositiveFiniteF64::new(3.0).unwrap());
    negative.count = spec.positive.count + 1;
    negative.ratio = PositiveFiniteF64::new(spec.positive.ratio.get() + 0.2).unwrap();
}

/// The step is geometric, by the ladder's own ratio, and it lands on both
/// halves because base, count and ratio describe one shared ladder.
#[test]
fn a_step_moves_the_base_by_the_ladders_own_ratio() {
    let (mut app, target) = contour_app();
    let before = contour_spec(&app, &target);
    let ratio = before.positive.ratio.get();
    let magnitude = match &before.positive.base {
        ContourBasePolicy::NoiseFloor { multiplier, .. } => multiplier.get(),
        other => panic!("the fixture defaults to a noise-anchored base, got {other:?}"),
    };

    let commit = app
        .plan_property_step(
            contour::BASE_MAGNITUDE,
            std::slice::from_ref(&target),
            PropertyStep::Raise,
        )
        .expect("a contour series can be stepped");
    app.commit_property(commit);

    let raised = contour_spec(&app, &target);
    let stepped = |base: &ContourBasePolicy| match base {
        ContourBasePolicy::NoiseFloor { multiplier, .. } => multiplier.get(),
        other => panic!("stepping must not change the anchor, got {other:?}"),
    };
    assert!((stepped(&raised.positive.base) - magnitude * ratio).abs() < 1.0e-9);
    assert!(
        (stepped(&raised.negative.as_ref().expect("signed").base) - magnitude * ratio).abs()
            < 1.0e-9,
        "the shared ladder is written to every half that exists"
    );

    let commit = app
        .plan_property_step(
            contour::BASE_MAGNITUDE,
            std::slice::from_ref(&target),
            PropertyStep::Lower,
        )
        .expect("a contour series can be stepped");
    app.commit_property(commit);
    assert!((stepped(&contour_spec(&app, &target).positive.base) - magnitude).abs() < 1.0e-9);
}

/// §1 principle 1. The gesture is an entry point, not a state source: stepping
/// leaves the document in exactly the state typing the same number would.
#[test]
fn a_step_lands_where_the_typed_value_would() {
    let (mut gestured, target) = contour_app();
    let (mut typed, typed_target) = contour_app();
    let spec = contour_spec(&gestured, &target);
    let ratio = spec.positive.ratio.get();
    let magnitude = match &spec.positive.base {
        ContourBasePolicy::NoiseFloor { multiplier, .. } => multiplier.get(),
        other => panic!("unexpected default base {other:?}"),
    };

    let commit = gestured
        .plan_property_step(
            contour::BASE_MAGNITUDE,
            std::slice::from_ref(&target),
            PropertyStep::Raise,
        )
        .expect("the gesture plans");
    assert_eq!(gestured.commit_property(commit), 1);

    let commit = typed
        .plan_property_write(
            contour::BASE_MAGNITUDE,
            std::slice::from_ref(&typed_target),
            &PropertyValue::Float(magnitude * ratio),
        )
        .expect("the control plans");
    assert_eq!(typed.commit_property(commit), 1);

    assert_eq!(
        contour_spec(&gestured, &target),
        contour_spec(&typed, &typed_target),
        "one planner, one validation, one action — whichever entry point was used"
    );
}

/// A gesture must not be able to reach a value the panel would reject. The
/// multiplier ceiling is the same one a typed value meets, and running into it
/// is reported rather than silently doing nothing.
#[test]
fn a_step_past_the_ceiling_is_refused_with_a_reason() {
    let (mut app, target) = contour_app();
    let commit = app
        .plan_property_write(
            contour::BASE_MAGNITUDE,
            std::slice::from_ref(&target),
            &PropertyValue::Float(1.0e4),
        )
        .expect("the ceiling itself is a legal multiplier");
    app.commit_property(commit);

    let error = app
        .plan_property_step(
            contour::BASE_MAGNITUDE,
            std::slice::from_ref(&target),
            PropertyStep::Raise,
        )
        .expect_err("a step past the ceiling cannot land");
    assert!(
        matches!(error, PropertyError::InvalidValue { .. }),
        "got {error:?}"
    );
    assert!(error.to_string().contains("highest value"));
}

/// The gesture is declared per property. Asking for a step on one that has no
/// direction is refused, rather than being silently mapped onto some other
/// setting.
#[test]
fn a_property_with_no_step_gesture_refuses_one() {
    let (app, target) = contour_app();
    for property in [contour::COUNT, contour::POSITIVE_COLOR] {
        let error = app
            .plan_property_step(property, std::slice::from_ref(&target), PropertyStep::Raise)
            .expect_err("only a declared step gesture may be taken");
        assert!(
            matches!(error, PropertyError::InvalidValue { .. }),
            "{property} produced {error:?}"
        );
    }
}

/// A gesture is still an ordinary write: it steps from what the *positive* half
/// holds and then makes both halves agree, exactly as a typed value does, so an
/// asymmetric ladder is resolved rather than stepped twice over.
#[test]
fn a_step_on_an_asymmetric_ladder_makes_the_halves_agree() {
    let (mut app, target) = contour_app();
    desynchronize_halves(&mut app, &target);
    let before = contour_spec(&app, &target);
    assert_ne!(
        contour_base_kind(&before.positive.base),
        contour_base_kind(&before.negative.as_ref().unwrap().base),
        "the fixture is deliberately asymmetric"
    );

    let commit = app
        .plan_property_step(
            contour::BASE_MAGNITUDE,
            std::slice::from_ref(&target),
            PropertyStep::Lower,
        )
        .expect("an asymmetric ladder can still be stepped");
    app.commit_property(commit);

    let after = contour_spec(&app, &target);
    let negative = after.negative.as_ref().expect("signed");
    assert_eq!(
        contour_base_kind(&after.positive.base),
        CONTOUR_BASE_NOISE_FLOOR
    );
    assert_eq!(contour_base_kind(&negative.base), CONTOUR_BASE_NOISE_FLOOR);
    assert_eq!(
        contour::read(contour::BASE_MAGNITUDE, &after),
        Some(AggregateValue::Uniform(PropertyValue::Float(
            match &after.positive.base {
                ContourBasePolicy::NoiseFloor { multiplier, .. } => multiplier.get(),
                other => panic!("unexpected base {other:?}"),
            }
        )))
    );
}

/// The marker phase 5a called for, checked against what the reader actually
/// does rather than trusted as a comment: a definition that says its value is
/// held once per mirrored half must be the one that can report `Mixed` from a
/// single target, and a definition that says otherwise must not.
#[test]
fn the_shared_marker_matches_what_a_single_target_can_disagree_about() {
    let (mut app, target) = contour_app();
    desynchronize_halves(&mut app, &target);

    for definition in catalog() {
        let address = PropertyAddress::new(target.clone(), definition.id);
        let Ok(resolved) = app.resolve_property(&address) else {
            continue;
        };
        let mixed = matches!(resolved.value, AggregateValue::Mixed);
        match definition.copies {
            ValueCopies::PerMirroredHalf => assert!(
                mixed,
                "{} is declared as held once per half, so two different halves \
                 must read as Mixed",
                definition.id
            ),
            ValueCopies::PerTarget => assert!(
                !mixed,
                "{} is declared as held once per target, so it cannot disagree \
                 with itself",
                definition.id
            ),
        }
    }
}

/// A ceiling belongs to the anchor, not to the gesture. An absolute level is
/// bounded by the field rather than by the catalog, so it steps as far as the
/// user wants — the refusal above is a property of the multiplier, not of
/// stepping.
#[test]
fn an_absolute_anchor_has_no_catalog_ceiling_to_run_into() {
    let (mut app, target) = contour_app();
    let commit = app
        .plan_property_write(
            contour::BASE_POLICY,
            std::slice::from_ref(&target),
            &PropertyValue::Enum(CONTOUR_BASE_ABSOLUTE),
        )
        .expect("an absolute base needs no capability");
    app.commit_property(commit);

    for _ in 0..12 {
        let commit = app
            .plan_property_step(
                contour::BASE_MAGNITUDE,
                std::slice::from_ref(&target),
                PropertyStep::Raise,
            )
            .expect("an absolute level is bounded by the field, not by the catalog");
        app.commit_property(commit);
    }
    assert_eq!(
        contour_base_kind(&contour_spec(&app, &target).positive.base),
        CONTOUR_BASE_ABSOLUTE
    );
}
