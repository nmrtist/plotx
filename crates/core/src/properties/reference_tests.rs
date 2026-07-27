use super::processing_test_support::{add_step, step, time_domain_app};
use super::*;
use plotx_processing::{ReferenceParams, StepKind};

#[test]
fn reference_properties_address_the_user_step_by_stable_id() {
    let mut app = time_domain_app();
    let target = add_step(
        &mut app,
        StepKind::Reference(ReferenceParams {
            at_ppm: 4.7,
            target_ppm: 0.0,
        }),
    );
    let commit = app
        .plan_property_write(
            reference::TARGET_PPM,
            std::slice::from_ref(&target),
            &PropertyValue::Float(1.25),
        )
        .expect("the target ppm plans");
    app.commit_property(commit);
    assert!(matches!(
        step(&app, &target).kind,
        StepKind::Reference(ReferenceParams { target_ppm, .. })
            if (target_ppm - 1.25).abs() < f64::EPSILON
    ));
}

#[test]
fn reference_reset_honestly_skips_a_user_only_step() {
    let mut app = time_domain_app();
    let target = add_step(
        &mut app,
        StepKind::Reference(ReferenceParams {
            at_ppm: 4.7,
            target_ppm: 0.0,
        }),
    );
    assert_eq!(
        definition(reference::AT_PPM).unwrap().default_policy,
        DefaultPolicy::None
    );
    let reset = app
        .plan_property_reset(reference::AT_PPM, std::slice::from_ref(&target))
        .unwrap();
    assert!(reset.applied.is_empty());
    assert_eq!(reset.skipped.len(), 1);
}
