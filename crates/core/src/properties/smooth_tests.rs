use super::processing_test_support::{add_step, step, time_domain_app};
use super::*;
use plotx_processing::{SmoothMethod, StepKind};

#[test]
fn smoothing_rejects_even_windows_and_orders_that_do_not_fit_the_actual_window() {
    let mut app = time_domain_app();
    let target = add_step(&mut app, StepKind::Smooth(SmoothMethod::DEFAULT));

    let even = app
        .plan_property_write(
            smooth::WINDOW,
            std::slice::from_ref(&target),
            &PropertyValue::Int(10),
        )
        .expect_err("an even Savitzky-Golay window is invalid");
    let message = even.to_string();
    assert!(message.contains("10"), "{message}");
    assert!(message.contains("odd value"), "{message}");

    let window = app
        .plan_property_write(
            smooth::WINDOW,
            std::slice::from_ref(&target),
            &PropertyValue::Int(5),
        )
        .expect("an odd window plans");
    app.commit_property(window);
    let order = app
        .plan_property_write(
            smooth::POLYNOMIAL_ORDER,
            std::slice::from_ref(&target),
            &PropertyValue::Int(5),
        )
        .expect_err("the polynomial order must be below the current window");
    let message = order.to_string();
    assert!(message.contains("order 5"), "{message}");
    assert!(message.contains("window 5"), "{message}");
    assert!(message.contains("between 1 and 4"), "{message}");
}

#[test]
fn switching_to_savitzky_golay_seeds_an_order_admitted_by_its_schema() {
    let mut app = time_domain_app();
    let target = add_step(
        &mut app,
        StepKind::Smooth(SmoothMethod::MovingAverage { window: 3 }),
    );
    let commit = app
        .plan_property_write(
            smooth::METHOD,
            std::slice::from_ref(&target),
            &PropertyValue::Enum(smooth::SAVITZKY_GOLAY),
        )
        .expect("the method switch plans");
    app.commit_property(commit);
    let StepKind::Smooth(SmoothMethod::SavitzkyGolay { window, poly_order }) =
        step(&app, &target).kind
    else {
        panic!("the method switched");
    };
    assert_eq!((window, poly_order), (3, 2));
    let schema = app
        .resolve_property(&PropertyAddress::new(target, smooth::POLYNOMIAL_ORDER))
        .expect("the seeded order resolves")
        .schema;
    assert!(matches!(
        schema,
        ResolvedSchema::IntWithDrag {
            min: 1,
            max: 2,
            drag_step: 0.1,
            unit: ""
        }
    ));
}

#[test]
fn smoothing_bounds_follow_the_real_spectrum_after_binning() {
    let mut app = time_domain_app();
    let bin = add_step(
        &mut app,
        StepKind::Bin(plotx_processing::BinParams {
            width: 0.5,
            method: plotx_processing::BinMethod::Mean,
        }),
    );
    let target = add_step(&mut app, StepKind::Smooth(SmoothMethod::DEFAULT));
    let before = super::processing_common::spectrum_before_step(
        &super::processing_common::step_context(
            &app,
            &PropertyAddress::new(target.clone(), smooth::WINDOW),
            definition(smooth::WINDOW).unwrap(),
            |kind| matches!(kind, StepKind::Smooth(_)),
        )
        .unwrap(),
    )
    .unwrap();
    assert!(before.values.len() < 64);
    let expected_max = if before.values.len().is_multiple_of(2) {
        before.values.len() - 1
    } else {
        before.values.len()
    } as i64;
    let resolved = app
        .resolve_property(&PropertyAddress::new(target.clone(), smooth::WINDOW))
        .unwrap();
    assert!(matches!(
        resolved.schema,
        ResolvedSchema::SteppedInt {
            max,
            unit: "points",
            ..
        } if max == expected_max
    ));
    let error = app
        .plan_property_write(
            smooth::WINDOW,
            std::slice::from_ref(&target),
            &PropertyValue::Int(expected_max + 2),
        )
        .expect_err("the catalog must reject a window the kernel would clamp");
    let message = error.to_string();
    assert!(
        message.contains(&(expected_max + 2).to_string()),
        "{message}"
    );
    assert!(message.contains(&expected_max.to_string()), "{message}");
    assert!(step(&app, &bin).enabled);
}

#[test]
fn method_switch_rejects_an_invalid_stored_window_instead_of_rewriting_it() {
    let mut app = time_domain_app();
    let target = add_step(
        &mut app,
        StepKind::Smooth(SmoothMethod::MovingAverage { window: 8 }),
    );
    let error = app
        .plan_property_write(
            smooth::METHOD,
            std::slice::from_ref(&target),
            &PropertyValue::Enum(smooth::SAVITZKY_GOLAY),
        )
        .expect_err("an invalid persisted value must not be silently normalized");
    let message = error.to_string();
    assert!(message.contains("stored smoothing window 8"), "{message}");
    assert!(message.contains("odd value between 3 and 63"), "{message}");
    assert_eq!(
        step(&app, &target).kind,
        StepKind::Smooth(SmoothMethod::MovingAverage { window: 8 })
    );
}

#[test]
fn smoothing_reset_honestly_skips_a_user_only_step() {
    let mut app = time_domain_app();
    let target = add_step(&mut app, StepKind::Smooth(SmoothMethod::DEFAULT));
    assert_eq!(
        definition(smooth::WINDOW).unwrap().default_policy,
        DefaultPolicy::None
    );
    let reset = app
        .plan_property_reset(smooth::WINDOW, std::slice::from_ref(&target))
        .unwrap();
    assert!(reset.applied.is_empty());
    assert_eq!(reset.skipped.len(), 1);
}
