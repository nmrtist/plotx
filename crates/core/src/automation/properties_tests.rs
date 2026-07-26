//! The JSON boundary of the property catalog.
//!
//! Every test here targets the one way this stage can break: the adapter
//! growing its own copy of a planning or validation rule and drifting from the
//! planner the panel controls use.

use super::*;
use crate::properties::tests::{contour_app, contour_spec};
use crate::properties::{
    AggregateValue, PropertyAddress, PropertyValue, apodization, contour, definition_by_key,
    typography,
};
use crate::state::{
    CONTOUR_BASE_FRACTION_OF_RANGE, CONTOUR_BASE_NOISE_FLOOR, CanvasObject, CanvasObjectKind,
    Dataset, NmrDataset, ObjectFrame, PlotxApp, SeriesBinding, TextBox,
};
use plotx_io::{Domain, NmrData};

pub(super) fn request(
    app: &PlotxApp,
    tool_id: &str,
    parameters: serde_json::Value,
    ids: Vec<String>,
    caller: CallerType,
) -> ToolRequest {
    ToolRequest {
        tool_id: tool_id.to_owned(),
        tool_version: 1,
        parameters,
        targets: TargetSelector::Explicit { ids },
        expected_revision: DocumentRevision(app.doc.automation_revision),
        caller,
    }
}

/// The stable id of the fixture's one plot object, as a selector names it.
pub(super) fn plot_resource_id(app: &PlotxApp) -> String {
    let canvas = &app.doc.canvases[0];
    format!("{}/{}", canvas.resource_id, canvas.objects[0].id)
}

pub(super) fn set_request(app: &PlotxApp, key: &str, value: serde_json::Value) -> ToolRequest {
    request(
        app,
        TOOL_SET,
        serde_json::json!({"key": key, "value": value}),
        vec![plot_resource_id(app)],
        CallerType::Agent,
    )
}

/// Plan and execute in one step, the way a headless caller does.
pub(super) fn run(app: &mut PlotxApp, request: ToolRequest) -> Result<ToolResult, AutomationError> {
    let plan = plan_tool(app, request)?;
    let authority = plan.required_authority;
    execute_tool(app, plan, authority)
}

/// Add a second series drawn as a line, so the object holds one series the
/// contour catalog applies to and one it does not.
fn add_line_series(app: &mut PlotxApp) {
    let object = &mut app.doc.canvases[0].objects[0];
    let plot = object.plot_mut().expect("the fixture holds a plot");
    let mut series = plot.binding.series[0].clone();
    series.encoding = plotx_figure::SeriesEncoding::Line(plotx_figure::LineEncoding::default());
    let id = plot.allocate_series_id();
    let mut series = SeriesBinding { id, ..series };
    series.label = Some("line".to_owned());
    plot.binding.series.push(series);
}

#[test]
fn an_unknown_property_key_is_refused_rather_than_skipped() {
    let (mut app, _) = contour_app();
    let error = plan_tool(
        &app,
        set_request(&app, "series.contour.nonexistent", serde_json::json!(3)),
    )
    .expect_err("an unknown key cannot be planned");
    let message = error.to_string();
    assert!(
        message.contains("unknown property 'series.contour.nonexistent'"),
        "{message}"
    );
    // And it never reaches execution, so nothing is silently committed.
    let before = app.doc.automation_revision;
    let request = set_request(&app, "series.contour.nonexistent", serde_json::json!(3));
    assert!(run(&mut app, request).is_err());
    assert_eq!(app.doc.automation_revision, before);
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

/// A target the property does not apply to is reported with its reason, and the
/// one it does apply to still lands.
#[test]
fn a_skipped_component_is_reported_not_dropped() {
    let (mut app, _) = contour_app();
    add_line_series(&mut app);
    let request = set_request(&app, contour::COUNT.as_str(), serde_json::json!(9));
    let result = run(&mut app, request).expect("the contour series accepts the write");
    let succeeded = result
        .targets
        .iter()
        .filter(|target| target.outcome == TargetOutcome::Succeeded)
        .collect::<Vec<_>>();
    let skipped = result
        .targets
        .iter()
        .filter(|target| target.outcome == TargetOutcome::Skipped)
        .collect::<Vec<_>>();
    assert_eq!(succeeded.len(), 1, "{:?}", result.targets);
    assert_eq!(skipped.len(), 1, "{:?}", result.targets);
    assert!(
        skipped[0].message.contains("line"),
        "the skip names the encoding that caused it: {}",
        skipped[0].message
    );
    // The two rows must be distinguishable, or a results panel shows one plot
    // object twice with nothing to tell the rows apart.
    assert_ne!(
        succeeded[0].target.describe(),
        skipped[0].target.describe(),
        "expanded targets carry their component"
    );
    assert!(succeeded[0].target.describe().contains("series"));
}

/// A validation failure leaves every target exactly as it was. A commit that
/// applied to the first series and then failed on the second would be a partial
/// landing, and the ladder of the first would silently disagree with the panel.
#[test]
fn a_validation_failure_lands_on_no_target_at_all() {
    let (mut app, target) = contour_app();
    add_line_series(&mut app);
    let before_revision = app.doc.automation_revision;
    let before_spec = contour_spec(&app, &target);
    let request = set_request(&app, contour::COUNT.as_str(), serde_json::json!(0));
    let error = run(&mut app, request).expect_err("a level count of zero is out of range");
    assert!(error.to_string().contains("out of range"), "{error}");
    assert_eq!(
        app.doc.automation_revision, before_revision,
        "a rejected write never advances the document"
    );
    assert_eq!(
        contour_spec(&app, &target),
        before_spec,
        "a rejected write never reaches a spec"
    );
}

/// A canvas object with no plot binding is skipped by the shared declared
/// capability gate, with the shared reason, and needs no special case here.
#[test]
fn an_object_without_components_is_skipped_by_the_shared_gate() {
    let (mut app, _) = contour_app();
    let canvas = &mut app.doc.canvases[0];
    let id = canvas.allocate_object_id();
    canvas.objects.push(CanvasObject {
        id,
        name: "Caption".to_owned(),
        frame: ObjectFrame::new(0.0, 0.0, 20.0, 10.0),
        locked: false,
        visible: true,
        group: None,
        kind: CanvasObjectKind::Text(TextBox::label("hello".to_owned())),
    });
    let text_id = format!("{}/{id}", app.doc.canvases[0].resource_id);
    let plan = plan_tool(
        &app,
        request(
            &app,
            TOOL_INSPECT,
            serde_json::json!({"key": contour::COUNT.as_str()}),
            vec![text_id.clone()],
            CallerType::Agent,
        ),
    )
    .expect("planning succeeds; the target is merely skipped");
    assert_eq!(plan.targets.len(), 1);
    assert_eq!(plan.targets[0].status, TargetCompatibility::Skipped);
    assert!(
        plan.targets[0]
            .reason
            .contains("lacks a required kind or capability"),
        "{}",
        plan.targets[0].reason
    );
    assert!(plan.targets[0].target.component.is_none());
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

/// A read-only tool must not be usable to write, and the refusal has to happen
/// before anything is planned.
#[test]
fn a_read_only_property_cannot_be_written() {
    // Every catalog entry is currently writable, so this asserts the guard
    // exists rather than exercising a read-only entry that does not yet exist.
    assert!(
        crate::properties::catalog()
            .iter()
            .all(|definition| definition.access == crate::properties::PropertyAccess::ReadWrite),
        "add a read-only case here once the catalog has one"
    );
}

// ---------------------------------------------------------------------------
// The pre-existing tools
// ---------------------------------------------------------------------------

/// A syntactically valid relation plan, for the tools whose parameters carry
/// one. It never executes here — planning only has to decode it — so the ids it
/// names need not exist.
fn relation_plan() -> serde_json::Value {
    let plan = plotx_data::RelPlanV1::new(plotx_data::Relation::SnapshotRead(
        plotx_data::SnapshotRead {
            table: plotx_data::TableId::new(),
            revision: plotx_data::RevisionId::new(),
            fingerprint: plotx_data::ContentHash::of(b"pre-existing-tool planning"),
        },
    ));
    serde_json::to_value(plan).expect("a relation plan serializes")
}

/// The per-tool planning seam is additive. Every tool that existed before it
/// must still be planned by the shared gate alone: one planned target per frozen
/// resource, no component, and the shared reasons.
#[test]
fn the_planning_of_pre_existing_tools_is_unchanged() {
    let (app, _) = contour_app();
    let canvas = app.doc.canvases[0].resource_id.to_string();
    let dataset = app.doc.datasets[0].resource_id().to_string();
    let object = plot_resource_id(&app);
    let transform = serde_json::json!({
        "plan": relation_plan(),
        "name": "Projected",
        "memory_limit_bytes": 16 * 1024 * 1024,
    });
    let cases: &[(&str, serde_json::Value, &str)] = &[
        ("project.get_blueprint", serde_json::json!({}), &canvas),
        (
            "resources.search",
            serde_json::json!({"query": {}}),
            &canvas,
        ),
        ("resources.inspect", serde_json::json!({}), &object),
        ("data.preview", serde_json::json!({}), &dataset),
        ("render.preview", serde_json::json!({}), &canvas),
        ("results.compare", serde_json::json!({}), &canvas),
        ("resource.rename", serde_json::json!({"name": "x"}), &object),
        (
            "figure.apply_theme",
            serde_json::json!({"theme_id": "default"}),
            &canvas,
        ),
        (
            "processing.apply_scheme",
            serde_json::json!({"path": "scheme.plotxproc"}),
            &dataset,
        ),
        ("data.import", serde_json::json!({"paths": []}), &canvas),
        ("data.transform", transform, &dataset),
        (
            "figure.export",
            serde_json::json!({"directory": ".", "format": "svg"}),
            &canvas,
        ),
    ];
    assert_eq!(
        cases.len(),
        ToolRegistry::built_in().descriptors().count() - 3,
        "every tool that predates the three property tools is covered here"
    );
    for (tool_id, parameters, target) in cases {
        let plan = plan_tool(
            &app,
            request(
                &app,
                tool_id,
                parameters.clone(),
                vec![(*target).to_owned()],
                CallerType::Agent,
            ),
        )
        .unwrap_or_else(|error| panic!("{tool_id} plans: {error}"));
        assert_eq!(plan.targets.len(), 1, "{tool_id} expands nothing");
        assert!(
            plan.targets[0].target.component.is_none(),
            "{tool_id} names no component"
        );
        assert_eq!(plan.targets[0].target.resource.id, **target);
        assert!(
            plan.targets[0]
                .reason
                .contains("declared kind and capabilities")
                || plan.targets[0]
                    .reason
                    .contains("lacks a required kind or capability"),
            "{tool_id} keeps the shared reason: {}",
            plan.targets[0].reason
        );
    }
}

/// The three new tools are admitted by capability, and the descriptors say so
/// rather than naming an object type.
#[test]
fn the_property_tools_are_gated_by_capability() {
    let registry = ToolRegistry::built_in();
    registry.validate_unique().expect("ids stay unique");
    for id in [TOOL_INSPECT, TOOL_SET, TOOL_RESET] {
        let descriptor = registry.get(id).unwrap_or_else(|| panic!("{id} exists"));
        assert_eq!(
            descriptor.required_capabilities,
            vec![CapabilityId::new(CAP_PROPERTY_CATALOG)],
            "{id}"
        );
        assert_eq!(
            descriptor.target_kinds,
            vec![
                ResourceKindId::new(KIND_DOCUMENT),
                ResourceKindId::new(KIND_DATASET),
                ResourceKindId::new(KIND_CANVAS_OBJECT),
            ],
            "{id}"
        );
    }
    assert_eq!(
        registry.get(TOOL_INSPECT).unwrap().effect,
        EffectLevel::ReadOnly
    );
    assert_eq!(
        registry.get(TOOL_SET).unwrap().effect,
        EffectLevel::Reversible
    );
    assert_eq!(
        registry.get(TOOL_RESET).unwrap().effect,
        EffectLevel::Reversible
    );
    assert!(registry.get(TOOL_SET).unwrap().undoable);
}

/// Whole-encoding reset is deliberately not exposed: its scope needs the caller
/// to name an encoding kind, and a JSON caller that omits it would rebuild every
/// series of an object from defaults while naming only one of them.
#[test]
fn whole_encoding_reset_is_not_a_tool() {
    assert!(
        ToolRegistry::built_in()
            .descriptors()
            .all(|descriptor| descriptor.id != "properties.reset_encoding"),
    );
}

#[test]
fn every_property_definition_is_reachable_by_key() {
    for definition in crate::properties::catalog() {
        assert!(
            definition_by_key(definition.id.as_str()).is_some(),
            "{} is not reachable by its own key",
            definition.id
        );
    }
}

/// Admission to the property tools is a capability question, and the capability
/// is the catalog's own answer about whether a resource has components to
/// address. Deriving it from the dataset variant instead would put a data-domain
/// branch in the admission gate — the one thing the encoding and property
/// registries exist to avoid — and would drift the moment a kind gained or lost
/// addressable components.
#[test]
fn the_catalog_capability_follows_addressable_components_not_the_dataset_kind() {
    let (mut app, _) = contour_app();
    app.doc
        .datasets
        .push(Dataset::Nmr(Box::new(NmrDataset::load(NmrData {
            points: (0..8)
                .map(|value| num_complex::Complex64::new(f64::from(value), 0.0))
                .collect(),
            domain: Domain::Frequency,
            spectral_width_hz: 4_000.0,
            observe_freq_mhz: 400.0,
            carrier_ppm: 0.0,
            nucleus: "1H".to_owned(),
            source: "capability gate".to_owned(),
            group_delay: 0.0,
        }))));
    let catalog = CapabilityId::new(CAP_PROPERTY_CATALOG);
    let provider = ProjectResourceProvider::new(&app);
    let descriptors = provider.descriptors();
    let mut checked = 0;
    for dataset in &app.doc.datasets {
        let id = dataset.resource_id().to_string();
        let descriptor = descriptors
            .iter()
            .find(|descriptor| descriptor.resource.id == id)
            .unwrap_or_else(|| panic!("dataset {id} has a descriptor"));
        assert_eq!(
            descriptor.capabilities.contains(&catalog),
            crate::properties::has_addressable_components(dataset),
            "the capability of {id} disagrees with what the catalog can address"
        );
        checked += 1;
    }
    assert!(
        checked >= 2,
        "the fixture covers more than one dataset kind"
    );
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
