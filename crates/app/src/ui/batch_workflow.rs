//! Desktop Automation panel backed exclusively by `plotx-core::automation`.
//! The UI owns presentation and confirmation; resource resolution, authority,
//! execution, manifests and undo transactions remain in core.

use plotx_core::automation::{
    CallerType, DocumentRevision, ExecutionAuthority, FrozenTargetSet, ProjectResourceProvider,
    ResourceProvider, ResourceQuery, TargetSelector, TaskCancellation, TaskEvent, ToolPlan,
    ToolRegistry, ToolRequest, ToolResult, WorkflowDefinition, execute_tool, execute_workflow,
    plan_tool, search_resources,
};
use plotx_core::state::PlotxApp;
use std::collections::BTreeSet;
use std::path::PathBuf;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum InputSource {
    #[default]
    CurrentProject,
    ExternalInputs,
}

#[derive(Default)]
pub(crate) struct AutomationUi {
    open: bool,
    source: InputSource,
    query: String,
    selected: BTreeSet<String>,
    tool_id: String,
    parameters: String,
    plan: Option<ToolPlan>,
    result: Option<ToolResult>,
    error: Option<String>,
    workflow_path: Option<PathBuf>,
    workflow: Option<WorkflowDefinition>,
    workflow_error: Option<String>,
    events: Vec<TaskEvent>,
    cancellation: TaskCancellation,
}

impl AutomationUi {
    pub(crate) fn request_open(ctx: &egui::Context) {
        ctx.data_mut(|data| data.insert_temp(egui::Id::new("automation_open_request"), true));
    }

    pub(crate) fn is_open(&self) -> bool {
        self.open
    }

    pub(crate) fn show(&mut self, app: &mut PlotxApp, ctx: &egui::Context) {
        if ctx
            .data_mut(|data| data.remove_temp::<bool>(egui::Id::new("automation_open_request")))
            .unwrap_or(false)
        {
            self.open = true;
        }
        if !self.open {
            return;
        }
        let mut open = self.open;
        egui::Window::new("Automation")
            .id(egui::Id::new("automation_panel"))
            .open(&mut open)
            .default_size([760.0, 680.0])
            .show(ctx, |ui| self.contents(app, ui));
        self.open = open;
    }

    fn contents(&mut self, app: &mut PlotxApp, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.selectable_value(
                &mut self.source,
                InputSource::CurrentProject,
                "Current Project",
            );
            ui.selectable_value(
                &mut self.source,
                InputSource::ExternalInputs,
                "External Inputs",
            );
            ui.separator();
            ui.label(format!("Revision {}", app.doc.automation_revision));
            if ui.button("Undo automation").clicked() && app.can_undo() {
                app.undo();
            }
        });
        ui.separator();
        match self.source {
            InputSource::CurrentProject => self.current_project(app, ui),
            InputSource::ExternalInputs => self.workflow_editor(app, ui),
        }
        ui.separator();
        self.results(app, ui);
    }

    fn current_project(&mut self, app: &mut PlotxApp, ui: &mut egui::Ui) {
        ui.heading("Observe and select");
        ui.horizontal(|ui| {
            ui.label("Search");
            if ui.text_edit_singleline(&mut self.query).changed() {
                self.plan = None;
            }
            if ui.button("Current selection").clicked() {
                self.selected = ProjectResourceProvider::new(app)
                    .current_selection()
                    .into_iter()
                    .map(|target| target.id)
                    .collect();
                self.plan = None;
            }
        });
        let query = ResourceQuery {
            name_contains: (!self.query.trim().is_empty()).then(|| self.query.trim().to_owned()),
            limit: 100,
            ..Default::default()
        };
        let found = search_resources(&ProjectResourceProvider::new(app), &query);
        self.resource_list(app, ui, &found);
        ui.separator();
        ui.heading("Plan a registered tool");
        let registry = ToolRegistry::built_in();
        let descriptors = registry.descriptors().collect::<Vec<_>>();
        if self.tool_id.is_empty() {
            self.tool_id = "resource.rename".to_owned();
            self.parameters = r#"{"name":"Renamed resource"}"#.to_owned();
        }
        egui::ComboBox::from_id_salt("automation_tool")
            .selected_text(
                registry
                    .get(&self.tool_id)
                    .map(|tool| tool.title.as_str())
                    .unwrap_or("Choose a tool"),
            )
            .show_ui(ui, |ui| {
                for descriptor in descriptors {
                    if ui
                        .selectable_value(
                            &mut self.tool_id,
                            descriptor.id.clone(),
                            &descriptor.title,
                        )
                        .clicked()
                    {
                        self.parameters = default_parameters(&descriptor.id);
                        self.plan = None;
                    }
                }
            });
        if let Some(descriptor) = registry.get(&self.tool_id) {
            ui.small(&descriptor.description);
            ui.small(format!(
                "Effect: {:?} · Undoable: {} · Deterministic: {}",
                descriptor.effect, descriptor.undoable, descriptor.deterministic
            ));
            ui.collapsing("v1 parameter schema", |ui| {
                ui.monospace(
                    serde_json::to_string_pretty(&descriptor.parameter_schema).unwrap_or_default(),
                );
            });
        }
        ui.label("Parameters (JSON)");
        ui.add(
            egui::TextEdit::multiline(&mut self.parameters)
                .font(egui::TextStyle::Monospace)
                .desired_rows(5),
        );
        ui.horizontal(|ui| {
            if ui.button("Preflight").clicked() {
                self.preflight(app);
            }
            if let Some(plan) = &self.plan {
                ui.label(format!(
                    "{} target(s) · {:?}",
                    plan.targets.len(),
                    plan.required_authority
                ));
                if ui.button("Confirm and execute").clicked() {
                    self.execute_plan(app);
                }
            }
            if ui.button("Cancel").clicked() {
                self.cancellation.cancel();
            }
        });
        if let Some(plan) = &self.plan {
            egui::ScrollArea::vertical()
                .max_height(120.0)
                .show(ui, |ui| {
                    for target in &plan.targets {
                        ui.label(format!(
                            "{:?} · {} · {}",
                            target.status, target.target.id, target.reason
                        ));
                    }
                });
        }
    }

    fn resource_list(&mut self, app: &mut PlotxApp, ui: &mut egui::Ui, found: &FrozenTargetSet) {
        ui.small(format!(
            "{} match(es){}",
            found.total_matches,
            if found.truncated { " (truncated)" } else { "" }
        ));
        let labels = {
            let provider = ProjectResourceProvider::new(app);
            found
                .targets
                .iter()
                .map(|target| {
                    provider
                        .inspect(&target.id)
                        .map(|item| format!("{} · {}", item.name, item.resource.kind.0))
                        .unwrap_or_else(|| target.id.clone())
                })
                .collect::<Vec<_>>()
        };
        egui::ScrollArea::vertical()
            .max_height(190.0)
            .show(ui, |ui| {
                for (target, label) in found.targets.iter().zip(labels) {
                    let mut selected = self.selected.contains(&target.id);
                    if ui.checkbox(&mut selected, label).changed() {
                        if selected {
                            self.selected.insert(target.id.clone());
                        } else {
                            self.selected.remove(&target.id);
                        }
                        self.plan = None;
                        highlight(app, &target.id);
                    }
                }
            });
    }

    fn preflight(&mut self, app: &PlotxApp) {
        self.error = None;
        let parameters = match serde_json::from_str(&self.parameters) {
            Ok(parameters) => parameters,
            Err(error) => {
                self.error = Some(format!("Parameter JSON is invalid: {error}"));
                return;
            }
        };
        let request = ToolRequest {
            tool_id: self.tool_id.clone(),
            tool_version: 1,
            parameters,
            targets: TargetSelector::Explicit {
                ids: self.selected.iter().cloned().collect(),
            },
            expected_revision: DocumentRevision(app.doc.automation_revision),
            caller: CallerType::Human,
        };
        match plan_tool(app, request) {
            Ok(plan) => self.plan = Some(plan),
            Err(error) => self.error = Some(error.to_string()),
        }
    }

    fn execute_plan(&mut self, app: &mut PlotxApp) {
        let Some(plan) = self.plan.take() else {
            return;
        };
        let authority = plan.required_authority;
        match execute_tool(app, plan, authority) {
            Ok(result) => {
                self.result = Some(result);
                self.error = None;
            }
            Err(error) => self.error = Some(error.to_string()),
        }
    }

    fn workflow_editor(&mut self, app: &mut PlotxApp, ui: &mut egui::Ui) {
        ui.heading("Workflow v1 DAG");
        ui.label("External files enter through a data.import node; subsequent nodes consume stable resource outputs.");
        ui.horizontal(|ui| {
            if ui.button("Open workflow…").clicked()
                && let Some(path) = rfd::FileDialog::new()
                    .add_filter("PlotX workflow", &["json"])
                    .pick_file()
            {
                self.load_workflow(path);
            }
            if let Some(path) = &self.workflow_path {
                ui.monospace(path.display().to_string());
            }
        });
        if let Some(error) = &self.workflow_error {
            ui.colored_label(ui.visuals().error_fg_color, error);
        }
        if let Some(workflow) = self.workflow.clone() {
            ui.label(format!(
                "{} node(s) · {:?} failure policy",
                workflow.nodes.len(),
                workflow.failure_policy
            ));
            egui::ScrollArea::vertical()
                .max_height(250.0)
                .show(ui, |ui| {
                    for node in &workflow.nodes {
                        ui.group(|ui| {
                            ui.strong(format!(
                                "{} · {} v{}",
                                node.id, node.tool_id, node.tool_version
                            ));
                            ui.label(format!(
                                "Dependencies: {}",
                                if node.dependencies.is_empty() {
                                    "none".to_owned()
                                } else {
                                    node.dependencies.join(", ")
                                }
                            ));
                            ui.monospace(
                                serde_json::to_string(&node.parameters).unwrap_or_default(),
                            );
                        });
                    }
                });
            ui.horizontal(|ui| {
                if ui.button("Validate").clicked() {
                    self.workflow_error = workflow
                        .validate(&ToolRegistry::built_in())
                        .err()
                        .map(|error| error.to_string());
                }
                if ui.button("Confirm and run workflow").clicked() {
                    self.run_workflow(app);
                }
                if ui.button("Cancel").clicked() {
                    self.cancellation.cancel();
                }
            });
        }
    }

    fn load_workflow(&mut self, path: PathBuf) {
        self.workflow_error = None;
        match std::fs::read(&path)
            .map_err(|error| error.to_string())
            .and_then(|bytes| {
                serde_json::from_slice::<WorkflowDefinition>(&bytes)
                    .map_err(|error| error.to_string())
            }) {
            Ok(mut workflow) => {
                workflow.resolve_paths_from(&path);
                self.workflow = Some(workflow);
                self.workflow_path = Some(path);
            }
            Err(error) => self.workflow_error = Some(error),
        }
    }

    fn run_workflow(&mut self, app: &mut PlotxApp) {
        let Some(workflow) = self.workflow.clone() else {
            return;
        };
        self.cancellation = TaskCancellation::default();
        self.events.clear();
        match execute_workflow(
            app,
            &workflow,
            CallerType::Human,
            ExecutionAuthority::ExternalWrite,
            &self.cancellation,
            &mut |event| self.events.push(event),
        ) {
            Ok(manifest) => {
                self.error = None;
                self.result = manifest.nodes.last().map(|node| node.result.clone());
                app.session.status = format!("Automation run {} completed.", manifest.run_id);
            }
            Err(error) => self.error = Some(error.to_string()),
        }
    }

    fn results(&self, app: &PlotxApp, ui: &mut egui::Ui) {
        ui.heading("Progress and results");
        if let Some(error) = &self.error {
            ui.colored_label(ui.visuals().error_fg_color, error);
        }
        for event in self.events.iter().rev().take(6).rev() {
            ui.small(format!("{event:?}"));
        }
        if let Some(result) = &self.result {
            ui.label(format!(
                "{} · revision {} -> {}",
                result.tool_id, result.before_revision.0, result.after_revision.0
            ));
            for target in &result.targets {
                ui.label(format!("{:?} · {}", target.outcome, target.message));
            }
        }
        ui.small(format!(
            "{} persisted run record(s)",
            app.doc.automation_runs.len()
        ));
        if let Some(canvas) = app
            .session
            .active_canvas
            .and_then(|index| app.doc.canvases.get(index))
        {
            let target = plotx_core::automation::ResourceRef {
                id: canvas.resource_id.to_string(),
                kind: plotx_core::automation::ResourceKindId::new(
                    plotx_core::automation::KIND_CANVAS,
                ),
                parent_id: None,
                local_id: None,
            };
            if let Ok(svg) = ProjectResourceProvider::new(app).render_preview(&target) {
                ui.small(format!(
                    "Live PlotX render preview: {} SVG bytes",
                    svg.len()
                ));
            }
        }
    }
}

fn highlight(app: &mut PlotxApp, id: &str) {
    if let Some(index) = app
        .doc
        .datasets
        .iter()
        .position(|dataset| dataset.resource_id().to_string() == id)
    {
        app.set_active_dataset(Some(index));
    }
    if let Some(index) = app
        .doc
        .canvases
        .iter()
        .position(|canvas| canvas.resource_id.to_string() == id)
    {
        app.session.active_canvas = Some(index);
        app.sync_selection_to_active_canvas();
    }
}

fn default_parameters(tool: &str) -> String {
    match tool {
        "resource.rename" => r#"{"name":"Renamed resource"}"#,
        "figure.apply_theme" => r#"{"theme_id":"publication"}"#,
        "processing.apply_scheme" => r#"{"path":"scheme.plotxproc","compatible_only":true}"#,
        "data.preview" => r#"{"limit":100}"#,
        "resources.search" => r#"{"query":{"limit":50}}"#,
        "results.compare" => r#"{"before":[]}"#,
        "data.import" => r#"{"paths":[]}"#,
        "figure.export" => r#"{"directory":"exports","format":"svg","overwrite":false}"#,
        _ => "{}",
    }
    .to_owned()
}
