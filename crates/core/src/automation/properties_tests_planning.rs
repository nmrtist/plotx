//! Property-tool plan refinement tests.

use super::*;

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
