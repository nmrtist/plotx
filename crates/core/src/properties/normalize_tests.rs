use super::processing_test_support::{add_step, spectrum, time_domain_app};
use super::*;
use plotx_processing::{NormalizeMethod, StepKind};

#[test]
fn constant_normalization_rejects_zero_in_the_schema_and_provider() {
    let mut app = time_domain_app();
    let target = add_step(
        &mut app,
        StepKind::Normalize(NormalizeMethod::Constant { divisor: 1.0 }),
    );
    let resolved = app
        .resolve_property(&PropertyAddress::new(target.clone(), normalize::DIVISOR))
        .expect("the divisor resolves");
    let ResolvedSchema::Float { bounds, .. } = resolved.schema else {
        panic!("the divisor is a float");
    };
    assert!(!bounds.admits(0.0));

    let error = app
        .plan_property_write(
            normalize::DIVISOR,
            std::slice::from_ref(&target),
            &PropertyValue::Float(0.0),
        )
        .expect_err("zero has no normalization meaning");
    let message = error.to_string();
    assert!(message.contains("divisor 0"), "{message}");
    assert!(message.contains("magnitude greater than"), "{message}");
}

#[test]
fn constant_normalization_rejects_subnormal_values_the_kernel_ignores() {
    let mut app = time_domain_app();
    let target = add_step(
        &mut app,
        StepKind::Normalize(NormalizeMethod::Constant { divisor: 1.0 }),
    );
    let error = app
        .plan_property_write(
            normalize::DIVISOR,
            std::slice::from_ref(&target),
            &PropertyValue::Float(1.0e-320),
        )
        .expect_err("subnormal divisors are numerical no-ops");
    let message = error.to_string();
    assert!(message.contains("0.000000"), "{message}");
    assert!(message.contains("magnitude greater than"), "{message}");
}

#[test]
fn magnitude_exclusion_reports_the_rule_it_actually_enforces() {
    let bounds = FloatBounds::excluding_magnitude(-f64::MAX, f64::MAX, f64::MIN_POSITIVE);
    assert_eq!(bounds.excluded, None);
    assert_eq!(bounds.excluded_magnitude, Some(f64::MIN_POSITIVE));
    assert_eq!(bounds.lowest(), -f64::MAX);
    assert!(!bounds.admits(0.0));
    assert!(!bounds.admits(f64::MIN_POSITIVE));
    assert!(!bounds.admits(-f64::MIN_POSITIVE));
    assert!(bounds.admits(f64::MIN_POSITIVE.next_up()));
    assert_eq!(
        bounds.describe(),
        format!(
            "at least {} and at most {}, with magnitude greater than {}",
            -f64::MAX,
            f64::MAX,
            f64::MIN_POSITIVE
        )
    );
    let error = bounds
        .check(normalize::DIVISOR, "normalization divisor", 0.0)
        .expect_err("zero remains excluded");
    assert!(
        error.to_string().contains("magnitude greater than"),
        "{error}"
    );
}

#[test]
fn normalization_reset_honestly_skips_a_user_only_step() {
    let mut app = time_domain_app();
    let target = add_step(
        &mut app,
        StepKind::Normalize(NormalizeMethod::Constant { divisor: 2.0 }),
    );
    assert_eq!(
        definition(normalize::DIVISOR).unwrap().default_policy,
        DefaultPolicy::None
    );
    let reset = app
        .plan_property_reset(normalize::DIVISOR, std::slice::from_ref(&target))
        .unwrap();
    assert!(reset.applied.is_empty());
    assert_eq!(reset.skipped.len(), 1);
}

#[test]
fn normalization_catalog_write_reprocesses_real_fid_and_one_undo_restores_it() {
    let mut app = time_domain_app();
    let target = add_step(
        &mut app,
        StepKind::Normalize(NormalizeMethod::Constant { divisor: 1.0 }),
    );
    let before = spectrum(&app);
    let commit = app
        .plan_property_write(
            normalize::DIVISOR,
            std::slice::from_ref(&target),
            &PropertyValue::Float(2.0),
        )
        .expect("the divisor plans through the real property entry");
    app.commit_property(commit);
    let after = spectrum(&app);
    assert_ne!(after.0, before.0);
    for (scaled, original) in after.0.iter().zip(&before.0) {
        assert!((*scaled * 2.0 - *original).norm() < 1.0e-9);
    }

    app.undo();
    assert_eq!(spectrum(&app), before);
}
