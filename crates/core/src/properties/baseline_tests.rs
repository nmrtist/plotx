use super::processing_test_support::{step_mut, target_for, time_domain_app};
use super::*;
use plotx_processing::{BaselineMethod, StepKind};

#[test]
fn baseline_schema_is_dependent_and_smoothness_remains_the_domain_value() {
    let mut app = time_domain_app();
    let target = target_for(&app, |kind| matches!(kind, StepKind::Baseline(_)));
    assert!(matches!(
        app.resolve_property(&PropertyAddress::new(
            target.clone(),
            baseline::POLYNOMIAL_ORDER
        )),
        Err(PropertyError::NotApplicable(_))
    ));

    step_mut(&mut app, &target).kind = StepKind::Baseline(BaselineMethod::AUTO);
    let smoothness = app
        .resolve_property(&PropertyAddress::new(target.clone(), baseline::SMOOTHNESS))
        .expect("AsLS exposes smoothness");
    assert_eq!(
        smoothness.value,
        AggregateValue::Uniform(PropertyValue::Float(5.0e4))
    );
    assert!(matches!(
        smoothness.schema,
        ResolvedSchema::Float {
            display: FloatDisplay::Log10("λ"),
            ..
        }
    ));
    // The definition carries the domain unit; the exponent the control shows is
    // announced by the caption the same value derives.
    assert_eq!(FloatDisplay::Log10("λ").caption(), "log₁₀ λ");
}

#[test]
fn baseline_bounds_reject_the_actual_value_and_name_the_effective_limit() {
    let app = time_domain_app();
    let target = target_for(&app, |kind| matches!(kind, StepKind::Baseline(_)));
    let error = app
        .plan_property_write(
            baseline::ASYMMETRY,
            std::slice::from_ref(&target),
            &PropertyValue::Float(0.75),
        )
        .expect_err("the kernel never uses an asymmetry above one half");
    let message = error.to_string();
    assert!(message.contains("0.75"), "{message}");
    assert!(message.contains("at most 0.5"), "{message}");
}

#[test]
fn baseline_reset_restores_the_factory_asls_parameter() {
    let mut app = time_domain_app();
    let target = target_for(&app, |kind| matches!(kind, StepKind::Baseline(_)));
    let changed = app
        .plan_property_write(
            baseline::SMOOTHNESS,
            std::slice::from_ref(&target),
            &PropertyValue::Float(1.0e7),
        )
        .unwrap();
    app.commit_property(changed);
    let reset = app
        .plan_property_reset(baseline::SMOOTHNESS, std::slice::from_ref(&target))
        .unwrap();
    assert_eq!(reset.applied.len(), 1);
    app.commit_property(reset);
    assert_eq!(
        app.resolve_property(&PropertyAddress::new(target, baseline::SMOOTHNESS))
            .unwrap()
            .value,
        AggregateValue::Uniform(PropertyValue::Float(baseline::SMOOTHNESS_SEED))
    );
}

#[test]
fn polynomial_order_does_not_claim_a_factory_default_the_factory_never_contains() {
    assert_eq!(
        definition(baseline::POLYNOMIAL_ORDER)
            .unwrap()
            .default_policy,
        DefaultPolicy::None
    );
    let mut app = time_domain_app();
    let target = target_for(&app, |kind| matches!(kind, StepKind::Baseline(_)));
    let polynomial = app
        .plan_property_write(
            baseline::METHOD,
            std::slice::from_ref(&target),
            &PropertyValue::Enum(baseline::POLYNOMIAL),
        )
        .unwrap();
    app.commit_property(polynomial);
    assert_eq!(
        app.resolve_property(&PropertyAddress::new(
            target.clone(),
            baseline::POLYNOMIAL_ORDER,
        ))
        .unwrap()
        .default_value,
        None
    );
    let reset = app
        .plan_property_reset(baseline::POLYNOMIAL_ORDER, std::slice::from_ref(&target))
        .unwrap();
    assert!(reset.applied.is_empty());
    assert_eq!(reset.skipped.len(), 1);
}
