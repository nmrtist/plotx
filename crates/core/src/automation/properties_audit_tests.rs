//! What a tool call leaves behind: run manifests, the document's unsaved flag,
//! and the operation record an external write produces.
//!
//! The JSON boundary itself is covered by `properties_tests.rs`, whose fixtures
//! these tests share — the audit axes are the same whichever tool ran, so they
//! are grouped by the question they answer rather than by the tool that
//! triggered them.

use super::tests::{plot_resource_id, request, run, set_request};
use super::*;
use crate::properties::contour;
use crate::properties::tests::contour_app;

#[test]
fn a_human_single_call_writes_no_run_manifest() {
    let (mut app, _) = contour_app();
    let mut request = set_request(&app, contour::COUNT.as_str(), serde_json::json!(7));
    request.caller = CallerType::Human;
    run(&mut app, request).expect("the write lands");
    assert!(
        app.doc.automation_runs.is_empty(),
        "an interactive edit is audited by document state and undo, not by a run record"
    );
}

#[test]
fn an_agent_single_call_writes_a_run_manifest() {
    let (mut app, _) = contour_app();
    let request = set_request(&app, contour::COUNT.as_str(), serde_json::json!(7));
    run(&mut app, request).expect("the write lands");
    assert_eq!(app.doc.automation_runs.len(), 1);
    let manifest = &app.doc.automation_runs[0];
    assert_eq!(manifest.caller, CallerType::Agent);
    assert_eq!(manifest.schema, RUN_MANIFEST_SCHEMA);
    assert_eq!(manifest.nodes.len(), 1);
    assert_eq!(manifest.nodes[0].tool_id, TOOL_SET);
    assert!(!manifest.workflow_hash.is_empty());
    // The recorded workflow is a real one-node workflow, not a lookalike: it
    // has to validate against the same registry a stored workflow does.
    let workflow: WorkflowDefinition =
        serde_json::from_value(manifest.workflow.clone()).expect("the manifest holds a workflow");
    workflow
        .validate(&ToolRegistry::built_in())
        .expect("the synthesized workflow is valid");
    assert!(manifest.end_revision > manifest.start_revision);
}

/// A read-only agent call changes nothing in the document and so produces no
/// `Action` — but it does append a run manifest, and a manifest is document
/// content. If appending one left the document clean, closing the window would
/// discard the audit record we promised to keep without ever offering to save.
#[test]
fn a_read_only_agent_call_leaves_the_document_unsaved() {
    let (mut app, _) = contour_app();
    app.doc.dirty = false;
    let before_revision = app.doc.automation_revision;
    let inspect = request(
        &app,
        TOOL_INSPECT,
        serde_json::json!({"key": contour::COUNT.as_str()}),
        vec![plot_resource_id(&app)],
        CallerType::Agent,
    );
    run(&mut app, inspect).expect("inspect succeeds");
    assert_eq!(app.doc.automation_runs.len(), 1);
    assert!(
        app.doc.dirty,
        "a run record is unsaved document content, even when the run read only"
    );
    assert_eq!(
        app.doc.automation_revision, before_revision,
        "recording a run must not advance the optimistic-concurrency baseline"
    );
}

/// The same for a workflow whose every node is read-only: it too writes one
/// manifest and executes no action.
#[test]
fn a_read_only_workflow_leaves_the_document_unsaved() {
    let (mut app, _) = contour_app();
    app.doc.dirty = false;
    let before_revision = app.doc.automation_revision;
    let workflow = WorkflowDefinition {
        schema: WORKFLOW_SCHEMA.to_owned(),
        inputs: std::collections::BTreeMap::new(),
        nodes: vec![WorkflowNode {
            id: "blueprint".to_owned(),
            tool_id: "project.get_blueprint".to_owned(),
            tool_version: 1,
            parameters: serde_json::json!({}),
            targets: TargetSelector::Explicit { ids: Vec::new() },
            dependencies: Vec::new(),
            bindings: Vec::new(),
            condition: NodeCondition::Always,
            failure_policy: NodeFailurePolicy::Inherit,
        }],
        failure_policy: WorkflowFailurePolicy::Strict,
    };
    execute_workflow(
        &mut app,
        &workflow,
        CallerType::Workflow,
        ExecutionAuthority::Read,
        &TaskCancellation::default(),
        &mut |_| {},
    )
    .expect("the workflow runs");
    assert_eq!(app.doc.automation_runs.len(), 1);
    assert!(app.doc.dirty, "the manifest is unsaved document content");
    assert_eq!(app.doc.automation_revision, before_revision);
}

/// Marking the document unsaved is a statement about the file on disk, not
/// about the state a caller planned against. A plan held across an intervening
/// audit record must still commit.
#[test]
fn recording_a_run_does_not_invalidate_a_plan_in_flight() {
    let (mut app, _) = contour_app();
    let plan = plan_tool(
        &app,
        set_request(&app, contour::COUNT.as_str(), serde_json::json!(8)),
    )
    .expect("the write plans");
    let inspect = request(
        &app,
        TOOL_INSPECT,
        serde_json::json!({"key": contour::COUNT.as_str()}),
        vec![plot_resource_id(&app)],
        CallerType::Agent,
    );
    run(&mut app, inspect).expect("a read-only call lands its own run record");
    execute_tool(&mut app, plan, ExecutionAuthority::ReversibleModify)
        .expect("the earlier plan is still current");
}

/// A workflow already records one manifest for the whole run; its nodes must
/// not each record a second one.
#[test]
fn a_workflow_node_does_not_also_record_a_single_call_manifest() {
    let (mut app, _) = contour_app();
    let workflow = WorkflowDefinition {
        schema: WORKFLOW_SCHEMA.to_owned(),
        inputs: std::collections::BTreeMap::new(),
        nodes: vec![WorkflowNode {
            id: "set-count".to_owned(),
            tool_id: TOOL_SET.to_owned(),
            tool_version: 1,
            parameters: serde_json::json!({"key": contour::COUNT.as_str(), "value": 6}),
            targets: TargetSelector::Explicit {
                ids: vec![plot_resource_id(&app)],
            },
            dependencies: Vec::new(),
            bindings: Vec::new(),
            condition: NodeCondition::Always,
            failure_policy: NodeFailurePolicy::Inherit,
        }],
        failure_policy: WorkflowFailurePolicy::Strict,
    };
    let cancellation = TaskCancellation::default();
    execute_workflow(
        &mut app,
        &workflow,
        CallerType::Workflow,
        ExecutionAuthority::ReversibleModify,
        &cancellation,
        &mut |_| {},
    )
    .expect("the workflow runs");
    assert_eq!(
        app.doc.automation_runs.len(),
        1,
        "one run, one manifest — not one per node as well"
    );
    assert_eq!(app.doc.automation_runs[0].nodes.len(), 1);
    assert_eq!(app.doc.automation_runs[0].nodes[0].node_id, "set-count");
}

#[test]
fn an_external_write_records_an_operation_with_its_fingerprints() {
    let (mut app, _) = contour_app();
    let directory = std::env::temp_dir().join(format!("plotx-export-{}", uuid::Uuid::new_v4()));
    let before = app.session.operation_history.operations().count();
    let export = request(
        &app,
        "figure.export",
        serde_json::json!({"directory": directory.display().to_string(), "format": "svg"}),
        vec![app.doc.canvases[0].resource_id.to_string()],
        CallerType::Agent,
    );
    let result = run(&mut app, export).expect("the export runs");
    assert!(
        result
            .targets
            .iter()
            .any(|target| target.outcome == TargetOutcome::Succeeded)
    );
    let records = app
        .session
        .operation_history
        .operations()
        .collect::<Vec<_>>();
    assert_eq!(
        records.len(),
        before + 1,
        "an external write leaves exactly one operation record"
    );
    let record = records.last().expect("the record exists");
    assert_eq!(record.kind, crate::operation::OperationKind::Export);
    assert!(
        record.summary.contains("figure.export"),
        "{}",
        record.summary
    );
    assert!(record.summary.contains("file(s)"), "{}", record.summary);
    let text = record
        .diagnostics
        .iter()
        .map(|diagnostic| diagnostic.sanitized_text())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        text.contains("sha256="),
        "the fingerprint is recorded: {text}"
    );
    let _ = std::fs::remove_dir_all(&directory);
}

/// An export that fails partway must still account for the bytes it already
/// wrote. Returning early on the first failure would leave files on disk with
/// nothing in the operation history or the run manifest naming them, which is
/// the exact question the external-write audit exists to answer.
#[test]
fn a_partly_failed_export_still_records_what_it_wrote() {
    let (mut app, _) = contour_app();
    app.doc.canvases.push(crate::state::CanvasDocument::new(
        "blocked".to_owned(),
        [120.0, 80.0],
    ));
    let directory = std::env::temp_dir().join(format!("plotx-export-{}", uuid::Uuid::new_v4()));
    // A directory where the exporter must write a file: the write fails for this
    // canvas alone, with the other canvas' output already committed.
    std::fs::create_dir_all(directory.join("blocked.svg")).expect("the obstacle is created");
    let ok_canvas = app.doc.canvases[0].resource_id.to_string();
    let blocked_canvas = app.doc.canvases[1].resource_id.to_string();
    let export = request(
        &app,
        "figure.export",
        serde_json::json!({
            "directory": directory.display().to_string(),
            "format": "svg",
            "overwrite": true,
        }),
        vec![ok_canvas, blocked_canvas],
        CallerType::Agent,
    );
    let result = run(&mut app, export).expect("a failed target is an outcome, not an abort");
    let succeeded = result
        .targets
        .iter()
        .filter(|target| target.outcome == TargetOutcome::Succeeded)
        .collect::<Vec<_>>();
    let failed = result
        .targets
        .iter()
        .filter(|target| target.outcome == TargetOutcome::Failed)
        .collect::<Vec<_>>();
    assert_eq!(succeeded.len(), 1, "{:?}", result.targets);
    assert_eq!(failed.len(), 1, "{:?}", result.targets);
    assert!(
        !succeeded[0].fingerprints.is_empty(),
        "the file that was written is fingerprinted"
    );

    let record = app
        .session
        .operation_history
        .operations()
        .last()
        .expect("the external write is recorded even though one target failed");
    let text = record
        .diagnostics
        .iter()
        .map(|diagnostic| diagnostic.sanitized_text())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        text.contains("sha256="),
        "the bytes that did reach disk are named: {text}"
    );
    assert!(
        record
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == crate::operation::DiagnosticCode::ExportFailed),
        "and the target that failed is named too: {text}"
    );

    let manifest = app
        .doc
        .automation_runs
        .last()
        .expect("the agent call left a manifest");
    assert!(
        !manifest.errors.is_empty(),
        "a manifest whose node failed a target says so, as a workflow's does"
    );
    let recorded = &manifest.nodes[0].result.targets;
    assert!(
        recorded
            .iter()
            .any(|target| !target.fingerprints.is_empty()),
        "and the manifest carries the fingerprints, not one generic error"
    );
    let _ = std::fs::remove_dir_all(&directory);
}

/// A tool that does not write outside the document leaves no operation record.
#[test]
fn a_document_only_tool_records_no_external_write() {
    let (mut app, _) = contour_app();
    let before = app.session.operation_history.operations().count();
    let request = set_request(&app, contour::COUNT.as_str(), serde_json::json!(5));
    run(&mut app, request).expect("the write lands");
    assert_eq!(app.session.operation_history.operations().count(), before);
}
