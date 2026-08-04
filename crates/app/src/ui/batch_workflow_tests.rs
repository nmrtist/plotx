use super::*;
use plotx_core::actions::Action;
use plotx_core::state::{DEFAULT_CANVAS_SIZE_MM, Dataset, MassSpecDataset};
use plotx_io::{
    AcquisitionStream, AcquisitionStreamId, MassSpecRun, MassSpectrum, Polarity, SpectrumId,
    SpectrumRepresentation, StreamRole,
};

#[test]
fn selecting_a_canvas_resolves_its_mass_spec_dataset() {
    let mut app = PlotxApp::new_with_settings(plotx_core::settings::Settings::default());
    let run = MassSpecRun {
        source: "selected.raw".to_owned(),
        metadata: Default::default(),
        instrument: None,
        streams: vec![AcquisitionStream {
            id: AcquisitionStreamId::new(1),
            source_native_id: None,
            source_label: None,
            role: StreamRole::Primary,
            acquisition_range: None,
            spectra: vec![MassSpectrum {
                id: SpectrumId::new(1),
                source_native_id: None,
                retention_time_min: 1.0,
                ms_level: 1,
                polarity: Polarity::Unknown,
                representation: SpectrumRepresentation::Centroid,
                mz: vec![100.0],
                intensity: vec![1.0],
                tic: 1.0,
                base_peak_mz: Some(100.0),
                base_peak_intensity: Some(1.0),
                precursor: None,
            }],
        }],
        chromatograms: Vec::new(),
        import_warnings: Vec::new(),
    };
    app.execute_action(Action::insert_dataset_with_default_canvas(
        &app,
        Dataset::MassSpec(Box::new(MassSpecDataset::load(run))),
        "LC–MS canvas".to_owned(),
        DEFAULT_CANVAS_SIZE_MM,
    ));
    let expected = app.doc.datasets[0].resource_id().to_string();
    let selected = BTreeSet::from([app.doc.canvases[0].resource_id.to_string()]);

    assert_eq!(selected_mass_spec_ids(&app, &selected), vec![expected]);
    let prepared = prepare_selected_inputs(&app, &selected);
    assert_eq!(prepared[0].0, app.doc.datasets[0].resource_id().to_string());
    assert!(prepared[0].2.as_ref().unwrap()["lc_method"].is_null());
}

#[test]
fn every_background_script_error_keeps_its_dataset_id() {
    let mut ui = AutomationUi::default();
    let (sender, receiver) = mpsc::channel();
    sender
        .send((
            "dataset-1".to_owned(),
            "First".to_owned(),
            Err("one".to_owned()),
        ))
        .unwrap();
    sender
        .send((
            "dataset-2".to_owned(),
            "Second".to_owned(),
            Err("two".to_owned()),
        ))
        .unwrap();
    drop(sender);
    ui.script_task = Some(ScriptTask {
        receiver,
        total: 2,
        completed: 0,
    });

    ui.poll_script_task(&egui::Context::default());

    assert_eq!(ui.script_results.len(), 2);
    assert_eq!(ui.script_results[0]["dataset_id"], "dataset-1");
    assert_eq!(ui.script_results[0]["error"], "one");
    assert_eq!(ui.script_results[1]["dataset_id"], "dataset-2");
    assert_eq!(ui.script_results[1]["error"], "two");
    assert!(ui.script_error.is_none());
}
