use super::*;
use crate::operation::{
    Diagnostic, DiagnosticCode, OperationId, OperationKind, OperationReport, Severity,
};

impl PlotxApp {
    pub fn load_project_from(&mut self, path: &std::path::Path) -> bool {
        let operation_id = self.session.begin_operation();
        match crate::project::load_project(path) {
            Ok(mut loaded) => {
                loaded.doc.project_path = Some(path.to_owned());
                loaded.doc.dirty = false;
                loaded.session.project_present = true;
                loaded.clear_history();
                self.install_loaded_project(loaded);
                // A dataset whose stored analysis result could not be restored is
                // still a successful load, but the canvas then shows something
                // other than what was saved. Reporting it only on the dataset
                // would hide it behind a tool panel the user has no reason to
                // open, so it is promoted to the load report and the status bar.
                let restore_warnings = self
                    .doc
                    .datasets
                    .iter()
                    .filter_map(|dataset| {
                        let dataset = dataset.as_nmr2d()?;
                        // Same precedence the panel uses, including the derived
                        // "selected map is missing" note: that case is not stored
                        // on the dataset, but it is exactly the case where the
                        // canvas silently shows something other than what was
                        // saved, so it has to reach the load report too.
                        let warning = dataset
                            .dosy_provenance_warning
                            .clone()
                            .or_else(|| dataset.missing_selected_map_note().map(str::to_owned))?;
                        Some((dataset.name.clone(), warning))
                    })
                    .collect::<Vec<_>>();
                let asset_warnings = std::mem::take(&mut self.session.project_load_warnings);
                let mut report = OperationReport::success(
                    operation_id,
                    OperationKind::ProjectLoad,
                    format!("Opened project {}", path.display()),
                    (),
                )
                .with_diagnostic(
                    Diagnostic::new(
                        Severity::Info,
                        DiagnosticCode::ProjectLoadSucceeded,
                        "Project opened successfully.",
                    )
                    .with_source("core.project")
                    .with_context("path", path.display().to_string()),
                );
                for (name, warning) in &restore_warnings {
                    report = report.with_diagnostic(
                        Diagnostic::new(
                            Severity::Warning,
                            DiagnosticCode::ProjectLoadWarning,
                            warning.clone(),
                        )
                        .with_source("core.project")
                        .with_context("path", path.display().to_string())
                        .with_context("dataset", name.clone().unwrap_or_default()),
                    );
                }
                for warning in &asset_warnings {
                    report = report.with_diagnostic(
                        Diagnostic::new(
                            Severity::Warning,
                            DiagnosticCode::ProjectLoadWarning,
                            warning.clone(),
                        )
                        .with_source("core.project.assets")
                        .with_context("path", path.display().to_string()),
                    );
                }
                if let Some(first) = asset_warnings
                    .first()
                    .or_else(|| restore_warnings.first().map(|(_, warning)| warning))
                {
                    self.session.status = first.clone();
                }
                self.session.record_operation(report);
                self.note_recent_file(path);
                true
            }
            Err(e) => {
                self.session
                    .record_operation(OperationReport::<()>::failure(
                        operation_id,
                        OperationKind::ProjectLoad,
                        format!("Failed to open project {}: {e}", path.display()),
                        Diagnostic::new(
                            Severity::Error,
                            DiagnosticCode::ProjectLoadFailed,
                            "Project could not be opened.",
                        )
                        .with_source("core.project")
                        .with_context("path", path.display().to_string())
                        .with_context("error", e.to_string()),
                    ));
                false
            }
        }
    }

    /// Replace this app with a freshly loaded project, carrying the session
    /// state that outlives a document swap: the operation history (including
    /// its ID and completion-order counters) and the feedback acknowledgement
    /// watermark that refers to it — or every pre-load report would resurface
    /// in the banner after each project open.
    pub(crate) fn install_loaded_project(&mut self, mut loaded: PlotxApp) {
        let settings = self.settings.clone();
        let save_include_view_snapshots = loaded.doc.save_include_view_snapshots;
        loaded.session.operation_history = std::mem::take(&mut self.session.operation_history);
        loaded.session.ui.dismissed_feedback_order = self.session.ui.dismissed_feedback_order;
        *self = loaded;
        // Project loading constructs a fresh app, but app preferences outlive
        // document swaps. In particular, a value whose disk flush failed must
        // not revert merely because a project was opened.
        self.apply_settings(settings);
        // A project records the save profile that produced it. Preserve that
        // existing load behavior; the next explicit Preferences or save choice
        // can replace it and update the live default.
        self.doc.save_include_view_snapshots = save_include_view_snapshots;
    }

    pub fn request_save_project(&mut self) {
        self.session.ui.save_project_options = true;
    }

    /// Open the Preferences panel, seeding its draft from the live settings.
    /// A no-op when it is already open, so re-triggering focuses the live window.
    pub fn open_settings(&mut self) {
        if self.session.ui.settings_dialog.is_none() {
            self.session.ui.settings_dialog = Some(SettingsDialog::new(self.settings.clone()));
        }
    }

    /// Reconcile the egui-free live state to a settings snapshot. Idempotent, so
    /// the instant-apply path may call it on every edit. The chrome theme is an
    /// egui concern and is applied separately by the app shell.
    pub fn apply_settings(&mut self, settings: crate::settings::Settings) {
        if !settings.general.snap_enabled {
            self.session.ui.snap_guides.clear();
        }
        self.doc.save_include_view_snapshots = settings.export.include_view_snapshots;
        let mut recent = settings.recent.files.clone();
        recent.truncate(crate::settings::MAX_RECENT_FILES);
        self.session.recent_files = recent;
        self.session.updates.configure(&settings.updates);
        // Mirror the current monitor's scale record so command gates and the
        // status line agree with an edit made in the Preferences dialog. The
        // egui zoom itself is applied by the app shell, like the chrome theme.
        if let Some(monitor) = self.session.monitor.as_mut()
            && let Some(scale) = settings.appearance.ui_scale.monitors.get(&monitor.key)
        {
            monitor.auto = scale.auto;
            monitor.user = scale.user;
        }
        self.settings = settings;
    }

    /// Flush the authoritative live settings and keep a Preferences draft in
    /// lockstep so its next debounce cannot overwrite an edit made elsewhere.
    pub fn persist_settings(&mut self) -> bool {
        self.persist_settings_with(crate::settings::save)
    }

    pub(crate) fn persist_settings_with(
        &mut self,
        writer: impl FnOnce(&crate::settings::Settings) -> std::io::Result<()>,
    ) -> bool {
        if let Some(dialog) = self.session.ui.settings_dialog.as_mut() {
            dialog.draft = self.settings.clone();
        }
        match writer(&self.settings) {
            Ok(()) => true,
            Err(error) => {
                self.session.status = format!(
                    "Couldn't save preferences — changes apply this session only ({error})"
                );
                false
            }
        }
    }

    /// Record a successfully opened or saved path at the front of the recent
    /// list. Project open/save call this from their success paths; data opens
    /// note at the gesture layer (file dialogs, drops), which alone knows
    /// whether the user picked one file or a whole folder batch.
    pub fn note_recent_file(&mut self, path: &std::path::Path) {
        let path = std::path::absolute(path).unwrap_or_else(|_| path.to_owned());
        let mut recent = crate::settings::RecentFiles {
            files: std::mem::take(&mut self.session.recent_files),
        };
        recent.note(path);
        self.session.recent_files = recent.files.clone();
        self.sync_recent_files_to_settings(recent.files);
    }

    pub fn clear_recent_files(&mut self) {
        self.session.recent_files.clear();
        self.session.status = "Cleared the recent files list.".to_owned();
        self.sync_recent_files_to_settings(Vec::new());
    }

    /// Persist the list and mirror it into an open Preferences draft, so a
    /// later draft flush cannot resurrect entries with a stale copy.
    fn sync_recent_files_to_settings(&mut self, files: Vec<std::path::PathBuf>) {
        self.settings.recent.files = files;
        self.persist_settings();
    }

    pub fn load_from(&mut self, path: &std::path::Path) {
        if plotx_io::archive::is_zip(path) {
            self.load_archive_from(path);
            return;
        }
        self.install_import_result(path, plotx_io::load_path(path));
    }

    pub fn load_nmr_with_sampling(
        &mut self,
        path: &std::path::Path,
        declaration: plotx_io::nmr_sampling::SamplingDeclaration,
    ) -> bool {
        self.install_import_result(path, plotx_io::nmr_sampling::load(path, declaration))
    }

    fn install_import_result(
        &mut self,
        path: &std::path::Path,
        result: Result<plotx_io::LoadResult, plotx_io::IoError>,
    ) -> bool {
        let prepared = result
            .map_err(|error| error.to_string())
            .and_then(|loaded| {
                super::data_import::PreparedImport::new(
                    loaded,
                    self.settings.general.equal_scale_homonuclear_2d_imports,
                )
            });
        self.install_prepared_import(path, prepared)
    }

    pub(super) fn install_prepared_import(
        &mut self,
        path: &std::path::Path,
        result: Result<super::data_import::PreparedImport, String>,
    ) -> bool {
        let operation_id = self.session.begin_operation();
        match result {
            Ok(prepared) => {
                let super::data_import::PreparedImport {
                    dataset,
                    source,
                    format,
                    warnings,
                } = prepared;
                let reconstruction_warning = dataset
                    .as_nmr2d()
                    .and_then(|data| data.reconstruction_warning.clone());
                let before = self.doc.datasets.len();
                if let Err(error) = self.insert_prepared_dataset(dataset, &source) {
                    return self.install_prepared_import(path, Err(error));
                }
                if self.doc.datasets.len() == before {
                    return self.install_prepared_import(path, Err(self.session.status.clone()));
                }
                let mut report = if let Some(warning) = reconstruction_warning {
                    OperationReport::warning(
                        operation_id,
                        OperationKind::DatasetLoad,
                        format!("Loaded {source}. {warning}"),
                        (),
                    )
                    .with_diagnostic(
                        Diagnostic::new(
                            Severity::Warning,
                            DiagnosticCode::DatasetLoadWarning,
                            warning,
                        )
                        .with_source("core.nmr_reconstruction"),
                    )
                } else if warnings.is_empty() {
                    OperationReport::success(
                        operation_id,
                        OperationKind::DatasetLoad,
                        format!("Loaded {source}"),
                        (),
                    )
                } else {
                    OperationReport::warning(
                        operation_id,
                        OperationKind::DatasetLoad,
                        format!("Loaded {source} with {} warning(s)", warnings.len()),
                        (),
                    )
                };
                report = report.with_diagnostic(
                    Diagnostic::new(
                        Severity::Info,
                        DiagnosticCode::DatasetLoadSucceeded,
                        "Dataset loaded",
                    )
                    .with_context("format", format.as_str())
                    .with_context("path", path.display().to_string())
                    .with_source("core.loading"),
                );
                for warning in warnings {
                    report = report.with_diagnostic(load_warning_diagnostic(warning));
                }
                self.session.status = report.summary.clone();
                self.session.record_operation(report);
                true
            }
            Err(e) => {
                self.session.status = format!("Failed to load {}: {e}", path.display());
                self.session
                    .record_operation(OperationReport::<()>::failure(
                        operation_id,
                        OperationKind::DatasetLoad,
                        self.session.status.clone(),
                        Diagnostic::new(
                            Severity::Error,
                            DiagnosticCode::DatasetLoadFailed,
                            e.to_string(),
                        )
                        .with_context("path", path.display().to_string())
                        .with_source("core.loading"),
                    ));
                false
            }
        }
    }

    /// Open a `.zip` archive as a batch: extract it and load every supported
    /// loose file and atomic acquisition folder inside, each as its own dataset and
    /// canvas.
    pub fn load_archive_from(&mut self, path: &std::path::Path) {
        let archive = Self::short_name(&path.to_string_lossy());
        match plotx_io::archive::load_zip(path) {
            Ok(result) => {
                let operation_id = self.session.begin_operation();
                if result.items.is_empty() {
                    self.session.status = format!("No spectra found in {archive}");
                    let mut report = OperationReport::warning(
                        operation_id,
                        OperationKind::DatasetLoad,
                        self.session.status.clone(),
                        (),
                    );
                    for warning in result.warnings {
                        report = report.with_diagnostic(load_warning_diagnostic(warning));
                    }
                    self.session.record_operation(report);
                    return;
                }
                let mut count = 0;
                let mut warnings = result.warnings;
                for item in result.items {
                    let (acquisition, acquisition_identity, _, _, item_warnings) =
                        item.into_parts();
                    warnings.extend(item_warnings);
                    match self.insert_acquisition(acquisition, acquisition_identity) {
                        Ok((_, warning)) => {
                            count += 1;
                            if let Some(message) = warning {
                                warnings.push(plotx_io::LoadWarning {
                                    code: plotx_io::LoadWarningCode::UnsupportedFunction,
                                    message,
                                    path: None,
                                });
                            }
                        }
                        Err(error) => warnings.push(plotx_io::LoadWarning {
                            code: plotx_io::LoadWarningCode::InvalidMetadata,
                            message: format!("Archive dataset could not be opened: {error}"),
                            path: Some(path.to_owned()),
                        }),
                    }
                }
                let summary = if warnings.is_empty() {
                    format!("Loaded {count} spectra from {archive}")
                } else {
                    format!(
                        "Loaded {count} spectra from {archive} with {} warning(s)",
                        warnings.len()
                    )
                };
                let mut report = if warnings.is_empty() {
                    OperationReport::success(
                        operation_id,
                        OperationKind::DatasetLoad,
                        summary.clone(),
                        (),
                    )
                } else {
                    OperationReport::warning(
                        operation_id,
                        OperationKind::DatasetLoad,
                        summary.clone(),
                        (),
                    )
                };
                report = report.with_diagnostic(
                    Diagnostic::new(
                        Severity::Info,
                        DiagnosticCode::DatasetLoadSucceeded,
                        format!("Loaded {count} archive dataset(s)"),
                    )
                    .with_context("path", path.display().to_string())
                    .with_source("core.loading"),
                );
                for warning in warnings {
                    report = report.with_diagnostic(load_warning_diagnostic(warning));
                }
                self.session.status = summary;
                self.session.record_operation(report);
            }
            Err(e) => {
                let operation_id = self.session.begin_operation();
                self.session.status = format!("Failed to open archive {}: {e}", path.display());
                self.session
                    .record_operation(OperationReport::<()>::failure(
                        operation_id,
                        OperationKind::DatasetLoad,
                        self.session.status.clone(),
                        Diagnostic::new(
                            Severity::Error,
                            DiagnosticCode::DatasetLoadFailed,
                            e.to_string(),
                        )
                        .with_context("path", path.display().to_string())
                        .with_source("core.loading"),
                    ));
            }
        }
    }

    // Turn a loaded acquisition into a dataset on its own default canvas, as one
    // undoable step, and return its source label.
    fn insert_acquisition(
        &mut self,
        acq: plotx_io::Acquisition,
        acquisition_identity: plotx_io::AcquisitionIdentity,
    ) -> Result<(String, Option<String>), crate::workflow::WorkflowError> {
        let (dataset, source) = crate::workflow::dataset_from_loaded_acquisition(
            acq,
            acquisition_identity,
            self.settings.general.equal_scale_homonuclear_2d_imports,
        )?;
        let warning = dataset
            .as_nmr2d()
            .and_then(|data| data.reconstruction_warning.clone());
        self.insert_prepared_dataset(dataset, &source)
            .map_err(crate::workflow::WorkflowError::FieldRuntime)?;
        Ok((source, warning))
    }

    fn insert_prepared_dataset(&mut self, dataset: Dataset, source: &str) -> Result<(), String> {
        let name = Self::short_name(source);
        self.try_execute_action(Action::insert_dataset_with_default_canvas(
            self,
            dataset,
            format!("Canvas {} — {}", self.doc.canvases.len() + 1, name),
            DEFAULT_CANVAS_SIZE_MM,
        ))
        .map_err(|error| error.to_string())
    }

    pub fn request_export(&mut self, format: ExportFormat) {
        let Some(ci) = self.session.active_canvas else {
            self.record_export_unavailable(format);
            return;
        };
        if ci >= self.doc.canvases.len() {
            self.record_export_unavailable(format);
            return;
        }
        let mut state = ExportDialogState::from_defaults(format, &self.settings.export);
        let canvas = &self.doc.canvases[ci];
        if let Some(preset) = crate::export::ExportPreset::matching_canvas(
            format,
            canvas.size_mm,
            canvas.size_preset_id.as_deref(),
        ) {
            state.apply_preset(Some(preset));
        }
        self.session.ui.export_options = Some(state);
    }

    pub fn export_to(&mut self, settings: ExportSettings, path: &std::path::Path) {
        let operation_id = self.session.begin_operation();
        if self.doc.canvases.is_empty() {
            self.session
                .record_operation(export_unavailable_report(operation_id, settings.format));
            return;
        }
        match crate::export::export_canvases_with_assets(
            &self.doc.canvases,
            &self.doc.assets,
            self.session.active_canvas,
            &settings,
            path,
        ) {
            Ok(paths) if paths.is_empty() => {
                self.session.record_operation(
                    OperationReport::warning(
                        operation_id,
                        OperationKind::Export,
                        "Export produced no files.",
                        (),
                    )
                    .with_diagnostic(
                        Diagnostic::new(
                            Severity::Warning,
                            DiagnosticCode::ExportProducedNoFiles,
                            "Figure export completed without producing any files.",
                        )
                        .with_source("core.export")
                        .with_context("format", settings.format.label())
                        .with_context("path", path.display().to_string())
                        .with_context("output_count", "0"),
                    ),
                );
            }
            Ok(paths) => {
                let summary = export_status(settings.format, &paths);
                let mut diagnostic = Diagnostic::new(
                    Severity::Info,
                    DiagnosticCode::ExportSucceeded,
                    "Figure export completed successfully.",
                )
                .with_source("core.export")
                .with_context("format", settings.format.label())
                .with_context("path", path.display().to_string())
                .with_context("output_count", paths.len().to_string());
                for (index, output) in paths.iter().enumerate() {
                    diagnostic = diagnostic.with_context(
                        format!("output_path_{}", index + 1),
                        output.display().to_string(),
                    );
                }
                self.session.record_operation(
                    OperationReport::success(operation_id, OperationKind::Export, summary, ())
                        .with_diagnostic(diagnostic),
                );
            }
            Err(error) => {
                let code = export_error_code(&error);
                self.session
                    .record_operation(OperationReport::<()>::failure(
                        operation_id,
                        OperationKind::Export,
                        format!("Export failed: {error}"),
                        Diagnostic::new(Severity::Error, code, "Figure export failed.")
                            .with_source("core.export")
                            .with_context("format", settings.format.label())
                            .with_context("path", path.display().to_string())
                            .with_context("error_kind", export_error_kind(&error))
                            .with_context("error", error.to_string()),
                    ));
            }
        }
    }

    fn record_export_unavailable(&mut self, format: ExportFormat) {
        let operation_id = self.session.begin_operation();
        self.session
            .record_operation(export_unavailable_report(operation_id, format));
    }
}

fn load_warning_diagnostic(warning: plotx_io::LoadWarning) -> Diagnostic {
    let mut diagnostic = Diagnostic::new(
        Severity::Warning,
        DiagnosticCode::DatasetLoadWarning,
        warning.message,
    )
    .with_source("core.loading");
    if let Some(path) = warning.path {
        diagnostic = diagnostic.with_context("path", path.display().to_string());
    }
    diagnostic
}

fn export_unavailable_report(
    operation_id: OperationId,
    format: ExportFormat,
) -> OperationReport<()> {
    OperationReport::failure(
        operation_id,
        OperationKind::Export,
        "Nothing to export — open a spectrum first.",
        Diagnostic::new(
            Severity::Error,
            DiagnosticCode::ExportUnavailable,
            "No figure is available to export.",
        )
        .with_source("core.export")
        .with_context("format", format.label()),
    )
}

fn export_error_code(error: &crate::export::ExportError) -> DiagnosticCode {
    match error {
        crate::export::ExportError::EmptyDocument
        | crate::export::ExportError::MissingCurrentPage => DiagnosticCode::ExportUnavailable,
        crate::export::ExportError::MissingImageAsset { .. }
        | crate::export::ExportError::CorruptImageAsset { .. }
        | crate::export::ExportError::InvalidRange { .. }
        | crate::export::ExportError::SvgParse(_)
        | crate::export::ExportError::Pdf(_)
        | crate::export::ExportError::Image(_)
        | crate::export::ExportError::Io(_)
        | crate::export::ExportError::Raster(_) => DiagnosticCode::ExportFailed,
    }
}

fn export_error_kind(error: &crate::export::ExportError) -> &'static str {
    match error {
        crate::export::ExportError::EmptyDocument => "empty_document",
        crate::export::ExportError::MissingCurrentPage => "missing_current_page",
        crate::export::ExportError::MissingImageAsset { .. } => "missing_image_asset",
        crate::export::ExportError::CorruptImageAsset { .. } => "corrupt_image_asset",
        crate::export::ExportError::InvalidRange { .. } => "invalid_page_range",
        crate::export::ExportError::SvgParse(_) => "svg_parse",
        crate::export::ExportError::Pdf(_) => "pdf_conversion",
        crate::export::ExportError::Image(_) => "image_encoding",
        crate::export::ExportError::Io(_) => "io",
        crate::export::ExportError::Raster(_) => "rasterization",
    }
}

fn export_status(format: ExportFormat, paths: &[std::path::PathBuf]) -> String {
    match paths {
        [] => "Export produced no files.".into(),
        [path] => format!("Exported {} \u{2192} {}", format.label(), path.display()),
        paths => format!(
            "Exported {} pages as {} files next to {}",
            paths.len(),
            format.label(),
            paths[0].display()
        ),
    }
}

#[cfg(test)]
#[path = "app_impl_io_tests.rs"]
mod tests;
