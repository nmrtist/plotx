//! Capability gating and reset behavior tests.

use super::*;

/// The base-policy gate is a capability gate, not a domain check: a signed field
/// is never offered a fraction of its value range, and a field with no noise
/// estimator is never offered a multiple of σ.
#[test]
fn base_policies_are_gated_by_field_capability() {
    let signed = crate::state::FieldCapabilities::new([
        CapabilityId::new(crate::automation::CAP_FIELD_SCALAR_GRID_2D_REGULAR),
        CapabilityId::new(CAP_FIELD_SIGNED),
        CapabilityId::new(CAP_FIELD_NOISE_SCALE),
    ]);
    let schema = definition(contour::BASE_POLICY)
        .expect("the policy property is registered")
        .value_schema;
    let offered: Vec<&str> = permitted_variants(&schema, &signed)
        .into_iter()
        .map(|variant| variant.id)
        .collect();
    assert!(offered.contains(&CONTOUR_BASE_NOISE_FLOOR));
    assert!(
        !offered.contains(&CONTOUR_BASE_FRACTION_OF_RANGE),
        "a fraction of a range that straddles zero is not a threshold"
    );

    let bounded = crate::state::FieldCapabilities::new([
        CapabilityId::new(crate::automation::CAP_FIELD_SCALAR_GRID_2D_REGULAR),
        CapabilityId::new(crate::automation::CAP_FIELD_BOUNDED),
    ]);
    let offered: Vec<&str> = permitted_variants(&schema, &bounded)
        .into_iter()
        .map(|variant| variant.id)
        .collect();
    assert!(offered.contains(&CONTOUR_BASE_FRACTION_OF_RANGE));
    assert!(!offered.contains(&CONTOUR_BASE_NOISE_FLOOR));
}

/// The gate must also hold on the write path, not only in the control that
/// offers the choices.
#[test]
fn an_ungated_base_policy_is_refused_on_write() {
    let (app, target) = contour_app();
    let error = app
        .plan_property_write(
            contour::BASE_POLICY,
            std::slice::from_ref(&target),
            &PropertyValue::Enum(CONTOUR_BASE_FRACTION_OF_RANGE),
        )
        .expect_err("the signed NMR plane must not accept a range fraction");
    assert!(matches!(error, PropertyError::InvalidValue { .. }));
}

/// Reset re-derives the value from the factory in the target's current context
/// rather than restoring a stored snapshot.
#[test]
fn reset_rederives_the_factory_default() {
    let (mut app, target) = contour_app();
    let commit = app
        .plan_property_write(
            contour::BASE_MAGNITUDE,
            std::slice::from_ref(&target),
            &PropertyValue::Float(11.0),
        )
        .expect("the multiplier is writable");
    app.commit_property(commit);
    let address = PropertyAddress::new(target.clone(), contour::BASE_MAGNITUDE);
    let resolved = app.resolve_property(&address).expect("resolves");
    assert!(resolved.is_modified());
    assert_eq!(resolved.default_value, Some(PropertyValue::Float(5.0)));

    let commit = app
        .plan_property_reset(contour::BASE_MAGNITUDE, std::slice::from_ref(&target))
        .expect("reset plans");
    app.commit_property(commit);
    let resolved = app.resolve_property(&address).expect("resolves");
    assert!(!resolved.is_modified());
    assert_eq!(
        resolved.value,
        AggregateValue::Uniform(PropertyValue::Float(5.0))
    );
}

/// Resetting a whole encoding goes back through the default factory, so it
/// yields a complete concrete encoding rather than a patched-up old one.
#[test]
fn resetting_an_encoding_calls_the_default_factory() {
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

    let commit = app
        .plan_encoding_reset(EncodingKind::Contour, std::slice::from_ref(&target))
        .expect("encoding reset plans");
    app.commit_property(commit);
    let spec = contour_spec(&app, &target);
    assert!(
        spec.negative.is_some(),
        "the factory restores the negative half a signed field gets by default"
    );
    assert_eq!(spec.positive.count, 14);
}
