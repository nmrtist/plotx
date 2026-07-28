use super::processing_test_support::{add_step, step, target_for, time_domain_app};
use super::*;
use plotx_processing::StepKind;

#[test]
fn enabled_applies_to_any_processing_step_and_uses_the_typed_undo_path() {
    let mut app = time_domain_app();
    let target = add_step(&mut app, StepKind::Reverse);
    assert!(step(&app, &target).enabled);

    let commit = app
        .plan_property_write(
            step_enabled::ENABLED,
            std::slice::from_ref(&target),
            &PropertyValue::Bool(false),
        )
        .expect("the cross-step flag plans");
    app.commit_property(commit);
    assert!(!step(&app, &target).enabled);
    app.undo();
    assert!(step(&app, &target).enabled);
}

#[test]
fn a_user_step_has_no_invented_enabled_default() {
    let app = time_domain_app();
    let mut app = app;
    let target = add_step(&mut app, StepKind::Invert);
    let resolved = app
        .resolve_property(&PropertyAddress::new(target.clone(), step_enabled::ENABLED))
        .expect("the flag resolves");
    assert_eq!(resolved.default_value, None);
    let reset = app
        .plan_property_reset(step_enabled::ENABLED, std::slice::from_ref(&target))
        .expect("a missing factory default becomes a skip");
    assert!(reset.applied.is_empty());
    assert_eq!(reset.skipped.len(), 1);
}

#[test]
fn a_factory_step_enabled_flag_resets_through_the_catalog() {
    let mut app = time_domain_app();
    let target = target_for(&app, |kind| matches!(kind, StepKind::Phase(_)));
    let changed = app
        .plan_property_write(
            step_enabled::ENABLED,
            std::slice::from_ref(&target),
            &PropertyValue::Bool(false),
        )
        .unwrap();
    app.commit_property(changed);
    let reset = app
        .plan_property_reset(step_enabled::ENABLED, std::slice::from_ref(&target))
        .unwrap();
    assert_eq!(reset.applied.len(), 1);
    app.commit_property(reset);
    assert!(step(&app, &target).enabled);
}

#[test]
fn disabling_fft_switches_the_pipeline_to_time_domain() {
    let mut app = time_domain_app();
    let target = target_for(&app, |kind| matches!(kind, StepKind::Fft));
    let changed = app
        .plan_property_write(
            step_enabled::ENABLED,
            std::slice::from_ref(&target),
            &PropertyValue::Bool(false),
        )
        .unwrap();
    app.commit_property(changed);
    let dataset = app.doc.datasets[0].as_nmr().unwrap();
    assert_eq!(dataset.output_domain(), plotx_io::Domain::Time);
    assert!(dataset.time_trace().is_some());
}
