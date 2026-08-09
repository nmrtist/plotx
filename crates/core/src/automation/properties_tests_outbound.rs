//! Outbound property DTO and skip-shape tests.

use super::*;

#[test]
fn automation_reports_text_values_and_schema() {
    let (mut app, _) = contour_app();
    let set = set_request(&app, axis::Y_LABEL.as_str(), serde_json::json!("Intensity"));
    run(&mut app, set).expect("text property writes");
    let inspect = request(
        &app,
        TOOL_INSPECT,
        serde_json::json!({"key": axis::Y_LABEL.as_str()}),
        vec![plot_resource_id(&app)],
        CallerType::Agent,
    );
    let result = run(&mut app, inspect).expect("text property inspects");
    let reading = &result.value["readings"][0];
    assert_eq!(reading["value"]["value"]["type"], "text");
    assert_eq!(reading["value"]["value"]["value"], "Intensity");
    assert_eq!(reading["schema"]["type"], "text");
}

#[test]
fn automation_reports_new_object_color_and_enum_values_and_schemas() {
    let (mut app, _) = contour_app();
    let canvas = &mut app.doc.canvases[0];
    let id = canvas.allocate_object_id();
    let mut text = TextBox::label("caption".to_owned());
    text.color = plotx_figure::Color::rgb(0x12, 0x34, 0x56);
    text.align = crate::state::TextAlign::Right;
    canvas.objects.push(CanvasObject {
        id,
        name: "Caption".to_owned(),
        frame: ObjectFrame::new(0.0, 0.0, 40.0, 20.0),
        locked: false,
        visible: true,
        kind: CanvasObjectKind::Text(text),
    });
    let resource = format!("{}/{id}", canvas.resource_id);
    for (property, kind, value) in [
        (
            crate::properties::object::TEXT_COLOR,
            "color",
            serde_json::json!("#123456"),
        ),
        (
            crate::properties::object::TEXT_ALIGN,
            "enum",
            serde_json::json!(crate::properties::object::ALIGN_RIGHT),
        ),
    ] {
        let inspect = request(
            &app,
            TOOL_INSPECT,
            serde_json::json!({"key": property.as_str()}),
            vec![resource.clone()],
            CallerType::Agent,
        );
        let result = run(&mut app, inspect).unwrap_or_else(|error| panic!("{property}: {error}"));
        let reading = &result.value["readings"][0];
        assert_eq!(reading["value"]["value"]["type"], kind);
        assert_eq!(reading["value"]["value"]["value"], value);
        assert_eq!(reading["schema"]["type"], kind);
    }
}

#[test]
fn automation_reports_app_preference_enum_and_color_values_and_schemas() {
    let mut settings = crate::settings::Settings::default();
    settings.appearance.theme = crate::settings::ThemeMode::Dark;
    settings.appearance.canvas_accent = Some([0x12, 0x34, 0x56]);
    let mut app = PlotxApp::new_with_settings(settings);

    for (property, kind, value) in [
        (
            app_preferences::THEME,
            "enum",
            serde_json::json!(app_preferences::THEME_DARK),
        ),
        (
            app_preferences::ACCENT_COLOR,
            "color",
            serde_json::json!("#123456"),
        ),
    ] {
        let inspect = request(
            &app,
            TOOL_INSPECT,
            serde_json::json!({"key": property.as_str()}),
            vec![APP_RESOURCE_ID.to_owned()],
            CallerType::Agent,
        );
        let result = run(&mut app, inspect).unwrap_or_else(|error| panic!("{property}: {error}"));
        let reading = &result.value["readings"][0];
        assert_eq!(
            reading["value"]["value"]["type"], kind,
            "{}; targets: {:?}",
            result.value, result.targets
        );
        assert_eq!(reading["value"]["value"]["value"], value);
        assert_eq!(reading["schema"]["type"], kind);
    }
}

#[test]
fn automation_inspects_stored_ilt_provenance_and_refuses_set_and_reset_as_read_only() {
    let (mut app, target) = ilt_app(0.07);
    let id = target.resource.id.clone();
    let inspect = request(
        &app,
        TOOL_INSPECT,
        serde_json::json!({"key": ilt::RESULT_LAMBDA.as_str()}),
        vec![id.clone()],
        CallerType::Agent,
    );
    let result = run(&mut app, inspect).expect("properties.inspect reads provenance");
    let value = result.value.to_string();
    assert!(value.contains("0.07"), "{value}");
    assert!(value.contains("read_only"), "{value}");

    for (tool, parameters) in [
        (
            TOOL_SET,
            serde_json::json!({"key": ilt::RESULT_LAMBDA.as_str(), "value": 0.2}),
        ),
        (
            TOOL_RESET,
            serde_json::json!({"key": ilt::RESULT_LAMBDA.as_str()}),
        ),
    ] {
        let error = plan_tool(
            &app,
            request(&app, tool, parameters, vec![id.clone()], CallerType::Agent),
        )
        .expect_err("non-inspect automation must refuse read-only provenance");
        let message = error.to_string();
        assert!(message.contains("read-only"), "{message}");
        assert!(message.contains(ilt::RESULT_LAMBDA.as_str()), "{message}");
    }
}

#[test]
fn inspect_reads_the_value_and_reports_skips() {
    let (mut app, _) = contour_app();
    add_line_series(&mut app);
    let inspect = request(
        &app,
        TOOL_INSPECT,
        serde_json::json!({"key": contour::COUNT.as_str()}),
        vec![plot_resource_id(&app)],
        CallerType::Agent,
    );
    let result = run(&mut app, inspect).expect("inspect succeeds");
    assert_eq!(result.value["property"], contour::COUNT.as_str());
    assert_eq!(result.value["aggregate"]["state"], "uniform");
    assert_eq!(result.value["readings"].as_array().map(Vec::len), Some(1));
    assert_eq!(result.value["readings"][0]["schema"]["type"], "int");
    assert_eq!(
        result
            .targets
            .iter()
            .filter(|target| target.outcome == TargetOutcome::Skipped)
            .count(),
        1,
        "the line series is reported, not dropped"
    );
}

/// A document-scoped property expands to the document root itself instead of
/// pretending it owns a series. This is the `ComponentKind::None` counterpart
/// to the existing plot-object expansion test.
#[test]
fn document_property_tools_address_the_document_root() {
    let (mut app, _) = contour_app();
    let request = request(
        &app,
        TOOL_SET,
        serde_json::json!({"key": typography::TICK_PT.as_str(), "value": 9.5}),
        vec![DOCUMENT_RESOURCE_ID.to_owned()],
        CallerType::Agent,
    );
    let plan = plan_tool(&app, request).expect("the document property plans");
    assert_eq!(plan.targets.len(), 1);
    assert_eq!(plan.targets[0].status, TargetCompatibility::Compatible);
    assert!(plan.targets[0].target.component.is_none());
    let authority = plan.required_authority;
    let result = execute_tool(&mut app, plan, authority).expect("the document property executes");
    assert!(
        result
            .targets
            .iter()
            .any(|target| target.outcome == TargetOutcome::Succeeded),
        "the document root is reported as an applied target"
    );
    assert_eq!(app.doc.style_library.figure_typography.tick_pt, 9.5);
}

/// Dataset resources expand to their stable processing-step components. Only
/// the apodization step accepts this property; the other real pipeline steps
/// remain visible as reported skips rather than being silently omitted.
#[test]
fn dataset_property_tools_expand_processing_steps_and_report_non_apodization_skips() {
    let mut app = PlotxApp::new();
    app.doc
        .datasets
        .push(Dataset::Nmr(Box::new(NmrDataset::load(NmrData {
            points: (0..32)
                .map(|value| num_complex::Complex64::new(f64::from(value), 0.0))
                .collect(),
            domain: Domain::Time,
            spectral_width_hz: 4_000.0,
            observe_freq_mhz: 400.0,
            carrier_ppm: 0.0,
            nucleus: "1H".to_owned(),
            source: "automation apodization".to_owned(),
            group_delay: 0.0,
        }))));
    let dataset = app.doc.datasets[0].resource_id().to_string();
    let request = request(
        &app,
        TOOL_SET,
        serde_json::json!({
            "key": apodization::KIND.as_str(),
            "value": apodization::APODIZATION_EXPONENTIAL,
        }),
        vec![dataset],
        CallerType::Agent,
    );
    let plan = plan_tool(&app, request).expect("the dataset step property plans");
    let compatible = plan
        .targets
        .iter()
        .filter(|target| target.status == TargetCompatibility::Compatible)
        .collect::<Vec<_>>();
    assert_eq!(compatible.len(), 1, "only the apodization step accepts it");
    assert!(
        plan.targets
            .iter()
            .any(|target| target.status == TargetCompatibility::Skipped),
        "the rest of the real pipeline is reported as skipped"
    );
    let target = compatible[0].target.clone();
    let authority = plan.required_authority;
    let result = execute_tool(&mut app, plan, authority).expect("the accepted step executes");
    assert!(
        result
            .targets
            .iter()
            .any(|target| target.outcome == TargetOutcome::Succeeded),
        "the apodization component reports success"
    );
    assert!(
        result
            .targets
            .iter()
            .any(|target| target.outcome == TargetOutcome::Skipped),
        "the non-apodization components report their skips"
    );
    assert_eq!(
        app.resolve_property(&PropertyAddress::new(target, apodization::KIND))
            .expect("the stable step target still resolves")
            .value,
        AggregateValue::Uniform(PropertyValue::Enum(apodization::APODIZATION_EXPONENTIAL)),
    );
}

#[test]
fn inspect_reports_the_actionable_reason_for_a_disabled_phase_parameter() {
    let mut app = PlotxApp::new();
    app.doc
        .datasets
        .push(Dataset::Nmr(Box::new(NmrDataset::load(NmrData {
            points: (0..32)
                .map(|value| num_complex::Complex64::new(f64::from(value), 0.25))
                .collect(),
            domain: Domain::Time,
            spectral_width_hz: 4_000.0,
            observe_freq_mhz: 400.0,
            carrier_ppm: 0.0,
            nucleus: "1H".to_owned(),
            source: "automation phase availability".to_owned(),
            group_delay: 0.0,
        }))));
    let dataset = app.doc.datasets[0].resource_id().to_string();
    let inspect = request(
        &app,
        TOOL_INSPECT,
        serde_json::json!({"key": phase::PHASE0.as_str()}),
        vec![dataset],
        CallerType::Agent,
    );
    let result = run(&mut app, inspect).expect("the real JSON entry inspects phase0");
    assert_eq!(result.value["readings"][0]["availability"], "disabled");
    assert_eq!(
        result.value["readings"][0]["disabled_reason"],
        phase::MANUAL_PHASE0_REASON
    );
}

#[test]
fn degree_schema_dto_keeps_display_log_and_unit_consistent() {
    let mut app = PlotxApp::new();
    app.doc
        .datasets
        .push(Dataset::Nmr(Box::new(NmrDataset::load(NmrData {
            points: (0..32)
                .map(|value| num_complex::Complex64::new(f64::from(value), 0.25))
                .collect(),
            domain: Domain::Time,
            spectral_width_hz: 4_000.0,
            observe_freq_mhz: 400.0,
            carrier_ppm: 0.0,
            nucleus: "1H".to_owned(),
            source: "automation phase display".to_owned(),
            group_delay: 0.0,
        }))));
    let dataset = app.doc.datasets[0].resource_id().to_string();
    let inspect = request(
        &app,
        TOOL_INSPECT,
        serde_json::json!({"key": phase::PHASE0.as_str()}),
        vec![dataset],
        CallerType::Agent,
    );
    let result = run(&mut app, inspect).expect("phase0 schema serializes");
    let schema = &result.value["readings"][0]["schema"];
    assert_eq!(schema["log"], false);
    assert_eq!(schema["unit"], "°");
    assert_eq!(schema["display"], "degrees");
}

/// A client reads `unit` together with `display`. The unit is the stored
/// quantity's, so a client that plots or converts the value does not have to
/// parse an exponent out of a caption written for a human.
#[test]
fn a_logarithmic_schema_reports_the_domain_unit_beside_its_display() {
    let dto = schema_dto(&ResolvedSchema::Float {
        bounds: FloatBounds::inclusive(1.0, 1.0e6),
        display: FloatDisplay::Log10("λ"),
    });
    let json = serde_json::to_value(dto).expect("the schema DTO serializes");
    assert_eq!(json["unit"], "λ");
    assert_eq!(json["display"], "log10");
    assert_eq!(json["log"], true);
}

#[test]
fn magnitude_exclusion_dto_does_not_understate_the_rejected_interval() {
    let bounds = FloatBounds::excluding_magnitude(-f64::MAX, f64::MAX, f64::MIN_POSITIVE);
    let dto = schema_dto(&ResolvedSchema::Float {
        bounds,
        display: FloatDisplay::Linear(""),
    });
    let json = serde_json::to_value(dto).expect("the schema DTO serializes");
    assert!(
        json.get("excluded").is_none(),
        "zero is not a separately excluded value: {json}"
    );
    assert_eq!(json["excluded_magnitude"], f64::MIN_POSITIVE);
}

/// A caller has to be able to tell "that is already the value" from "that does
/// not apply here" without reading English. The two are opposite answers to
/// whether the call addressed the right thing, and a re-sent value is the
/// ordinary way a skip reaches the result at all.
#[test]
fn a_same_value_write_reports_a_typed_skip_rather_than_a_denial() {
    let (mut app, _) = contour_app();
    add_line_series(&mut app);
    let request = set_request(&app, contour::COUNT.as_str(), serde_json::json!(9));
    run(&mut app, request).expect("the contour series accepts the write");

    let request = set_request(&app, contour::COUNT.as_str(), serde_json::json!(9));
    let result = run(&mut app, request).expect("re-sending the same value is not an error");
    let skipped: Vec<_> = result
        .targets
        .iter()
        .filter(|target| target.outcome == TargetOutcome::Skipped)
        .collect();
    assert_eq!(skipped.len(), 2, "{:?}", result.targets);

    let reasons: Vec<Option<&str>> = skipped
        .iter()
        .map(|target| target.skip_reason.as_deref())
        .collect();
    assert!(
        reasons.contains(&Some("already_at_value")),
        "the contour series held the value already: {reasons:?}"
    );
    // The line series was ruled out at plan time, by the same applicability
    // question, and reaches the result through the shared gate's own list. It
    // carries no catalog reason — which is precisely what makes the two
    // distinguishable without reading either message.
    assert!(
        reasons.contains(&None),
        "the line series never had this property: {reasons:?}"
    );

    // The verification line may not claim the property was refused: one of these
    // targets accepted it and simply had nothing to change.
    let verification = &result.verification[0];
    assert!(
        !verification.message.contains("no target accepted"),
        "a same-value write is not a denial: {}",
        verification.message
    );
}
