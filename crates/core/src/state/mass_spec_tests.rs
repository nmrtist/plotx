use super::*;
use crate::actions::Action;
use crate::state::{
    AxisRange, Dataset, ObjectFrame, PlotxApp, SeriesBinding, SeriesSource, ToolGroup,
};
use plotx_io::{ChromatogramChannel, ChromatogramChannelId};
use std::path::Path;

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
    assert!(canvas.objects[0].name.starts_with("UV chromatograms"));
    assert!(canvas.objects[1].name.starts_with("Total ion chromatogram"));
    assert_ne!(canvas.objects[0].name, canvas.objects[1].name);

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
    assert!(canvas.objects[0].name.starts_with("Total ion chromatogram"));
}

#[test]
fn chromatogram_only_run_uses_existing_fields_and_default_canvas() {
    let mut run = sample_mass_spec_run();
    run.streams.clear();
    run.chromatograms.truncate(1);
    run.chromatograms[0].kind = ChromatogramKind::SelectedReactionMonitoring;
    run.chromatograms[0].description = "SRM 455.2 -> 520.2".to_owned();
    assert!(run.validate().is_ok());

    let dataset = Dataset::MassSpec(Box::new(MassSpecDataset::load(run)));
    let mass_spec = dataset.as_mass_spec().unwrap();
    assert_eq!(mass_spec.active_stream, AcquisitionStreamId::new(0));
    assert!(mass_spec.supported_ms_streams().next().is_none());
    assert_eq!(mass_spec_field_keys(&mass_spec.run).len(), 1);

    let canvas = crate::workflow::build_default_canvas(&dataset, "mrm.mzML");
    assert_eq!(canvas.objects.len(), 1);
    assert_eq!(canvas.objects[0].name, "SRM 455.2 -> 520.2");
    let plot = canvas.objects[0].plot().unwrap();
    assert_eq!(plot.chart.type_id, "mass_chromatogram");
    assert_eq!(plot.figure().series[0].points, [[0.5, -1.0], [1.0, 2.0]]);
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
fn stream_tic_prefers_bound_chromatogram_points() {
    let mut run = sample_mass_spec_run();
    run.chromatograms.push(ChromatogramChannel {
        id: ChromatogramChannelId("tic:bound".to_owned()),
        kind: ChromatogramKind::TotalIonCurrent,
        polarity: plotx_io::Polarity::Unknown,
        transition: None,
        source_stream: Some(AcquisitionStreamId::new(3)),
        coordinate: None,
        description: "Total ion current".to_owned(),
        unit: "cps".to_owned(),
        time_min: vec![0.0, 2.0],
        values: vec![11.0, 22.0],
    });
    let dataset = MassSpecDataset::load(run);
    let field = dataset
        .field_catalog
        .id_for_key(&stream_tic_key(AcquisitionStreamId::new(3)))
        .expect("stream TIC field");
    let (_, _, _, points, stick) = dataset.field_values(field).expect("TIC values");
    assert!(!stick);
    assert_eq!(points, [[0.0, 11.0], [2.0, 22.0]]);
}

#[test]
fn stream_tic_without_a_bound_channel_uses_spectrum_summaries() {
    let dataset = MassSpecDataset::load(sample_mass_spec_run());
    let field = dataset
        .field_catalog
        .id_for_key(&stream_tic_key(AcquisitionStreamId::new(3)))
        .expect("stream TIC field");
    let (_, _, _, points, stick) = dataset.field_values(field).expect("TIC values");

    assert!(!stick);
    assert_eq!(points, [[0.5, 2.0], [1.0, 9.0]]);
    assert_eq!(
        dataset.run.streams[0].spectra[1]
            .intensity
            .iter()
            .sum::<f64>(),
        10.0
    );
}

#[test]
fn local_small_mzml_figure_matches_the_14_point_ms1_tic_when_present() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(".tmp/MS-data/hupo-psi-mzpeak-small/small.mzML");
    if !path.is_file() {
        return;
    }
    let loaded = plotx_io::mzml::load(&path).unwrap();
    let plotx_io::Acquisition::MassSpec(run) = loaded.acquisition else {
        panic!("small.mzML did not import as mass spectrometry data");
    };
    let dataset = MassSpecDataset::load(*run);
    let field = dataset
        .field_catalog
        .id_for_key(&stream_tic_key(dataset.active_stream))
        .unwrap();
    let figure = dataset.field_figure(field).unwrap();

    assert_eq!(figure.series[0].points.len(), 14);
    assert_eq!(
        figure.series[0]
            .points
            .iter()
            .copied()
            .max_by(|left, right| left[1].total_cmp(&right[1])),
        Some([0.285483333333, 22_136_832.0])
    );
    assert_eq!(figure.series[0].points[1], [0.007896666667, 12_901_166.0]);
}

#[test]
fn displayed_mass_spec_trace_uses_bound_tic_channel() {
    let mut run = sample_mass_spec_run();
    run.chromatograms.push(ChromatogramChannel {
        id: ChromatogramChannelId("tic:rendered".to_owned()),
        kind: ChromatogramKind::TotalIonCurrent,
        polarity: plotx_io::Polarity::Unknown,
        transition: None,
        source_stream: Some(AcquisitionStreamId::new(3)),
        coordinate: None,
        description: "Total ion current".to_owned(),
        unit: "cps".to_owned(),
        time_min: vec![0.0, 1.0],
        values: vec![100.0, 200.0],
    });
    let dataset = Dataset::MassSpec(Box::new(MassSpecDataset::load(run)));
    let trace = dataset.displayed_trace(None).expect("mass-spec trace");
    assert_eq!(trace.xs, [0.0, 1.0]);
    assert_eq!(trace.ys, [100.0, 200.0]);
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
    assert!(app.doc.canvases[0].objects[1].name.contains("Function 7"));
    assert!(
        app.doc.canvases[0].objects[1]
            .name
            .contains("negative polarity")
    );

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
fn chromatogram_interaction_dispatches_cursor_and_normalized_minute_range() {
    let dataset = Dataset::MassSpec(Box::new(MassSpecDataset::load(sample_mass_spec_run())));
    let dataset_id = dataset.resource_id();
    let mut app = PlotxApp::new();
    app.doc.canvases.push(crate::workflow::build_default_canvas(
        &dataset,
        "synthetic.raw",
    ));
    app.doc.datasets.push(dataset);
    let object = app.doc.canvases[0].objects[1].id;
    let descriptor = app
        .plot_interaction_descriptor(0, object)
        .expect("TIC declares a semantic x interaction");
    assert_eq!(descriptor.unit, "min");

    assert!(app.dispatch_plot_interaction(descriptor.cursor(1.3).unwrap()));
    assert_eq!(
        app.doc.datasets[0]
            .as_mass_spec()
            .unwrap()
            .selected_spectrum,
        Some(SpectrumId::new(12))
    );
    assert_eq!(
        app.session.status,
        "Selected the nearest scan in Function 3 at 1.300 min."
    );
    assert!(!app.can_undo(), "cursor preview is transient");

    assert!(app.dispatch_plot_interaction(descriptor.range(1.2, 0.6).unwrap()));
    let selection = app.session.ui.analysis_selection.as_ref().unwrap();
    assert_eq!(selection.x_range, AxisRange::new(0.6, 1.2));
    assert_eq!(selection.dataset, dataset_id);
}

#[test]
fn mixed_or_cross_dataset_chromatogram_overlays_do_not_declare_interactions() {
    let dataset = Dataset::MassSpec(Box::new(MassSpecDataset::load(sample_mass_spec_run())));
    let mut app = PlotxApp::new();
    app.doc.canvases.push(crate::workflow::build_default_canvas(
        &dataset,
        "synthetic.raw",
    ));
    app.doc.datasets.push(dataset);
    let object = app.doc.canvases[0].objects[1].id;
    let spectrum = app.doc.datasets[0]
        .as_mass_spec()
        .unwrap()
        .field_catalog
        .id_for_key(&stream_spectrum_key(AcquisitionStreamId::new(3)))
        .unwrap();
    let dataset_id = app.doc.datasets[0].resource_id();
    app.doc.canvases[0]
        .object_mut(object)
        .unwrap()
        .plot_mut()
        .unwrap()
        .binding
        .series
        .push(SeriesBinding::with_source(SeriesSource {
            resource: dataset_id,
            field: spectrum,
            item: None,
        }));
    assert!(app.plot_interaction_descriptor(0, object).is_none());

    app.doc.canvases[0]
        .object_mut(object)
        .unwrap()
        .plot_mut()
        .unwrap()
        .binding
        .series
        .truncate(1);
    let other = Dataset::MassSpec(Box::new(MassSpecDataset::load(sample_mass_spec_run())));
    let other_id = other.resource_id();
    let other_field = other
        .as_mass_spec()
        .unwrap()
        .field_catalog
        .id_for_key(&stream_tic_key(AcquisitionStreamId::new(3)))
        .unwrap();
    app.doc.datasets.push(other);
    app.doc.canvases[0]
        .object_mut(object)
        .unwrap()
        .plot_mut()
        .unwrap()
        .binding
        .series
        .push(SeriesBinding::with_source(SeriesSource {
            resource: other_id,
            field: other_field,
            item: None,
        }));
    assert!(app.plot_interaction_descriptor(0, object).is_none());
}

#[test]
fn cross_stream_cursor_uses_one_stream_action_and_a_transient_preview() {
    let dataset = Dataset::MassSpec(Box::new(MassSpecDataset::load(sample_mass_spec_run())));
    let mut app = PlotxApp::new();
    app.doc.canvases.push(crate::workflow::build_default_canvas(
        &dataset,
        "synthetic.raw",
    ));
    app.doc.datasets.push(dataset);
    let object = app.doc.canvases[0].objects[1].id;
    let field = app.doc.datasets[0]
        .as_mass_spec()
        .unwrap()
        .field_catalog
        .id_for_key(&stream_tic_key(AcquisitionStreamId::new(7)))
        .unwrap();
    app.doc.canvases[0]
        .object_mut(object)
        .unwrap()
        .plot_mut()
        .unwrap()
        .binding
        .series[0]
        .source
        .field = field;
    let descriptor = app.plot_interaction_descriptor(0, object).unwrap();
    assert!(app.dispatch_plot_interaction(descriptor.cursor(1.3).unwrap()));
    assert_eq!(
        app.doc.datasets[0].as_mass_spec().unwrap().active_stream,
        AcquisitionStreamId::new(7)
    );
    assert_eq!(
        app.doc.datasets[0]
            .as_mass_spec()
            .unwrap()
            .selected_spectrum,
        Some(SpectrumId::new(105))
    );
    assert_eq!(
        app.session.status,
        "Selected the nearest scan in Function 7 at 1.300 min."
    );
    assert!(app.can_undo());
    app.undo();
    assert_eq!(
        app.doc.datasets[0].as_mass_spec().unwrap().active_stream,
        AcquisitionStreamId::new(3)
    );
    assert!(
        !app.can_undo(),
        "preview did not create a second undo entry"
    );
    app.redo();
    assert_eq!(
        app.doc.datasets[0].as_mass_spec().unwrap().active_stream,
        AcquisitionStreamId::new(7)
    );
}

#[test]
fn stale_or_invalid_chromatogram_interaction_is_dropped() {
    let dataset = Dataset::MassSpec(Box::new(MassSpecDataset::load(sample_mass_spec_run())));
    let mut app = PlotxApp::new();
    app.doc.canvases.push(crate::workflow::build_default_canvas(
        &dataset,
        "synthetic.raw",
    ));
    app.doc.datasets.push(dataset);
    let object = app.doc.canvases[0].objects[1].id;
    let descriptor = app.plot_interaction_descriptor(0, object).unwrap();
    assert!(descriptor.cursor(f64::NAN).is_none());
    assert!(descriptor.range(1.0, f64::INFINITY).is_none());
    app.doc.canvases.clear();
    assert!(!app.dispatch_plot_interaction(descriptor.cursor(1.0).unwrap()));
    assert!(app.session.ui.analysis_selection.is_none());
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
    assert!(app.doc.canvases[0].objects[1].name.contains("Function 7"));

    app.undo();
    assert_eq!(
        app.doc.datasets[0].as_mass_spec().unwrap().active_stream,
        AcquisitionStreamId::new(3)
    );
    assert!(app.doc.canvases[0].objects[1].name.contains("Function 3"));

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
    assert!(
        app.doc.canvases[0].objects[2]
            .name
            .contains("0.400–1.000 min")
    );
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

#[test]
fn xic_is_pinned_to_its_stream_and_undo_redo_preserve_its_identity() {
    let dataset = Dataset::MassSpec(Box::new(MassSpecDataset::load(sample_mass_spec_run())));
    let dataset_id = dataset.resource_id();
    let mut app = PlotxApp::new();
    app.doc.canvases.push(crate::workflow::build_default_canvas(
        &dataset,
        "synthetic.raw",
    ));
    app.doc.datasets.push(dataset);

    let reversed = app.doc.datasets[0]
        .as_mass_spec()
        .unwrap()
        .plan_ion_chromatogram(AcquisitionStreamId::new(3), 20.0, 10.0)
        .unwrap();
    assert_eq!((reversed.mz_min, reversed.mz_max), (10.0, 20.0));
    assert_eq!(reversed.intensity, [2.0, 9.0]);

    let id = app
        .pin_ion_chromatogram(dataset_id, AcquisitionStreamId::new(3), 10.0, 20.0)
        .unwrap();
    assert_eq!(
        app.doc.canvases[0].objects[2].plot().unwrap().chart.type_id,
        "mass_chromatogram"
    );
    let xic = &app.doc.datasets[0]
        .as_mass_spec()
        .unwrap()
        .extracted_ion_chromatograms[0];
    assert_eq!(xic.id, id);
    assert_eq!(xic.stream, AcquisitionStreamId::new(3));
    assert_eq!(xic.time_min, [0.5, 1.0]);
    assert_eq!(xic.intensity, [2.0, 9.0]);
    let field = app.doc.datasets[0]
        .as_mass_spec()
        .unwrap()
        .field_catalog
        .id_for_key(&xic_key(id))
        .unwrap();
    assert_eq!(app.doc.canvases[0].objects.len(), 3);
    assert_eq!(
        app.doc.canvases[0].objects[2]
            .plot()
            .unwrap()
            .binding
            .series[0]
            .source
            .field,
        field
    );

    assert!(app.select_mass_spec_stream(dataset_id, AcquisitionStreamId::new(7)));
    assert_eq!(
        app.doc.datasets[0]
            .as_mass_spec()
            .unwrap()
            .extracted_ion_chromatograms[0]
            .stream,
        AcquisitionStreamId::new(3)
    );
    app.undo(); // stream switch
    app.undo(); // XIC
    assert!(
        app.doc.datasets[0]
            .as_mass_spec()
            .unwrap()
            .extracted_ion_chromatograms
            .is_empty()
    );
    assert_eq!(app.doc.canvases[0].objects.len(), 2);
    app.redo();
    let restored = &app.doc.datasets[0]
        .as_mass_spec()
        .unwrap()
        .extracted_ion_chromatograms[0];
    assert_eq!(restored.id, id);
    assert_eq!(restored.time_min, [0.5, 1.0]);
    assert_eq!(restored.intensity, [2.0, 9.0]);
    assert_eq!(
        app.doc.datasets[0]
            .as_mass_spec()
            .unwrap()
            .field_catalog
            .id_for_key(&xic_key(id)),
        Some(field)
    );
}

#[test]
fn current_spectrum_declares_mz_range_while_chromatograms_remain_minutes() {
    let dataset = Dataset::MassSpec(Box::new(MassSpecDataset::load(sample_mass_spec_run())));
    let dataset_id = dataset.resource_id();
    let mut app = PlotxApp::new();
    app.doc.canvases.push(crate::workflow::build_default_canvas(
        &dataset,
        "synthetic.raw",
    ));
    app.doc.datasets.push(dataset);
    let chromatogram = app.doc.canvases[0].objects[1].id;
    assert_eq!(
        app.plot_interaction_descriptor(0, chromatogram)
            .unwrap()
            .unit,
        "min"
    );
    assert!(app.select_mass_spec_spectrum_near(dataset_id, AcquisitionStreamId::new(3), 0.5));
    let spectrum_field = app.doc.datasets[0]
        .as_mass_spec()
        .unwrap()
        .field_catalog
        .id_for_key(&stream_spectrum_key(AcquisitionStreamId::new(3)))
        .unwrap();
    let object_id = app.doc.canvases[0].allocate_object_id();
    let mut object = app.build_plot_object(
        0,
        ObjectFrame::new(0.0, 0.0, 100.0, 100.0),
        object_id,
        "Current spectrum".to_owned(),
    );
    object.plot_mut().unwrap().binding.series[0].source.field = spectrum_field;
    app.doc.canvases[0].objects.push(object);
    app.rebuild_canvases_for(0);
    let descriptor = app.plot_interaction_descriptor(0, object_id).unwrap();
    assert_eq!(descriptor.unit, "m/z");
    assert!(descriptor.cursor(10.0).is_none());
    assert!(app.dispatch_plot_interaction(descriptor.range(20.0, 10.0).unwrap()));
    assert!(!app.can_undo(), "m/z selection is transient");
    let selection = app.session.ui.analysis_selection.as_ref().unwrap();
    assert_eq!(selection.field, Some(spectrum_field));
    assert_eq!(selection.x_range, AxisRange::new(10.0, 20.0));
}

#[test]
fn persisted_xic_validation_rejects_missing_stream_and_mismatched_arrays() {
    let run = sample_mass_spec_run();
    let invalid_stream = ExtractedIonChromatogram {
        id: IonChromatogramId::new(1),
        stream: AcquisitionStreamId::new(999),
        mz_min: 10.0,
        mz_max: 20.0,
        time_min: vec![0.5],
        intensity: vec![1.0],
    };
    assert!(
        MassSpecDataset::validate_ion_chromatogram_state(
            &run,
            &mut [invalid_stream],
            &mut IonChromatogramId::new(2)
        )
        .unwrap_err()
        .contains("missing stream")
    );
    let mismatched = ExtractedIonChromatogram {
        id: IonChromatogramId::new(1),
        stream: AcquisitionStreamId::new(3),
        mz_min: 10.0,
        mz_max: 20.0,
        time_min: vec![0.5],
        intensity: vec![1.0, 2.0],
    };
    assert!(
        MassSpecDataset::validate_ion_chromatogram_state(
            &run,
            &mut [mismatched],
            &mut IonChromatogramId::new(2)
        )
        .is_err()
    );
}
