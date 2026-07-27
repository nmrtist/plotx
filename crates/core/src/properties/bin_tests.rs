use super::processing_test_support::{add_step, step, time_domain_app};
use super::*;
use plotx_processing::{BinMethod, BinParams, StepKind};

#[test]
fn bin_width_is_strictly_positive_and_method_is_independently_addressable() {
    let mut app = time_domain_app();
    let target = add_step(&mut app, StepKind::Bin(BinParams::DEFAULT));
    let error = app
        .plan_property_write(
            bin::WIDTH,
            std::slice::from_ref(&target),
            &PropertyValue::Float(0.0),
        )
        .expect_err("a zero-width bin cannot aggregate an axis");
    let message = error.to_string();
    assert!(message.contains("Bin width 0"), "{message}");
    assert!(message.contains("greater than"), "{message}");

    let commit = app
        .plan_property_write(
            bin::METHOD,
            std::slice::from_ref(&target),
            &PropertyValue::Enum(bin::MEAN),
        )
        .expect("mean aggregation plans");
    app.commit_property(commit);
    assert!(matches!(
        step(&app, &target).kind,
        StepKind::Bin(BinParams {
            method: BinMethod::Mean,
            ..
        })
    ));
}

#[test]
fn bin_width_lower_bound_tracks_the_real_axis_step() {
    let mut app = time_domain_app();
    let target = add_step(&mut app, StepKind::Bin(BinParams::DEFAULT));
    let resolved = app
        .resolve_property(&PropertyAddress::new(target.clone(), bin::WIDTH))
        .unwrap();
    let ResolvedSchema::Float {
        bounds,
        display: FloatDisplay::Linear("ppm"),
        ..
    } = resolved.schema
    else {
        panic!("bin width is a ppm float");
    };
    assert!(bounds.min > 0.0);
    let refused = bounds.min;
    let error = app
        .plan_property_write(
            bin::WIDTH,
            std::slice::from_ref(&target),
            &PropertyValue::Float(refused),
        )
        .expect_err("the open effective-width boundary is not a bin");
    let message = error.to_string();
    assert!(message.contains(&refused.to_string()), "{message}");
    assert!(message.contains("greater than"), "{message}");
}

#[test]
fn bin_reset_honestly_skips_a_user_only_step() {
    let mut app = time_domain_app();
    let target = add_step(&mut app, StepKind::Bin(BinParams::DEFAULT));
    assert_eq!(
        definition(bin::WIDTH).unwrap().default_policy,
        DefaultPolicy::None
    );
    let reset = app
        .plan_property_reset(bin::WIDTH, std::slice::from_ref(&target))
        .unwrap();
    assert!(reset.applied.is_empty());
    assert_eq!(reset.skipped.len(), 1);
}
