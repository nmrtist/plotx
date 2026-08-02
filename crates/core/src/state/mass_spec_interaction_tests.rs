use super::*;
use crate::state::{Dataset, MassSpecRangeSelection, ObjectFrame, ObjectId, PlotxApp};

#[test]
fn semantic_lcms_ranges_reject_cross_unit_and_stale_bindings() {
    let dataset = Dataset::MassSpec(Box::new(MassSpecDataset::load(sample_mass_spec_run())));
    let dataset_id = dataset.resource_id();
    let mut app = PlotxApp::new();
    app.doc.canvases.push(crate::workflow::build_default_canvas(
        &dataset,
        "synthetic.raw",
    ));
    app.doc.datasets.push(dataset);

    let chromatogram = app.doc.canvases[0].objects[1].id;
    let minute_descriptor = app.plot_interaction_descriptor(0, chromatogram).unwrap();
    assert!(app.dispatch_plot_interaction(minute_descriptor.range(1.0, 0.5).unwrap()));
    assert!(matches!(
        app.mass_spec_range_selection(dataset_id),
        Some(MassSpecRangeSelection::Chromatogram { stream, .. })
            if stream == AcquisitionStreamId::new(3)
    ));
    assert!(
        !matches!(
            app.mass_spec_range_selection(dataset_id),
            Some(MassSpecRangeSelection::Spectrum { .. })
        ),
        "a minute range cannot invoke XIC creation"
    );
    assert!(!app.can_undo(), "a range selection is transient");
    assert!(app.pin_ion_chromatogram_from_selection(dataset_id).is_err());
    assert!(
        app.doc.datasets[0]
            .as_mass_spec()
            .unwrap()
            .extracted_ion_chromatograms
            .is_empty()
    );
    assert!(
        app.pin_mass_spectrum_extraction_from_selection(
            dataset_id,
            MassSpectrumExtractionMethod::HighestTic,
        )
        .is_ok()
    );

    let (spectrum, spectrum_field) = add_current_spectrum_plot(&mut app, dataset_id);
    let mz_descriptor = app.plot_interaction_descriptor(0, spectrum).unwrap();
    assert!(app.dispatch_plot_interaction(mz_descriptor.range(20.0, 10.0).unwrap()));
    assert!(matches!(
        app.mass_spec_range_selection(dataset_id),
        Some(MassSpecRangeSelection::Spectrum { stream, .. })
            if stream == AcquisitionStreamId::new(3)
    ));
    assert!(
        !matches!(
            app.mass_spec_range_selection(dataset_id),
            Some(MassSpecRangeSelection::Chromatogram { .. })
        ),
        "an m/z range cannot invoke spectrum extraction"
    );
    assert!(
        app.pin_mass_spectrum_extraction_from_selection(
            dataset_id,
            MassSpectrumExtractionMethod::HighestTic,
        )
        .is_err()
    );
    assert!(app.pin_ion_chromatogram_from_selection(dataset_id).is_ok());
    let mz_descriptor = app.plot_interaction_descriptor(0, spectrum).unwrap();
    assert!(app.dispatch_plot_interaction(mz_descriptor.range(20.0, 10.0).unwrap()));
    app.session.ui.analysis_selection.as_mut().unwrap().unit = Some("min".to_owned());
    assert!(
        app.mass_spec_range_selection(dataset_id).is_none(),
        "the stored descriptor unit must still match the current binding"
    );
    app.session.ui.analysis_selection.as_mut().unwrap().unit = Some("m/z".to_owned());

    let tic_field = app.doc.datasets[0]
        .as_mass_spec()
        .unwrap()
        .field_catalog
        .id_for_key(&stream_tic_key(AcquisitionStreamId::new(3)))
        .unwrap();
    app.doc.canvases[0]
        .object_mut(spectrum)
        .unwrap()
        .plot_mut()
        .unwrap()
        .binding
        .series[0]
        .source
        .field = tic_field;
    assert!(app.mass_spec_range_selection(dataset_id).is_none());
    assert!(app.pin_ion_chromatogram_from_selection(dataset_id).is_err());
    assert!(
        app.pin_mass_spectrum_extraction_from_selection(
            dataset_id,
            MassSpectrumExtractionMethod::HighestTic,
        )
        .is_err()
    );
    assert_eq!(
        app.doc.datasets[0]
            .as_mass_spec()
            .unwrap()
            .extracted_spectra
            .len(),
        1
    );
    assert_eq!(
        app.doc.datasets[0]
            .as_mass_spec()
            .unwrap()
            .extracted_ion_chromatograms
            .len(),
        1,
        "a stale selection creates no persisted result"
    );

    app.doc.canvases[0]
        .object_mut(spectrum)
        .unwrap()
        .plot_mut()
        .unwrap()
        .binding
        .series[0]
        .source
        .field = spectrum_field;
    let mz_descriptor = app.plot_interaction_descriptor(0, spectrum).unwrap();
    assert!(app.dispatch_plot_interaction(mz_descriptor.range(10.0, 20.0).unwrap()));
    assert!(app.select_mass_spec_stream(dataset_id, AcquisitionStreamId::new(7)));
    assert!(
        app.mass_spec_range_selection(dataset_id).is_none(),
        "a stream switch invalidates the spectrum binding captured by the range"
    );
    assert!(app.pin_ion_chromatogram_from_selection(dataset_id).is_err());
}

#[test]
fn replacement_after_undo_restores_the_planned_xic_field_identity() {
    let dataset = Dataset::MassSpec(Box::new(MassSpecDataset::load(sample_mass_spec_run())));
    let dataset_id = dataset.resource_id();
    let mut app = PlotxApp::new();
    app.doc.canvases.push(crate::workflow::build_default_canvas(
        &dataset,
        "synthetic.raw",
    ));
    app.doc.datasets.push(dataset);

    let id = app
        .pin_ion_chromatogram(dataset_id, AcquisitionStreamId::new(3), 10.0, 20.0)
        .unwrap();
    let first_field = app.doc.datasets[0]
        .as_mass_spec()
        .unwrap()
        .field_catalog
        .id_for_key(&xic_key(id))
        .unwrap();
    app.undo();

    let replacement = app
        .pin_ion_chromatogram(dataset_id, AcquisitionStreamId::new(3), 10.0, 20.0)
        .unwrap();
    assert_eq!(replacement, id);
    let dataset = app.doc.datasets[0].as_mass_spec().unwrap();
    let field = dataset
        .field_catalog
        .id_for_key(&xic_key(replacement))
        .unwrap();
    assert_eq!(field, first_field);
    assert!(dataset.field_values(field).is_some());
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

    app.undo();
    app.redo();
    let dataset = app.doc.datasets[0].as_mass_spec().unwrap();
    assert_eq!(
        dataset.field_catalog.id_for_key(&xic_key(replacement)),
        Some(field)
    );
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
}

#[test]
fn extracted_spectrum_replacement_after_undo_uses_the_restored_field_identity() {
    let dataset = Dataset::MassSpec(Box::new(MassSpecDataset::load(sample_mass_spec_run())));
    let dataset_id = dataset.resource_id();
    let mut app = PlotxApp::new();
    app.doc.canvases.push(crate::workflow::build_default_canvas(
        &dataset,
        "synthetic.raw",
    ));
    app.doc.datasets.push(dataset);

    let id = app
        .pin_mass_spectrum_extraction(
            dataset_id,
            0.5,
            1.0,
            MassSpectrumExtractionMethod::HighestTic,
        )
        .unwrap();
    let first_field = app.doc.datasets[0]
        .as_mass_spec()
        .unwrap()
        .field_catalog
        .id_for_key(&extracted_stream_spectrum_key(id))
        .unwrap();
    app.undo();

    let replacement = app
        .pin_mass_spectrum_extraction(
            dataset_id,
            0.5,
            1.0,
            MassSpectrumExtractionMethod::HighestTic,
        )
        .unwrap();
    assert_eq!(replacement, id);
    let field = app.doc.datasets[0]
        .as_mass_spec()
        .unwrap()
        .field_catalog
        .id_for_key(&extracted_stream_spectrum_key(replacement))
        .unwrap();
    assert_eq!(field, first_field);
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
}

fn add_current_spectrum_plot(app: &mut PlotxApp, dataset_id: DatasetId) -> (ObjectId, FieldId) {
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
    (object_id, spectrum_field)
}
