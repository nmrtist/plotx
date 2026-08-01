use super::*;
use plotx_core::actions::Action;
use plotx_core::state::{DEFAULT_CANVAS_SIZE_MM, MassSpecDataset};
use plotx_io::{
    AcquisitionFunction, FunctionId, FunctionKind, MassScan, MassSpecRun, Polarity, ScanEncoding,
    ScanId, WatersDecoder,
};

fn app_with_mass_spec() -> PlotxApp {
    let mut app = PlotxApp::new_with_settings(plotx_core::settings::Settings::default());
    let run = MassSpecRun {
        source: "synthetic.raw".to_owned(),
        metadata: std::collections::BTreeMap::new(),
        instrument: Some("Synthetic MS".to_owned()),
        functions: vec![AcquisitionFunction {
            id: FunctionId::new(1),
            kind: FunctionKind::MassSpectrum,
            polarity: Polarity::Positive,
            acquisition_range: Some([10.0, 100.0]),
            encoding: ScanEncoding {
                idx_stride: 22,
                pair_width: 6,
                decoder: WatersDecoder::LowResolution6,
            },
            scans: vec![MassScan {
                id: ScanId::new(1),
                retention_time_min: 0.5,
                mz: vec![20.0, 40.0],
                intensity: vec![2.0, 5.0],
                tic: 7.0,
                base_peak_mz: Some(40.0),
                base_peak_intensity: Some(5.0),
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
