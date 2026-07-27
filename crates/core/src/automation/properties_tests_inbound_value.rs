//! Inbound property value and schema decoding tests.

use super::*;
use crate::properties::{
    Applicability, ComponentKind, DefaultPolicy, PropertyAccess, PropertyDefinition, PropertyId,
    ScopeKind, Tier, ValueCopies, ValueSchema,
};

#[test]
fn automation_accepts_text_for_an_axis_label() {
    let (mut app, _) = contour_app();
    let request = set_request(
        &app,
        axis::X_LABEL.as_str(),
        serde_json::json!("Chemical shift"),
    );
    run(&mut app, request).expect("text property writes through automation");

    assert_eq!(
        app.doc.canvases[0].objects[0]
            .plot()
            .expect("plot")
            .axis_overrides
            .x_label
            .as_deref(),
        Some("Chemical shift")
    );
}

#[test]
fn automation_writes_new_object_text_color_and_enum_properties() {
    let (mut app, _) = contour_app();
    let canvas = &mut app.doc.canvases[0];
    let id = canvas.allocate_object_id();
    canvas.objects.push(CanvasObject {
        id,
        name: "Caption".to_owned(),
        frame: ObjectFrame::new(0.0, 0.0, 40.0, 20.0),
        locked: false,
        visible: true,
        group: None,
        kind: CanvasObjectKind::Text(TextBox::label(String::new())),
    });
    let resource = format!("{}/{id}", canvas.resource_id);
    for (property, value) in [
        (
            crate::properties::object::TEXT,
            serde_json::json!("Automation caption"),
        ),
        (
            crate::properties::object::TEXT_COLOR,
            serde_json::json!("#123456"),
        ),
        (
            crate::properties::object::TEXT_ALIGN,
            serde_json::json!(crate::properties::object::ALIGN_CENTER),
        ),
    ] {
        let request = request(
            &app,
            TOOL_SET,
            serde_json::json!({"key": property.as_str(), "value": value}),
            vec![resource.clone()],
            CallerType::Agent,
        );
        run(&mut app, request).unwrap_or_else(|error| panic!("{property}: {error}"));
    }
    let text = app.doc.canvases[0].object(id).unwrap().text().unwrap();
    assert_eq!(text.text, "Automation caption");
    assert_eq!(text.color, plotx_figure::Color::rgb(0x12, 0x34, 0x56));
    assert_eq!(text.align, crate::state::TextAlign::Center);
}

/// A value outside the declared bound must report both the value that was
/// rejected and the bound that rejected it.
#[test]
fn an_out_of_range_value_names_the_value_and_the_bound() {
    let (app, _) = contour_app();
    let error = plan_tool(
        &app,
        set_request(&app, contour::RATIO.as_str(), serde_json::json!(50.0)),
    )
    .expect_err("50 is above the declared ratio bound");
    let message = error.to_string();
    assert!(
        message.contains("50"),
        "the rejected value is named: {message}"
    );
    assert!(
        message.contains("greater than 1") && message.contains("at most 10"),
        "the bound is named: {message}"
    );
}

/// The bound a *context-dependent* schema imposes is enforced by the shared
/// planner, not by the adapter, and it too has to name both numbers. The
/// definition's static bound admits this value; only the target's current
/// anchor rejects it.
#[test]
fn a_bound_that_only_the_anchor_knows_still_names_the_value() {
    let (mut app, target) = contour_app();
    assert!(matches!(
        contour_spec(&app, &target).positive.base,
        plotx_figure::ContourBasePolicy::NoiseFloor { .. }
    ));
    let request = set_request(
        &app,
        contour::BASE_MAGNITUDE.as_str(),
        serde_json::json!(1.0e9),
    );
    let error =
        run(&mut app, request).expect_err("a multiplier of 1e9 is beyond what the anchor accepts");
    let message = error.to_string();
    assert!(
        message.contains("1000000000"),
        "the rejected value is named: {message}"
    );
    assert!(
        message.contains("10000"),
        "the anchor's own bound is named: {message}"
    );
}

/// A string that names no choice at all is a wire-format error, and it lists
/// the choices the setting has.
#[test]
fn an_unknown_enum_choice_lists_the_settings_options() {
    let (app, _) = contour_app();
    let error = plan_tool(
        &app,
        set_request(
            &app,
            contour::BASE_POLICY.as_str(),
            serde_json::json!("dark_magic"),
        ),
    )
    .expect_err("'dark_magic' is not a base policy");
    let message = error.to_string();
    assert!(message.contains("dark_magic"), "{message}");
    assert!(
        message.contains(CONTOUR_BASE_NOISE_FLOOR)
            && message.contains(CONTOUR_BASE_FRACTION_OF_RANGE),
        "every declared choice is listed: {message}"
    );
}

/// A choice the setting has but this field's capabilities withhold is refused
/// by the planner, and the refusal lists what the field does allow. The fixture
/// draws a signed plane, which is exactly the case where a fraction of the
/// value range is meaningless.
#[test]
fn a_capability_withheld_choice_lists_what_the_field_allows() {
    let (mut app, _) = contour_app();
    let request = set_request(
        &app,
        contour::BASE_POLICY.as_str(),
        serde_json::json!(CONTOUR_BASE_FRACTION_OF_RANGE),
    );
    let error =
        run(&mut app, request).expect_err("a signed field withholds the fraction-of-range anchor");
    let message = error.to_string();
    assert!(
        message.contains(CONTOUR_BASE_FRACTION_OF_RANGE),
        "{message}"
    );
    assert!(
        message.contains("this field allows") && message.contains(CONTOUR_BASE_NOISE_FLOOR),
        "the permitted choices are named: {message}"
    );
}

#[test]
fn a_value_of_the_wrong_shape_is_refused() {
    let (app, _) = contour_app();
    let error = plan_tool(
        &app,
        set_request(&app, contour::COUNT.as_str(), serde_json::json!(true)),
    )
    .expect_err("a count is not a boolean");
    assert!(error.to_string().contains("expected an integer"), "{error}");
}

/// A read-only tool must not be usable to write, and the refusal has to happen
/// before anything is planned.
#[test]
fn a_read_only_property_cannot_be_written() {
    let (app, target) = ilt_app(0.07);
    let error = plan_tool(
        &app,
        request(
            &app,
            TOOL_SET,
            serde_json::json!({"key": ilt::RESULT_LAMBDA.as_str(), "value": 0.2}),
            vec![target.resource.id],
            CallerType::Agent,
        ),
    );
    let message = error
        .expect_err("the read-only definition must be refused before planning")
        .to_string();
    assert!(message.contains("read-only"), "{message}");
    assert!(message.contains(ilt::RESULT_LAMBDA.as_str()), "{message}");
}

fn smoothing_app() -> (PlotxApp, String) {
    let data = NmrData {
        points: (0..64)
            .map(|index| num_complex::Complex64::new(index as f64, 0.0))
            .collect(),
        domain: Domain::Time,
        spectral_width_hz: 2_000.0,
        observe_freq_mhz: 400.0,
        carrier_ppm: 4.7,
        nucleus: "1H".to_owned(),
        source: "automation smoothing".to_owned(),
        group_delay: 0.0,
    };
    let mut dataset = NmrDataset::load(data);
    let id = dataset.allocate_step_id();
    dataset
        .pipeline
        .steps
        .push(plotx_processing::ProcessingStep::new(
            id,
            plotx_processing::StepKind::Smooth(plotx_processing::SmoothMethod::DEFAULT),
            plotx_processing::StepSource::User,
        ));
    let resource = ResourceRef::from(dataset.resource_id).id;
    let mut app = PlotxApp::new();
    app.doc.datasets.push(Dataset::Nmr(Box::new(dataset)));
    (app, resource)
}

fn smoothing_request(
    app: &PlotxApp,
    resource: &str,
    key: PropertyId,
    value: serde_json::Value,
) -> ToolRequest {
    request(
        app,
        TOOL_SET,
        serde_json::json!({"key": key.as_str(), "value": value}),
        vec![resource.to_owned()],
        CallerType::Agent,
    )
}

#[test]
fn stepped_int_wire_values_accept_the_lattice_and_name_step_and_bounds_on_rejection() {
    let (app, resource) = smoothing_app();
    plan_tool(
        &app,
        smoothing_request(&app, &resource, smooth::WINDOW, serde_json::json!(11)),
    )
    .expect("an odd window on the declared lattice decodes");

    for (value, expected) in [(10, "steps of 2"), (203, "between 3 and 201")] {
        let error = plan_tool(
            &app,
            smoothing_request(&app, &resource, smooth::WINDOW, serde_json::json!(value)),
        )
        .expect_err("the wire value violates the static stepped schema");
        let message = error.to_string();
        assert!(message.contains(&value.to_string()), "{message}");
        assert!(message.contains(expected), "{message}");
    }
}

#[test]
fn int_with_drag_wire_values_decode_as_integers_and_report_actual_bounds() {
    let (app, resource) = smoothing_app();
    plan_tool(
        &app,
        smoothing_request(
            &app,
            &resource,
            smooth::POLYNOMIAL_ORDER,
            serde_json::json!(4),
        ),
    )
    .expect("a polynomial order inside the static IntWithDrag bound decodes");
    let error = plan_tool(
        &app,
        smoothing_request(
            &app,
            &resource,
            smooth::POLYNOMIAL_ORDER,
            serde_json::json!(9),
        ),
    )
    .expect_err("nine exceeds the IntWithDrag bound");
    let message = error.to_string();
    assert!(message.contains('9'), "{message}");
    assert!(message.contains("between 1 and 8"), "{message}");
}

#[test]
fn a_non_positive_schema_step_is_reported_as_an_internal_error() {
    const MALFORMED: PropertyDefinition = PropertyDefinition {
        id: PropertyId("test.malformed.step"),
        scope_kind: ScopeKind::Dataset,
        value_schema: ValueSchema::SteppedInt {
            min: 1,
            max: 9,
            step: 0,
            drag_step: 1.0,
        },
        access: PropertyAccess::ReadWrite,
        applicability: Applicability::component(ComponentKind::None),
        default_policy: DefaultPolicy::None,
        tier: Tier::Essential,
        copies: ValueCopies::PerTarget,
        canonical_label: "Malformed test",
        canonical_aliases: &[],
    };
    let error = decode_value(TOOL_SET, &MALFORMED, &serde_json::json!(3))
        .expect_err("a malformed catalog schema is not a user range error");
    let message = error.to_string();
    assert!(message.contains("internal schema error"), "{message}");
    assert!(!message.contains("steps of 0"), "{message}");
}

// ---------------------------------------------------------------------------
// The pre-existing tools
// ---------------------------------------------------------------------------
