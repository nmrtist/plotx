use super::*;

#[test]
fn schema_v1_project_round_trip_preserves_stream_bindings_and_extractions() {
    let mut app = crate::state::PlotxApp::new();
    let mut dataset = crate::state::MassSpecDataset::load(crate::state::sample_mass_spec_run());
    assert!(dataset.select_stream(AcquisitionStreamId::new(7)));
    dataset
        .add_extraction(
            AcquisitionStreamId::new(7),
            0.4,
            1.4,
            crate::state::MassSpectrumExtractionMethod::Mean,
        )
        .unwrap();
    let xic = dataset
        .plan_ion_chromatogram(AcquisitionStreamId::new(7), 10.0, 20.0)
        .unwrap();
    dataset
        .replace_ion_chromatograms(vec![xic], IonChromatogramId::new(2))
        .unwrap();
    let expected_catalog = dataset.field_catalog.clone();
    app.doc
        .datasets
        .push(crate::state::Dataset::MassSpec(Box::new(dataset)));
    let path = std::env::temp_dir().join(format!(
        "plotx-stream-round-trip-{}.plotx",
        std::process::id()
    ));
    crate::project::save_project(&app, &path, false).unwrap();
    let loaded = crate::project::load_project(&path).unwrap();
    std::fs::remove_file(path).unwrap();
    let loaded = loaded.doc.datasets[0].as_mass_spec().unwrap();
    assert_eq!(loaded.active_stream, AcquisitionStreamId::new(7));
    assert_eq!(
        loaded.extracted_spectra[0].stream,
        AcquisitionStreamId::new(7)
    );
    assert_eq!(
        loaded.extracted_ion_chromatograms[0].stream,
        AcquisitionStreamId::new(7)
    );
    assert_eq!(loaded.extracted_ion_chromatograms[0].mz_min, 10.0);
    assert_eq!(loaded.extracted_ion_chromatograms[0].intensity, [0.0, 0.0]);
    assert_eq!(loaded.field_catalog, expected_catalog);
}

#[test]
fn imported_mzml_run_survives_project_round_trip() {
    let xml = r#"<mzML><run id="r"><spectrumList count="1"><spectrum id="scan=1" defaultArrayLength="1"><cvParam accession="MS:1000511" value="1"/><cvParam accession="MS:1000130"/><scanList><scan><cvParam accession="MS:1000016" value="30" unitAccession="UO:0000010"/></scan></scanList><binaryDataArrayList count="2"><binaryDataArray><cvParam accession="MS:1000514"/><cvParam accession="MS:1000523"/><cvParam accession="MS:1000576"/><binary>AAAAAAAA8D8=</binary></binaryDataArray><binaryDataArray><cvParam accession="MS:1000515"/><cvParam accession="MS:1000523"/><cvParam accession="MS:1000576"/><binary>AAAAAAAAAEA=</binary></binaryDataArray></binaryDataArrayList></spectrum></spectrumList></run></mzML>"#;
    let run = plotx_io::mzml::parse(std::io::Cursor::new(xml), "roundtrip.mzML".into())
        .expect("synthetic repository-owned mzML should import");
    let mut app = crate::state::PlotxApp::new();
    app.doc
        .datasets
        .push(crate::state::Dataset::MassSpec(Box::new(
            crate::state::MassSpecDataset::load(run),
        )));
    let path = std::env::temp_dir().join(format!(
        "plotx-mzml-round-trip-{}.plotx",
        std::process::id()
    ));
    crate::project::save_project(&app, &path, false).unwrap();
    let loaded = crate::project::load_project(&path).unwrap();
    std::fs::remove_file(path).unwrap();
    let spectrum = &loaded.doc.datasets[0].as_mass_spec().unwrap().run.streams[0].spectra[0];
    assert_eq!(spectrum.source_native_id.as_deref(), Some("scan=1"));
    assert_eq!(spectrum.retention_time_min, 0.5);
    assert_eq!(spectrum.mz, [1.0]);
    assert_eq!(spectrum.intensity, [2.0]);
}
