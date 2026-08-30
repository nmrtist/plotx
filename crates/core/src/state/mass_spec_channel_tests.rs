use super::*;
use plotx_io::ChromatogramKind;

#[test]
fn channel_selection_uses_stable_identity_and_shared_binding_history() {
    let mut run = sample_mass_spec_run();
    run.streams.clear();
    run.chromatograms
        .retain(|channel| channel.kind == ChromatogramKind::Optical);
    let first = run.chromatograms[0].id.clone();
    let second = run.chromatograms[1].id.clone();
    let dataset = Dataset::MassSpec(Box::new(MassSpecDataset::load(run)));
    let dataset_id = dataset.resource_id();
    let mut app = PlotxApp::new();
    app.doc.canvases.push(crate::workflow::build_default_canvas(
        &dataset,
        "channels.mzML",
    ));
    app.doc.datasets.push(dataset);
    app.session.active_canvas = Some(0);

    assert_eq!(
        app.selected_mass_spec_channel(dataset_id),
        Some(first.clone())
    );
    assert!(app.select_mass_spec_channel(dataset_id, &second).unwrap());
    assert_eq!(app.selected_mass_spec_channel(dataset_id), Some(second));
    let selected_binding = &app.doc.canvases[0].objects[0].plot().unwrap().binding;
    assert_eq!(selected_binding.series.len(), 1);
    app.undo();
    assert_eq!(app.selected_mass_spec_channel(dataset_id), Some(first));
    let restored_binding = &app.doc.canvases[0].objects[0].plot().unwrap().binding;
    assert_eq!(restored_binding.series.len(), 2);
    assert_ne!(
        restored_binding.series[0].source.field,
        restored_binding.series[1].source.field
    );
}

#[test]
fn channel_selection_never_retargets_the_selected_mass_spectrum_plot() {
    let dataset = Dataset::MassSpec(Box::new(MassSpecDataset::load(sample_mass_spec_run())));
    let dataset_id = dataset.resource_id();
    let channel = dataset.as_mass_spec().unwrap().run.chromatograms[1]
        .id
        .clone();
    let channel_field = dataset
        .as_mass_spec()
        .unwrap()
        .channel_field_id(&channel)
        .unwrap();
    let mut app = PlotxApp::new();
    app.doc.canvases.push(crate::workflow::build_default_canvas(
        &dataset,
        "channels.mzML",
    ));
    app.doc.datasets.push(dataset);
    app.session.active_canvas = Some(0);
    app.pin_mass_spectrum_extraction(
        dataset_id,
        0.4,
        1.0,
        MassSpectrumExtractionMethod::HighestTic,
    )
    .unwrap();

    let spectrum_object = app.doc.canvases[0].objects[2].id;
    assert_eq!(
        app.doc.canvases[0].selected_plot_object_id(),
        Some(spectrum_object)
    );
    let spectrum_field = app.doc.canvases[0]
        .object(spectrum_object)
        .unwrap()
        .plot()
        .unwrap()
        .binding
        .series[0]
        .source
        .field;

    assert!(app.select_mass_spec_channel(dataset_id, &channel).unwrap());
    let spectrum = app.doc.canvases[0]
        .object(spectrum_object)
        .unwrap()
        .plot()
        .unwrap();
    assert_eq!(spectrum.chart.type_id, "mass_spectrum");
    assert_eq!(spectrum.binding.series[0].source.field, spectrum_field);
    let chromatogram = app.doc.canvases[0].objects[0].plot().unwrap();
    assert_eq!(chromatogram.chart.type_id, "mass_chromatogram");
    assert_eq!(chromatogram.binding.series.len(), 1);
    assert_eq!(chromatogram.binding.series[0].source.field, channel_field);
}
