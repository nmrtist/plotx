use super::*;
use crate::actions::Action;
use crate::state::{Dataset, PlotxApp, ToolGroup};

#[test]
fn dynamic_catalog_and_stable_selection_follow_stream_identity() {
    let mut dataset = MassSpecDataset::load(sample_mass_spec_run());
    assert_eq!(dataset.active_stream, AcquisitionStreamId::new(3));
    assert_eq!(dataset.selected_spectrum, None);
    assert_eq!(mass_spec_field_keys(&dataset.run).len(), 8);
    assert!(
        mass_spec_field_keys(&dataset.run)
            .iter()
            .any(|key| key.contains("217.5"))
    );
    assert!(dataset.select_nearest_spectrum(AcquisitionStreamId::new(7), 1.3));
    assert_eq!(dataset.active_stream, AcquisitionStreamId::new(7));
    assert_eq!(dataset.selected_spectrum, Some(SpectrumId::new(105)));
}

#[test]
fn lcms_declares_its_tools_through_the_dataset_capability_registry() {
    let dataset = Dataset::MassSpec(Box::new(MassSpecDataset::load(sample_mass_spec_run())));
    assert_eq!(dataset.tool_groups(), &[ToolGroup::MassSpectrometry]);
}

#[test]
fn default_lcms_canvas_shows_uv_and_tic_with_distinct_semantic_notes() {
    let dataset = Dataset::MassSpec(Box::new(MassSpecDataset::load(sample_mass_spec_run())));
    let canvas = crate::workflow::build_default_canvas(&dataset, "synthetic.raw");
    assert_eq!(canvas.objects.len(), 2);
    let top = canvas.objects[0].plot().unwrap();
    let bottom = canvas.objects[1].plot().unwrap();
    assert_eq!(top.chart.type_id, "mass_chromatogram");
    assert_eq!(top.binding.series.len(), 2);
    assert_eq!(bottom.chart.type_id, "mass_chromatogram");
    assert!(top.panel.user_note.starts_with("UV chromatograms"));
    assert!(bottom.panel.user_note.starts_with("Total ion chromatogram"));
    assert_ne!(top.panel.user_note, bottom.panel.user_note);

    let mut app = PlotxApp::new();
    app.doc.canvases.push(canvas);
    app.doc.datasets.push(dataset);
    app.rebuild_canvases_for(0);
    let rebuilt = app.doc.canvases[0].objects[0].plot().unwrap().figure();
    assert_eq!(rebuilt.series.len(), 2);
    assert_eq!(rebuilt.series[0].points, [[0.5, -1.0], [1.0, 2.0]]);
    assert_eq!(rebuilt.series[1].points, [[0.5, 3.0], [1.0, 4.0]]);
    assert_eq!(rebuilt.series[0].name, "217.5 nm");
    assert_eq!(rebuilt.series[1].name, "280 nm");
    assert_ne!(rebuilt.series[0].color, rebuilt.series[1].color);
    assert_eq!(
        rebuilt.guide_visibility,
        plotx_figure::GuideVisibility::Auto
    );
}

#[test]
fn default_lcms_canvas_without_optical_data_contains_only_tic() {
    let mut run = sample_mass_spec_run();
    run.chromatograms
        .retain(|channel| channel.kind != ChromatogramKind::Optical);
    let dataset = Dataset::MassSpec(Box::new(MassSpecDataset::load(run)));
    let canvas = crate::workflow::build_default_canvas(&dataset, "synthetic.raw");
    assert_eq!(canvas.objects.len(), 1);
    let plot = canvas.objects[0].plot().unwrap();
    assert_eq!(plot.chart.type_id, "mass_chromatogram");
    assert!(plot.panel.user_note.starts_with("Total ion chromatogram"));
}

#[test]
fn mean_extraction_averages_missing_profile_coordinates_as_zero() {
    let mut dataset = MassSpecDataset::load(sample_mass_spec_run());
    let (_, field) = dataset
        .add_extraction(
            AcquisitionStreamId::new(3),
            0.5,
            1.0,
            MassSpectrumExtractionMethod::Mean,
        )
        .unwrap();
    let (_, _, _, points, stick) = dataset.field_values(field).unwrap();
    assert!(stick);
    assert_eq!(points, [[10.0, 1.0], [20.0, 4.5], [30.0, 0.5]]);
}

#[test]
fn stream_and_retention_time_selection_retarget_all_linked_plots() {
    let dataset = Dataset::MassSpec(Box::new(MassSpecDataset::load(sample_mass_spec_run())));
    let dataset_id = dataset.resource_id();
    let mut app = PlotxApp::new();
    app.doc.canvases.push(crate::workflow::build_default_canvas(
        &dataset,
        "synthetic.raw",
    ));
    app.doc.datasets.push(dataset);

    assert!(app.select_mass_spec_spectrum_near(dataset_id, AcquisitionStreamId::new(7), 1.3));
    let dataset = app.doc.datasets[0].as_mass_spec().unwrap();
    assert_eq!(dataset.selected_spectrum, Some(SpectrumId::new(105)));
    let bottom = app.doc.canvases[0].objects[1].plot().unwrap();
    assert_eq!(
        bottom.binding.series[0].source.field,
        dataset
            .field_catalog
            .id_for_key(&stream_tic_key(AcquisitionStreamId::new(7)))
            .unwrap()
    );
    assert!(bottom.panel.user_note.contains("Function 7"));
    assert!(bottom.panel.user_note.contains("negative polarity"));

    app.undo();
    let dataset = app.doc.datasets[0].as_mass_spec().unwrap();
    assert_eq!(dataset.active_stream, AcquisitionStreamId::new(3));
    assert_eq!(dataset.selected_spectrum, None);
    let bottom = app.doc.canvases[0].objects[1].plot().unwrap();
    assert_eq!(
        bottom.binding.series[0].source.field,
        dataset
            .field_catalog
            .id_for_key(&stream_tic_key(AcquisitionStreamId::new(3)))
            .unwrap()
    );
}

#[test]
fn stream_switch_uses_the_shared_undo_history() {
    let dataset = Dataset::MassSpec(Box::new(MassSpecDataset::load(sample_mass_spec_run())));
    let dataset_id = dataset.resource_id();
    let mut app = PlotxApp::new();
    app.doc.canvases.push(crate::workflow::build_default_canvas(
        &dataset,
        "synthetic.raw",
    ));
    app.doc.datasets.push(dataset);

    assert!(app.select_mass_spec_stream(dataset_id, AcquisitionStreamId::new(7)));
    assert_eq!(
        app.doc.datasets[0].as_mass_spec().unwrap().active_stream,
        AcquisitionStreamId::new(7)
    );
    assert!(
        app.doc.canvases[0].objects[1]
            .plot()
            .unwrap()
            .panel
            .user_note
            .contains("Function 7")
    );

    app.undo();
    assert_eq!(
        app.doc.datasets[0].as_mass_spec().unwrap().active_stream,
        AcquisitionStreamId::new(3)
    );
    assert!(
        app.doc.canvases[0].objects[1]
            .plot()
            .unwrap()
            .panel
            .user_note
            .contains("Function 3")
    );

    app.redo();
    assert_eq!(
        app.doc.datasets[0].as_mass_spec().unwrap().active_stream,
        AcquisitionStreamId::new(7)
    );
}

#[test]
fn invalid_stream_actions_are_rejected_before_the_document_changes() {
    let dataset = Dataset::MassSpec(Box::new(MassSpecDataset::load(sample_mass_spec_run())));
    let dataset_id = dataset.resource_id();
    let mut app = PlotxApp::new();
    app.doc.datasets.push(dataset);

    let result = app.try_execute_action(Action::SetMassSpecStream {
        dataset: dataset_id,
        before: AcquisitionStreamId::new(3),
        after: AcquisitionStreamId::new(999),
    });

    assert!(result.is_err());
    assert_eq!(
        app.doc.datasets[0].as_mass_spec().unwrap().active_stream,
        AcquisitionStreamId::new(3)
    );
    assert!(!app.can_undo());
}

#[test]
fn missing_persisted_selection_uses_deterministic_fallback() {
    let mut dataset = MassSpecDataset::load(sample_mass_spec_run());
    dataset.active_stream = AcquisitionStreamId::new(999);
    dataset.selected_spectrum = Some(SpectrumId::new(999));
    dataset.repair_selection().unwrap();
    assert_eq!(dataset.active_stream, AcquisitionStreamId::new(3));
    assert_eq!(dataset.selected_spectrum, None);
}

#[test]
fn extracted_spectrum_is_pinned_and_does_not_follow_preview_cursor() {
    let dataset = Dataset::MassSpec(Box::new(MassSpecDataset::load(sample_mass_spec_run())));
    let dataset_id = dataset.resource_id();
    let mut app = PlotxApp::new();
    app.doc.canvases.push(crate::workflow::build_default_canvas(
        &dataset,
        "synthetic.raw",
    ));
    app.doc.datasets.push(dataset);

    let extraction = app
        .pin_mass_spectrum_extraction(
            dataset_id,
            0.4,
            1.0,
            MassSpectrumExtractionMethod::HighestTic,
        )
        .unwrap();
    assert_eq!(app.doc.canvases[0].objects.len(), 3);
    let spectrum = app.doc.canvases[0].objects[2].plot().unwrap();
    assert_eq!(spectrum.chart.type_id, "mass_spectrum");
    assert_eq!(spectrum.figure().series[0].kind, SeriesKind::Stick);
    assert!(spectrum.panel.user_note.contains("0.400–1.000 min"));
    let before = spectrum.figure().series[0].points.clone();

    assert!(app.select_mass_spec_spectrum_near(dataset_id, AcquisitionStreamId::new(3), 0.5));
    let spectrum = app.doc.canvases[0].objects[2].plot().unwrap();
    assert_eq!(spectrum.figure().series[0].points, before);
    assert_eq!(
        app.doc.datasets[0]
            .as_mass_spec()
            .unwrap()
            .extraction(extraction)
            .unwrap()
            .stream,
        AcquisitionStreamId::new(3)
    );

    app.undo();
    let dataset = app.doc.datasets[0].as_mass_spec().unwrap();
    assert!(dataset.extracted_spectra.is_empty());
    assert_eq!(dataset.next_extraction_id, ExtractionId::new(1));
    assert_eq!(app.doc.canvases[0].objects.len(), 2);

    app.redo();
    let dataset = app.doc.datasets[0].as_mass_spec().unwrap();
    assert_eq!(dataset.extracted_spectra.len(), 1);
    assert_eq!(dataset.next_extraction_id, ExtractionId::new(2));
    assert_eq!(app.doc.canvases[0].objects.len(), 3);
    assert_eq!(
        app.doc.canvases[0].objects[2]
            .plot()
            .unwrap()
            .figure()
            .series[0]
            .kind,
        SeriesKind::Stick
    );
}
