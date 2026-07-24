use super::*;
use crate::actions::Action;
use crate::state::{CanvasDocument, Dataset, PlotxApp, TableDataset, TableSeriesBinding};
use std::collections::BTreeMap;

fn app_with_table_and_canvas() -> PlotxApp {
    let mut app = PlotxApp::new();
    let x = plotx_data::ColumnSchema::new("time", plotx_data::LogicalType::Float64);
    let y = plotx_data::ColumnSchema::new("signal", plotx_data::LogicalType::Float64);
    let x_id = x.id;
    let y_id = y.id;
    let store = std::sync::Arc::new(plotx_data::MemoryBlockStore::default());
    let codecs = plotx_data::CodecRegistry::with_arrow_ipc();
    let mut builder = plotx_data::SnapshotBuilder::new(
        plotx_data::TableId::new(),
        plotx_data::TableSchema::new(vec![x, y]).unwrap(),
        store.as_ref(),
        &codecs,
    )
    .unwrap();
    let rows = (0..3).map(|_| plotx_data::RowId::new()).collect::<Vec<_>>();
    builder
        .push_batch(
            &rows,
            &[
                plotx_data::ColumnChunk::all_valid(plotx_data::ColumnValues::Float64(vec![
                    1.0, 2.0, 3.0,
                ])),
                plotx_data::ColumnChunk::all_valid(plotx_data::ColumnValues::Float64(vec![
                    2.0, 4.0, 8.0,
                ])),
            ],
        )
        .unwrap();
    let typed = crate::state::TypedTableState::imported(builder.finish().unwrap(), store).unwrap();
    let mut dataset = TableDataset::from_typed(typed);
    dataset.x_binding = Some(x_id);
    dataset.series_bindings = vec![TableSeriesBinding {
        value_column: y_id,
        uncertainty_column: None,
        fit: None,
    }];
    app.doc.datasets.push(Dataset::Table(Box::new(dataset)));
    app.doc
        .canvases
        .push(CanvasDocument::new("Figure 1".to_owned(), [120.0, 90.0]));
    app.set_active_dataset(Some(0));
    app.session.active_canvas = Some(0);
    app
}

fn request(
    tool_id: &str,
    parameters: serde_json::Value,
    ids: Vec<String>,
    revision: u64,
) -> ToolRequest {
    ToolRequest {
        tool_id: tool_id.to_owned(),
        tool_version: 1,
        parameters,
        targets: TargetSelector::Explicit { ids },
        expected_revision: DocumentRevision(revision),
        caller: CallerType::Agent,
    }
}

#[test]
fn query_reports_reasons_pagination_and_stale_frozen_sets_are_rejected() {
    let mut app = app_with_table_and_canvas();
    let query = ResourceQuery {
        capabilities: vec![CapabilityId::new(CAP_RENAME)],
        limit: 1,
        ..Default::default()
    };
    let frozen = search_resources(&ProjectResourceProvider::new(&app), &query);
    assert_eq!(frozen.targets.len(), 1);
    assert_eq!(frozen.total_matches, 2);
    assert!(frozen.truncated);
    assert!(frozen.reasons[0].reason.contains("capability"));

    let id = app.doc.datasets[0].resource_id().to_owned();
    let plan = plan_tool(
        &app,
        request(
            "resource.rename",
            serde_json::json!({"name":"next"}),
            vec![id.to_string()],
            0,
        ),
    )
    .unwrap();
    app.execute_action(Action::rename_canvas(
        0,
        "Figure 1".to_owned(),
        "Changed".to_owned(),
    ));
    assert!(matches!(
        execute_tool(&mut app, plan, ExecutionAuthority::ReversibleModify),
        Err(AutomationError::StaleRevision { .. })
    ));
}

#[test]
fn unknown_tool_parameters_are_rejected_and_registry_ids_are_unique() {
    let app = app_with_table_and_canvas();
    ToolRegistry::built_in().validate_unique().unwrap();
    let id = app.doc.datasets[0].resource_id().to_owned();
    let error = plan_tool(
        &app,
        request(
            "resource.rename",
            serde_json::json!({"name":"ok", "surprise":true}),
            vec![id.to_string()],
            0,
        ),
    )
    .unwrap_err();
    assert!(matches!(error, AutomationError::InvalidParameters { .. }));
    for descriptor in ToolRegistry::built_in().descriptors() {
        assert_eq!(descriptor.parameter_schema["additionalProperties"], false);
    }
}

#[test]
fn data_transform_executes_the_same_persisted_relplan_and_is_undoable() {
    let mut app = app_with_table_and_canvas();
    let source = app.doc.datasets[0].as_table().unwrap();
    let revision = &source.typed_state.envelope.revision;
    let signal = source.series_bindings[0].value_column;
    let relplan = plotx_data::RelPlanV1::new(plotx_data::Relation::Project {
        input: Box::new(plotx_data::Relation::SnapshotRead(
            plotx_data::SnapshotRead {
                table: revision.table_id,
                revision: revision.id,
                fingerprint: revision.snapshot.fingerprint,
            },
        )),
        columns: vec![signal],
    });
    let resource_id = source.resource_id;
    let tool_plan = plan_tool(
        &app,
        request(
            "data.transform",
            serde_json::json!({
                "plan": relplan.clone(),
                "name": "Projected",
                "memory_limit_bytes": 16 * 1024 * 1024,
            }),
            vec![resource_id.to_string()],
            app.doc.automation_revision,
        ),
    )
    .unwrap();
    let result = execute_tool(&mut app, tool_plan, ExecutionAuthority::ReversibleModify).unwrap();

    assert_eq!(result.produced.len(), 1);
    let derived = app.doc.datasets[1].as_table().unwrap();
    let operation = &derived.typed_state.envelope.revision.operation;
    assert_eq!(operation.plan.as_ref(), Some(&relplan));
    assert_eq!(operation.plan_fingerprint, relplan.fingerprint().unwrap());
    assert_eq!(
        operation.parameters["backend"],
        serde_json::Value::String(
            if cfg!(feature = "datafusion") {
                "plotx.datafusion.v1"
            } else {
                "plotx.reference.v1"
            }
            .into()
        )
    );
    assert_eq!(
        derived
            .typed_state
            .envelope
            .revision
            .snapshot
            .schema
            .columns[0]
            .id,
        signal
    );
    app.undo();
    assert_eq!(app.doc.datasets.len(), 1);

    let workflow = WorkflowDefinition {
        schema: WORKFLOW_SCHEMA.into(),
        inputs: BTreeMap::new(),
        nodes: vec![WorkflowNode {
            id: "transform".into(),
            tool_id: "data.transform".into(),
            tool_version: 1,
            parameters: serde_json::json!({
                "plan": relplan,
                "name": "Workflow Projected",
                "memory_limit_bytes": 16 * 1024 * 1024,
            }),
            targets: TargetSelector::Explicit {
                ids: vec![resource_id.to_string()],
            },
            dependencies: Vec::new(),
            bindings: Vec::new(),
            condition: NodeCondition::Always,
            failure_policy: NodeFailurePolicy::Inherit,
        }],
        failure_policy: WorkflowFailurePolicy::Strict,
    };
    let manifest = execute_workflow(
        &mut app,
        &workflow,
        CallerType::Workflow,
        ExecutionAuthority::ReversibleModify,
        &TaskCancellation::default(),
        &mut |_| {},
    )
    .unwrap();
    assert_eq!(manifest.table_plans.len(), 1);
    assert_eq!(
        manifest.table_plans[0].backend,
        if cfg!(feature = "datafusion") {
            "plotx.datafusion.v1"
        } else {
            "plotx.reference.v1"
        }
    );
    assert_eq!(manifest.table_revisions.len(), 2);
}

#[test]
fn composite_validation_prevents_partial_application() {
    let mut app = app_with_table_and_canvas();
    let before = app.doc.datasets[0].display_name();
    let action = Action::Composite(vec![
        Action::rename_dataset(
            app.doc.datasets[0].resource_id(),
            app.doc.datasets[0].name(),
            Some("partial".to_owned()),
        ),
        Action::rename_canvas(99, String::new(), "invalid".to_owned()),
    ]);
    assert!(app.try_execute_action(action).is_err());
    assert_eq!(app.doc.datasets[0].display_name(), before);
    assert!(app.session.undo_stack.is_empty());
}

#[test]
fn dag_executes_by_output_binding_and_collapses_to_one_undo() {
    let mut app = app_with_table_and_canvas();
    let dataset_id = app.doc.datasets[0].resource_id().to_owned();
    let canvas_id = app.doc.canvases[0].resource_id;
    let workflow = WorkflowDefinition {
        schema: WORKFLOW_SCHEMA.to_owned(),
        inputs: BTreeMap::from([(
            "targets".to_owned(),
            WorkflowInput::Resources {
                ids: vec![dataset_id.to_string(), canvas_id.to_string()],
            },
        )]),
        nodes: vec![
            WorkflowNode {
                id: "first".to_owned(),
                tool_id: "resource.rename".to_owned(),
                tool_version: 1,
                parameters: serde_json::json!({"name":"First"}),
                targets: TargetSelector::WorkflowInput {
                    name: "targets".to_owned(),
                },
                dependencies: Vec::new(),
                bindings: Vec::new(),
                condition: NodeCondition::Always,
                failure_policy: NodeFailurePolicy::Inherit,
            },
            WorkflowNode {
                id: "second".to_owned(),
                tool_id: "resource.rename".to_owned(),
                tool_version: 1,
                parameters: serde_json::json!({"name":"Second"}),
                targets: TargetSelector::NodeOutput {
                    node: "first".to_owned(),
                    port: "modified".to_owned(),
                },
                dependencies: vec!["first".to_owned()],
                bindings: Vec::new(),
                condition: NodeCondition::IfSucceeded {
                    node: "first".to_owned(),
                },
                failure_policy: NodeFailurePolicy::Inherit,
            },
        ],
        failure_policy: WorkflowFailurePolicy::Strict,
    };
    let manifest = execute_workflow(
        &mut app,
        &workflow,
        CallerType::Agent,
        ExecutionAuthority::ReversibleModify,
        &TaskCancellation::default(),
        &mut |_| {},
    )
    .unwrap();
    assert_eq!(app.doc.datasets[0].name().as_deref(), Some("Second"));
    assert_eq!(app.doc.canvases[0].name, "Second");
    assert_eq!(app.session.undo_stack.len(), 1);
    assert_eq!(manifest.nodes.len(), 2);
    app.undo();
    assert_eq!(app.doc.canvases[0].name, "Figure 1");
    assert!(app.doc.datasets[0].name().is_none());
}

#[test]
fn dag_validation_rejects_cycles_missing_ports_and_wrong_parameters() {
    let node = |id: &str, dependency: &str| WorkflowNode {
        id: id.to_owned(),
        tool_id: "project.get_blueprint".to_owned(),
        tool_version: 1,
        parameters: serde_json::json!({}),
        targets: TargetSelector::Explicit { ids: Vec::new() },
        dependencies: vec![dependency.to_owned()],
        bindings: Vec::new(),
        condition: NodeCondition::Always,
        failure_policy: NodeFailurePolicy::Inherit,
    };
    let cyclic = WorkflowDefinition {
        schema: WORKFLOW_SCHEMA.to_owned(),
        inputs: BTreeMap::new(),
        nodes: vec![node("a", "b"), node("b", "a")],
        failure_policy: WorkflowFailurePolicy::Strict,
    };
    assert!(cyclic.validate(&ToolRegistry::built_in()).is_err());

    let mut wrong = cyclic;
    wrong.nodes = vec![WorkflowNode {
        id: "bad".to_owned(),
        tool_id: "resource.rename".to_owned(),
        tool_version: 1,
        parameters: serde_json::json!({"unknown": 1}),
        targets: TargetSelector::Explicit { ids: Vec::new() },
        dependencies: Vec::new(),
        bindings: Vec::new(),
        condition: NodeCondition::Always,
        failure_policy: NodeFailurePolicy::Inherit,
    }];
    assert!(wrong.validate(&ToolRegistry::built_in()).is_err());
}

#[test]
fn stable_ids_rows_columns_runs_and_revision_survive_project_roundtrip() {
    let mut app = app_with_table_and_canvas();
    let dataset_id = app.doc.datasets[0].resource_id().to_owned();
    let canvas_id = app.doc.canvases[0].resource_id;
    let table = app.doc.datasets[0].as_table().unwrap();
    let row_ids = table.typed_rows(usize::MAX, &[]).unwrap().row_ids;
    let column_ids = table
        .typed_state
        .envelope
        .revision
        .snapshot
        .schema
        .columns
        .iter()
        .map(|column| column.id)
        .collect::<Vec<_>>();
    app.doc.automation_revision = 7;
    let workflow = WorkflowDefinition {
        schema: WORKFLOW_SCHEMA.to_owned(),
        inputs: BTreeMap::new(),
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
        CallerType::Agent,
        ExecutionAuthority::Read,
        &TaskCancellation::default(),
        &mut |_| {},
    )
    .unwrap();
    app.doc.datasets.reverse();
    let path =
        std::env::temp_dir().join(format!("plotx-automation-{}.plotx", uuid::Uuid::new_v4()));
    crate::project::save_project(&app, &path, false).unwrap();
    let restored = crate::project::load_project(&path).unwrap();
    std::fs::remove_file(path).unwrap();
    assert_eq!(restored.doc.datasets[0].resource_id(), dataset_id);
    assert_eq!(restored.doc.canvases[0].resource_id, canvas_id);
    let table = restored.doc.datasets[0].as_table().unwrap();
    assert_eq!(table.typed_rows(usize::MAX, &[]).unwrap().row_ids, row_ids);
    assert_eq!(
        table
            .typed_state
            .envelope
            .revision
            .snapshot
            .schema
            .columns
            .iter()
            .map(|column| column.id)
            .collect::<Vec<_>>(),
        column_ids
    );
    assert_eq!(restored.doc.automation_revision, 7);
    assert_eq!(restored.doc.automation_runs.len(), 1);
    assert_eq!(restored.doc.automation_runs[0].schema, RUN_MANIFEST_SCHEMA);
}

struct SyntheticProvider;

impl ResourceProvider for SyntheticProvider {
    fn revision(&self) -> DocumentRevision {
        DocumentRevision(3)
    }
    fn blueprint(&self) -> ProjectBlueprint {
        ProjectBlueprint {
            revision: self.revision(),
            resource_counts: BTreeMap::new(),
            relationships: Vec::new(),
            warnings: Vec::new(),
        }
    }
    fn descriptors(&self) -> Vec<ResourceDescriptor> {
        vec![ResourceDescriptor {
            resource: ResourceRef {
                id: "synthetic-1".to_owned(),
                kind: ResourceKindId::new("test.synthetic"),
                parent_id: None,
                local_id: None,
            },
            name: "Synthetic".to_owned(),
            capabilities: vec![CapabilityId::new(CAP_RENAME)],
            children: Vec::new(),
            dimensions: Vec::new(),
            units: Vec::new(),
            metadata: BTreeMap::new(),
            lineage: Vec::new(),
            revision: self.revision(),
        }]
    }
    fn current_selection(&self) -> Vec<ResourceRef> {
        Vec::new()
    }
    fn preview(&self, _: &ResourceRef, _: usize) -> Result<DataPreview, AutomationError> {
        unreachable!()
    }
    fn render_preview(&self, _: &ResourceRef) -> Result<String, AutomationError> {
        unreachable!()
    }
}

#[test]
fn new_provider_kind_uses_generic_query_without_executor_changes() {
    let found = search_resources(
        &SyntheticProvider,
        &ResourceQuery {
            kinds: vec![ResourceKindId::new("test.synthetic")],
            capabilities: vec![CapabilityId::new(CAP_RENAME)],
            ..Default::default()
        },
    );
    assert_eq!(found.targets.len(), 1);
    assert_eq!(found.targets[0].id, "synthetic-1");
    let rename = ToolRegistry::built_in()
        .get("resource.rename")
        .unwrap()
        .clone();
    assert!(rename.target_kinds.is_empty());
    assert_eq!(
        rename.required_capabilities,
        vec![CapabilityId::new(CAP_RENAME)]
    );
}

#[test]
fn simulated_agent_observes_plans_modifies_renders_exports_and_undoes() {
    let mut app = app_with_table_and_canvas();
    let original_dataset = app.doc.datasets[0].display_name();
    let original_canvas = app.doc.canvases[0].name.clone();
    let provider = ProjectResourceProvider::new(&app);
    assert_eq!(provider.blueprint().resource_counts[KIND_DATASET], 1);
    let targets = search_resources(
        &provider,
        &ResourceQuery {
            capabilities: vec![CapabilityId::new(CAP_RENAME)],
            ..Default::default()
        },
    );
    assert!(targets.total_matches >= 2);
    let ids = vec![
        app.doc.datasets[0].resource_id().to_string(),
        app.doc.canvases[0].resource_id.to_string(),
    ];
    let output = std::env::temp_dir().join(format!("plotx-agent-{}", uuid::Uuid::new_v4()));
    let workflow = WorkflowDefinition {
        schema: WORKFLOW_SCHEMA.to_owned(),
        inputs: BTreeMap::from([("targets".to_owned(), WorkflowInput::Resources { ids })]),
        nodes: vec![
            WorkflowNode {
                id: "rename".to_owned(),
                tool_id: "resource.rename".to_owned(),
                tool_version: 1,
                parameters: serde_json::json!({"name":"Agent result"}),
                targets: TargetSelector::WorkflowInput {
                    name: "targets".to_owned(),
                },
                dependencies: Vec::new(),
                bindings: Vec::new(),
                condition: NodeCondition::Always,
                failure_policy: NodeFailurePolicy::Inherit,
            },
            WorkflowNode {
                id: "theme".to_owned(),
                tool_id: "figure.apply_theme".to_owned(),
                tool_version: 1,
                parameters: serde_json::json!({"theme_id":"presentation_dark"}),
                targets: TargetSelector::WorkflowInput {
                    name: "targets".to_owned(),
                },
                dependencies: vec!["rename".to_owned()],
                bindings: Vec::new(),
                condition: NodeCondition::Always,
                failure_policy: NodeFailurePolicy::Inherit,
            },
            WorkflowNode {
                id: "preview".to_owned(),
                tool_id: "render.preview".to_owned(),
                tool_version: 1,
                parameters: serde_json::json!({}),
                targets: TargetSelector::WorkflowInput {
                    name: "targets".to_owned(),
                },
                dependencies: vec!["theme".to_owned()],
                bindings: Vec::new(),
                condition: NodeCondition::Always,
                failure_policy: NodeFailurePolicy::Inherit,
            },
            WorkflowNode {
                id: "export".to_owned(),
                tool_id: "figure.export".to_owned(),
                tool_version: 1,
                parameters: serde_json::json!({"directory":output, "format":"svg", "overwrite":false}),
                targets: TargetSelector::WorkflowInput {
                    name: "targets".to_owned(),
                },
                dependencies: vec!["preview".to_owned()],
                bindings: Vec::new(),
                condition: NodeCondition::Always,
                failure_policy: NodeFailurePolicy::Inherit,
            },
        ],
        failure_policy: WorkflowFailurePolicy::Strict,
    };
    let manifest = execute_workflow(
        &mut app,
        &workflow,
        CallerType::Agent,
        ExecutionAuthority::ExternalWrite,
        &TaskCancellation::default(),
        &mut |_| {},
    )
    .unwrap();
    assert!(manifest.errors.is_empty());
    assert_eq!(manifest.table_revisions.len(), 2);
    assert!(
        manifest
            .table_revisions
            .iter()
            .any(|revision| revision.role == "input")
    );
    assert!(
        manifest
            .table_revisions
            .iter()
            .any(|revision| revision.role == "output")
    );
    assert!(manifest.nodes[2].result.value.to_string().contains("svg"));
    assert!(
        !manifest.nodes[3]
            .result
            .targets
            .iter()
            .flat_map(|target| &target.fingerprints)
            .collect::<Vec<_>>()
            .is_empty()
    );
    assert_eq!(app.session.undo_stack.len(), 1);
    app.undo();
    assert_eq!(app.doc.datasets[0].display_name(), original_dataset);
    assert_eq!(app.doc.canvases[0].name, original_canvas);
    std::fs::remove_dir_all(output).unwrap();
}

#[test]
fn task_manager_propagates_cancellation_and_worker_events() {
    let mut manager = TaskManager::default();
    let id = manager.spawn(|id, cancellation, events| {
        events
            .send(TaskEvent::Started {
                id: id.clone(),
                total: 1,
            })
            .unwrap();
        while !cancellation.is_cancelled() {
            std::thread::yield_now();
        }
        events
            .send(TaskEvent::Finished {
                id,
                cancelled: true,
            })
            .unwrap();
    });
    assert!(manager.cancel(&id));
    let mut observed = Vec::new();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    while std::time::Instant::now() < deadline {
        observed.extend(manager.poll(&id));
        if observed
            .iter()
            .any(|event| matches!(event, TaskEvent::Finished { .. }))
        {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    assert!(
        observed
            .iter()
            .any(|event| matches!(event, TaskEvent::Started { .. }))
    );
    assert!(observed.iter().any(|event| matches!(
        event,
        TaskEvent::Finished {
            cancelled: true,
            ..
        }
    )));
    manager.forget(&id);
}
