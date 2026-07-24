use super::*;
use crate::actions::Action;
use crate::state::PlotxApp;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkflowDefinition {
    pub schema: String,
    #[serde(default)]
    pub inputs: BTreeMap<String, WorkflowInput>,
    pub nodes: Vec<WorkflowNode>,
    #[serde(default)]
    pub failure_policy: WorkflowFailurePolicy,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum WorkflowInput {
    Resources { ids: Vec<String> },
    ExternalFiles { paths: Vec<std::path::PathBuf> },
    Parameter { value: serde_json::Value },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkflowNode {
    pub id: String,
    pub tool_id: String,
    #[serde(default = "v1")]
    pub tool_version: u32,
    #[serde(default)]
    pub parameters: serde_json::Value,
    pub targets: TargetSelector,
    #[serde(default)]
    pub dependencies: Vec<String>,
    #[serde(default)]
    pub bindings: Vec<InputBinding>,
    #[serde(default)]
    pub condition: NodeCondition,
    #[serde(default)]
    pub failure_policy: NodeFailurePolicy,
}

fn v1() -> u32 {
    1
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InputBinding {
    pub parameter: String,
    pub source: ValueSource,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ValueSource {
    WorkflowInput { name: String },
    NodeOutput { node: String, port: String },
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowFailurePolicy {
    #[default]
    Strict,
    ContinueCompatible,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NodeFailurePolicy {
    #[default]
    Inherit,
    Abort,
    Continue,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum NodeCondition {
    #[default]
    Always,
    IfSucceeded {
        node: String,
    },
}

impl WorkflowDefinition {
    pub fn validate(&self, registry: &ToolRegistry) -> Result<Vec<String>, AutomationError> {
        if self.schema != WORKFLOW_SCHEMA {
            return Err(AutomationError::InvalidWorkflow(format!(
                "expected schema {WORKFLOW_SCHEMA}, got {}",
                self.schema
            )));
        }
        if self.nodes.is_empty() {
            return Err(AutomationError::InvalidWorkflow(
                "nodes must not be empty".to_owned(),
            ));
        }
        let mut ids = BTreeSet::new();
        for node in &self.nodes {
            if node.id.trim().is_empty() || !ids.insert(node.id.clone()) {
                return Err(AutomationError::InvalidWorkflow(format!(
                    "node id '{}' is empty or duplicated",
                    node.id
                )));
            }
            let descriptor = registry.get(&node.tool_id).ok_or_else(|| {
                AutomationError::InvalidWorkflow(format!(
                    "node {} references unknown tool {}",
                    node.id, node.tool_id
                ))
            })?;
            if node.tool_version != descriptor.version {
                return Err(AutomationError::InvalidWorkflow(format!(
                    "node {} requests unsupported {} v{}",
                    node.id, node.tool_id, node.tool_version
                )));
            }
            validate_node_parameters(node, descriptor)?;
        }
        let all_ids = ids;
        for node in &self.nodes {
            for dependency in node_dependencies(node) {
                if !all_ids.contains(&dependency) {
                    return Err(AutomationError::InvalidWorkflow(format!(
                        "node {} references missing dependency {}",
                        node.id, dependency
                    )));
                }
            }
            if let TargetSelector::WorkflowInput { name } = &node.targets
                && !self.inputs.contains_key(name)
            {
                return Err(AutomationError::InvalidWorkflow(format!(
                    "node {} references missing workflow input {}",
                    node.id, name
                )));
            }
            for binding in &node.bindings {
                if binding.parameter.trim().is_empty() {
                    return Err(AutomationError::InvalidWorkflow(format!(
                        "node {} has an empty binding parameter",
                        node.id
                    )));
                }
                match &binding.source {
                    ValueSource::WorkflowInput { name } if !self.inputs.contains_key(name) => {
                        return Err(AutomationError::InvalidWorkflow(format!(
                            "node {} binds missing input {}",
                            node.id, name
                        )));
                    }
                    ValueSource::NodeOutput { port, .. } if !known_output_port(port) => {
                        return Err(AutomationError::InvalidWorkflow(format!(
                            "node {} references unknown output port {}",
                            node.id, port
                        )));
                    }
                    _ => {}
                }
            }
        }
        topological_order(self)
    }

    /// Resolve filesystem-bearing v1 inputs and registered tool parameters
    /// relative to the workflow file. File formats remain canonical data inputs;
    /// this does not introduce format-specific workflow branches.
    pub fn resolve_paths_from(&mut self, workflow_path: &std::path::Path) {
        let base = std::path::absolute(workflow_path)
            .unwrap_or_else(|_| workflow_path.to_owned())
            .parent()
            .unwrap_or_else(|| std::path::Path::new(""))
            .to_owned();
        for input in self.inputs.values_mut() {
            if let WorkflowInput::ExternalFiles { paths } = input {
                for path in paths {
                    if path.is_relative() {
                        *path = base.join(&*path);
                    }
                }
            }
        }
        for node in &mut self.nodes {
            let keys: &[&str] = match node.tool_id.as_str() {
                "processing.apply_scheme" => &["path"],
                "figure.export" => &["directory"],
                _ => &[],
            };
            if let serde_json::Value::Object(parameters) = &mut node.parameters {
                for key in keys {
                    if let Some(serde_json::Value::String(value)) = parameters.get_mut(*key) {
                        let path = std::path::Path::new(value);
                        if path.is_relative() {
                            *value = base.join(path).to_string_lossy().into_owned();
                        }
                    }
                }
                if node.tool_id == "data.import"
                    && let Some(serde_json::Value::Array(paths)) = parameters.get_mut("paths")
                {
                    for value in paths {
                        if let serde_json::Value::String(value) = value {
                            let path = std::path::Path::new(value);
                            if path.is_relative() {
                                *value = base.join(path).to_string_lossy().into_owned();
                            }
                        }
                    }
                }
            }
        }
    }
}

fn validate_node_parameters(
    node: &WorkflowNode,
    descriptor: &ToolDescriptor,
) -> Result<(), AutomationError> {
    let parameters = node.parameters.as_object().ok_or_else(|| {
        AutomationError::InvalidWorkflow(format!("node {} parameters must be an object", node.id))
    })?;
    let properties = descriptor
        .parameter_schema
        .get("properties")
        .and_then(serde_json::Value::as_object)
        .cloned()
        .unwrap_or_default();
    for key in parameters.keys() {
        if !properties.contains_key(key) {
            return Err(AutomationError::InvalidWorkflow(format!(
                "node {} has unknown parameter {}",
                node.id, key
            )));
        }
    }
    for binding in &node.bindings {
        if !properties.contains_key(&binding.parameter) {
            return Err(AutomationError::InvalidWorkflow(format!(
                "node {} binds unknown parameter {}",
                node.id, binding.parameter
            )));
        }
    }
    let bound = node
        .bindings
        .iter()
        .map(|binding| binding.parameter.as_str())
        .collect::<BTreeSet<_>>();
    let required = descriptor
        .parameter_schema
        .get("required")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(serde_json::Value::as_str);
    for required in required {
        if !parameters.contains_key(required) && !bound.contains(required) {
            return Err(AutomationError::InvalidWorkflow(format!(
                "node {} is missing required parameter {}",
                node.id, required
            )));
        }
    }
    if node.bindings.is_empty() {
        super::registry::validate_parameters(&node.tool_id, node.parameters.clone())?;
    }
    Ok(())
}

pub fn execute_workflow(
    app: &mut PlotxApp,
    workflow: &WorkflowDefinition,
    caller: CallerType,
    authority: ExecutionAuthority,
    cancellation: &TaskCancellation,
    observer: &mut impl FnMut(TaskEvent),
) -> Result<RunManifest, AutomationError> {
    let registry = ToolRegistry::built_in();
    let order = workflow.validate(&registry)?;
    let workflow_value = serde_json::to_value(workflow)
        .map_err(|error| AutomationError::InvalidWorkflow(error.to_string()))?;
    let canonical = serde_json::to_vec(&workflow_value)
        .map_err(|error| AutomationError::InvalidWorkflow(error.to_string()))?;
    let workflow_hash = format!("{:x}", Sha256::digest(&canonical));
    let run_id = uuid::Uuid::new_v4().to_string();
    let started_unix_ms = unix_ms();
    let start_revision = DocumentRevision(app.doc.automation_revision);
    let start_table_revisions = typed_table_revisions(app, "input");
    let undo_start = app.session.undo_stack.len();
    let mut outputs = BTreeMap::<String, ToolResult>::new();
    let mut records = Vec::new();
    let mut errors = Vec::new();
    let mut warnings = Vec::new();
    let mut cancelled = false;
    observer(TaskEvent::Started {
        id: TaskId(run_id.clone()),
        total: order.len(),
    });
    for (position, node_id) in order.iter().enumerate() {
        if cancellation.is_cancelled() {
            cancelled = true;
            warnings.push(format!("cancelled before node {node_id}"));
            break;
        }
        let node = workflow
            .nodes
            .iter()
            .find(|node| &node.id == node_id)
            .expect("validated node");
        if !condition_met(&node.condition, &outputs) {
            warnings.push(format!(
                "node {} skipped because its condition was false",
                node.id
            ));
            observer(TaskEvent::Progress {
                id: TaskId(run_id.clone()),
                completed: position + 1,
                total: order.len(),
                message: format!("Skipped {}", node.id),
            });
            continue;
        }
        let started = Instant::now();
        let parameters = resolve_parameters(node, workflow, &outputs)?;
        let effective_parameters = parameters.clone();
        let targets = resolve_selector(&node.targets, workflow, &outputs)?;
        let request = ToolRequest {
            tool_id: node.tool_id.clone(),
            tool_version: node.tool_version,
            parameters,
            targets,
            expected_revision: DocumentRevision(app.doc.automation_revision),
            caller,
        };
        let outcome = plan_tool(app, request).and_then(|plan| {
            let frozen = plan.frozen_targets.clone();
            execute_tool(app, plan, authority).map(|result| (frozen, result))
        });
        match outcome {
            Ok((frozen_targets, result)) => {
                records.push(NodeRunRecord {
                    node_id: node.id.clone(),
                    tool_id: node.tool_id.clone(),
                    parameters: effective_parameters,
                    frozen_targets,
                    result: result.clone(),
                    duration_ms: started.elapsed().as_millis(),
                });
                let target_failed = result
                    .targets
                    .iter()
                    .any(|target| target.outcome == TargetOutcome::Failed);
                if target_failed {
                    errors.push(format!("{}: one or more targets failed", node.id));
                }
                outputs.insert(node.id.clone(), result);
                if target_failed && should_abort(workflow.failure_policy, node.failure_policy) {
                    rollback_workflow_actions(app, undo_start);
                    break;
                }
            }
            Err(error) => {
                errors.push(format!("{}: {error}", node.id));
                let revision = DocumentRevision(app.doc.automation_revision);
                records.push(NodeRunRecord {
                    node_id: node.id.clone(),
                    tool_id: node.tool_id.clone(),
                    parameters: effective_parameters,
                    frozen_targets: FrozenTargetSet {
                        revision,
                        targets: Vec::new(),
                        reasons: Vec::new(),
                        total_matches: 0,
                        truncated: false,
                    },
                    result: ToolResult {
                        tool_id: node.tool_id.clone(),
                        before_revision: revision,
                        after_revision: revision,
                        targets: Vec::new(),
                        produced: Vec::new(),
                        modified: Vec::new(),
                        diagnostics: vec![error.to_string()],
                        verification: Vec::new(),
                        value: serde_json::Value::Null,
                    },
                    duration_ms: started.elapsed().as_millis(),
                });
                if should_abort(workflow.failure_policy, node.failure_policy) {
                    rollback_workflow_actions(app, undo_start);
                    observer(TaskEvent::Failed {
                        id: TaskId(run_id.clone()),
                        message: error.to_string(),
                    });
                    break;
                }
            }
        }
        observer(TaskEvent::Progress {
            id: TaskId(run_id.clone()),
            completed: position + 1,
            total: order.len(),
            message: format!("Completed {}", node.id),
        });
    }
    collapse_workflow_actions(app, undo_start);
    let end_revision = DocumentRevision(app.doc.automation_revision);
    let verification = vec![VerificationRecord {
        check: "workflow_revision".to_owned(),
        passed: end_revision >= start_revision,
        message: format!("workflow ended at revision {}", end_revision.0),
    }];
    let tool_versions = workflow
        .nodes
        .iter()
        .map(|node| (node.tool_id.clone(), node.tool_version))
        .collect();
    let (table_revisions, table_plans) = table_run_records(app, &records, &start_table_revisions);
    let manifest = RunManifest {
        schema: RUN_MANIFEST_SCHEMA.to_owned(),
        run_id: run_id.clone(),
        caller,
        workflow_hash,
        workflow: workflow_value,
        application_version: env!("CARGO_PKG_VERSION").to_owned(),
        tool_versions,
        start_revision,
        end_revision,
        started_unix_ms,
        finished_unix_ms: unix_ms(),
        cancelled,
        nodes: records,
        warnings,
        errors,
        verification,
        table_revisions,
        table_plans,
    };
    app.doc.automation_runs.push(manifest.clone());
    observer(TaskEvent::Finished {
        id: TaskId(run_id),
        cancelled,
    });
    Ok(manifest)
}

fn typed_table_revisions(app: &PlotxApp, role: &str) -> BTreeMap<String, TableRevisionRecord> {
    app.doc
        .datasets
        .iter()
        .filter_map(|dataset| {
            let table = dataset.as_table()?;
            let revision = &table.typed_state.envelope.revision;
            Some((
                table.resource_id.to_string(),
                TableRevisionRecord {
                    resource_id: table.resource_id.to_string(),
                    role: role.into(),
                    table_id: revision.table_id,
                    revision_id: revision.id,
                    snapshot_fingerprint: revision.snapshot.fingerprint,
                    followed_latest: false,
                },
            ))
        })
        .collect()
}

fn table_run_records(
    app: &PlotxApp,
    nodes: &[NodeRunRecord],
    start: &BTreeMap<String, TableRevisionRecord>,
) -> (Vec<TableRevisionRecord>, Vec<TablePlanRunRecord>) {
    let input_ids = nodes
        .iter()
        .flat_map(|node| &node.frozen_targets.targets)
        .map(table_resource_id)
        .collect::<BTreeSet<_>>();
    let output_ids = nodes
        .iter()
        .flat_map(|node| node.result.produced.iter().chain(&node.result.modified))
        .map(table_resource_id)
        .collect::<BTreeSet<_>>();
    let mut revisions = start
        .iter()
        .filter(|(resource, _)| input_ids.contains(resource.as_str()))
        .map(|(_, revision)| revision.clone())
        .collect::<Vec<_>>();
    let current = typed_table_revisions(app, "output");
    revisions.extend(
        current
            .iter()
            .filter(|(resource, _)| output_ids.contains(resource.as_str()))
            .map(|(_, revision)| revision.clone()),
    );
    let plans = app
        .doc
        .datasets
        .iter()
        .filter_map(|dataset| {
            let table = dataset.as_table()?;
            if !output_ids.contains(table.resource_id.to_string().as_str()) {
                return None;
            }
            let revision = &table.typed_state.envelope.revision;
            let plan = revision.operation.plan.as_ref()?;
            let backend = revision
                .operation
                .parameters
                .get("backend")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_else(|| {
                    if revision.operation.name == "patch.v1" {
                        "plotx.snapshot-patch.v1"
                    } else {
                        "plotx.reference.v1"
                    }
                });
            Some(TablePlanRunRecord {
                plan_fingerprint: plan.fingerprint().ok()?,
                backend: backend.into(),
                input_revisions: revision
                    .operation
                    .inputs
                    .iter()
                    .map(|input| input.revision)
                    .collect(),
                output_revision: revision.id,
                diagnostics: revision.operation.diagnostics.clone(),
            })
        })
        .collect();
    (revisions, plans)
}

fn table_resource_id(resource: &ResourceRef) -> &str {
    resource.parent_id.as_deref().unwrap_or(&resource.id)
}

fn topological_order(workflow: &WorkflowDefinition) -> Result<Vec<String>, AutomationError> {
    let mut incoming = workflow
        .nodes
        .iter()
        .map(|node| {
            (
                node.id.clone(),
                node_dependencies(node).into_iter().collect::<BTreeSet<_>>(),
            )
        })
        .collect::<BTreeMap<_, _>>();
    let mut ready = incoming
        .iter()
        .filter(|(_, dependencies)| dependencies.is_empty())
        .map(|(id, _)| id.clone())
        .collect::<VecDeque<_>>();
    let mut order = Vec::with_capacity(workflow.nodes.len());
    while let Some(id) = ready.pop_front() {
        order.push(id.clone());
        for (candidate, dependencies) in &mut incoming {
            if dependencies.remove(&id)
                && dependencies.is_empty()
                && !order.contains(candidate)
                && !ready.contains(candidate)
            {
                ready.push_back(candidate.clone());
            }
        }
    }
    if order.len() != workflow.nodes.len() {
        return Err(AutomationError::InvalidWorkflow(
            "workflow graph contains a cycle".to_owned(),
        ));
    }
    Ok(order)
}

fn node_dependencies(node: &WorkflowNode) -> Vec<String> {
    let mut dependencies = node.dependencies.clone();
    if let TargetSelector::NodeOutput { node, .. } = &node.targets {
        dependencies.push(node.clone());
    }
    for binding in &node.bindings {
        if let ValueSource::NodeOutput { node, .. } = &binding.source {
            dependencies.push(node.clone());
        }
    }
    if let NodeCondition::IfSucceeded { node } = &node.condition {
        dependencies.push(node.clone());
    }
    dependencies.sort();
    dependencies.dedup();
    dependencies
}

fn resolve_selector(
    selector: &TargetSelector,
    workflow: &WorkflowDefinition,
    outputs: &BTreeMap<String, ToolResult>,
) -> Result<TargetSelector, AutomationError> {
    match selector {
        TargetSelector::WorkflowInput { name } => match workflow.inputs.get(name) {
            Some(WorkflowInput::Resources { ids }) => {
                Ok(TargetSelector::Explicit { ids: ids.clone() })
            }
            Some(WorkflowInput::ExternalFiles { .. }) => {
                Ok(TargetSelector::Explicit { ids: Vec::new() })
            }
            _ => Err(AutomationError::InvalidWorkflow(format!(
                "input {name} is not a resource set"
            ))),
        },
        TargetSelector::NodeOutput { node, port } => {
            let result = outputs.get(node).ok_or_else(|| {
                AutomationError::InvalidWorkflow(format!(
                    "node output {node}.{port} is unavailable"
                ))
            })?;
            let resources = match port.as_str() {
                "produced" | "resources" => &result.produced,
                "modified" => &result.modified,
                _ => {
                    return Err(AutomationError::InvalidWorkflow(format!(
                        "port {port} is not a resource output"
                    )));
                }
            };
            Ok(TargetSelector::Explicit {
                ids: resources
                    .iter()
                    .map(|resource| resource.id.clone())
                    .collect(),
            })
        }
        _ => Ok(selector.clone()),
    }
}

fn resolve_parameters(
    node: &WorkflowNode,
    workflow: &WorkflowDefinition,
    outputs: &BTreeMap<String, ToolResult>,
) -> Result<serde_json::Value, AutomationError> {
    let mut parameters = match &node.parameters {
        serde_json::Value::Object(parameters) => parameters.clone(),
        _ => {
            return Err(AutomationError::InvalidWorkflow(format!(
                "node {} parameters must be an object",
                node.id
            )));
        }
    };
    for binding in &node.bindings {
        let value = match &binding.source {
            ValueSource::WorkflowInput { name } => match workflow.inputs.get(name) {
                Some(WorkflowInput::ExternalFiles { paths }) => serde_json::to_value(paths),
                Some(WorkflowInput::Resources { ids }) => serde_json::to_value(ids),
                Some(WorkflowInput::Parameter { value }) => Ok(value.clone()),
                None => {
                    return Err(AutomationError::InvalidWorkflow(format!(
                        "missing input {name}"
                    )));
                }
            },
            ValueSource::NodeOutput { node, port } => {
                let result = outputs.get(node).ok_or_else(|| {
                    AutomationError::InvalidWorkflow(format!(
                        "node output {node}.{port} is unavailable"
                    ))
                })?;
                match port.as_str() {
                    "value" | "result" => Ok(result.value.clone()),
                    "produced" | "resources" => serde_json::to_value(&result.produced),
                    "modified" => serde_json::to_value(&result.modified),
                    _ => {
                        return Err(AutomationError::InvalidWorkflow(format!(
                            "unknown output port {port}"
                        )));
                    }
                }
            }
        }
        .map_err(|error| AutomationError::InvalidWorkflow(error.to_string()))?;
        parameters.insert(binding.parameter.clone(), value);
    }
    Ok(serde_json::Value::Object(parameters))
}

fn condition_met(condition: &NodeCondition, outputs: &BTreeMap<String, ToolResult>) -> bool {
    match condition {
        NodeCondition::Always => true,
        NodeCondition::IfSucceeded { node } => outputs.contains_key(node),
    }
}

fn should_abort(workflow: WorkflowFailurePolicy, node: NodeFailurePolicy) -> bool {
    match node {
        NodeFailurePolicy::Abort => true,
        NodeFailurePolicy::Continue => false,
        NodeFailurePolicy::Inherit => workflow == WorkflowFailurePolicy::Strict,
    }
}

fn collapse_workflow_actions(app: &mut PlotxApp, start: usize) {
    if app.session.undo_stack.len() <= start + 1 {
        return;
    }
    let actions = app.session.undo_stack.split_off(start);
    app.session.undo_stack.push(Action::Composite(actions));
}

fn rollback_workflow_actions(app: &mut PlotxApp, start: usize) {
    while app.session.undo_stack.len() > start {
        app.undo();
    }
    app.session.redo_stack.clear();
}

fn known_output_port(port: &str) -> bool {
    matches!(
        port,
        "value" | "result" | "produced" | "resources" | "modified"
    )
}

fn unix_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}
