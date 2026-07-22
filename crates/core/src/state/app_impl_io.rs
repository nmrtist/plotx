use super::*;
use crate::operation::{
    Diagnostic, DiagnosticCode, OperationId, OperationKind, OperationReport, Severity,
};

impl PlotxApp {
    pub fn load_project_from(&mut self, path: &std::path::Path) {
        let operation_id = self.session.begin_operation();
        match crate::project::load_project(path) {
            Ok(mut loaded) => {
                loaded.doc.project_path = Some(path.to_owned());
                loaded.doc.dirty = false;
                loaded.clear_history();
                self.install_loaded_project(loaded);
                self.session.record_operation(
                    OperationReport::success(
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
                    ),
                );
                self.note_recent_file(path);
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
            }
        }
    }

    /// Replace this app with a freshly loaded project, carrying the session
    /// state that outlives a document swap: the operation history (including
    /// its ID and completion-order counters) and the feedback acknowledgement
    /// watermark that refers to it — or every pre-load report would resurface
    /// in the banner after each project open.
    pub(crate) fn install_loaded_project(&mut self, mut loaded: PlotxApp) {
        loaded.session.operation_history = std::mem::take(&mut self.session.operation_history);
        loaded.session.ui.dismissed_feedback_order = self.session.ui.dismissed_feedback_order;
        // Like the history: the session list is the runtime truth and
        // must survive the swap even when a settings save failed.
        loaded.session.recent_files = std::mem::take(&mut self.session.recent_files);
        *self = loaded;
    }

    pub fn request_save_project(&mut self) {
        self.session.ui.save_project_options = true;
    }

    /// Open the Preferences panel, seeding its draft from the on-disk settings.
    /// A no-op when it is already open, so re-triggering focuses the live window.
    pub fn open_settings(&mut self) {
        if self.session.ui.settings_dialog.is_none() {
            let mut settings = crate::settings::load();
            // The session list is the runtime truth for recents; seeding from
            // disk would let a draft flush resurrect a stale copy whenever a
            // background settings save had failed.
            settings.recent.files = self.session.recent_files.clone();
            self.session.ui.settings_dialog = Some(SettingsDialog::new(settings));
        }
    }

    /// Reconcile the egui-free live state to a settings snapshot. Idempotent, so
    /// the instant-apply path may call it on every edit. The chrome theme is an
    /// egui concern and is applied separately by the app shell.
    pub fn apply_settings(&mut self, settings: &crate::settings::Settings) {
        self.session.ui.snap_enabled = settings.general.snap_enabled;
        self.session.canvas_accent = settings.appearance.canvas_accent;
        if !settings.general.snap_enabled {
            self.session.ui.snap_guides.clear();
        }
        self.session.project_backup_generations = settings
            .general
            .project_backup_generations
            .min(crate::settings::MAX_PROJECT_BACKUP_GENERATIONS);
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
        self.sync_recent_files_to_settings(Vec::new());
        self.session.status = "Cleared the recent files list.".to_owned();
    }

    /// Persist the list and mirror it into an open Preferences draft, so a
    /// later draft flush cannot resurrect entries with a stale copy.
    fn sync_recent_files_to_settings(&mut self, files: Vec<std::path::PathBuf>) {
        if let Some(dialog) = self.session.ui.settings_dialog.as_mut() {
            dialog.draft.recent.files = files.clone();
        }
        crate::settings::update(move |settings| settings.recent.files = files);
    }

    /// Save the project and report whether persistence completed. The return
    /// value lets modal close/quit flows remain open on failure and offer a
    /// visible recovery path instead of relying on the status bar.
    pub fn save_project_to(
        &mut self,
        path: &std::path::Path,
        include_view_snapshots: bool,
    ) -> bool {
        let operation_id = self.session.begin_operation();
        match crate::project::save_project(self, path, include_view_snapshots) {
            Ok(outcome) => {
                self.doc.project_path = Some(path.to_owned());
                self.doc.save_include_view_snapshots = include_view_snapshots;
                crate::settings::update(|settings| {
                    settings.export.include_view_snapshots = include_view_snapshots;
                    settings.general.snap_enabled = self.session.ui.snap_enabled;
                    settings.general.project_backup_generations =
                        self.session.project_backup_generations;
                });
                self.doc.dirty = false;
                self.doc.project_revision = Some(outcome.revision.clone());
                let mut report = OperationReport::success(
                    operation_id,
                    OperationKind::ProjectSave,
                    format!("Saved project {}", path.display()),
                    (),
                )
                .with_diagnostic(
                    Diagnostic::new(
                        Severity::Info,
                        DiagnosticCode::ProjectSaveSucceeded,
                        "Project saved successfully.",
                    )
                    .with_source("core.project")
                    .with_context("path", path.display().to_string()),
                );
                if let Some(warning) = outcome.backup_warning {
                    report = report.with_diagnostic(
                        Diagnostic::new(
                            Severity::Warning,
                            DiagnosticCode::ProjectSaveSucceeded,
                            "The project was saved, but its backup could not be hidden.",
                        )
                        .with_source("core.project.backup")
                        .with_context("error", warning),
                    );
                }
                self.session.record_operation(report);
                self.note_recent_file(path);
                true
            }
            Err(e) => {
                self.session
                    .record_operation(OperationReport::<()>::failure(
                        operation_id,
                        OperationKind::ProjectSave,
                        format!("Save failed: {e}"),
                        Diagnostic::new(
                            Severity::Error,
                            DiagnosticCode::ProjectSaveFailed,
                            "Project could not be saved.",
                        )
                        .with_source("core.project")
                        .with_context("path", path.display().to_string())
                        .with_context("error", e.to_string()),
                    ));
                false
            }
        }
    }

    pub fn load_from(&mut self, path: &std::path::Path) {
        if plotx_io::archive::is_zip(path) {
            self.load_archive_from(path);
            return;
        }
        let operation_id = self.session.begin_operation();
        match plotx_io::load_path(path) {
            Ok(result) => {
                let format = result.format.as_str();
                let warnings = result.warnings;
                let source = self.insert_acquisition(result.acquisition);
                let mut report = if warnings.is_empty() {
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
                    .with_context("format", format)
                    .with_context("path", path.display().to_string())
                    .with_source("core.loading"),
                );
                for warning in warnings {
                    report = report.with_diagnostic(load_warning_diagnostic(warning));
                }
                self.session.status = report.summary.clone();
                self.session.record_operation(report);
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
            }
        }
    }

    /// Open a `.zip` archive as a batch: extract it and load every JEOL `.jdf`
    /// file and Bruker acquisition folder inside, each as its own dataset and
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
                let count = result.items.len();
                let mut warnings = result.warnings;
                for item in result.items {
                    warnings.extend(item.warnings);
                    self.insert_acquisition(item.acquisition);
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
    fn insert_acquisition(&mut self, acq: plotx_io::Acquisition) -> String {
        let (dataset, source) = crate::workflow::dataset_from_acquisition(acq);
        let name = Self::short_name(&source);
        self.execute_action(Action::insert_dataset_with_default_canvas(
            self,
            dataset,
            format!("Canvas {} — {}", self.doc.canvases.len() + 1, name),
            DEFAULT_CANVAS_SIZE_MM,
        ));
        source
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
        let defaults = crate::settings::load().export;
        let mut state = ExportDialogState::from_defaults(format, &defaults);
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
        match crate::export::export_canvases(
            &self.doc.canvases,
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
        crate::export::ExportError::InvalidRange { .. }
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
mod export_operation_tests {
    use super::*;
    use crate::operation::{DiagnosticCode, OperationOutcome};

    #[test]
    fn unavailable_export_is_recorded_and_projects_its_summary() {
        let mut app = PlotxApp::new_with_settings(crate::settings::Settings::default());

        app.request_export(ExportFormat::Svg);

        let operation = app
            .session
            .operation_history
            .operations()
            .next_back()
            .unwrap();
        assert_eq!(operation.kind, OperationKind::Export);
        assert_eq!(operation.outcome, OperationOutcome::Failure);
        assert_eq!(operation.summary, app.session.status);
        assert_eq!(operation.diagnostics.len(), 1);
        assert_eq!(
            operation.diagnostics[0].code,
            DiagnosticCode::ExportUnavailable
        );
    }

    #[test]
    fn typed_export_error_is_mapped_at_the_workflow_boundary() {
        let mut app = PlotxApp::new_with_settings(crate::settings::Settings::default());
        app.doc.canvases.push(CanvasDocument::new(
            "page".to_owned(),
            DEFAULT_CANVAS_SIZE_MM,
        ));
        app.session.active_canvas = Some(0);

        app.export_to(
            ExportSettings {
                format: ExportFormat::Svg,
                scope: crate::export::ExportPageScope::Range { start: 2, end: 1 },
                dpi: crate::export::DEFAULT_BITMAP_DPI,
                target_width_mm: None,
                trim_to_visible_content: false,
            },
            std::path::Path::new("unused.svg"),
        );

        let operation = app
            .session
            .operation_history
            .operations()
            .next_back()
            .unwrap();
        assert_eq!(operation.outcome, OperationOutcome::Failure);
        assert_eq!(operation.summary, app.session.status);
        assert_eq!(operation.diagnostics[0].code, DiagnosticCode::ExportFailed);
        assert_eq!(
            operation.diagnostics[0]
                .context
                .get("error_kind")
                .map(String::as_str),
            Some("invalid_page_range")
        );
    }
}

#[cfg(test)]
mod install_loaded_project_tests {
    use super::*;

    fn record_failure(app: &mut PlotxApp) -> OperationId {
        let id = app.session.begin_operation();
        app.session.record_operation(OperationReport::<()>::failure(
            id,
            OperationKind::DatasetLoad,
            "boom",
            Diagnostic::new(Severity::Error, DiagnosticCode::DatasetLoadFailed, "boom"),
        ));
        id
    }

    /// The invariant the feedback watermark hinges on: a project swap carries
    /// the operation history *including its counters*, so reports recorded
    /// after the load always come after a pre-load acknowledgement.
    #[test]
    fn project_swap_carries_history_counter_and_watermark() {
        let mut app = PlotxApp::new_with_settings(crate::settings::Settings::default());
        let before = record_failure(&mut app);
        let before_order = app
            .session
            .operation_history
            .operations()
            .next_back()
            .expect("failure recorded")
            .completion_order;
        app.session.ui.dismissed_feedback_order = Some(before_order);

        let loaded = PlotxApp::new_with_settings(crate::settings::Settings::default());
        app.install_loaded_project(loaded);

        assert_eq!(app.session.ui.dismissed_feedback_order, Some(before_order));
        let after = record_failure(&mut app);
        let after_order = app
            .session
            .operation_history
            .operations()
            .next_back()
            .expect("failure recorded")
            .completion_order;
        assert!(
            after > before,
            "post-load ids must stay above the watermark"
        );
        assert!(
            after_order > before_order,
            "post-load reports must stay after the acknowledgement"
        );
        assert!(
            app.session
                .operation_history
                .operations()
                .any(|operation| operation.id == before),
            "pre-load history is carried across the swap"
        );
    }
}
