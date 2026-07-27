use super::processing_test_support::{
    spectrum, states_2d_app, step, step_mut, target_for, target_for_axis, time_domain_app,
};
use super::*;
use crate::state::Dataset;
use plotx_processing::{StepKind, ZeroFill};

#[test]
fn unsupported_factors_are_lossless_custom_values_with_a_target_readout() {
    let mut app = time_domain_app();
    let target = target_for(&app, |kind| matches!(kind, StepKind::ZeroFill(_)));
    step_mut(&mut app, &target).kind = StepKind::ZeroFill(ZeroFill::Factor(5));

    let mode = app
        .resolve_property(&PropertyAddress::new(target.clone(), zero_fill::MODE))
        .expect("the mode resolves");
    assert_eq!(
        mode.value,
        AggregateValue::Uniform(PropertyValue::Enum(zero_fill::CUSTOM))
    );
    let points = app
        .resolve_property(&PropertyAddress::new(target.clone(), zero_fill::POINTS))
        .expect("an unlisted factor exposes its exact effective size");
    assert_eq!(
        points.value,
        AggregateValue::Uniform(PropertyValue::Int(1_024))
    );
    assert_eq!(
        app.property_readout(&PropertyAddress::new(target.clone(), zero_fill::MODE))
            .expect("the derived target resolves"),
        PropertyReadout::ZeroFillTarget(ZeroFillTargetReadout { points: 1_024 })
    );

    let commit = app
        .plan_property_write(
            zero_fill::MODE,
            std::slice::from_ref(&target),
            &PropertyValue::Enum(zero_fill::CUSTOM),
        )
        .expect("writing the displayed mode plans");
    assert!(commit.applied.is_empty());
    assert_eq!(
        step(&app, &target).kind,
        StepKind::ZeroFill(ZeroFill::Factor(5)),
        "setting the already displayed mode preserves the stored factor"
    );
}

#[test]
fn custom_points_reject_the_set_value_and_name_the_dataset_boundary() {
    let mut app = time_domain_app();
    let target = target_for(&app, |kind| matches!(kind, StepKind::ZeroFill(_)));
    let commit = app
        .plan_property_write(
            zero_fill::MODE,
            std::slice::from_ref(&target),
            &PropertyValue::Enum(zero_fill::CUSTOM),
        )
        .expect("custom mode plans");
    app.commit_property(commit);

    let error = app
        .plan_property_write(
            zero_fill::POINTS,
            std::slice::from_ref(&target),
            &PropertyValue::Int(32),
        )
        .expect_err("the FFT target cannot shrink the 64-point FID");
    let message = error.to_string();
    assert!(message.contains("32"), "{message}");
    assert!(message.contains("64 original points"), "{message}");
}

#[test]
fn zero_fill_catalog_write_reprocesses_real_fid_and_one_undo_restores_it() {
    let mut app = time_domain_app();
    let target = target_for(&app, |kind| matches!(kind, StepKind::ZeroFill(_)));
    let before = spectrum(&app);
    assert_eq!(before.0.len(), 64);

    let commit = app
        .plan_property_write(
            zero_fill::MODE,
            std::slice::from_ref(&target),
            &PropertyValue::Enum(zero_fill::X2),
        )
        .expect("doubling plans through the real property entry");
    app.commit_property(commit);
    let after = spectrum(&app);
    assert_eq!(after.0.len(), 128);
    assert_ne!(after.0, before.0);

    app.undo();
    assert_eq!(spectrum(&app), before);
}

#[test]
fn states_f1_uses_complex_increments_as_its_raw_point_count() {
    let mut app = states_2d_app(10, 6);
    let target = target_for_axis(&app, crate::state::PhaseAxis::F1, |kind| {
        matches!(kind, StepKind::ZeroFill(_))
    });
    let commit = app
        .plan_property_write(
            zero_fill::MODE,
            std::slice::from_ref(&target),
            &PropertyValue::Enum(zero_fill::CUSTOM),
        )
        .expect("custom F1 zero fill plans");
    app.commit_property(commit);

    let resolved = app
        .resolve_property(&PropertyAddress::new(target.clone(), zero_fill::POINTS))
        .expect("F1 points resolve");
    assert!(matches!(
        resolved.schema,
        ResolvedSchema::IntWithDrag { min: 5, .. }
    ));
    let accepted = app
        .plan_property_write(
            zero_fill::POINTS,
            std::slice::from_ref(&target),
            &PropertyValue::Int(8),
        )
        .expect("eight points exceeds the five complex States increments");
    app.commit_property(accepted);
    assert_eq!(
        app.property_readout(&PropertyAddress::new(target, zero_fill::MODE))
            .expect("the F1 target readout resolves"),
        PropertyReadout::ZeroFillTarget(ZeroFillTargetReadout { points: 8 })
    );
}

#[test]
fn nus_f1_uses_the_nominal_reconstruction_grid_as_its_raw_count() {
    let mut app = states_2d_app(10, 6);
    let Dataset::Nmr2D(dataset) = &mut app.doc.datasets[0] else {
        panic!("the fixture is 2D NMR");
    };
    std::sync::Arc::make_mut(&mut dataset.data).nus = Some(plotx_io::NusMeta {
        grid: 17,
        acquired: 5,
        idx_base: 0,
        mode: "test".to_owned(),
        echo_antiecho: false,
        schedule: Some(vec![0, 2, 5, 9, 16]),
    });
    let target = target_for_axis(&app, crate::state::PhaseAxis::F1, |kind| {
        matches!(kind, StepKind::ZeroFill(_))
    });
    let custom = app
        .plan_property_write(
            zero_fill::MODE,
            std::slice::from_ref(&target),
            &PropertyValue::Enum(zero_fill::CUSTOM),
        )
        .unwrap();
    app.commit_property(custom);
    assert!(matches!(
        app.resolve_property(&PropertyAddress::new(target, zero_fill::POINTS))
            .unwrap()
            .schema,
        ResolvedSchema::IntWithDrag { min: 17, .. }
    ));
}

#[test]
fn zero_fill_reset_restores_the_factory_value_through_the_catalog() {
    let mut app = time_domain_app();
    let target = target_for(&app, |kind| matches!(kind, StepKind::ZeroFill(_)));
    let changed = app
        .plan_property_write(
            zero_fill::MODE,
            std::slice::from_ref(&target),
            &PropertyValue::Enum(zero_fill::X2),
        )
        .unwrap();
    app.commit_property(changed);
    let reset = app
        .plan_property_reset(zero_fill::MODE, std::slice::from_ref(&target))
        .unwrap();
    assert_eq!(reset.applied.len(), 1);
    app.commit_property(reset);
    assert_eq!(step(&app, &target).kind, StepKind::ZeroFill(ZeroFill::None));
}
