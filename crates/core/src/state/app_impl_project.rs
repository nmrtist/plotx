use super::*;
use crate::operation::{
    Diagnostic, DiagnosticCode, OperationId, OperationKind, OperationReport, Severity,
};

impl PlotxApp {
    /// Synchronous save retained for non-GUI callers and tests. The desktop
    /// shell uses the captured request on a background worker instead.
    pub fn save_project_to(
        &mut self,
        path: &std::path::Path,
        include_view_snapshots: bool,
    ) -> bool {
        let operation_id = self.session.begin_operation();
        let captured_generation = self.doc.edit_generation;
        let request = crate::project::prepare_project_save(self, path, include_view_snapshots);
        let result = crate::project::save_project_snapshot(request);
        self.complete_project_save(
            operation_id,
            path,
            include_view_snapshots,
            captured_generation,
            result,
        )
    }

    /// Project a worker result back into the live document. A successful save
    /// only clears `dirty` when no edit landed after the worker captured state.
    pub fn complete_project_save(
        &mut self,
        operation_id: OperationId,
        path: &std::path::Path,
        include_view_snapshots: bool,
        captured_generation: u64,
        result: Result<crate::project::SaveOutcome, crate::project::ProjectError>,
    ) -> bool {
        match result {
            Ok(outcome) => {
                self.doc.project_path = Some(path.to_owned());
                self.session.project_present = true;
                self.doc.save_include_view_snapshots = include_view_snapshots;
                self.settings.export.include_view_snapshots = include_view_snapshots;
                if self.doc.edit_generation == captured_generation {
                    self.doc.dirty = false;
                }
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
                self.persist_settings();
                self.note_recent_file(path);
                true
            }
            Err(error) => {
                self.session
                    .record_operation(OperationReport::<()>::failure(
                        operation_id,
                        OperationKind::ProjectSave,
                        format!("Save failed: {error}"),
                        Diagnostic::new(
                            Severity::Error,
                            DiagnosticCode::ProjectSaveFailed,
                            "Project could not be saved.",
                        )
                        .with_source("core.project")
                        .with_context("path", path.display().to_string())
                        .with_context("error", error.to_string()),
                    ));
                false
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn save_completion_does_not_clear_a_newer_edit() {
        let mut app = PlotxApp::new_with_settings(crate::settings::Settings::default());
        app.mark_document_dirty();
        let captured = app.doc.edit_generation;
        app.mark_document_dirty();
        let path = std::env::temp_dir().join(format!(
            "plotx-generation-save-{}.plotx",
            uuid::Uuid::new_v4()
        ));
        let request = crate::project::prepare_project_save(&app, &path, false);
        let outcome = crate::project::save_project_snapshot(request);
        let operation = app.session.begin_operation();

        assert!(app.complete_project_save(operation, &path, false, captured, outcome));
        assert!(app.doc.dirty);
        let _ = std::fs::remove_file(path);
    }
}
