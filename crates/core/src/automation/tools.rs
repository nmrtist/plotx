use super::*;
use crate::actions::Action;
use crate::export::{ExportFormat, ExportPageScope, ExportSettings};
use crate::project::{SchemeApplicationPolicy, load_scheme, plan_scheme_application};
use crate::state::PlotxApp;
use crate::theme::Theme;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use super::registry::*;

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
    let targets = frozen_targets
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
                target: target.clone(),
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
        .collect::<Vec<_>>();
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

pub fn execute_tool(
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
        _ => return Err(AutomationError::UnknownTool(tool_id)),
    };
    result.before_revision = before;
    result.after_revision = DocumentRevision(app.doc.automation_revision);
    Ok(result)
}

fn execute_search(app: &PlotxApp, plan: &ToolPlan) -> Result<ToolResult, AutomationError> {
    let params: SearchParams = parse(&plan.request.tool_id, plan.request.parameters.clone())?;
    let found = search_resources(&ProjectResourceProvider::new(app), &params.query);
    serde_result(
        &plan.request.tool_id,
        DocumentRevision(app.doc.automation_revision),
        found,
    )
}

fn execute_inspect(app: &PlotxApp, plan: &ToolPlan) -> Result<ToolResult, AutomationError> {
    let provider = ProjectResourceProvider::new(app);
    let values = compatible_targets(plan)
        .filter_map(|target| provider.inspect(&target.id))
        .collect::<Vec<_>>();
    serde_result(&plan.request.tool_id, provider.revision(), values)
}

fn execute_data_preview(app: &PlotxApp, plan: &ToolPlan) -> Result<ToolResult, AutomationError> {
    let params: PreviewParams = parse(&plan.request.tool_id, plan.request.parameters.clone())?;
    let provider = ProjectResourceProvider::new(app);
    let values = compatible_targets(plan)
        .map(|target| provider.preview(target, params.limit))
        .collect::<Result<Vec<_>, _>>()?;
    serde_result(&plan.request.tool_id, provider.revision(), values)
}

fn execute_render_preview(app: &PlotxApp, plan: &ToolPlan) -> Result<ToolResult, AutomationError> {
    let provider = ProjectResourceProvider::new(app);
    let values = compatible_targets(plan)
        .map(|target| {
            provider
                .render_preview(target)
                .map(|svg| serde_json::json!({"resource_id": target.id, "svg": svg}))
        })
        .collect::<Result<Vec<_>, _>>()?;
    serde_result(&plan.request.tool_id, provider.revision(), values)
}

fn execute_compare(app: &PlotxApp, plan: &ToolPlan) -> Result<ToolResult, AutomationError> {
    let params: CompareParams = parse(&plan.request.tool_id, plan.request.parameters.clone())?;
    let provider = ProjectResourceProvider::new(app);
    let values = compatible_targets(plan)
        .filter_map(|target| provider.inspect(&target.id))
        .map(|current| {
            let before = params
                .before
                .iter()
                .find(|item| item.resource.id == current.resource.id);
            ResourceComparison {
                resource_id: current.resource.id.clone(),
                changed: before != Some(&current),
                fields: BTreeMap::from([
                    (
                        "before".to_owned(),
                        serde_json::to_value(before).unwrap_or_default(),
                    ),
                    (
                        "after".to_owned(),
                        serde_json::to_value(current).unwrap_or_default(),
                    ),
                ]),
            }
        })
        .collect::<Vec<_>>();
    serde_result(&plan.request.tool_id, provider.revision(), values)
}

fn execute_rename(app: &mut PlotxApp, plan: &ToolPlan) -> Result<ToolResult, AutomationError> {
    let params: RenameParams = parse(&plan.request.tool_id, plan.request.parameters.clone())?;
    if params.name.trim().is_empty() {
        return Err(AutomationError::InvalidParameters {
            tool_id: plan.request.tool_id.clone(),
            message: "name must not be empty".to_owned(),
        });
    }
    let mut actions = Vec::new();
    for target in compatible_targets(plan) {
        if let Some(index) = dataset_index(app, &target.id) {
            actions.push(Action::rename_dataset(
                index,
                app.doc.datasets[index].name(),
                Some(params.name.clone()),
            ));
        } else if let Some(index) = canvas_index(app, &target.id) {
            actions.push(Action::rename_canvas(
                index,
                app.doc.canvases[index].name.clone(),
                params.name.clone(),
            ));
        } else if target.kind.0 == KIND_CANVAS_OBJECT
            && let (Some(parent), Some(local)) = (&target.parent_id, &target.local_id)
            && let Some(canvas) = canvas_index(app, parent)
            && let Ok(object_id) = local.parse::<u64>()
            && let Some(object) = app.doc.canvases[canvas].object(object_id)
        {
            actions.push(Action::rename_object(
                canvas,
                object_id,
                object.name.clone(),
                params.name.clone(),
            ));
        }
    }
    commit_actions(app, plan, actions, "renamed")
}

fn execute_theme(app: &mut PlotxApp, plan: &ToolPlan) -> Result<ToolResult, AutomationError> {
    let params: ThemeParams = parse(&plan.request.tool_id, plan.request.parameters.clone())?;
    let theme =
        Theme::by_id(&params.theme_id).ok_or_else(|| AutomationError::InvalidParameters {
            tool_id: plan.request.tool_id.clone(),
            message: format!("unknown theme '{}'", params.theme_id),
        })?;
    let actions = compatible_targets(plan)
        .filter_map(|target| canvas_index(app, &target.id))
        .filter_map(|canvas| app.theme_action(canvas, &theme))
        .collect();
    commit_actions(app, plan, actions, "theme applied")
}

fn execute_scheme(app: &mut PlotxApp, plan: &ToolPlan) -> Result<ToolResult, AutomationError> {
    let params: SchemeParams = parse(&plan.request.tool_id, plan.request.parameters.clone())?;
    let scheme =
        load_scheme(&params.path).map_err(|error| AutomationError::Execution(error.to_string()))?;
    let indices = compatible_targets(plan)
        .filter_map(|target| dataset_index(app, &target.id))
        .collect::<Vec<_>>();
    let scheme_plan = plan_scheme_application(&scheme, &app.doc.datasets, &indices);
    let policy = if params.compatible_only {
        SchemeApplicationPolicy::CompatibleOnly
    } else {
        SchemeApplicationPolicy::StrictAll
    };
    let prepared = scheme_plan.prepare(policy).ok_or_else(|| {
        AutomationError::Execution(
            "no compatible processing targets, or strict policy rejected the selection".to_owned(),
        )
    })?;
    let applied_ids = prepared
        .applied_targets
        .iter()
        .filter_map(|index| app.doc.datasets.get(*index))
        .map(|dataset| dataset.resource_id().to_owned())
        .collect::<BTreeSet<_>>();
    let skipped = scheme_plan
        .targets()
        .iter()
        .filter_map(|target| {
            target.result.incompatibility_reason().map(|reason| {
                let id = app
                    .doc
                    .datasets
                    .get(target.dataset)
                    .map(|dataset| dataset.resource_id().to_owned())
                    .unwrap_or_else(|| format!("stale-{}", target.dataset));
                (id, reason.to_owned())
            })
        })
        .collect::<BTreeMap<_, _>>();
    let mut result = commit_actions(
        app,
        plan,
        vec![prepared.action],
        "processing scheme applied",
    )?;
    result
        .modified
        .retain(|target| applied_ids.contains(&target.id));
    for target in &mut result.targets {
        if let Some(reason) = skipped.get(&target.target.id) {
            target.outcome = TargetOutcome::Skipped;
            target.message.clone_from(reason);
        }
    }
    if let Some(fingerprint) = fingerprint_file(&params.path, "processing_scheme") {
        result.diagnostics.push(format!(
            "processing scheme sha256 {} ({})",
            fingerprint.sha256,
            fingerprint.path.display()
        ));
    }
    Ok(result)
}

fn execute_import(app: &mut PlotxApp, plan: &ToolPlan) -> Result<ToolResult, AutomationError> {
    let params: ImportParams = parse(&plan.request.tool_id, plan.request.parameters.clone())?;
    if params.paths.is_empty() {
        return Err(AutomationError::InvalidParameters {
            tool_id: plan.request.tool_id.clone(),
            message: "paths must contain at least one input".to_owned(),
        });
    }
    let base_dataset = app.doc.datasets.len();
    let base_canvas = app.doc.canvases.len();
    let mut actions = Vec::new();
    let mut produced = Vec::new();
    let mut item_results = Vec::new();
    for path in &params.paths {
        let external = ResourceRef {
            id: path.display().to_string(),
            kind: ResourceKindId::new(KIND_EXTERNAL_INPUT),
            parent_id: None,
            local_id: None,
        };
        let loaded = match crate::workflow::load_dataset(path) {
            Ok(loaded) => loaded,
            Err(error) => {
                item_results.push(TargetResult {
                    target: external,
                    outcome: TargetOutcome::Failed,
                    message: error.to_string(),
                    fingerprints: fingerprint_file(path, "selected_input")
                        .into_iter()
                        .collect(),
                });
                continue;
            }
        };
        let fingerprints = input_fingerprints(&loaded.inspection);
        produced.push(ResourceRef {
            id: loaded.dataset.resource_id().to_owned(),
            kind: ResourceKindId::new(KIND_DATASET),
            parent_id: None,
            local_id: None,
        });
        let offset = actions.len();
        actions.push(Action::InsertDatasetWithCanvas {
            dataset_index: base_dataset + offset,
            canvas_index: base_canvas + offset,
            canvas_resource_id: uuid::Uuid::new_v4().to_string(),
            dataset: Box::new(loaded.dataset),
            canvas_name: path
                .file_stem()
                .map(|value| value.to_string_lossy().into_owned())
                .unwrap_or_else(|| "Imported Data".to_owned()),
            size_mm: crate::state::DEFAULT_CANVAS_SIZE_MM,
            active_canvas_before: app.session.active_canvas,
            active_dataset_before: app.active_dataset(),
            inserted_into_existing_canvas: None,
            inserted_object_id: None,
        });
        item_results.push(TargetResult {
            target: external,
            outcome: TargetOutcome::Succeeded,
            message: "imported into the canonical PlotX data model".to_owned(),
            fingerprints,
        });
    }
    if actions.is_empty() {
        return Ok(ToolResult {
            tool_id: plan.request.tool_id.clone(),
            before_revision: DocumentRevision(app.doc.automation_revision),
            after_revision: DocumentRevision(app.doc.automation_revision),
            targets: item_results,
            produced: Vec::new(),
            modified: Vec::new(),
            diagnostics: Vec::new(),
            verification: Vec::new(),
            value: serde_json::Value::Null,
        });
    }
    let mut result = commit_actions(app, plan, actions, "imported")?;
    produced.extend(
        app.doc
            .canvases
            .iter()
            .skip(base_canvas)
            .map(|canvas| ResourceRef {
                id: canvas.resource_id.clone(),
                kind: ResourceKindId::new(KIND_CANVAS),
                parent_id: None,
                local_id: None,
            }),
    );
    result.produced = produced;
    result.targets.extend(item_results);
    Ok(result)
}

fn execute_transform(app: &mut PlotxApp, plan: &ToolPlan) -> Result<ToolResult, AutomationError> {
    let params: TransformParams = parse(&plan.request.tool_id, plan.request.parameters.clone())?;
    if params.name.trim().is_empty() {
        return Err(AutomationError::InvalidParameters {
            tool_id: plan.request.tool_id.clone(),
            message: "name must not be empty".into(),
        });
    }
    if params.memory_limit_bytes == 0 {
        return Err(AutomationError::InvalidParameters {
            tool_id: plan.request.tool_id.clone(),
            message: "memory_limit_bytes must be positive".into(),
        });
    }
    let input_datasets = compatible_targets(plan)
        .map(|target| {
            dataset_index(app, &target.id).ok_or_else(|| {
                AutomationError::Execution(format!("table resource {} disappeared", target.id))
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    if input_datasets.is_empty() {
        return Err(AutomationError::InvalidParameters {
            tool_id: plan.request.tool_id.clone(),
            message: "select at least one typed table input".into(),
        });
    }
    let index = app
        .derive_table_from_plan(
            params.plan,
            &input_datasets,
            params.name,
            params.memory_limit_bytes,
        )
        .map_err(AutomationError::Execution)?;
    let output = app.doc.datasets[index].as_table().ok_or_else(|| {
        AutomationError::Execution("transform did not produce a typed table".into())
    })?;
    let produced = ResourceRef {
        id: output.resource_id.clone(),
        kind: ResourceKindId::new(KIND_DATASET),
        parent_id: None,
        local_id: None,
    };
    Ok(ToolResult {
        tool_id: plan.request.tool_id.clone(),
        before_revision: DocumentRevision(app.doc.automation_revision),
        after_revision: DocumentRevision(app.doc.automation_revision),
        targets: compatible_targets(plan)
            .cloned()
            .map(|target| TargetResult {
                target,
                outcome: TargetOutcome::Succeeded,
                message: "executed as a pinned PlotX RelPlanV1 input".into(),
                fingerprints: Vec::new(),
            })
            .collect(),
        produced: vec![produced],
        modified: Vec::new(),
        diagnostics: output
            .typed_state
            .envelope
            .revision
            .operation
            .diagnostics
            .iter()
            .map(|diagnostic| diagnostic.message.clone())
            .collect(),
        verification: Vec::new(),
        value: serde_json::json!({
            "table_id": output.typed_state.envelope.revision.table_id,
            "revision_id": output.typed_state.envelope.revision.id,
            "fingerprint": output.typed_state.envelope.revision.snapshot.fingerprint,
        }),
    })
}

fn execute_export(app: &PlotxApp, plan: &ToolPlan) -> Result<ToolResult, AutomationError> {
    let params: ExportParams = parse(&plan.request.tool_id, plan.request.parameters.clone())?;
    let format =
        parse_export_format(&params.format).ok_or_else(|| AutomationError::InvalidParameters {
            tool_id: plan.request.tool_id.clone(),
            message: format!("unsupported export format '{}'", params.format),
        })?;
    std::fs::create_dir_all(&params.directory).map_err(|source| AutomationError::Io {
        path: params.directory.clone(),
        source,
    })?;
    let mut outputs = Vec::new();
    let mut targets = plan
        .targets
        .iter()
        .filter(|target| target.status != TargetCompatibility::Compatible)
        .map(|target| TargetResult {
            target: target.target.clone(),
            outcome: TargetOutcome::Skipped,
            message: target.reason.clone(),
            fingerprints: Vec::new(),
        })
        .collect::<Vec<_>>();
    for target in compatible_targets(plan) {
        let Some(index) = canvas_index(app, &target.id) else {
            continue;
        };
        let canvas = &app.doc.canvases[index];
        let safe_name = sanitize_file_name(&canvas.name);
        let base = params.directory.join(safe_name);
        let expected = crate::export::export_output_paths(&base, format, 1);
        if !params.overwrite && expected.iter().any(|path| path.exists()) {
            targets.push(TargetResult {
                target: target.clone(),
                outcome: TargetOutcome::Failed,
                message: "output already exists and overwrite is false".to_owned(),
                fingerprints: expected
                    .iter()
                    .filter_map(|path| fingerprint_file(path, "existing_output"))
                    .collect(),
            });
            continue;
        }
        let written = crate::export::export_canvases(
            std::slice::from_ref(canvas),
            Some(0),
            &ExportSettings {
                format,
                scope: ExportPageScope::Current,
                dpi: params.dpi,
                target_width_mm: None,
            },
            &base,
        )
        .map_err(|error| AutomationError::Execution(error.to_string()))?;
        outputs.extend(written.iter().map(|path| path.display().to_string()));
        targets.push(TargetResult {
            target: target.clone(),
            outcome: TargetOutcome::Succeeded,
            message: written
                .iter()
                .map(|path| path.display().to_string())
                .collect::<Vec<_>>()
                .join(", "),
            fingerprints: written
                .iter()
                .filter_map(|path| fingerprint_file(path, "output"))
                .collect(),
        });
    }
    Ok(ToolResult {
        tool_id: plan.request.tool_id.clone(),
        before_revision: DocumentRevision(app.doc.automation_revision),
        after_revision: DocumentRevision(app.doc.automation_revision),
        targets,
        produced: Vec::new(),
        modified: Vec::new(),
        diagnostics: outputs,
        verification: vec![VerificationRecord {
            check: "outputs_exist".to_owned(),
            passed: true,
            message: "all successful exports were committed by the PlotX exporter".to_owned(),
        }],
        value: serde_json::Value::Null,
    })
}

fn commit_actions(
    app: &mut PlotxApp,
    plan: &ToolPlan,
    actions: Vec<Action>,
    verb: &str,
) -> Result<ToolResult, AutomationError> {
    if actions.is_empty() {
        return Err(AutomationError::Execution(
            "tool had no compatible targets".to_owned(),
        ));
    }
    app.try_execute_action(Action::Composite(actions))
        .map_err(|error| AutomationError::Execution(error.to_string()))?;
    let modified = compatible_targets(plan).cloned().collect::<Vec<_>>();
    Ok(ToolResult {
        tool_id: plan.request.tool_id.clone(),
        before_revision: plan.frozen_targets.revision,
        after_revision: DocumentRevision(app.doc.automation_revision),
        targets: plan
            .targets
            .iter()
            .map(|target| TargetResult {
                target: target.target.clone(),
                outcome: if target.status == TargetCompatibility::Compatible {
                    TargetOutcome::Succeeded
                } else {
                    TargetOutcome::Skipped
                },
                message: if target.status == TargetCompatibility::Compatible {
                    verb.to_owned()
                } else {
                    target.reason.clone()
                },
                fingerprints: Vec::new(),
            })
            .collect(),
        produced: Vec::new(),
        modified,
        diagnostics: Vec::new(),
        verification: vec![VerificationRecord {
            check: "revision_advanced".to_owned(),
            passed: app.doc.automation_revision > plan.frozen_targets.revision.0,
            message: "atomic document commit completed".to_owned(),
        }],
        value: serde_json::Value::Null,
    })
}

fn compatible_targets(plan: &ToolPlan) -> impl Iterator<Item = &ResourceRef> {
    plan.targets
        .iter()
        .filter(|target| target.status == TargetCompatibility::Compatible)
        .map(|target| &target.target)
}

fn dataset_index(app: &PlotxApp, id: &str) -> Option<usize> {
    app.doc
        .datasets
        .iter()
        .position(|dataset| dataset.resource_id() == id)
}

fn canvas_index(app: &PlotxApp, id: &str) -> Option<usize> {
    app.doc
        .canvases
        .iter()
        .position(|canvas| canvas.resource_id == id)
}

fn serde_result(
    tool_id: &str,
    revision: DocumentRevision,
    value: impl serde::Serialize,
) -> Result<ToolResult, AutomationError> {
    Ok(readonly_value(
        tool_id,
        revision,
        serde_json::to_value(value)
            .map_err(|error| AutomationError::Execution(error.to_string()))?,
    ))
}

fn readonly_value(
    tool_id: &str,
    revision: DocumentRevision,
    value: serde_json::Value,
) -> ToolResult {
    ToolResult {
        tool_id: tool_id.to_owned(),
        before_revision: revision,
        after_revision: revision,
        targets: Vec::new(),
        produced: Vec::new(),
        modified: Vec::new(),
        diagnostics: Vec::new(),
        verification: Vec::new(),
        value,
    }
}

fn parse_export_format(value: &str) -> Option<ExportFormat> {
    match value.to_ascii_lowercase().as_str() {
        "svg" => Some(ExportFormat::Svg),
        "pdf" => Some(ExportFormat::Pdf),
        "png" => Some(ExportFormat::Png),
        "tiff" | "tif" => Some(ExportFormat::Tiff),
        "jpeg" | "jpg" => Some(ExportFormat::Jpeg),
        _ => None,
    }
}

fn sanitize_file_name(value: &str) -> String {
    let value = value
        .chars()
        .map(|ch| if "<>:\"/\\|?*".contains(ch) { '_' } else { ch })
        .collect::<String>();
    let value = value.trim().trim_end_matches(['.', ' ']);
    if value.is_empty() {
        "figure".to_owned()
    } else {
        value.to_owned()
    }
}

fn input_fingerprints(inspection: &crate::workflow::InspectionReport) -> Vec<FingerprintRecord> {
    let mut paths = vec![("selected_input", &inspection.provenance.selected_path)];
    paths.push(("data", &inspection.provenance.data_path));
    paths.extend(
        inspection
            .provenance
            .parameter_paths
            .iter()
            .map(|path| ("parameter", path)),
    );
    let mut seen = BTreeSet::new();
    paths
        .into_iter()
        .filter(|(_, path)| seen.insert((*path).clone()))
        .filter_map(|(role, path)| fingerprint_file(path, role))
        .collect()
}

fn fingerprint_file(path: &Path, role: &str) -> Option<FingerprintRecord> {
    let bytes = std::fs::read(path).ok()?;
    Some(FingerprintRecord {
        role: role.to_owned(),
        path: path.to_owned(),
        sha256: format!("{:x}", Sha256::digest(&bytes)),
        bytes: bytes.len() as u64,
    })
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
