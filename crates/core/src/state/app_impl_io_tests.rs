use super::*;

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
                allow_missing_images: false,
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

    #[test]
    fn image_pages_open_export_options_for_precheck_and_placeholder_choice() {
        let mut app = PlotxApp::new_with_settings(crate::settings::Settings::default());
        app.doc.canvases.push(CanvasDocument::new(
            "clean".to_owned(),
            DEFAULT_CANVAS_SIZE_MM,
        ));
        let mut raster_page = CanvasDocument::new("raster".to_owned(), DEFAULT_CANVAS_SIZE_MM);
        let id = raster_page.allocate_object_id();
        raster_page.objects.push(crate::state::CanvasObject {
            id,
            name: "image".to_owned(),
            frame: crate::state::ObjectFrame::new(0.0, 0.0, 10.0, 10.0),
            locked: false,
            visible: true,
            kind: crate::state::CanvasObjectKind::RasterImage(
                crate::state::RasterImageContent::new(crate::state::AssetId::new()),
            ),
        });
        app.doc.canvases.push(raster_page);
        app.session.active_canvas = Some(0);

        app.request_export(ExportFormat::Svg);
        assert!(app.session.ui.export_options.is_some());
        app.session.ui.export_options = None;
        app.session.active_canvas = Some(1);
        app.request_export(ExportFormat::Svg);
        assert!(app.session.ui.export_options.is_some());
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
