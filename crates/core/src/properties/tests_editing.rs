//! Typed property editing and atomic composite tests.

use super::*;

/// A panel edit must reach the document as the ordinary typed binding action,
/// so it undoes, redoes and rebuilds like every other binding change.
#[test]
fn an_edit_compiles_into_a_typed_binding_action() {
    let (mut app, target) = contour_app();
    let before = contour_spec(&app, &target);
    let commit = app
        .plan_property_write(
            contour::COUNT,
            std::slice::from_ref(&target),
            &PropertyValue::Int(7),
        )
        .expect("count is writable");
    assert_eq!(commit.applied.len(), 1);
    assert!(commit.skipped.is_empty());
    let Some(Action::Composite(actions)) = &commit.document_action else {
        panic!("a commit is always one atomic composite");
    };
    assert_eq!(actions.len(), 1);
    assert!(matches!(actions[0], Action::SetDataBinding { .. }));

    app.commit_property(commit);
    let after = contour_spec(&app, &target);
    assert_eq!(before.positive.count, 14);
    assert_eq!(after.positive.count, 7);
    assert_eq!(
        after.negative.as_ref().map(|half| half.count),
        Some(7),
        "the mirrored half follows the shared ladder"
    );
}

/// Two series of one object must fold into a single action. Two actions built
/// from the same pre-edit snapshot would make the second overwrite the first.
#[test]
fn several_series_of_one_object_fold_into_one_action() {
    let (mut app, first) = contour_app();
    let object: crate::state::ObjectId =
        first.resource.local_id.as_deref().unwrap().parse().unwrap();
    let second_id = {
        let plot = app.doc.canvases[0]
            .object_mut(object)
            .and_then(|object| object.plot_mut())
            .expect("plot");
        let id = plot.allocate_series_id();
        let mut extra = plot.binding.series[0].clone();
        extra.id = id;
        plot.binding.series.push(extra);
        id
    };
    let second = app.series_target(0, object, second_id).expect("target");

    let commit = app
        .plan_property_write(
            contour::COUNT,
            &[first.clone(), second.clone()],
            &PropertyValue::Int(9),
        )
        .expect("count is writable");
    assert_eq!(commit.applied.len(), 2);
    let Some(Action::Composite(actions)) = &commit.document_action else {
        panic!("expected a composite");
    };
    assert_eq!(
        actions.len(),
        1,
        "both series belong to one object and share one binding action"
    );

    app.commit_property(commit);
    for target in [first, second] {
        assert_eq!(contour_spec(&app, &target).positive.count, 9);
    }
}

/// A target the property does not apply to is reported with a reason. Silently
/// dropping it would leave the user believing the edit landed everywhere.
#[test]
fn an_inapplicable_target_is_reported_rather_than_ignored() {
    let (mut app, target) = contour_app();
    let object: crate::state::ObjectId = target
        .resource
        .local_id
        .as_deref()
        .unwrap()
        .parse()
        .unwrap();
    let line_id = {
        let plot = app.doc.canvases[0]
            .object_mut(object)
            .and_then(|object| object.plot_mut())
            .expect("plot");
        let id = plot.allocate_series_id();
        let mut extra = plot.binding.series[0].clone();
        extra.id = id;
        extra.encoding = plotx_figure::SeriesEncoding::Line(plotx_figure::LineEncoding::default());
        plot.binding.series.push(extra);
        id
    };
    let line = app.series_target(0, object, line_id).expect("target");

    let set = app.resolve_property_set(contour::COUNT, &[target.clone(), line.clone()]);
    assert_eq!(set.applicable_targets.len(), 1);
    assert_eq!(set.skipped_targets.len(), 1);
    assert!(
        set.skipped_targets[0].message.contains("contour"),
        "the reason names the mismatch: {}",
        set.skipped_targets[0].message
    );

    let commit = app
        .plan_property_write(contour::COUNT, &[target, line], &PropertyValue::Int(5))
        .expect("the compatible target still commits");
    assert_eq!(commit.applied.len(), 1);
    assert_eq!(commit.skipped.len(), 1);
}

/// A value one target rejects must abort the entire commit; a partially applied
/// multi-selection edit is exactly what the atomic composite exists to prevent.
#[test]
fn a_rejected_value_aborts_the_whole_commit() {
    let (app, target) = contour_app();
    let before = contour_spec(&app, &target);
    let error = app
        .plan_property_write(
            contour::COUNT,
            std::slice::from_ref(&target),
            &PropertyValue::Int(0),
        )
        .expect_err("zero levels is not a ladder");
    assert!(matches!(error, PropertyError::InvalidValue { .. }));
    assert_eq!(
        contour_spec(&app, &target).positive.count,
        before.positive.count,
        "nothing may change when planning failed"
    );
}

/// The typed entry point's own out-of-range refusal names both the value that
/// was rejected and the rule that rejected it.
///
/// The automation adapter checks the declared bound before it ever builds a
/// typed value, so this bound is reached only through the panel's path — and
/// a panel user who typed a number and saw "must be greater than 1 and at most
/// 10" still cannot tell which end their number fell off.
#[test]
fn the_typed_planner_names_the_rejected_value_and_the_bound() {
    let (app, target) = contour_app();
    let error = app
        .plan_property_write(
            contour::RATIO,
            std::slice::from_ref(&target),
            &PropertyValue::Float(42.0),
        )
        .expect_err("42 is above the declared ratio bound");
    let message = error.to_string();
    assert!(
        message.contains("42"),
        "the rejected value is named: {message}"
    );
    assert!(
        message.contains("greater than 1") && message.contains("at most 10"),
        "the bound is named: {message}"
    );
}

/// A write that is valid for one target and refused by the next may not leave
/// the first one changed.
///
/// The refusal has to arise *inside* the planner to mean anything: a value the
/// wire format already rejects never reaches a target at all, so a test that
/// fails at decoding proves nothing about the transaction. Here both values are
/// well-formed numbers and both series accept the property — the second series
/// is anchored to its noise floor, whose ceiling is a fact about that target's
/// current state, so only the planner can know the write is out of range. By
/// then the first series' working copy has already been modified.
#[test]
fn a_refusal_on_a_later_target_leaves_the_earlier_one_untouched() {
    let (mut app, first) = contour_app();
    let object: crate::state::ObjectId =
        first.resource.local_id.as_deref().unwrap().parse().unwrap();
    let second_id = {
        let plot = app.doc.canvases[0]
            .object_mut(object)
            .and_then(|object| object.plot_mut())
            .expect("plot");
        let id = plot.allocate_series_id();
        let mut extra = plot.binding.series[0].clone();
        extra.id = id;
        plot.binding.series.push(extra);
        id
    };
    let second = app.series_target(0, object, second_id).expect("target");

    // The two series share one binding, which is exactly the case a per-target
    // rollback exists for: the second target selects a working copy the first
    // has already written to.
    for (target, policy) in [
        (&first, CONTOUR_BASE_ABSOLUTE),
        (&second, CONTOUR_BASE_NOISE_FLOOR),
    ] {
        let commit = app
            .plan_property_write(
                contour::BASE_POLICY,
                std::slice::from_ref(target),
                &PropertyValue::Enum(policy),
            )
            .expect("both anchors are available on this field");
        app.commit_property(commit);
    }
    let before_first = contour_spec(&app, &first).positive.base.clone();
    let before_second = contour_spec(&app, &second).positive.base.clone();
    let revision = app.doc.automation_revision;

    // Well above any multiplier, well inside an absolute level.
    let error = app
        .plan_property_write(
            contour::BASE_MAGNITUDE,
            &[first.clone(), second.clone()],
            &PropertyValue::Float(1.0e6),
        )
        .expect_err("the noise-anchored series has a ceiling this value clears");
    assert!(
        matches!(error, PropertyError::InvalidValue { .. }),
        "an out-of-range value is a refusal, not a skip: {error}"
    );

    assert_eq!(
        contour_spec(&app, &first).positive.base,
        before_first,
        "the first series was already written in the transaction and must be rolled back with it"
    );
    assert_eq!(contour_spec(&app, &second).positive.base, before_second);
    assert_eq!(app.doc.automation_revision, revision);
}
