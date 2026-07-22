use plotx_core::export::ExportSettings;
use plotx_core::operation::{Diagnostic, DiagnosticCode, OperationKind, OperationReport, Severity};
use plotx_core::project::{
    PreparedSchemeApplication, SchemeApplicationPlan, SchemeApplicationPolicy, SchemeTargetResult,
};
use plotx_core::state::PlotxApp;
use plotx_core::state::ProcessingSchemeDialogState;

mod delimited;
mod discovery;
mod origin;
mod path;
mod preview;
mod recent;
mod xlsx;
pub(crate) use delimited::DelimitedTableSource;
use path::{ensure_extension, ensure_plotx_extension, io_error_category};
pub(crate) use preview::table_import_preview_window;
#[cfg(test)]
use recent::RecentOpenKind;
pub(crate) use recent::open_recent_path;
use xlsx::import_xlsx_table_path;

pub(crate) fn import_delimited_table(app: &mut PlotxApp) {
    let Some(path) = rfd::FileDialog::new()
        .add_filter(
            "Table (*.csv, *.tsv, *.txt, *.xlsx, *.opj)",
            origin::IMPORT_TABLE_FILTER_EXTENSIONS,
        )
        .add_filter(
            origin::ORIGIN_PROJECT_FILTER_LABEL,
            origin::ORIGIN_PROJECT_FILTER_EXTENSIONS,
        )
        .add_filter("Excel workbook (*.xlsx)", &["xlsx"])
        .add_filter("CSV (*.csv)", &["csv"])
        .add_filter("TSV (*.tsv)", &["tsv"])
        .add_filter("All files", &["*"])
        .set_title("Import a table")
        .pick_file()
    else {
        return;
    };
    open_recent_path(app, &path);
}

fn import_delimited_table_path(app: &mut PlotxApp, path: &std::path::Path) {
    let input = match std::fs::read_to_string(path) {
        Ok(input) => input,
        Err(error) => {
            let operation_id = app.session.begin_operation();
            app.session.record_operation(OperationReport::<()>::failure(
                operation_id,
                OperationKind::TableImport,
                "Table import failed while reading the selected file.",
                Diagnostic::new(
                    Severity::Error,
                    DiagnosticCode::TableImportFailed,
                    "The selected table could not be read.",
                )
                .with_source("app.table_import")
                .with_context("path", path.display().to_string())
                .with_context("stage", "read")
                .with_context("category", io_error_category(&error))
                .with_context("error", error.to_string()),
            ));
            return;
        }
    };
    import_delimited_text(app, &input, DelimitedTableSource::File(path.to_owned()));
}

pub(crate) fn import_delimited_text(app: &mut PlotxApp, input: &str, source: DelimitedTableSource) {
    import_delimited_text_with_schema(app, input, source, None);
}

pub(crate) fn import_delimited_text_with_schema(
    app: &mut PlotxApp,
    input: &str,
    source: DelimitedTableSource,
    clipboard_schema_json: Option<&str>,
) {
    let operation_id = app.session.begin_operation();
    let parsed = match plotx_core::delimited::parse_delimited(
        input,
        plotx_core::delimited::ParseOptions::default(),
    ) {
        Ok(parsed) => parsed,
        Err(error) => {
            app.session.record_operation(OperationReport::<()>::failure(
                operation_id,
                OperationKind::TableImport,
                "Table import failed because the selected file is not a valid delimited table.",
                source.add_diagnostic_context(
                    Diagnostic::new(
                        Severity::Error,
                        DiagnosticCode::TableImportFailed,
                        "Delimited table parsing failed.",
                    )
                    .with_source("app.table_import")
                    .with_context("stage", "parse")
                    .with_context("category", "delimited_parse")
                    .with_context("error", error.to_string()),
                ),
            ));
            return;
        }
    };
    let delimiter = parsed.delimiter;
    let parsed_diagnostics = parsed.diagnostics.clone();
    let typed_store = std::sync::Arc::new(plotx_core::data::MemoryBlockStore::default());
    let codecs = plotx_core::data::CodecRegistry::with_arrow_ipc();
    let sidecar = match (&source, clipboard_schema_json) {
        (DelimitedTableSource::Clipboard, Some(schema)) => {
            match serde_json::from_str::<plotx_core::xlsx::PlotxDelimitedSchemaV1>(schema) {
                Ok(sidecar) => {
                    let mut retained = plotx_core::state::TableImportSource::new(
                        std::sync::Arc::<[u8]>::from(schema.as_bytes()),
                        "application/vnd.plotx.table-schema+json",
                    );
                    retained.name = Some("clipboard.plotx-schema.json".into());
                    Ok(Some((sidecar, retained)))
                }
                Err(error) => Err(format!("invalid PlotX clipboard schema: {error}")),
            }
        }
        (DelimitedTableSource::File(path), _) => xlsx::read_delimited_sidecar(path),
        (DelimitedTableSource::Clipboard, None) => Ok(None),
    };
    let (sidecar, retained_sidecar, mut metadata_diagnostics) = match sidecar {
        Ok(Some((sidecar, source))) => (Some(sidecar), Some(source), Vec::new()),
        Ok(None) => (None, None, Vec::new()),
        Err(error) => (None, None, vec![error]),
    };
    let mut inference_diagnostics = Vec::new();
    let typed_snapshot = match sidecar.as_ref() {
        Some(sidecar) => plotx_core::xlsx::import_delimited_with_schema(
            &parsed,
            sidecar,
            typed_store.as_ref(),
            &codecs,
        )
        .map(|imported| {
            metadata_diagnostics.extend(imported.diagnostics);
            imported.snapshot
        })
        .map_err(|error| error.to_string()),
        None => parsed
            .into_typed_snapshot(
                plotx_core::data::TableId::new(),
                typed_store.as_ref(),
                &codecs,
            )
            .map_err(|error| error.to_string()),
    };
    let typed_snapshot = match typed_snapshot {
        Ok(snapshot) => {
            if sidecar.is_none()
                && let Some(messages) = snapshot
                    .metadata
                    .get("space.nmrtist.plotx.import.inference")
                    .and_then(serde_json::Value::as_array)
            {
                inference_diagnostics.extend(
                    messages
                        .iter()
                        .filter_map(serde_json::Value::as_str)
                        .map(str::to_owned),
                );
            }
            snapshot
        }
        Err(error) => {
            app.session.record_operation(OperationReport::<()>::failure(
                operation_id,
                OperationKind::TableImport,
                "Table import failed while creating the typed snapshot.",
                source.add_diagnostic_context(
                    Diagnostic::new(
                        Severity::Error,
                        DiagnosticCode::TableImportFailed,
                        "Typed table materialization failed.",
                    )
                    .with_source("app.table_import")
                    .with_context("stage", "typed_materialization")
                    .with_context("error", error.to_string()),
                ),
            ));
            return;
        }
    };
    let row_count = typed_snapshot.row_count;
    let column_count = typed_snapshot.schema.columns.len();
    let typed_state =
        match plotx_core::state::TypedTableState::imported(typed_snapshot, typed_store) {
            Ok(state) => state,
            Err(error) => {
                app.session.record_operation(OperationReport::<()>::failure(
                    operation_id,
                    OperationKind::TableImport,
                    "Table import failed while recording its initial revision.",
                    source.add_diagnostic_context(
                        Diagnostic::new(
                            Severity::Error,
                            DiagnosticCode::TableImportFailed,
                            "Typed table revision creation failed.",
                        )
                        .with_source("app.table_import")
                        .with_context("stage", "revision")
                        .with_context("error", error.to_string()),
                    ),
                ));
                return;
            }
        };
    let mut import_diagnostics = parsed_diagnostics;
    import_diagnostics.extend(metadata_diagnostics.into_iter().map(|message| {
        plotx_core::delimited::DelimitedDiagnostic {
            level: plotx_core::delimited::DiagnosticLevel::Warning,
            message,
        }
    }));
    import_diagnostics.extend(inference_diagnostics.into_iter().map(|message| {
        plotx_core::delimited::DelimitedDiagnostic {
            level: plotx_core::delimited::DiagnosticLevel::Info,
            message,
        }
    }));
    let warning_count = import_diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.level == plotx_core::delimited::DiagnosticLevel::Warning)
        .count();
    let name = source.dataset_name();
    let recent_path = source.recent_path();
    let retained_source = source.retained_source(input, delimiter);
    let mut retained_sources = vec![retained_source];
    retained_sources.extend(retained_sidecar);
    let summary = if warning_count == 0 {
        format!("Imported a typed table with {row_count} row(s) and {column_count} column(s).")
    } else {
        format!(
            "Imported a typed table with {row_count} row(s), {column_count} column(s), and {warning_count} warning(s)."
        )
    };
    let mut report = if warning_count == 0 {
        OperationReport::success(operation_id, OperationKind::TableImport, summary, ())
    } else {
        OperationReport::warning(operation_id, OperationKind::TableImport, summary, ())
    }
    .with_diagnostic(
        source.add_diagnostic_context(
            Diagnostic::new(
                Severity::Info,
                DiagnosticCode::TableImportSucceeded,
                "Delimited table import completed.",
            )
            .with_source("app.table_import")
            .with_context("row_count", row_count.to_string())
            .with_context("column_count", column_count.to_string())
            .with_context("delimiter", delimiter.to_string()),
        ),
    );
    for diagnostic in import_diagnostics {
        let severity = match diagnostic.level {
            plotx_core::delimited::DiagnosticLevel::Info => Severity::Info,
            plotx_core::delimited::DiagnosticLevel::Warning => Severity::Warning,
        };
        report = report.with_diagnostic(
            Diagnostic::new(
                severity,
                DiagnosticCode::TableImportWarning,
                diagnostic.message,
            )
            .with_source("core.delimited"),
        );
    }
    app.session.ui.table_import_preview = Some(plotx_core::state::TableImportPreviewState {
        candidates: vec![plotx_core::state::TableImportCandidate {
            name,
            retained_sources,
            typed_state,
            x_binding: None,
            series_bindings: Vec::new(),
        }],
        selected: 0,
        report,
        recent_path,
    });
}

pub(crate) fn commit_table_import_preview(app: &mut PlotxApp) -> bool {
    commit_table_import_preview_with_recent(app, PlotxApp::note_recent_file)
}

pub(crate) fn commit_table_import_preview_with_recent<F>(
    app: &mut PlotxApp,
    mut note_recent_file: F,
) -> bool
where
    F: FnMut(&mut PlotxApp, &std::path::Path),
{
    let Some(preview) = app.session.ui.table_import_preview.take() else {
        return false;
    };
    if preview.candidates.is_empty() {
        app.session.record_operation(OperationReport::<()>::failure(
            preview.report.id,
            OperationKind::TableImport,
            "Table import failed because there are no supported tables to import.",
            Diagnostic::new(
                Severity::Error,
                DiagnosticCode::TableImportFailed,
                "The import preview contains no supported table candidates.",
            )
            .with_source("app.table_import")
            .with_context("stage", "preview_commit"),
        ));
        return false;
    }
    for candidate in preview.candidates {
        app.import_table_dataset_typed(
            candidate.name,
            candidate.retained_sources,
            candidate.typed_state,
            candidate.x_binding,
            candidate.series_bindings,
        );
    }
    if let Some(path) = preview.recent_path {
        note_recent_file(app, &path);
    }
    app.session.record_operation(preview.report);
    true
}

pub(crate) fn load_and_note(app: &mut PlotxApp, path: &std::path::Path) {
    let before = app.doc.datasets.len();
    app.load_from(path);
    if app.doc.datasets.len() > before {
        app.note_recent_file(path);
    }
}

pub(crate) fn open_file(app: &mut PlotxApp) {
    if let Some(paths) = rfd::FileDialog::new()
        .add_filter(
            "All supported data (*.abf, *.jdf, fid, ser, *.zip, *.opj)",
            origin::OPEN_FILE_FILTER_EXTENSIONS,
        )
        .add_filter(
            origin::ORIGIN_PROJECT_FILTER_LABEL,
            origin::ORIGIN_PROJECT_FILTER_EXTENSIONS,
        )
        .add_filter("Axon Binary Format 2 (*.abf)", &["abf"])
        .add_filter("JEOL Delta (*.jdf)", &["jdf"])
        .add_filter("Bruker TopSpin (fid, ser)", &["fid", "ser"])
        .add_filter("Archive (*.zip)", &["zip"])
        .add_filter("All files", &["*"])
        .set_title("Open data files — format is detected automatically")
        .pick_files()
    {
        for path in paths {
            open_recent_path(app, &path);
        }
    }
}

pub(crate) fn open_project(app: &mut PlotxApp) {
    if let Some(path) = rfd::FileDialog::new()
        .add_filter("PlotX project (*.plotx)", &["plotx"])
        .add_filter("All files", &["*"])
        .set_title("Open PlotX project")
        .pick_file()
    {
        app.load_project_from(&path);
    }
}

pub(crate) fn save_project_as(app: &mut PlotxApp, include_view_snapshots: bool) {
    if let Some(path) = rfd::FileDialog::new()
        .add_filter("PlotX project (*.plotx)", &["plotx"])
        .set_file_name("project.plotx")
        .set_title("Save PlotX project")
        .save_file()
    {
        let path = ensure_plotx_extension(path);
        let _ = app.save_project_to(&path, include_view_snapshots);
    }
}

pub(crate) fn open_folder(app: &mut PlotxApp) {
    if let Some(path) = rfd::FileDialog::new()
        .set_title("Open a data folder (Bruker acquisition or recursive ABF2 import)")
        .pick_folder()
    {
        open_folder_path(app, &path);
    }
}

/// The user gesture was "open this folder", so the recent list notes the
/// folder itself — never the individual files of an ABF batch, which would
/// flush every other entry out of the capped list. The folder is noted when
/// any file of the batch loaded, not just the last one.
fn open_folder_path(app: &mut PlotxApp, path: &std::path::Path) {
    let before = app.doc.datasets.len();
    let mut abf_files = Vec::new();
    discovery::collect_abf_files(path, &mut abf_files);
    if abf_files.is_empty() {
        app.load_from(path);
    } else {
        abf_files.sort();
        for file in abf_files {
            app.load_from(&file);
        }
    }
    if app.doc.datasets.len() > before {
        app.note_recent_file(path);
    }
}

pub(crate) fn export_with_options(app: &mut PlotxApp, settings: ExportSettings) {
    let Some(path) = rfd::FileDialog::new()
        .add_filter(settings.format.label(), &[settings.format.extension()])
        .set_file_name(settings.format.default_file_name())
        .set_title(settings.format.dialog_title())
        .save_file()
    else {
        return;
    };
    app.export_to(settings, &path);
}

pub(crate) fn load_processing_scheme(app: &mut PlotxApp, di: usize) {
    if app.has_pending_processing() {
        app.session.ui.processing_scheme_dialog =
            Some(ProcessingSchemeDialogState::ResolvePending {
                fallback_dataset: di,
            });
        return;
    }
    choose_and_plan_processing_scheme(app, di);
}

fn choose_and_plan_processing_scheme(app: &mut PlotxApp, fallback_dataset: usize) {
    let Some(path) = rfd::FileDialog::new()
        .add_filter("Processing scheme (*.plotxproc)", &["plotxproc"])
        .add_filter("All files", &["*"])
        .set_title("Load processing scheme")
        .pick_file()
    else {
        return;
    };
    let scheme = match plotx_core::project::load_scheme(&path) {
        Ok(s) => s,
        Err(e) => {
            let operation_id = app.session.begin_operation();
            app.session.record_operation(OperationReport::<()>::failure(
                operation_id,
                OperationKind::ProcessingSchemeLoadAndApply,
                format!("Could not load scheme: {e}"),
                Diagnostic::new(
                    Severity::Error,
                    DiagnosticCode::ProcessingSchemeLoadFailed,
                    "Processing scheme could not be loaded.",
                )
                .with_source("app.processing_scheme")
                .with_context("path", path.display().to_string())
                .with_context("error", e.to_string()),
            ));
            return;
        }
    };

    let mut targets = app.session.ui.data_selection.clone();
    if targets.is_empty() {
        targets.push(fallback_dataset);
    }
    let plan = plotx_core::project::plan_scheme_application(&scheme, &app.doc.datasets, &targets);
    app.session.ui.processing_scheme_dialog = Some(ProcessingSchemeDialogState::Review {
        path,
        plan,
        policy: SchemeApplicationPolicy::StrictAll,
    });
}

pub(crate) fn processing_scheme_window(app: &mut PlotxApp, ctx: &egui::Context) {
    let Some(mut dialog) = app.session.ui.processing_scheme_dialog.take() else {
        return;
    };
    match &mut dialog {
        ProcessingSchemeDialogState::ResolvePending { fallback_dataset } => {
            let fallback_dataset = *fallback_dataset;
            let mut choice = None;
            let modal = super::modal(
                ctx,
                "processing_scheme_pending_modal",
                super::ModalKind::Dialog,
            )
            .show(ctx, |ui| {
                ui.heading("Pending processing changes");
                ui.separator();
                ui.label("Resolve the paused processing edit before loading a scheme.");
                ui.label("Apply commits it separately; Discard restores the prior recipe.");
                ui.separator();
                ui.horizontal(|ui| {
                    if ui.button("Apply").clicked() {
                        choice = Some(0);
                    }
                    if ui.button("Discard").clicked() {
                        choice = Some(1);
                    }
                    if ui.button("Cancel").clicked() {
                        choice = Some(2);
                    }
                });
            });
            if choice.is_none() && modal.should_close() {
                choice = Some(2);
            }
            match choice {
                Some(0) => {
                    app.apply_paused_processing();
                    choose_and_plan_processing_scheme(app, fallback_dataset);
                }
                Some(1) => {
                    app.discard_paused_processing();
                    choose_and_plan_processing_scheme(app, fallback_dataset);
                }
                Some(2) => {}
                _ => app.session.ui.processing_scheme_dialog = Some(dialog),
            }
        }
        ProcessingSchemeDialogState::Review { path, plan, policy } => {
            let mut apply = false;
            let mut cancel = false;
            let can_apply = plan.prepare(*policy).is_some();
            let modal = super::modal(
                ctx,
                "processing_scheme_review_modal",
                super::ModalKind::Dialog,
            )
            .show(ctx, |ui| {
                ui.set_width(560.0);
                ui.heading("Apply processing scheme");
                ui.separator();
                ui.label(format!("Scheme: {}", path.display()));
                ui.label(format!(
                    "{} compatible, {} incompatible",
                    plan.compatible_count(),
                    plan.incompatible_count()
                ));
                ui.separator();
                ui.radio_value(
                    policy,
                    SchemeApplicationPolicy::StrictAll,
                    "Strict: require every selected dataset",
                );
                ui.radio_value(
                    policy,
                    SchemeApplicationPolicy::CompatibleOnly,
                    "Compatible only: skip incompatible datasets",
                );
                ui.separator();
                egui::ScrollArea::vertical()
                    .max_height(280.0)
                    .show(ui, |ui| {
                        for target in plan.targets() {
                            let label = app
                                .doc
                                .datasets
                                .get(target.dataset)
                                .map(|dataset| {
                                    format!("#{}  {}", target.dataset + 1, dataset.summary())
                                })
                                .unwrap_or_else(|| {
                                    format!("#{}  Missing dataset", target.dataset + 1)
                                });
                            match &target.result {
                                SchemeTargetResult::Compatible { .. } => {
                                    ui.horizontal(|ui| {
                                        ui.colored_label(
                                            egui::Color32::from_rgb(55, 150, 95),
                                            "Compatible",
                                        );
                                        ui.label(label);
                                    });
                                }
                                SchemeTargetResult::Incompatible { reason } => {
                                    ui.horizontal_wrapped(|ui| {
                                        ui.colored_label(
                                            ui.visuals().warn_fg_color,
                                            "Incompatible",
                                        );
                                        ui.label(label);
                                        ui.weak(reason);
                                    });
                                }
                            }
                        }
                    });
                if *policy == SchemeApplicationPolicy::StrictAll && !can_apply {
                    ui.colored_label(
                        ui.visuals().warn_fg_color,
                        "Strict mode makes no changes until every target is compatible.",
                    );
                }
                ui.separator();
                ui.horizontal(|ui| {
                    if ui
                        .add_enabled(can_apply, egui::Button::new("Apply scheme"))
                        .clicked()
                    {
                        apply = true;
                    }
                    if ui.button("Cancel").clicked() {
                        cancel = true;
                    }
                });
            });
            if apply {
                let prepared = plan
                    .prepare(*policy)
                    .expect("enabled apply requires a prepared scheme transaction");
                commit_scheme_application(app, path, plan, prepared);
            } else if !cancel && !modal.should_close() {
                app.session.ui.processing_scheme_dialog = Some(dialog);
            }
        }
    }
}

pub(crate) fn commit_scheme_application(
    app: &mut PlotxApp,
    path: &std::path::Path,
    plan: &SchemeApplicationPlan,
    prepared: PreparedSchemeApplication,
) {
    let PreparedSchemeApplication {
        action,
        applied_targets,
        skipped_targets,
    } = prepared;
    app.execute_action(action);
    let operation_id = app.session.begin_operation();
    let summary = format!(
        "Applied processing scheme to {} dataset(s); skipped {}.",
        applied_targets.len(),
        skipped_targets.len()
    );
    let mut report = if skipped_targets.is_empty() {
        OperationReport::success(
            operation_id,
            OperationKind::ProcessingSchemeLoadAndApply,
            summary,
            (),
        )
    } else {
        OperationReport::warning(
            operation_id,
            OperationKind::ProcessingSchemeLoadAndApply,
            summary,
            (),
        )
    };
    for target in plan.targets() {
        let (severity, code, message) = match &target.result {
            SchemeTargetResult::Compatible { .. } => (
                Severity::Info,
                DiagnosticCode::ProcessingSchemeApplySucceeded,
                "Processing scheme applied to selected dataset.",
            ),
            SchemeTargetResult::Incompatible { .. } => (
                Severity::Warning,
                DiagnosticCode::ProcessingSchemeApplyFailed,
                "Incompatible selected dataset was skipped.",
            ),
        };
        let mut diagnostic = Diagnostic::new(severity, code, message)
            .with_source("app.processing_scheme")
            .with_context("path", path.display().to_string())
            .with_context("dataset_index", target.dataset.to_string());
        if let Some(reason) = target.result.incompatibility_reason() {
            diagnostic = diagnostic.with_context("reason", reason);
        }
        report = report.with_diagnostic(diagnostic);
    }
    app.session.record_operation(report);
}

pub(crate) fn save_processing_scheme(app: &mut PlotxApp, di: usize) {
    let Some(path) = rfd::FileDialog::new()
        .add_filter("Processing scheme (*.plotxproc)", &["plotxproc"])
        .set_file_name("scheme.plotxproc")
        .set_title("Save processing scheme")
        .save_file()
    else {
        return;
    };
    let path = ensure_extension(path, "plotxproc");
    let operation_id = app.session.begin_operation();
    match plotx_core::project::save_scheme(&path, &app.doc.datasets[di]) {
        Ok(()) => {
            app.session.record_operation(
                OperationReport::success(
                    operation_id,
                    OperationKind::ProcessingSchemeSave,
                    format!("Saved scheme {}", path.display()),
                    (),
                )
                .with_diagnostic(
                    Diagnostic::new(
                        Severity::Info,
                        DiagnosticCode::ProcessingSchemeSaveSucceeded,
                        "Processing scheme saved successfully.",
                    )
                    .with_source("app.processing_scheme")
                    .with_context("path", path.display().to_string())
                    .with_context("dataset_index", di.to_string()),
                ),
            );
        }
        Err(e) => {
            app.session.record_operation(OperationReport::<()>::failure(
                operation_id,
                OperationKind::ProcessingSchemeSave,
                format!("Save failed: {e}"),
                Diagnostic::new(
                    Severity::Error,
                    DiagnosticCode::ProcessingSchemeSaveFailed,
                    "Processing scheme could not be saved.",
                )
                .with_source("app.processing_scheme")
                .with_context("path", path.display().to_string())
                .with_context("dataset_index", di.to_string())
                .with_context("error", e.to_string()),
            ));
        }
    }
}

#[cfg(test)]
#[path = "file_dialogs/tests.rs"]
mod tests;
