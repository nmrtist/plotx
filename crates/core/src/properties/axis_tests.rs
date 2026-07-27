use super::*;
use crate::automation::TargetRef;
use crate::properties::tests::contour_app;
use crate::state::{AxisOverrides, PlotxApp};

fn axis_app() -> (PlotxApp, TargetRef, crate::state::ObjectId) {
    let (app, series) = contour_app();
    let object = series
        .resource
        .local_id
        .as_deref()
        .expect("plot object local id")
        .parse()
        .expect("object id parses");
    (app, TargetRef::resource(series.resource), object)
}

fn overrides(app: &PlotxApp, object: crate::state::ObjectId) -> &AxisOverrides {
    &app.doc.canvases[0]
        .object(object)
        .and_then(|object| object.plot())
        .expect("plot")
        .axis_overrides
}

type ResetCase = (PropertyId, fn(&AxisOverrides) -> bool);

#[test]
fn every_axis_property_reset_clears_its_stored_override() {
    let (mut app, target, object) = axis_app();
    app.set_axis_overrides_value(
        0,
        object,
        &AxisOverrides {
            x_label: Some("X".to_owned()),
            y_label: Some("Y".to_owned()),
            x_show_tick_labels: Some(false),
            x_show_label: Some(false),
            y_show_tick_labels: Some(false),
            y_show_label: Some(false),
            ..AxisOverrides::default()
        },
    );

    let cases: &[ResetCase] = &[
        (axis::X_LABEL, |value: &AxisOverrides| {
            value.x_label.is_none()
        }),
        (axis::Y_LABEL, |value: &AxisOverrides| {
            value.y_label.is_none()
        }),
        (axis::X_SHOW_TICK_LABELS, |value: &AxisOverrides| {
            value.x_show_tick_labels.is_none()
        }),
        (axis::X_SHOW_LABEL, |value: &AxisOverrides| {
            value.x_show_label.is_none()
        }),
        (axis::Y_SHOW_TICK_LABELS, |value: &AxisOverrides| {
            value.y_show_tick_labels.is_none()
        }),
        (axis::Y_SHOW_LABEL, |value: &AxisOverrides| {
            value.y_show_label.is_none()
        }),
    ];
    for &(property, cleared) in cases {
        let commit = app
            .plan_property_reset(property, std::slice::from_ref(&target))
            .expect("axis reset plans");
        app.commit_property(commit);
        assert!(cleared(overrides(&app, object)), "{property} did not clear");
    }
}

#[test]
fn visibility_read_reports_effective_value_and_distinct_derived_default() {
    let (mut app, target, object) = axis_app();
    assert!(
        app.doc.canvases[0]
            .object(object)
            .and_then(|object| object.plot())
            .expect("plot")
            .derived_axes()
            .x_show_tick_labels
    );
    app.set_axis_overrides_value(
        0,
        object,
        &AxisOverrides {
            x_show_tick_labels: Some(false),
            ..AxisOverrides::default()
        },
    );

    let resolved = app
        .resolve_property(&PropertyAddress::new(target, axis::X_SHOW_TICK_LABELS))
        .expect("visibility resolves");
    assert_eq!(
        resolved.value,
        AggregateValue::Uniform(PropertyValue::Bool(false))
    );
    assert_eq!(resolved.default_value, Some(PropertyValue::Bool(true)));
}

#[test]
fn label_value_is_effective_while_modified_tracks_override_presence() {
    let (mut app, target, object) = axis_app();
    let derived_label = app.doc.canvases[0]
        .object(object)
        .and_then(|object| object.plot())
        .expect("plot")
        .derived_axes()
        .x_label
        .clone();
    let automatic_label = app
        .resolve_property(&PropertyAddress::new(target.clone(), axis::X_LABEL))
        .expect("label resolves");
    assert_eq!(
        automatic_label.value,
        AggregateValue::Uniform(PropertyValue::Text(derived_label.clone()))
    );
    assert_eq!(
        automatic_label.default_value,
        Some(PropertyValue::Text(derived_label.clone()))
    );
    assert!(!automatic_label.is_modified());

    let commit = app
        .plan_property_write(
            axis::X_LABEL,
            std::slice::from_ref(&target),
            &PropertyValue::Text(derived_label.clone()),
        )
        .expect("equal explicit override plans");
    app.commit_property(commit);
    let explicit = app
        .resolve_property(&PropertyAddress::new(target, axis::X_LABEL))
        .expect("label resolves");
    assert_eq!(
        explicit.value,
        AggregateValue::Uniform(PropertyValue::Text(derived_label.clone()))
    );
    assert_eq!(
        explicit.default_value,
        Some(PropertyValue::Text(derived_label))
    );
    assert!(explicit.is_modified());
}

#[test]
fn visibility_modified_tracks_override_presence_even_when_values_match() {
    let (mut app, target, object) = axis_app();
    let derived_visibility = app.doc.canvases[0]
        .object(object)
        .and_then(|object| object.plot())
        .expect("plot")
        .derived_axes()
        .x_show_label;
    let commit = app
        .plan_property_write(
            axis::X_SHOW_LABEL,
            std::slice::from_ref(&target),
            &PropertyValue::Bool(derived_visibility),
        )
        .expect("equal explicit override plans");
    app.commit_property(commit);
    let explicit = app
        .resolve_property(&PropertyAddress::new(target, axis::X_SHOW_LABEL))
        .expect("visibility resolves");
    assert!(explicit.is_modified());
}

#[test]
fn whitespace_axis_label_uses_the_existing_live_write_normalization() {
    let (mut app, target, object) = axis_app();
    let commit = app
        .plan_property_write(
            axis::X_LABEL,
            std::slice::from_ref(&target),
            &PropertyValue::Text(" \t ".to_owned()),
        )
        .expect("text write plans");
    app.commit_property(commit);

    assert_eq!(overrides(&app, object).x_label, None);
    let resolved = app
        .resolve_property(&PropertyAddress::new(target, axis::X_LABEL))
        .expect("label resolves");
    assert_eq!(
        resolved.value,
        resolved
            .default_value
            .clone()
            .map(AggregateValue::Uniform)
            .expect("derived label default")
    );
    assert!(!resolved.is_modified());
}

#[test]
fn one_multi_target_write_makes_two_plot_overrides_agree() {
    let (mut app, first_target, first) = axis_app();
    let mut second_object = app.doc.canvases[0]
        .object(first)
        .expect("first object")
        .clone();
    let second = app.doc.canvases[0].allocate_object_id();
    second_object.id = second;
    app.doc.canvases[0].objects.push(second_object);
    let second_target = app.object_target(0, second).expect("second target");

    let commit = app
        .plan_property_write(
            axis::Y_SHOW_LABEL,
            &[first_target, second_target],
            &PropertyValue::Bool(false),
        )
        .expect("multi-target write plans");
    assert_eq!(commit.applied.len(), 2);
    app.commit_property(commit);

    assert_eq!(overrides(&app, first).y_show_label, Some(false));
    assert_eq!(overrides(&app, second).y_show_label, Some(false));
}

#[test]
fn grouped_visibility_reset_clears_four_overrides_in_one_undo_step() {
    let (mut app, target, object) = axis_app();
    let explicit = AxisOverrides {
        x_show_tick_labels: Some(false),
        x_show_label: Some(false),
        y_show_tick_labels: Some(false),
        y_show_label: Some(false),
        ..AxisOverrides::default()
    };
    app.set_axis_overrides_value(0, object, &explicit);
    let undo_before = app.session.undo_stack.len();

    let commit = app
        .plan_property_resets(
            &[
                axis::X_SHOW_TICK_LABELS,
                axis::X_SHOW_LABEL,
                axis::Y_SHOW_TICK_LABELS,
                axis::Y_SHOW_LABEL,
            ],
            std::slice::from_ref(&target),
        )
        .expect("grouped reset plans");
    assert_eq!(commit.applied.len(), 4);
    app.commit_property(commit);
    assert_eq!(app.session.undo_stack.len(), undo_before + 1);
    assert_eq!(overrides(&app, object), &AxisOverrides::default());

    app.undo();
    assert_eq!(overrides(&app, object), &explicit);
}
