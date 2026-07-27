use super::processing_test_support::{add_step, spectrum, step, target_for, time_domain_app};
use super::*;
use plotx_processing::{PhaseParams, StepKind};

#[test]
fn automatic_phase_keeps_manual_parameters_visible_but_disabled_with_actions() {
    let app = time_domain_app();
    let target = target_for(&app, |kind| matches!(kind, StepKind::Phase(_)));
    for (property, reason) in [
        (phase::PHASE0, phase::MANUAL_PHASE0_REASON),
        (phase::PHASE1, phase::MANUAL_PHASE1_REASON),
        (phase::PIVOT, phase::MANUAL_PIVOT_REASON),
    ] {
        let resolved = app
            .resolve_property(&PropertyAddress::new(target.clone(), property))
            .expect("an automatic phase parameter remains present");
        assert_eq!(resolved.availability, Availability::Disabled(reason));
        assert!(matches!(resolved.value, AggregateValue::Uniform(_)));
    }
}

#[test]
fn switching_to_manual_seeds_the_live_automatic_phase_without_a_jump() {
    let mut app = time_domain_app();
    let target = target_for(&app, |kind| matches!(kind, StepKind::Phase(_)));
    let before = spectrum(&app);
    let expected = app.doc.datasets[0]
        .automatic_phase_params(crate::state::PhaseAxis::Direct)
        .expect("the enabled auto step has a live result");

    let commit = app
        .plan_property_write(
            phase::MODE,
            std::slice::from_ref(&target),
            &PropertyValue::Enum(phase::MANUAL),
        )
        .expect("manual mode plans");
    app.commit_property(commit);
    let StepKind::Phase(params) = step(&app, &target).kind else {
        panic!("the step remains Phase");
    };
    assert_eq!(
        params,
        PhaseParams {
            phase0: expected.0,
            phase1: expected.1,
            pivot_frac: expected.2,
            auto: None,
        }
    );
    let after = spectrum(&app);
    for (left, right) in after.0.iter().zip(&before.0) {
        assert!((*left - *right).norm() < 1.0e-9);
    }
}

#[test]
fn phase_catalog_write_reprocesses_real_fid_and_one_undo_restores_it() {
    let mut app = time_domain_app();
    let target = target_for(&app, |kind| matches!(kind, StepKind::Phase(_)));
    let manual = app
        .plan_property_write(
            phase::MODE,
            std::slice::from_ref(&target),
            &PropertyValue::Enum(phase::MANUAL),
        )
        .expect("manual mode plans");
    app.commit_property(manual);
    let before = spectrum(&app);

    let commit = app
        .plan_property_write(
            phase::PHASE0,
            std::slice::from_ref(&target),
            &PropertyValue::Float(0.4),
        )
        .expect("a radian phase writes through the real entry");
    app.commit_property(commit);
    assert_ne!(spectrum(&app).0, before.0);

    app.undo();
    assert_eq!(spectrum(&app), before);
}

#[test]
fn pivot_fraction_changes_only_the_addressed_phase_step() {
    let mut app = time_domain_app();
    let first = target_for(&app, |kind| matches!(kind, StepKind::Phase(_)));
    let second = add_step(&mut app, StepKind::Phase(PhaseParams::MANUAL_ZERO));
    let StepKind::Phase(first_before) = step(&app, &first).kind else {
        panic!("the factory step is Phase");
    };

    let commit = app
        .plan_property_write(
            phase::PIVOT,
            std::slice::from_ref(&second),
            &PropertyValue::Float(0.75),
        )
        .expect("the addressed pivot plans");
    app.commit_property(commit);
    let StepKind::Phase(first_after) = step(&app, &first).kind else {
        panic!("the factory step remains Phase");
    };
    let StepKind::Phase(second_after) = step(&app, &second).kind else {
        panic!("the added step remains Phase");
    };
    assert_eq!(first_after, first_before);
    assert_eq!(second_after.pivot_frac, 0.75);
}

#[test]
fn manual_mode_fallback_keeps_stored_terms_when_no_enabled_auto_step_can_seed_it() {
    let mut app = time_domain_app();
    let factory = target_for(&app, |kind| matches!(kind, StepKind::Phase(_)));
    super::processing_test_support::step_mut(&mut app, &factory).enabled = false;
    let target = add_step(
        &mut app,
        StepKind::Phase(PhaseParams {
            phase0: 0.2,
            phase1: -0.4,
            pivot_frac: 0.3,
            auto: Some(plotx_processing::AutoPhaseMethod::Entropy),
        }),
    );
    super::processing_test_support::step_mut(&mut app, &target).enabled = false;
    let commit = app
        .plan_property_write(
            phase::MODE,
            std::slice::from_ref(&target),
            &PropertyValue::Enum(phase::MANUAL),
        )
        .expect("a disabled auto step can still be made manual");
    app.commit_property(commit);
    assert_eq!(
        step(&app, &target).kind,
        StepKind::Phase(PhaseParams {
            phase0: 0.2,
            phase1: -0.4,
            pivot_frac: 0.3,
            auto: None,
        })
    );
}

#[test]
fn phase_reset_uses_radians_but_reports_degrees_and_uses_reset_wording() {
    let mut app = time_domain_app();
    let target = target_for(&app, |kind| matches!(kind, StepKind::Phase(_)));
    let auto_reset = app
        .plan_property_reset(phase::PHASE0, std::slice::from_ref(&target))
        .expect("an inapplicable reset is a typed skip");
    assert!(auto_reset.applied.is_empty());
    assert_eq!(auto_reset.skipped.len(), 1);
    let message = &auto_reset.skipped[0].message;
    assert!(message.contains("before resetting φ0"), "{message}");
    assert!(!message.contains("before setting φ0"), "{message}");

    let manual = app
        .plan_property_write(
            phase::MODE,
            std::slice::from_ref(&target),
            &PropertyValue::Enum(phase::MANUAL),
        )
        .unwrap();
    app.commit_property(manual);
    let changed = app
        .plan_property_write(
            phase::PHASE0,
            std::slice::from_ref(&target),
            &PropertyValue::Float(45.0_f64.to_radians()),
        )
        .unwrap();
    app.commit_property(changed);
    let schema = app
        .resolve_property(&PropertyAddress::new(target.clone(), phase::PHASE0))
        .unwrap()
        .schema;
    assert!(matches!(
        schema,
        ResolvedSchema::Float {
            display: FloatDisplay::Degrees,
            ..
        }
    ));
    assert!(matches!(
        definition(phase::PHASE0).unwrap().value_schema,
        ValueSchema::Float {
            display: FloatDisplay::Degrees,
            drag_step: Some(0.5),
            ..
        }
    ));
    let reset = app
        .plan_property_reset(phase::PHASE0, std::slice::from_ref(&target))
        .unwrap();
    assert_eq!(reset.applied.len(), 1);
}

#[test]
fn phase_pivot_keeps_fraction_storage_and_derives_a_ppm_readout() {
    let mut app = time_domain_app();
    let target = target_for(&app, |kind| matches!(kind, StepKind::Phase(_)));
    let manual = app
        .plan_property_write(
            phase::MODE,
            std::slice::from_ref(&target),
            &PropertyValue::Enum(phase::MANUAL),
        )
        .unwrap();
    app.commit_property(manual);
    let changed = app
        .plan_property_write(
            phase::PIVOT,
            std::slice::from_ref(&target),
            &PropertyValue::Float(0.25),
        )
        .unwrap();
    app.commit_property(changed);
    assert_eq!(
        app.resolve_property(&PropertyAddress::new(target.clone(), phase::PIVOT))
            .unwrap()
            .value,
        AggregateValue::Uniform(PropertyValue::Float(0.25))
    );
    let PropertyReadout::PhasePivotPpm { ppm } = app
        .property_readout(&PropertyAddress::new(target, phase::PIVOT))
        .unwrap()
    else {
        panic!("pivot supplies a ppm projection");
    };
    assert!(ppm.is_finite());
}
