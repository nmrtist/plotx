use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ResourceKindId(pub String);

impl ResourceKindId {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct CapabilityId(pub String);

impl CapabilityId {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct DocumentRevision(pub u64);

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResourceRef {
    pub id: String,
    pub kind: ResourceKindId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub local_id: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResourceDescriptor {
    pub resource: ResourceRef,
    pub name: String,
    pub capabilities: Vec<CapabilityId>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<ResourceRef>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub dimensions: Vec<usize>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub units: Vec<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub metadata: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub lineage: Vec<String>,
    pub revision: DocumentRevision,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResourceQuery {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub kinds: Vec<ResourceKindId>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub capabilities: Vec<CapabilityId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name_contains: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub units: Vec<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub metadata: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lineage_source: Option<String>,
    #[serde(default)]
    pub offset: usize,
    #[serde(default = "default_page_size")]
    pub limit: usize,
}

fn default_page_size() -> usize {
    50
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SelectionReason {
    pub resource_id: String,
    pub reason: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FrozenTargetSet {
    pub revision: DocumentRevision,
    pub targets: Vec<ResourceRef>,
    pub reasons: Vec<SelectionReason>,
    pub total_matches: usize,
    pub truncated: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum TargetSelector {
    Explicit { ids: Vec<String> },
    CurrentSelection,
    Query { query: ResourceQuery },
    WorkflowInput { name: String },
    NodeOutput { node: String, port: String },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CallerType {
    Human,
    Workflow,
    Agent,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionAuthority {
    Read,
    ReversibleModify,
    ExternalWrite,
    Destructive,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EffectLevel {
    ReadOnly,
    Reversible,
    ExternalWrite,
    Destructive,
}

impl EffectLevel {
    pub fn required_authority(self) -> ExecutionAuthority {
        match self {
            Self::ReadOnly => ExecutionAuthority::Read,
            Self::Reversible => ExecutionAuthority::ReversibleModify,
            Self::ExternalWrite => ExecutionAuthority::ExternalWrite,
            Self::Destructive => ExecutionAuthority::Destructive,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ToolDescriptor {
    pub id: String,
    pub version: u32,
    pub title: String,
    pub description: String,
    pub parameter_schema: serde_json::Value,
    pub result_schema: serde_json::Value,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub target_kinds: Vec<ResourceKindId>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub required_capabilities: Vec<CapabilityId>,
    pub effect: EffectLevel,
    pub undoable: bool,
    pub deterministic: bool,
    pub task_kind: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ToolRequest {
    pub tool_id: String,
    #[serde(default = "tool_version_v1")]
    pub tool_version: u32,
    #[serde(default)]
    pub parameters: serde_json::Value,
    pub targets: TargetSelector,
    pub expected_revision: DocumentRevision,
    pub caller: CallerType,
}

fn tool_version_v1() -> u32 {
    1
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TargetCompatibility {
    Compatible,
    Skipped,
    Failed,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlannedTarget {
    pub target: ResourceRef,
    pub status: TargetCompatibility,
    pub reason: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ToolPlan {
    pub request: ToolRequest,
    pub frozen_targets: FrozenTargetSet,
    pub targets: Vec<PlannedTarget>,
    pub estimated_changes: Vec<String>,
    pub outputs: Vec<String>,
    pub required_authority: ExecutionAuthority,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TargetOutcome {
    Succeeded,
    Skipped,
    Failed,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TargetResult {
    pub target: ResourceRef,
    pub outcome: TargetOutcome,
    pub message: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fingerprints: Vec<FingerprintRecord>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FingerprintRecord {
    pub role: String,
    pub path: PathBuf,
    pub sha256: String,
    pub bytes: u64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VerificationRecord {
    pub check: String,
    pub passed: bool,
    pub message: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ToolResult {
    pub tool_id: String,
    pub before_revision: DocumentRevision,
    pub after_revision: DocumentRevision,
    pub targets: Vec<TargetResult>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub produced: Vec<ResourceRef>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub modified: Vec<ResourceRef>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub diagnostics: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub verification: Vec<VerificationRecord>,
    #[serde(default)]
    pub value: serde_json::Value,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectBlueprint {
    pub revision: DocumentRevision,
    pub resource_counts: BTreeMap<String, usize>,
    pub relationships: Vec<String>,
    pub warnings: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DataPreview {
    pub target: ResourceRef,
    pub shape: Vec<usize>,
    pub values: serde_json::Value,
    pub returned: usize,
    pub total: usize,
    pub truncated: bool,
    pub statistics: BTreeMap<String, f64>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResourceComparison {
    pub resource_id: String,
    pub changed: bool,
    pub fields: BTreeMap<String, serde_json::Value>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunManifest {
    pub schema: String,
    pub run_id: String,
    pub caller: CallerType,
    pub workflow_hash: String,
    pub workflow: serde_json::Value,
    pub application_version: String,
    pub tool_versions: BTreeMap<String, u32>,
    pub start_revision: DocumentRevision,
    pub end_revision: DocumentRevision,
    pub started_unix_ms: u128,
    pub finished_unix_ms: u128,
    pub cancelled: bool,
    pub nodes: Vec<NodeRunRecord>,
    pub warnings: Vec<String>,
    pub errors: Vec<String>,
    pub verification: Vec<VerificationRecord>,
    /// Exact typed-table revisions consumed or produced by this run. Empty for
    /// workflows that do not touch the v1 table engine.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub table_revisions: Vec<TableRevisionRecord>,
    /// Relation plans actually executed, including backend identity. Stored
    /// plans remain PlotX IR; backend logical plans are never persisted.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub table_plans: Vec<TablePlanRunRecord>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TableRevisionRecord {
    pub resource_id: String,
    pub role: String,
    pub table_id: plotx_data::TableId,
    pub revision_id: plotx_data::RevisionId,
    pub snapshot_fingerprint: plotx_data::ContentHash,
    pub followed_latest: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TablePlanRunRecord {
    pub plan_fingerprint: plotx_data::ContentHash,
    pub backend: String,
    pub input_revisions: Vec<plotx_data::RevisionId>,
    pub output_revision: plotx_data::RevisionId,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub diagnostics: Vec<plotx_data::Diagnostic>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NodeRunRecord {
    pub node_id: String,
    pub tool_id: String,
    pub parameters: serde_json::Value,
    pub frozen_targets: FrozenTargetSet,
    pub result: ToolResult,
    pub duration_ms: u128,
}

#[derive(Debug, thiserror::Error)]
pub enum AutomationError {
    #[error("unknown tool '{0}'")]
    UnknownTool(String),
    #[error("unsupported tool version {version} for {tool_id}")]
    ToolVersion { tool_id: String, version: u32 },
    #[error("invalid parameters for {tool_id}: {message}")]
    InvalidParameters { tool_id: String, message: String },
    #[error("invalid target selector: {0}")]
    InvalidSelector(String),
    #[error("stale automation plan: expected revision {expected}, current revision {actual}")]
    StaleRevision { expected: u64, actual: u64 },
    #[error("authority {granted:?} does not permit {required:?}")]
    InsufficientAuthority {
        granted: ExecutionAuthority,
        required: ExecutionAuthority,
    },
    #[error("workflow is invalid: {0}")]
    InvalidWorkflow(String),
    #[error("tool execution failed: {0}")]
    Execution(String),
    #[error("I/O failed for {}: {source}", path.display())]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}
