use plotx_core::operation::{Diagnostic, DiagnosticCode, OperationKind, OperationReport, Severity};
use plotx_core::project::{self, SchemeApplicationPolicy};
use plotx_core::state::{
    Dataset, PlotxApp, ProcessingSchemeDialogState, ProcessingTemplateDialogState,
    TemplateBrowserEntry,
};

pub(crate) fn can_use_templates(app: &PlotxApp, di: usize) -> bool {
    app.doc
        .datasets
        .get(di)
        .is_some_and(|d| !matches!(d, Dataset::Table(_)))
}

pub(crate) fn open_save_template_dialog(app: &mut PlotxApp, di: usize) {
    if !can_use_templates(app, di) {
        return;
    }
    app.session.ui.processing_template_dialog = Some(ProcessingTemplateDialogState::SaveAs {
        dataset: di,
        name: String::new(),
    });
}

pub(crate) fn open_template_browser(app: &mut PlotxApp, di: usize) {
    if !can_use_templates(app, di) {
        return;
    }
    app.session.ui.processing_template_dialog = Some(ProcessingTemplateDialogState::Browse {
        dataset: di,
        entries: browse_entries(),
        confirm_delete: None,
    });
}

fn browse_entries() -> Vec<TemplateBrowserEntry> {
    let Some(dir) = project::templates_dir() else {
        return Vec::new();
    };
    project::list_templates(&dir)
        .into_iter()
        .map(|info| {
            let scheme = project::load_scheme(&info.path).map_err(|e| e.to_string());
            TemplateBrowserEntry {
                name: info.name,
                path: info.path,
                scheme,
            }
        })
        .collect()
}

pub(crate) fn processing_template_window(app: &mut PlotxApp, ctx: &egui::Context) {
    let Some(mut dialog) = app.session.ui.processing_template_dialog.take() else {
        return;
    };
    let keep = match &mut dialog {
        ProcessingTemplateDialogState::SaveAs { dataset, name } => {
            save_as_window(app, ctx, *dataset, name)
        }
        ProcessingTemplateDialogState::Browse {
            dataset,
            entries,
            confirm_delete,
        } => browse_window(app, ctx, *dataset, entries, confirm_delete),
    };
    if keep && app.session.ui.processing_template_dialog.is_none() {
        app.session.ui.processing_template_dialog = Some(dialog);
    }
}

fn save_as_window(
    app: &mut PlotxApp,
    ctx: &egui::Context,
    dataset: usize,
    name: &mut String,
) -> bool {
    if !can_use_templates(app, dataset) {
        return false;
    }
    let Some(dir) = project::templates_dir() else {
        return false;
    };
    let mut save = false;
    let mut cancel = false;
    let modal = super::modal(
        ctx,
        "processing_template_save_modal",
        super::ModalKind::Dialog,
    )
    .show(ctx, |ui| {
        ui.heading("Save processing template");
        ui.separator();
        ui.label("Save this dataset's processing pipeline as a reusable template.");
        ui.add(
            egui::TextEdit::singleline(name)
                .hint_text("Template name")
                .desired_width(280.0),
        );
        let validated = project::validate_template_name(name);
        if let Err(error) = &validated
            && !name.trim().is_empty()
        {
            ui.colored_label(ui.visuals().warn_fg_color, error.to_string());
        }
        let exists = validated
            .as_ref()
            .map(|n| project::template_exists(&dir, n))
            .unwrap_or(false);
        if exists {
            ui.colored_label(
                ui.visuals().warn_fg_color,
                "A template with this name already exists.",
            );
        }
        ui.separator();
        ui.horizontal(|ui| {
            let label = if exists { "Overwrite" } else { "Save" };
            if ui
                .add_enabled(validated.is_ok(), egui::Button::new(label))
                .clicked()
            {
                save = true;
            }
            if ui.button("Cancel").clicked() {
                cancel = true;
            }
        });
    });
    if save {
        commit_template_save(app, &dir, name, dataset);
        return false;
    }
    !(cancel || modal.should_close())
}

fn commit_template_save(app: &mut PlotxApp, dir: &std::path::Path, name: &str, dataset: usize) {
    let operation_id = app.session.begin_operation();
    match project::save_template(dir, name, &app.doc.datasets[dataset]) {
        Ok(path) => app.session.record_operation(
            OperationReport::success(
                operation_id,
                OperationKind::ProcessingSchemeSave,
                format!("Saved processing template \"{}\".", name.trim()),
                (),
            )
            .with_diagnostic(
                Diagnostic::new(
                    Severity::Info,
                    DiagnosticCode::ProcessingSchemeSaveSucceeded,
                    "Processing template saved.",
                )
                .with_source("app.processing_template")
                .with_context("path", path.display().to_string())
                .with_context("template", name.trim())
                .with_context("dataset_index", dataset.to_string()),
            ),
        ),
        Err(error) => app.session.record_operation(OperationReport::<()>::failure(
            operation_id,
            OperationKind::ProcessingSchemeSave,
            format!("Template save failed: {error}"),
            Diagnostic::new(
                Severity::Error,
                DiagnosticCode::ProcessingSchemeSaveFailed,
                "Processing template could not be saved.",
            )
            .with_source("app.processing_template")
            .with_context("template", name.trim())
            .with_context("dataset_index", dataset.to_string())
            .with_context("error", error.to_string()),
        )),
    };
}

enum RowAction {
    Apply(usize),
    ApplyTo(usize),
    Delete(usize),
}

fn browse_window(
    app: &mut PlotxApp,
    ctx: &egui::Context,
    dataset: usize,
    entries: &mut Vec<TemplateBrowserEntry>,
    confirm_delete: &mut Option<usize>,
) -> bool {
    if !can_use_templates(app, dataset) {
        return false;
    }
    let pending = app.has_pending_processing();
    let mut action: Option<RowAction> = None;
    let mut close = false;
    let modal = super::modal(ctx, "processing_template_browse_modal", super::ModalKind::Dialog)
        .show(ctx, |ui| {
            ui.set_width(560.0);
            ui.heading("Apply processing template");
            ui.separator();
            ui.label(format!(
                "Target: #{}  {}",
                dataset + 1,
                app.doc.datasets[dataset].summary()
            ));
            if pending {
                ui.colored_label(
                    ui.visuals().warn_fg_color,
                    "Resolve the paused processing edit in the panel before applying a template.",
                );
            }
            ui.separator();
            if entries.is_empty() {
                ui.weak("No templates saved yet. Use \"Save as template…\" in the Processing panel.");
            }
            egui::ScrollArea::vertical().max_height(300.0).show(ui, |ui| {
                for (i, entry) in entries.iter().enumerate() {
                    let compat = match &entry.scheme {
                        Ok(scheme) => project::apply_scheme(scheme, &app.doc.datasets[dataset])
                            .map(|_| ())
                            .map_err(|e| e.to_string()),
                        Err(error) => Err(error.clone()),
                    };
                    ui.horizontal_wrapped(|ui| {
                        ui.strong(&entry.name);
                        match &compat {
                            Ok(()) => {
                                ui.colored_label(
                                    egui::Color32::from_rgb(55, 150, 95),
                                    "Compatible",
                                );
                            }
                            Err(reason) => {
                                ui.colored_label(ui.visuals().warn_fg_color, "Incompatible");
                                ui.weak(reason);
                            }
                        }
                        ui.with_layout(
                            egui::Layout::right_to_left(egui::Align::Center),
                            |ui| {
                                let delete_label = if *confirm_delete == Some(i) {
                                    "Confirm delete"
                                } else {
                                    "Delete"
                                };
                                if ui.button(delete_label).clicked() {
                                    if *confirm_delete == Some(i) {
                                        action = Some(RowAction::Delete(i));
                                    } else {
                                        *confirm_delete = Some(i);
                                    }
                                }
                                if ui
                                    .add_enabled(
                                        entry.scheme.is_ok() && !pending,
                                        egui::Button::new("Apply to…"),
                                    )
                                    .on_hover_text(
                                        "Review and apply to the selected datasets, or to every dataset",
                                    )
                                    .clicked()
                                {
                                    action = Some(RowAction::ApplyTo(i));
                                }
                                if ui
                                    .add_enabled(
                                        compat.is_ok() && !pending,
                                        egui::Button::new("Apply"),
                                    )
                                    .clicked()
                                {
                                    action = Some(RowAction::Apply(i));
                                }
                            },
                        );
                    });
                    ui.separator();
                }
            });
            ui.horizontal(|ui| {
                if ui.button("Close").clicked() {
                    close = true;
                }
            });
        });

    match action {
        Some(RowAction::Apply(i)) => {
            if let Ok(scheme) = &entries[i].scheme {
                let plan = project::plan_scheme_application(scheme, &app.doc.datasets, &[dataset]);
                if let Some(prepared) = plan.prepare(SchemeApplicationPolicy::StrictAll) {
                    super::file_dialogs::commit_scheme_application(
                        app,
                        &entries[i].path,
                        &plan,
                        prepared,
                    );
                }
            }
            false
        }
        Some(RowAction::ApplyTo(i)) => {
            if let Ok(scheme) = &entries[i].scheme {
                let mut targets = app.session.ui.data_selection.clone();
                if targets.len() < 2 {
                    targets = (0..app.doc.datasets.len()).collect();
                }
                let plan = project::plan_scheme_application(scheme, &app.doc.datasets, &targets);
                app.session.ui.processing_scheme_dialog =
                    Some(ProcessingSchemeDialogState::Review {
                        path: entries[i].path.clone(),
                        plan,
                        policy: SchemeApplicationPolicy::CompatibleOnly,
                    });
            }
            false
        }
        Some(RowAction::Delete(i)) => {
            let name = entries[i].name.clone();
            if let Some(dir) = project::templates_dir() {
                app.session.status = match project::delete_template(&dir, &name) {
                    Ok(()) => format!("Deleted template \"{name}\"."),
                    Err(error) => format!("Could not delete template \"{name}\": {error}"),
                };
            }
            *entries = browse_entries();
            *confirm_delete = None;
            true
        }
        None => !(close || modal.should_close()),
    }
}
