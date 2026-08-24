use super::*;

#[test]
fn v1_rejects_dataset_objects_without_acquisition_identity() {
    let app = tests::sample_app();
    let mut objects = dataset_to_objects(&app.doc.datasets[0], "data-1", "recipe-1").unwrap();
    objects
        .data
        .extensions
        .as_object_mut()
        .expect("data extensions")
        .remove("plotx.acquisition_identity");

    let error = read_acquisition_identity(&objects.data).unwrap_err();
    assert!(
        error
            .to_string()
            .contains("missing plotx.acquisition_identity")
    );
}

#[test]
fn v1_requires_an_explicit_nmr_origin_and_preserves_it_exactly() {
    let mut app = tests::sample_app();
    let origin = plotx_io::NmrOrigin::Instrument {
        instrument: plotx_io::NmrInstrumentOrigin {
            format: plotx_io::NmrSourceFormat::BrukerRaw,
            source_sha256: [42; 32],
            portable: plotx_io::NmrPortableMetadata::default(),
            parameters: plotx_io::NmrSourceParameters::Bruker {
                acqus: "##$TD= 2048".to_owned(),
                title: Some("Sample".to_owned()),
                pulse_program: Some("zg30".to_owned()),
            },
        },
    };
    app.doc.datasets[0].as_nmr_mut().unwrap().origin = origin.clone();
    let mut objects = dataset_to_objects(&app.doc.datasets[0], "data-1", "recipe-1").unwrap();
    assert_eq!(read_nmr_origin(&objects.data).unwrap(), origin);

    objects.data.extensions["plotx.nmr"]
        .as_object_mut()
        .unwrap()
        .remove("origin");
    let error = read_nmr_origin(&objects.data).unwrap_err();
    assert!(
        error
            .to_string()
            .contains("missing required plotx.nmr.origin")
    );

    app.doc.datasets[0].as_nmr_mut().unwrap().origin = origin.clone();
    let path = tests::temp_project("nmr-instrument-origin");
    save_project(&app, &path, false).unwrap();
    let loaded = load_project(&path).unwrap();
    let restored = loaded.doc.datasets[0].as_nmr().unwrap();
    assert_eq!(restored.origin, origin);
    std::fs::remove_file(path).unwrap();
}
