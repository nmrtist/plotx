use super::*;
use serde::Deserialize;
use serde::de::DeserializeOwned;
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

pub struct ToolRegistry {
    descriptors: BTreeMap<String, ToolDescriptor>,
}

impl Default for ToolRegistry {
    fn default() -> Self {
        Self::built_in()
    }
}

impl ToolRegistry {
    pub fn built_in() -> Self {
        Self {
            descriptors: descriptors()
                .into_iter()
                .map(|item| (item.id.clone(), item))
                .collect(),
        }
    }
    pub fn descriptors(&self) -> impl Iterator<Item = &ToolDescriptor> {
        self.descriptors.values()
    }
    pub fn get(&self, id: &str) -> Option<&ToolDescriptor> {
        self.descriptors.get(id)
    }
    pub fn validate_unique(&self) -> Result<(), AutomationError> {
        let all = descriptors();
        let ids = all
            .iter()
            .map(|item| item.id.as_str())
            .collect::<BTreeSet<_>>();
        (ids.len() == all.len()).then_some(()).ok_or_else(|| {
            AutomationError::InvalidWorkflow(
                "tool registry contains a duplicate tool id".to_owned(),
            )
        })
    }
}

pub(super) fn validate_parameters(
    tool: &str,
    value: serde_json::Value,
) -> Result<(), AutomationError> {
    match tool {
        "project.get_blueprint" | "resources.inspect" | "render.preview" => {
            parse::<EmptyParams>(tool, value).map(drop)
        }
        "resources.search" => parse::<SearchParams>(tool, value).map(drop),
        "data.preview" => parse::<PreviewParams>(tool, value).map(drop),
        "results.compare" => parse::<CompareParams>(tool, value).map(drop),
        "resource.rename" => parse::<RenameParams>(tool, value).map(drop),
        "figure.apply_theme" => parse::<ThemeParams>(tool, value).map(drop),
        "processing.apply_scheme" => parse::<SchemeParams>(tool, value).map(drop),
        "data.import" => parse::<ImportParams>(tool, value).map(drop),
        "data.transform" => parse::<TransformParams>(tool, value).map(drop),
        "figure.export" => parse::<ExportParams>(tool, value).map(drop),
        _ => Err(AutomationError::UnknownTool(tool.to_owned())),
    }
}

pub(super) fn parse<T: DeserializeOwned>(
    tool: &str,
    value: serde_json::Value,
) -> Result<T, AutomationError> {
    serde_json::from_value(value).map_err(|error| AutomationError::InvalidParameters {
        tool_id: tool.to_owned(),
        message: error.to_string(),
    })
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct EmptyParams {}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct SearchParams {
    pub query: ResourceQuery,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct PreviewParams {
    #[serde(default = "preview_limit")]
    pub limit: usize,
}
fn preview_limit() -> usize {
    100
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct CompareParams {
    #[serde(default)]
    pub before: Vec<ResourceDescriptor>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct RenameParams {
    pub name: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ThemeParams {
    pub theme_id: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct SchemeParams {
    pub path: PathBuf,
    #[serde(default)]
    pub compatible_only: bool,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ImportParams {
    pub paths: Vec<PathBuf>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct TransformParams {
    pub plan: plotx_data::RelPlanV1,
    pub name: String,
    #[serde(default = "default_table_memory_bytes")]
    pub memory_limit_bytes: u64,
}
fn default_table_memory_bytes() -> u64 {
    512 * 1024 * 1024
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ExportParams {
    pub directory: PathBuf,
    pub format: String,
    #[serde(default = "default_dpi")]
    pub dpi: u16,
    #[serde(default)]
    pub overwrite: bool,
}
fn default_dpi() -> u16 {
    crate::export::DEFAULT_BITMAP_DPI
}

trait V1Schema {
    fn schema() -> serde_json::Value;
}
macro_rules! schema {
    ($type:ty, [$($required:literal => $rkind:literal),*], [$($optional:literal => $okind:literal),*]) => {
        impl V1Schema for $type {
            fn schema() -> serde_json::Value { object_schema(&[$(($required, $rkind)),*], &[$(($optional, $okind)),*]) }
        }
    };
}
schema!(EmptyParams, [], []);
schema!(SearchParams, ["query" => "object"], []);
schema!(PreviewParams, [], ["limit" => "integer"]);
schema!(CompareParams, [], ["before" => "array"]);
schema!(RenameParams, ["name" => "string"], []);
schema!(ThemeParams, ["theme_id" => "string"], []);
schema!(SchemeParams, ["path" => "string"], ["compatible_only" => "boolean"]);
schema!(ImportParams, ["paths" => "array"], []);
schema!(TransformParams, ["plan" => "object", "name" => "string"], ["memory_limit_bytes" => "integer"]);
schema!(ExportParams, ["directory" => "string", "format" => "string"], ["dpi" => "integer", "overwrite" => "boolean"]);

struct Spec {
    id: &'static str,
    title: &'static str,
    description: &'static str,
    schema: serde_json::Value,
    kinds: &'static [&'static str],
    capabilities: &'static [&'static str],
    effect: EffectLevel,
    deterministic: bool,
}

macro_rules! tool {
    ($id:literal, $title:literal, $description:literal, $params:ty, [$($kind:expr),*], [$($cap:expr),*], $effect:expr, $deterministic:literal) => {
        Spec { id: $id, title: $title, description: $description, schema: <$params>::schema(), kinds: &[$($kind),*], capabilities: &[$($cap),*], effect: $effect, deterministic: $deterministic }
    };
}

fn descriptors() -> Vec<ToolDescriptor> {
    vec![
        tool!(
            "project.get_blueprint",
            "Get project blueprint",
            "Read compact project structure",
            EmptyParams,
            [],
            [],
            EffectLevel::ReadOnly,
            true
        ),
        tool!(
            "resources.search",
            "Search resources",
            "Query resources by kind and capability",
            SearchParams,
            [],
            [],
            EffectLevel::ReadOnly,
            true
        ),
        tool!(
            "resources.inspect",
            "Inspect resources",
            "Read resource descriptors",
            EmptyParams,
            [],
            [],
            EffectLevel::ReadOnly,
            true
        ),
        tool!(
            "data.preview",
            "Preview data",
            "Read a bounded data slice and statistics",
            PreviewParams,
            [],
            [CAP_PREVIEW],
            EffectLevel::ReadOnly,
            true
        ),
        tool!(
            "render.preview",
            "Render preview",
            "Render a PlotX canvas to SVG",
            EmptyParams,
            [KIND_CANVAS],
            [CAP_RENDER],
            EffectLevel::ReadOnly,
            true
        ),
        tool!(
            "results.compare",
            "Compare results",
            "Compare resource descriptions",
            CompareParams,
            [],
            [],
            EffectLevel::ReadOnly,
            true
        ),
        tool!(
            "resource.rename",
            "Rename resource",
            "Rename resources that expose the generic rename capability",
            RenameParams,
            [],
            [CAP_RENAME],
            EffectLevel::Reversible,
            true
        ),
        tool!(
            "figure.apply_theme",
            "Apply theme",
            "Apply a registered PlotX theme",
            ThemeParams,
            [KIND_CANVAS],
            [CAP_THEME],
            EffectLevel::Reversible,
            true
        ),
        tool!(
            "processing.apply_scheme",
            "Apply processing scheme",
            "Apply a .plotxproc parameter resource",
            SchemeParams,
            [KIND_DATASET],
            [CAP_PROCESSING_SCHEME],
            EffectLevel::Reversible,
            true
        ),
        tool!(
            "data.import",
            "Import data",
            "Import canonical datasets into the project",
            ImportParams,
            [],
            [],
            EffectLevel::Reversible,
            false
        ),
        tool!(
            "data.transform",
            "Transform typed tables",
            "Execute a frozen PlotX RelPlanV1 against selected typed tables",
            TransformParams,
            [KIND_DATASET],
            [CAP_TRANSFORM],
            EffectLevel::Reversible,
            true
        ),
        tool!(
            "figure.export",
            "Export figures",
            "Export canvases with the PlotX renderer",
            ExportParams,
            [KIND_CANVAS],
            [CAP_EXPORT],
            EffectLevel::ExternalWrite,
            true
        ),
    ]
    .into_iter()
    .map(make_descriptor)
    .collect()
}

fn make_descriptor(spec: Spec) -> ToolDescriptor {
    ToolDescriptor {
        id: spec.id.to_owned(),
        version: 1,
        title: spec.title.to_owned(),
        description: spec.description.to_owned(),
        parameter_schema: spec.schema,
        result_schema: serde_json::json!({"type":"object","additionalProperties":false}),
        target_kinds: spec
            .kinds
            .iter()
            .copied()
            .map(ResourceKindId::new)
            .collect(),
        required_capabilities: spec
            .capabilities
            .iter()
            .copied()
            .map(CapabilityId::new)
            .collect(),
        effect: spec.effect,
        undoable: spec.effect == EffectLevel::Reversible,
        deterministic: spec.deterministic,
        task_kind: if spec.effect == EffectLevel::ReadOnly {
            "synchronous"
        } else {
            "snapshot_then_commit"
        }
        .to_owned(),
    }
}

fn object_schema(required: &[(&str, &str)], optional: &[(&str, &str)]) -> serde_json::Value {
    let properties = required
        .iter()
        .chain(optional)
        .map(|(name, kind)| ((*name).to_owned(), serde_json::json!({"type":kind})))
        .collect::<serde_json::Map<_, _>>();
    serde_json::json!({"type":"object","properties":properties,"required":required.iter().map(|(name, _)| *name).collect::<Vec<_>>(),"additionalProperties":false})
}

pub(super) fn tool_outputs(id: &str) -> Vec<String> {
    match id {
        "data.import" | "data.transform" => vec!["resources".to_owned()],
        "figure.export" => vec!["files".to_owned()],
        _ => vec!["result".to_owned()],
    }
}
