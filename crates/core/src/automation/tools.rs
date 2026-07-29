//! Planning, dispatch and audit for the registered tools.
//!
//! What each tool *does* lives in [`super::tool_executors`]; this module decides
//! which targets a tool may touch, routes an approved plan to its executor, and
//! records the evidence a run leaves behind.

use super::*;
use crate::state::{Document, PlotxApp};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::path::Path;

use super::registry::*;
use super::tool_executors::*;

pub fn plan_tool(app: &PlotxApp, request: ToolRequest) -> Result<ToolPlan, AutomationError> {
    let registry = ToolRegistry::built_in();
    let descriptor = registry
        .get(&request.tool_id)
        .ok_or_else(|| AutomationError::UnknownTool(request.tool_id.clone()))?;
    if request.tool_version != descriptor.version {
        return Err(AutomationError::ToolVersion {
            tool_id: request.tool_id.clone(),
            version: request.tool_version,
        });
    }
    validate_parameters(&request.tool_id, request.parameters.clone())?;
    let provider = ProjectResourceProvider::new(app);
    if request.expected_revision != provider.revision() {
        return Err(AutomationError::StaleRevision {
            expected: request.expected_revision.0,
            actual: provider.revision().0,
        });
    }
    let frozen_targets = freeze_targets(&provider, &request.targets)?;
    let targets = declared_targets(&provider, descriptor, &frozen_targets);
    // The one per-tool planning seam. The uniform gate above answers "may this
    // tool touch this resource at all", which is the same question for every
    // tool; a property tool additionally has to expand one resource into the
    // components it addresses, and its applicability comes from a property
    // definition rather than from the tool descriptor. Refining an already
    // gated list keeps that entirely additive: a tool with no refinement is
    // planned by the same code as before, byte for byte.
    let targets = properties::refine_plan(app, &request, targets)?;
    let compatible = targets
        .iter()
        .filter(|target| target.status == TargetCompatibility::Compatible)
        .count();
    Ok(ToolPlan {
        request,
        frozen_targets,
        targets,
        estimated_changes: match descriptor.effect {
            EffectLevel::ReadOnly => Vec::new(),
            _ => vec![format!("{} compatible resource(s)", compatible)],
        },
        outputs: tool_outputs(&descriptor.id),
        required_authority: descriptor.effect.required_authority(),
    })
}

/// The declared kind/capability gate every tool shares.
fn declared_targets(
    provider: &ProjectResourceProvider<'_>,
    descriptor: &ToolDescriptor,
    frozen_targets: &FrozenTargetSet,
) -> Vec<PlannedTarget> {
    frozen_targets
        .targets
        .iter()
        .map(|target| {
            let target_descriptor = provider.inspect(&target.id);
            let compatibility = target_descriptor
                .as_ref()
                .map(|target_descriptor| {
                    (descriptor.target_kinds.is_empty()
                        || descriptor
                            .target_kinds
                            .contains(&target_descriptor.resource.kind))
                        && descriptor
                            .required_capabilities
                            .iter()
                            .all(|required| target_descriptor.capabilities.contains(required))
                })
                .unwrap_or(false);
            PlannedTarget {
                target: TargetRef::resource(target.clone()),
                status: if compatibility {
                    TargetCompatibility::Compatible
                } else {
                    TargetCompatibility::Skipped
                },
                reason: if compatibility {
                    "target satisfies the declared kind and capabilities".to_owned()
                } else {
                    "target lacks a required kind or capability".to_owned()
                },
            }
        })
        .collect()
}

/// Run one tool and audit the call.
///
/// A single tool invocation by a workflow or an agent *is* a one-node workflow:
/// it has the same inputs, produces the same node record and needs the same
/// reproducibility evidence. Synthesizing that node here rather than inventing a
/// second audit shape means the manifest an agent leaves behind is byte-for-byte
/// the manifest a one-node workflow leaves behind, and every reader of
/// `automation_runs` keeps working unchanged (§9, principle 5).
///
/// A `Human` call is untouched: an interactive edit is audited by the document's
/// own final state plus undo, and writing a run manifest for every button press
/// would bury the runs that describe automation.
pub fn execute_tool(
    app: &mut PlotxApp,
    plan: ToolPlan,
    authority: ExecutionAuthority,
) -> Result<ToolResult, AutomationError> {
    let caller = plan.request.caller;
    if caller == CallerType::Human {
        return execute_planned_tool(app, plan, authority);
    }
    let node = single_node_workflow(&plan.request);
    let frozen_targets = plan.frozen_targets.clone();
    let parameters = plan.request.parameters.clone();
    let tool_id = plan.request.tool_id.clone();
    let tool_version = plan.request.tool_version;
    let start_revision = DocumentRevision(app.doc.automation_revision);
    // The same snapshot `execute_workflow` takes before its first node: the
    // input revisions a derived table was built from cannot be recovered once
    // the derivation has happened.
    let start_table_revisions = typed_table_revisions(app, "input");
    let started_unix_ms = unix_ms();
    let started = std::time::Instant::now();
    let outcome = execute_planned_tool(app, plan, authority);
    let end_revision = DocumentRevision(app.doc.automation_revision);
    let (result, errors) = match &outcome {
        Ok(result) => {
            // A tool that reports a per-target failure without failing outright
            // is recorded the way `execute_workflow` records the same node, so
            // the two manifests stay one shape (§9, principle 5).
            let errors = if result
                .targets
                .iter()
                .any(|target| target.outcome == TargetOutcome::Failed)
            {
                vec![format!("{SINGLE_CALL_NODE}: one or more targets failed")]
            } else {
                Vec::new()
            };
            (result.clone(), errors)
        }
        Err(error) => (
            ToolResult {
                tool_id: tool_id.clone(),
                before_revision: start_revision,
                after_revision: end_revision,
                targets: Vec::new(),
                produced: Vec::new(),
                modified: Vec::new(),
                diagnostics: vec![error.to_string()],
                verification: Vec::new(),
                value: serde_json::Value::Null,
            },
            vec![error.to_string()],
        ),
    };
    let record = NodeRunRecord {
        node_id: SINGLE_CALL_NODE.to_owned(),
        tool_id: tool_id.clone(),
        parameters,
        frozen_targets,
        result,
        duration_ms: started.elapsed().as_millis(),
    };
    let workflow = serde_json::to_value(&node)
        .map_err(|error| AutomationError::Execution(error.to_string()))?;
    let canonical = serde_json::to_vec(&workflow)
        .map_err(|error| AutomationError::Execution(error.to_string()))?;
    // Reuse the workflow executor's extraction rather than a second copy: a
    // `data.transform` called directly by an agent has to leave the same input
    // and output table revisions, plan fingerprint and backend identity that the
    // same transform leaves when a workflow runs it, or the run is not
    // reproducible from its own record.
    let records = vec![record];
    let (table_revisions, table_plans) = table_run_records(app, &records, &start_table_revisions);
    record_run(
        &mut app.doc,
        RunManifest {
            schema: RUN_MANIFEST_SCHEMA.to_owned(),
            run_id: uuid::Uuid::new_v4().to_string(),
            caller,
            workflow_hash: format!("{:x}", Sha256::digest(&canonical)),
            workflow,
            application_version: env!("CARGO_PKG_VERSION").to_owned(),
            tool_versions: BTreeMap::from([(tool_id, tool_version)]),
            start_revision,
            end_revision,
            started_unix_ms,
            finished_unix_ms: unix_ms(),
            cancelled: false,
            nodes: records,
            warnings: Vec::new(),
            errors,
            verification: Vec::new(),
            table_revisions,
            table_plans,
        },
    );
    outcome
}

/// The one place a finished run is appended to the document.
///
/// A manifest *is* document content, so appending one makes the document
/// unsaved. Nothing else establishes that: a run made entirely of read-only
/// tools executes no [`Action`](crate::actions::Action) at all, so without this
/// the audit record we promised to keep would be dropped when the window closes
/// without ever offering to save it. Both writers go through here so the next
/// one cannot forget.
///
/// `automation_revision` is deliberately untouched. It is the baseline of the
/// optimistic-concurrency check, and advancing it here would invalidate the plan
/// the caller is still holding — dirtiness says "the file on disk is stale",
/// which is a different question from "has the document the caller planned
/// against moved".
pub(super) fn record_run(document: &mut Document, manifest: RunManifest) {
    document.automation_runs.push(manifest);
    document.mark_dirty();
}

/// The node id a single call is recorded under.
pub const SINGLE_CALL_NODE: &str = "single-call";

/// The one-node workflow a single tool call is equivalent to.
fn single_node_workflow(request: &ToolRequest) -> WorkflowDefinition {
    WorkflowDefinition {
        schema: WORKFLOW_SCHEMA.to_owned(),
        inputs: BTreeMap::new(),
        nodes: vec![WorkflowNode {
            id: SINGLE_CALL_NODE.to_owned(),
            tool_id: request.tool_id.clone(),
            tool_version: request.tool_version,
            parameters: request.parameters.clone(),
            targets: request.targets.clone(),
            dependencies: Vec::new(),
            bindings: Vec::new(),
            condition: NodeCondition::Always,
            failure_policy: NodeFailurePolicy::Inherit,
        }],
        failure_policy: WorkflowFailurePolicy::Strict,
    }
}

use super::workflow::{table_run_records, typed_table_revisions, unix_ms};

/// Execute a plan without the single-call audit wrapper.
///
/// The workflow executor calls this: a run already writes one manifest for the
/// whole graph, and nesting a per-node manifest inside it would double-record
/// every node.
pub(super) fn execute_planned_tool(
    app: &mut PlotxApp,
    plan: ToolPlan,
    authority: ExecutionAuthority,
) -> Result<ToolResult, AutomationError> {
    if authority < plan.required_authority {
        return Err(AutomationError::InsufficientAuthority {
            granted: authority,
            required: plan.required_authority,
        });
    }
    if plan.frozen_targets.revision.0 != app.doc.automation_revision {
        return Err(AutomationError::StaleRevision {
            expected: plan.frozen_targets.revision.0,
            actual: app.doc.automation_revision,
        });
    }
    let before = DocumentRevision(app.doc.automation_revision);
    let tool_id = plan.request.tool_id.clone();
    let mut result = match tool_id.as_str() {
        "project.get_blueprint" => readonly_value(
            &tool_id,
            before,
            serde_json::to_value(ProjectResourceProvider::new(app).blueprint())
                .map_err(|error| AutomationError::Execution(error.to_string()))?,
        ),
        "resources.search" => execute_search(app, &plan)?,
        "resources.inspect" => execute_inspect(app, &plan)?,
        "data.preview" => execute_data_preview(app, &plan)?,
        "render.preview" => execute_render_preview(app, &plan)?,
        "results.compare" => execute_compare(app, &plan)?,
        "resource.rename" => execute_rename(app, &plan)?,
        "figure.apply_theme" => execute_theme(app, &plan)?,
        "processing.apply_scheme" => execute_scheme(app, &plan)?,
        "data.import" => execute_import(app, &plan)?,
        "data.transform" => execute_transform(app, &plan)?,
        "figure.export" => execute_export(app, &plan)?,
        id if properties::is_property_tool(id) => {
            let targets = compatible_target_refs(&plan).cloned().collect();
            properties::execute(app, &plan, targets)?
        }
        _ => return Err(AutomationError::UnknownTool(tool_id)),
    };
    result.before_revision = before;
    result.after_revision = DocumentRevision(app.doc.automation_revision);
    record_external_write(app, &plan, &result);
    Ok(result)
}

/// Record an operation for any tool that wrote outside the document.
///
/// The test is the declared [`EffectLevel::ExternalWrite`], never a tool id: a
/// file that left the application is the thing worth recording, and every future
/// tool that writes one inherits this audit without touching this function.
/// The record names what was written and carries the fingerprints the tool
/// already computed, so "which bytes did this run produce" is answerable from
/// the operation history alone.
fn record_external_write(app: &mut PlotxApp, plan: &ToolPlan, result: &ToolResult) {
    let external = ToolRegistry::built_in()
        .get(&plan.request.tool_id)
        .is_some_and(|descriptor| descriptor.effect == EffectLevel::ExternalWrite);
    if !external {
        return;
    }
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
    let files = succeeded
        .iter()
        .map(|target| target.fingerprints.len())
        .sum::<usize>();
    let summary = format!(
        "{} wrote {files} file(s) from {} target(s)",
        plan.request.tool_id,
        succeeded.len()
    );
    let mut diagnostics = Vec::new();
    for target in &succeeded {
        for fingerprint in &target.fingerprints {
            diagnostics.push(
                crate::operation::Diagnostic::new(
                    crate::operation::Severity::Info,
                    crate::operation::DiagnosticCode::ExportSucceeded,
                    format!(
                        "wrote {} bytes for {}",
                        fingerprint.bytes,
                        target.target.describe()
                    ),
                )
                .with_source("core.automation")
                .with_context("path", fingerprint.path.display().to_string())
                .with_context("sha256", fingerprint.sha256.clone())
                .with_context("role", fingerprint.role.clone()),
            );
        }
    }
    for target in &failed {
        diagnostics.push(
            crate::operation::Diagnostic::new(
                crate::operation::Severity::Error,
                crate::operation::DiagnosticCode::ExportFailed,
                format!("{}: {}", target.target.describe(), target.message),
            )
            .with_source("core.automation"),
        );
    }
    if succeeded.is_empty() && failed.is_empty() {
        diagnostics.push(
            crate::operation::Diagnostic::new(
                crate::operation::Severity::Warning,
                crate::operation::DiagnosticCode::ExportProducedNoFiles,
                "no target produced an external file".to_owned(),
            )
            .with_source("core.automation"),
        );
    }
    let id = app.session.begin_operation();
    let mut report = if failed.is_empty() {
        crate::operation::OperationReport::success(
            id,
            crate::operation::OperationKind::Export,
            summary,
            (),
        )
    } else {
        crate::operation::OperationReport::warning(
            id,
            crate::operation::OperationKind::Export,
            summary,
            (),
        )
    };
    report.diagnostics = diagnostics;
    app.session.record_operation(report);
}

/// The resources of every compatible planned target.
///
/// Tools that address whole resources keep reading a `ResourceRef` here: their
/// planned targets never carry a component, so projecting it away costs them
/// nothing and keeps the component out of code that has no meaning for it.
/// Tools whose plan expands into components read [`compatible_target_refs`].
pub(super) fn compatible_targets(plan: &ToolPlan) -> impl Iterator<Item = &ResourceRef> {
    compatible_target_refs(plan).map(|target| &target.resource)
}

pub(super) fn compatible_target_refs(plan: &ToolPlan) -> impl Iterator<Item = &TargetRef> {
    plan.targets
        .iter()
        .filter(|target| target.status == TargetCompatibility::Compatible)
        .map(|target| &target.target)
}

pub fn write_run_manifest(path: &Path, manifest: &RunManifest) -> Result<(), AutomationError> {
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        std::fs::create_dir_all(parent).map_err(|source| AutomationError::Io {
            path: parent.to_owned(),
            source,
        })?;
    }
    let bytes = serde_json::to_vec_pretty(manifest)
        .map_err(|error| AutomationError::Execution(error.to_string()))?;
    let temporary = path.with_file_name(format!(
        ".{}.{}.tmp",
        path.file_name().unwrap_or_default().to_string_lossy(),
        uuid::Uuid::new_v4()
    ));
    std::fs::write(&temporary, bytes).map_err(|source| AutomationError::Io {
        path: temporary.clone(),
        source,
    })?;
    crate::project::commit_atomic_file(&temporary, path).map_err(|source| AutomationError::Io {
        path: path.to_owned(),
        source,
    })
}
