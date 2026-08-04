use super::*;
use plotx_core::actions::Action;
use plotx_core::state::{DEFAULT_CANVAS_SIZE_MM, MassSpecDataset};
use plotx_io::{
    AcquisitionStream, AcquisitionStreamId, MassSpecRun, MassSpectrum, Polarity, SpectrumId,
    SpectrumRepresentation, StreamRole,
};

fn app_with_mass_spec() -> PlotxApp {
    let mut app = PlotxApp::new_with_settings(plotx_core::settings::Settings::default());
    let run = MassSpecRun {
        source: "synthetic.raw".to_owned(),
        metadata: std::collections::BTreeMap::new(),
        instrument: Some("Synthetic MS".to_owned()),
        streams: vec![AcquisitionStream {
            id: AcquisitionStreamId::new(1),
            source_native_id: Some("1".to_owned()),
            source_label: Some("Function 1".to_owned()),
            role: StreamRole::Primary,
            acquisition_range: Some([10.0, 100.0]),
            spectra: vec![MassSpectrum {
                id: SpectrumId::new(1),
                source_native_id: Some("1".to_owned()),
                retention_time_min: 0.5,
                ms_level: 1,
                polarity: Polarity::Positive,
                representation: SpectrumRepresentation::Profile,
                mz: vec![20.0, 40.0],
                intensity: vec![2.0, 5.0],
                tic: 7.0,
                base_peak_mz: Some(40.0),
                base_peak_intensity: Some(5.0),
                precursor: None,
            }],
        }],
        chromatograms: Vec::new(),
        import_warnings: Vec::new(),
    };
    let action = Action::insert_dataset_with_default_canvas(
        &app,
        Dataset::MassSpec(Box::new(MassSpecDataset::load(run))),
        "Canvas — LC–MS".to_owned(),
        DEFAULT_CANVAS_SIZE_MM,
    );
    app.execute_action(action);
    app
}

#[test]
fn extraction_uses_the_shared_command_and_tool_surfaces() {
    let mut app = app_with_mass_spec();
    let command = describe(&app, CommandId::ExtractMassSpectrum);
    assert!(command.enabled);
    assert_eq!(
        command.ribbon,
        Some(RibbonPlacement {
            tab: WorkflowTab::Analyze,
            group: "Extract",
            priority: 0,
            applicability: Applicability::ToolGroup(ToolGroup::MassSpectrometry),
        })
    );
    assert!(describe(&app, CommandId::SelectRange).enabled);

    app.session.secondary_sidebar_visible = false;
    let ctx = egui::Context::default();
    let mut clipboard = crate::ui::clipboard_table::ClipboardTablePaste::default();
    execute(
        CommandId::ExtractMassSpectrum,
        &mut app,
        &mut clipboard,
        &ctx,
    );
    assert_eq!(app.session.tool, Tool::SelectRegion);
    assert!(app.session.secondary_sidebar_visible);
    assert_eq!(
        app.session.ui.requested_tool_group,
        Some(ToolGroup::MassSpectrometry)
    );

    execute(
        CommandId::ExtractMassSpectrum,
        &mut app,
        &mut clipboard,
        &ctx,
    );
    assert_eq!(
        app.session.tool,
        Tool::SelectRegion,
        "reopening an extraction workflow keeps its range tool active"
    );
}

#[test]
fn scientific_script_run_uses_the_shared_command_catalog() {
    let mut app = app_with_mass_spec();
    let command = describe(&app, CommandId::RunScientificScript);
    assert!(command.enabled);
    assert_eq!(command.id.stable_id(), "tools.run_scientific_script");
    assert_eq!(command.execution_class, CommandExecutionClass::ToolEditor);

    let ctx = egui::Context::default();
    execute_without_clipboard(CommandId::RunScientificScript, &mut app, &ctx);

    assert!(ctx.data(|data| {
        data.get_temp::<bool>(egui::Id::new("automation_open_request"))
            .unwrap_or(false)
    }));
    assert!(ctx.data(|data| {
        data.get_temp::<bool>(egui::Id::new("automation_run_script_request"))
            .unwrap_or(false)
    }));
}
