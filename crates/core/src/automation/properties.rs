//! The JSON boundary of the property catalog.
//!
//! `properties.inspect` / `properties.set` / `properties.reset` are ordinary
//! [`ToolRegistry`](super::ToolRegistry) tools. Everything they do beyond the
//! wire format — resolving a target, deciding applicability, validating a
//! value, folding a write into one atomic action — is delegated to the planner
//! in [`crate::properties`] that the panel controls already call. This module
//! therefore contains no planning and no validation *rules*: it decodes JSON
//! into the typed values that planner accepts, and encodes what it returns.
//!
//! That split is the whole point of the module. A second copy of the rules here
//! would drift from the one the UI uses, and the two entry points would quietly
//! disagree about what a value means — which is precisely the failure the
//! differential test in `properties_tests.rs` exists to catch.

use super::registry::parse;
use super::*;
use crate::properties::{
    AggregateValue, Availability, EnumVariant, FloatBounds, PropertyAccess, PropertyAddress,
    PropertyDefinition, PropertyError, PropertySkip, PropertyValue, ResolvedProperty,
    ResolvedSchema, ValueSchema, definition_by_key, variant_list,
};
use crate::state::PlotxApp;
use serde::{Deserialize, Serialize};

pub const TOOL_INSPECT: &str = "properties.inspect";
pub const TOOL_SET: &str = "properties.set";
pub const TOOL_RESET: &str = "properties.reset";

/// Whether a tool id belongs to the property catalog.
pub(super) fn is_property_tool(tool_id: &str) -> bool {
    matches!(tool_id, TOOL_INSPECT | TOOL_SET | TOOL_RESET)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct PropertyKeyParams {
    pub key: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct PropertyWriteParams {
    pub key: String,
    pub value: serde_json::Value,
}

/// Expand a planned resource list into the components a property addresses.
///
/// Called for every tool; it is a no-op for all but the three property tools,
/// so the twelve pre-existing tools are planned by exactly the code that
/// planned them before. Targets the shared kind/capability gate already
/// rejected are passed through untouched, keeping their original reason: a text
/// box is skipped because it does not expose the property-catalog capability,
/// and nothing here needs to know that text boxes exist.
pub(super) fn refine_plan(
    app: &PlotxApp,
    request: &ToolRequest,
    targets: Vec<PlannedTarget>,
) -> Result<Vec<PlannedTarget>, AutomationError> {
    if !is_property_tool(&request.tool_id) {
        return Ok(targets);
    }
    let definition = requested_definition(&request.tool_id, &request.parameters)?;
    if request.tool_id != TOOL_INSPECT && definition.access == PropertyAccess::ReadOnly {
        return Err(AutomationError::InvalidParameters {
            tool_id: request.tool_id.clone(),
            message: format!("property '{}' is read-only", definition.id),
        });
    }
    if request.tool_id == TOOL_SET {
        // Decode here as well as at execution. Decoding is pure, so this
        // duplicates no rule — but a preflight that accepts a value the commit
        // will refuse is a preflight that answered the wrong question, and the
        // caller finds out only after confirming.
        let params: PropertyWriteParams = parse(&request.tool_id, request.parameters.clone())?;
        decode_value(&request.tool_id, definition, &params.value)?;
    }
    let mut expanded = Vec::new();
    for planned in targets {
        if planned.status != TargetCompatibility::Compatible {
            expanded.push(planned);
            continue;
        }
        let components = app.resource_property_targets(&planned.target.resource, definition);
        if components.is_empty() {
            expanded.push(PlannedTarget {
                target: planned.target,
                status: TargetCompatibility::Skipped,
                reason: format!(
                    "this resource has no {} component to address",
                    definition.applicability.component.as_str()
                ),
            });
            continue;
        }
        for target in components {
            // Applicability per component comes from the definition, never from
            // the tool descriptor. A read is the cheapest form of that same
            // question, and asking it here means the plan preview and the
            // commit agree by construction.
            let address = PropertyAddress::new(target.clone(), definition.id);
            let (status, reason) = match app.resolve_property(&address) {
                Ok(_) => (
                    TargetCompatibility::Compatible,
                    format!("{} applies to this component", definition.canonical_label),
                ),
                Err(error) => (TargetCompatibility::Skipped, error.to_string()),
            };
            expanded.push(PlannedTarget {
                target,
                status,
                reason,
            });
        }
    }
    Ok(expanded)
}

pub(super) fn execute(
    app: &mut PlotxApp,
    plan: &ToolPlan,
    compatible: Vec<TargetRef>,
) -> Result<ToolResult, AutomationError> {
    let tool_id = plan.request.tool_id.clone();
    let definition = requested_definition(&tool_id, &plan.request.parameters)?;
    match tool_id.as_str() {
        TOOL_INSPECT => inspect(app, plan, definition, compatible),
        TOOL_SET => {
            let params: PropertyWriteParams = parse(&tool_id, plan.request.parameters.clone())?;
            let value = decode_value(&tool_id, definition, &params.value)?;
            let commit = app
                .plan_property_write(definition.id, &compatible, &value)
                .map_err(|error| property_error(&tool_id, error))?;
            commit_and_report(app, plan, commit, "set")
        }
        TOOL_RESET => {
            let commit = app
                .plan_property_reset(definition.id, &compatible)
                .map_err(|error| property_error(&tool_id, error))?;
            commit_and_report(app, plan, commit, "reset to its default")
        }
        _ => Err(AutomationError::UnknownTool(tool_id)),
    }
}

fn inspect(
    app: &PlotxApp,
    plan: &ToolPlan,
    definition: &'static PropertyDefinition,
    compatible: Vec<TargetRef>,
) -> Result<ToolResult, AutomationError> {
    let revision = DocumentRevision(app.doc.automation_revision);
    let set = app.resolve_property_set(definition.id, &compatible);
    let mut readings = Vec::new();
    for address in &set.applicable_targets {
        let resolved = app
            .resolve_property(address)
            .map_err(|error| AutomationError::Execution(error.to_string()))?;
        readings.push(reading(&resolved)?);
    }
    let mut targets = skipped_from_plan(plan);
    targets.extend(set.applicable_targets.iter().map(|address| TargetResult {
        target: address.target.clone(),
        outcome: TargetOutcome::Succeeded,
        message: "read".to_owned(),
        skip_reason: None,
        fingerprints: Vec::new(),
    }));
    targets.extend(skipped_results(&set.skipped_targets));
    Ok(ToolResult {
        tool_id: plan.request.tool_id.clone(),
        before_revision: revision,
        after_revision: revision,
        targets,
        produced: Vec::new(),
        modified: Vec::new(),
        diagnostics: Vec::new(),
        verification: Vec::new(),
        value: serde_json::to_value(InspectValue {
            property: definition.id.as_str(),
            canonical_label: definition.canonical_label,
            tier: definition.tier.as_str(),
            aggregate: aggregate_dto(&set.value),
            readings,
        })
        .map_err(|error| AutomationError::Execution(error.to_string()))?,
    })
}

/// Execute a validated commit and report every target exactly once.
///
/// A commit that applies to nothing is not executed at all. What that buys is
/// the *report*, not the document: an empty composite is a no-op the action
/// layer already drops, so nothing would reach history either way. Stopping
/// here is what lets the result say the targets were skipped, and why, instead
/// of reporting a success a caller cannot tell apart from a real change.
fn commit_and_report(
    app: &mut PlotxApp,
    plan: &ToolPlan,
    commit: crate::properties::PropertyCommit,
    verb: &str,
) -> Result<ToolResult, AutomationError> {
    let applied = commit.applied.clone();
    let skipped = commit.skipped.clone();
    let changes_document = commit.has_document_action();
    let before = DocumentRevision(app.doc.automation_revision);
    if !applied.is_empty() {
        app.commit_property(commit);
    }
    let after = DocumentRevision(app.doc.automation_revision);
    let mut targets = skipped_from_plan(plan);
    targets.extend(applied.iter().map(|address| TargetResult {
        target: address.target.clone(),
        outcome: TargetOutcome::Succeeded,
        message: verb.to_owned(),
        skip_reason: None,
        fingerprints: Vec::new(),
    }));
    targets.extend(skipped_results(&skipped));
    let mut modified = Vec::new();
    for address in &applied {
        if !modified.contains(&address.target.resource) {
            modified.push(address.target.resource.clone());
        }
    }
    Ok(ToolResult {
        tool_id: plan.request.tool_id.clone(),
        before_revision: before,
        after_revision: after,
        targets,
        produced: Vec::new(),
        modified,
        diagnostics: Vec::new(),
        verification: vec![VerificationRecord {
            check: "revision_advanced".to_owned(),
            passed: applied.is_empty() || !changes_document || after > before,
            // A commit can apply to nothing because every target refused the
            // property *or* because every target already held the value, and
            // those are opposite answers to "did I address the right thing?".
            // The per-target reasons say which; this line must not assert one.
            message: if applied.is_empty() {
                "nothing was committed; see each target's reason".to_owned()
            } else if changes_document {
                "atomic document commit completed".to_owned()
            } else {
                "app preferences committed without changing the document revision".to_owned()
            },
        }],
        value: serde_json::Value::Null,
    })
}

/// The targets the shared gate rejected, carried into the result so a skip is
/// reported rather than dropped between planning and execution.
fn skipped_from_plan(plan: &ToolPlan) -> Vec<TargetResult> {
    plan.targets
        .iter()
        .filter(|target| target.status != TargetCompatibility::Compatible)
        .map(|target| TargetResult {
            target: target.target.clone(),
            outcome: TargetOutcome::Skipped,
            message: target.reason.clone(),
            // Plan-time skips keep prose only. Some come from the shared
            // kind/capability gate, which runs before the catalog sees the
            // target at all; the rest are `refine_plan`'s own applicability
            // answer. Neither is ever a same-value no-op, so the absence of a
            // reason here is itself the distinction a caller needs.
            skip_reason: None,
            fingerprints: Vec::new(),
        })
        .collect()
}

/// The targets the *planner* skipped, as opposed to the shared gate.
///
/// The ordinary way this is non-empty is a write whose value a target already
/// holds: `refine_plan` decides applicability by asking `resolve_property`, so
/// every target that is merely inapplicable was already marked `Skipped` at plan
/// time — but "does this apply" and "would this change anything" are different
/// questions, and only the planner asks the second. A `properties.set` that
/// re-sends the value a series is already showing reports every target here.
///
/// Each skip carries a typed reason as well as its prose, so a caller can tell
/// a same-value no-op from a target that refused the property without matching
/// on wording.
fn skipped_results(skipped: &[PropertySkip]) -> Vec<TargetResult> {
    skipped
        .iter()
        .map(|skip| TargetResult {
            target: skip.target.clone(),
            outcome: TargetOutcome::Skipped,
            message: skip.message.clone(),
            skip_reason: Some(skip.reason.as_str().to_owned()),
            fingerprints: Vec::new(),
        })
        .collect()
}

fn requested_definition(
    tool_id: &str,
    parameters: &serde_json::Value,
) -> Result<&'static PropertyDefinition, AutomationError> {
    let key = if tool_id == TOOL_SET {
        parse::<PropertyWriteParams>(tool_id, parameters.clone())?.key
    } else {
        parse::<PropertyKeyParams>(tool_id, parameters.clone())?.key
    };
    // An unknown key is refused outright. Skipping it would report a clean
    // success for a call that addressed nothing, which is the worst possible
    // answer to a typo in a headless caller's script.
    definition_by_key(&key).ok_or_else(|| AutomationError::InvalidParameters {
        tool_id: tool_id.to_owned(),
        message: format!("unknown property '{key}'"),
    })
}

/// Decode a JSON value into the typed value the planner accepts.
///
/// The decoding is driven by the definition's own [`ValueSchema`]: a property
/// value is a small closed set of shapes, and which shape is admissible is a
/// property of the definition, not of the JSON. A generic deserialization would
/// have to guess — and could never produce the `&'static str` an enumerated
/// choice is, because a choice is one of the schema's own variants rather than
/// arbitrary text.
fn decode_value(
    tool_id: &str,
    definition: &'static PropertyDefinition,
    value: &serde_json::Value,
) -> Result<PropertyValue, AutomationError> {
    let invalid = |message: String| AutomationError::InvalidParameters {
        tool_id: tool_id.to_owned(),
        message: format!("{}: {message}", definition.id),
    };
    match definition.value_schema {
        ValueSchema::Bool => value
            .as_bool()
            .map(PropertyValue::Bool)
            .ok_or_else(|| invalid(format!("expected true or false, got {}", json_kind(value)))),
        ValueSchema::Int { min, max } => {
            let number = value
                .as_i64()
                .ok_or_else(|| invalid(format!("expected an integer, got {}", json_kind(value))))?;
            if number < min || number > max {
                return Err(invalid(format!(
                    "{number} is out of range: it must be between {min} and {max}"
                )));
            }
            Ok(PropertyValue::Int(number))
        }
        ValueSchema::Float { bounds, .. } => {
            let number = value
                .as_f64()
                .ok_or_else(|| invalid(format!("expected a number, got {}", json_kind(value))))?;
            if !bounds.admits(number) {
                return Err(invalid(format!(
                    "{number} is out of range: it must be {}",
                    bounds.describe()
                )));
            }
            Ok(PropertyValue::Float(number))
        }
        ValueSchema::Enum { variants } => {
            let text = value
                .as_str()
                .ok_or_else(|| invalid(format!("expected a choice, got {}", json_kind(value))))?;
            // Only the *static* variant set is consulted here, because that is
            // the wire question: does this string name a choice at all? Whether
            // the target's field permits the choice depends on that field's
            // capabilities and is answered once, in the planner, for the UI and
            // for this adapter alike.
            variants
                .iter()
                .find(|variant| variant.id == text)
                .map(|variant| PropertyValue::Enum(variant.id))
                .ok_or_else(|| {
                    invalid(format!(
                        "'{text}' is not a choice of this setting; it accepts {}",
                        variant_list(&variants.iter().collect::<Vec<_>>())
                    ))
                })
        }
        ValueSchema::Color => {
            let text = value.as_str().ok_or_else(|| {
                invalid(format!(
                    "expected a '#rrggbb' colour, got {}",
                    json_kind(value)
                ))
            })?;
            parse_color(text)
                .map(PropertyValue::Color)
                .ok_or_else(|| invalid(format!("'{text}' is not a '#rrggbb' colour")))
        }
    }
}

fn parse_color(text: &str) -> Option<plotx_figure::Color> {
    let digits = text.strip_prefix('#')?;
    if digits.len() != 6 || !digits.chars().all(|ch| ch.is_ascii_hexdigit()) {
        return None;
    }
    let channel = |range: std::ops::Range<usize>| u8::from_str_radix(&digits[range], 16).ok();
    Some(plotx_figure::Color::rgb(
        channel(0..2)?,
        channel(2..4)?,
        channel(4..6)?,
    ))
}

fn json_kind(value: &serde_json::Value) -> &'static str {
    match value {
        serde_json::Value::Null => "null",
        serde_json::Value::Bool(_) => "a boolean",
        serde_json::Value::Number(_) => "a number",
        serde_json::Value::String(_) => "a string",
        serde_json::Value::Array(_) => "an array",
        serde_json::Value::Object(_) => "an object",
    }
}

fn property_error(tool_id: &str, error: PropertyError) -> AutomationError {
    match error {
        PropertyError::InvalidValue { .. }
        | PropertyError::ReadOnly(_)
        | PropertyError::UnknownProperty(_)
        | PropertyError::ComponentKind { .. } => AutomationError::InvalidParameters {
            tool_id: tool_id.to_owned(),
            message: error.to_string(),
        },
        PropertyError::UnknownTarget(_)
        | PropertyError::NotApplicable(_)
        | PropertyError::MixedStorage { .. } => AutomationError::Execution(error.to_string()),
    }
}

// ---------------------------------------------------------------------------
// Wire shapes
//
// The catalog model deliberately carries no `serde` derives: it is the
// language-neutral semantic layer, and giving it a wire format would make the
// serialized shape part of its public contract — every future field rename or
// variant split would become a compatibility question for JSON callers. The
// transport shapes live here instead, where changing one is a change to this
// adapter alone.
// ---------------------------------------------------------------------------

#[derive(Serialize)]
struct InspectValue {
    property: &'static str,
    canonical_label: &'static str,
    tier: &'static str,
    aggregate: AggregateValueDto,
    readings: Vec<ReadingDto>,
}

#[derive(Serialize)]
struct ReadingDto {
    target: TargetRef,
    value: AggregateValueDto,
    #[serde(skip_serializing_if = "Option::is_none")]
    default_value: Option<PropertyValueDto>,
    availability: &'static str,
    modified: bool,
    schema: ResolvedSchemaDto,
}

#[derive(Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
enum AggregateValueDto {
    Uniform { value: PropertyValueDto },
    Mixed,
    Unavailable,
}

#[derive(Serialize)]
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
enum PropertyValueDto {
    Bool(bool),
    Int(i64),
    Float(f64),
    Enum(&'static str),
    Color(String),
}

#[derive(Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum ResolvedSchemaDto {
    Bool,
    Int {
        min: i64,
        max: i64,
    },
    Float {
        min: f64,
        max: f64,
        exclusive_min: bool,
        log: bool,
        unit: &'static str,
    },
    Enum {
        variants: Vec<EnumVariantDto>,
    },
    Color,
}

#[derive(Serialize)]
struct EnumVariantDto {
    id: &'static str,
    canonical_label: &'static str,
}

fn reading(resolved: &ResolvedProperty) -> Result<ReadingDto, AutomationError> {
    Ok(ReadingDto {
        target: resolved.address.target.clone(),
        value: aggregate_dto(&resolved.value),
        default_value: resolved.default_value.map(value_dto),
        availability: match resolved.availability {
            Availability::Editable => "editable",
            Availability::ReadOnly => "read_only",
        },
        modified: resolved.is_modified(),
        schema: schema_dto(&resolved.schema),
    })
}

fn aggregate_dto(value: &AggregateValue<PropertyValue>) -> AggregateValueDto {
    match value {
        AggregateValue::Uniform(value) => AggregateValueDto::Uniform {
            value: value_dto(*value),
        },
        AggregateValue::Mixed => AggregateValueDto::Mixed,
        AggregateValue::Unavailable => AggregateValueDto::Unavailable,
    }
}

fn value_dto(value: PropertyValue) -> PropertyValueDto {
    match value {
        PropertyValue::Bool(value) => PropertyValueDto::Bool(value),
        PropertyValue::Int(value) => PropertyValueDto::Int(value),
        PropertyValue::Float(value) => PropertyValueDto::Float(value),
        PropertyValue::Enum(value) => PropertyValueDto::Enum(value),
        PropertyValue::Color(value) => PropertyValueDto::Color(value.to_hex()),
    }
}

fn schema_dto(schema: &ResolvedSchema) -> ResolvedSchemaDto {
    match schema {
        ResolvedSchema::Bool => ResolvedSchemaDto::Bool,
        ResolvedSchema::Int { min, max } => ResolvedSchemaDto::Int {
            min: *min,
            max: *max,
        },
        ResolvedSchema::Float { bounds, log, unit } => float_schema_dto(*bounds, *log, unit),
        ResolvedSchema::Enum { variants } => ResolvedSchemaDto::Enum {
            variants: variants.iter().copied().map(variant_dto).collect(),
        },
        ResolvedSchema::Color => ResolvedSchemaDto::Color,
    }
}

fn float_schema_dto(bounds: FloatBounds, log: bool, unit: &'static str) -> ResolvedSchemaDto {
    ResolvedSchemaDto::Float {
        min: bounds.min,
        max: bounds.max,
        exclusive_min: bounds.exclusive_min,
        log,
        unit,
    }
}

fn variant_dto(variant: &'static EnumVariant) -> EnumVariantDto {
    EnumVariantDto {
        id: variant.id,
        canonical_label: variant.canonical_label,
    }
}

#[cfg(test)]
#[path = "properties_audit_tests.rs"]
mod audit_tests;

#[cfg(test)]
#[path = "properties_tests.rs"]
mod tests;
