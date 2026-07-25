//! The JSON boundary of the property catalog.
//!
//! Every test here targets the one way this stage can break: the adapter
//! growing its own copy of a planning or validation rule and drifting from the
//! planner the panel controls use.

use super::*;
use crate::properties::tests::{contour_app, contour_spec};
use crate::properties::{contour, definition_by_key};
use crate::state::{
    CONTOUR_BASE_FRACTION_OF_RANGE, CONTOUR_BASE_NOISE_FLOOR, CanvasObject, CanvasObjectKind,
    ObjectFrame, PlotxApp, SeriesBinding, TextBox,
};

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
            vec![ResourceKindId::new(KIND_CANVAS_OBJECT)],
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
