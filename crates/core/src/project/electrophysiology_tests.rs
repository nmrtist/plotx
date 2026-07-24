use super::*;

#[test]
fn project_roundtrip_preserves_raw_data_and_settings() {
    let path = std::env::temp_dir().join(format!(
        "plotx-electrophysiology-roundtrip-{}.plotx",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&path);
    let command = plotx_io::CommandWaveform {
        name: "Command".to_owned(),
        unit: plotx_io::ElectricalUnit::from_symbol("mV"),
        holding_level: -70.0,
        samples: vec![-70.0, -90.0, -90.0, -70.0],
    };
    let data = plotx_io::ElectrophysiologyData {
        abf_version: "2.9.0.0".to_owned(),
        sample_rate_hz: 10_000.0,
        channels: vec![plotx_io::RecordedChannel {
            name: "Current".to_owned(),
            unit: plotx_io::ElectricalUnit::from_symbol("pA"),
        }],
        sweeps: vec![plotx_io::Sweep {
            start_time_s: 0.0,
            channels: vec![vec![1.0, -2.0, -4.0, 1.0]],
            commands: vec![command],
        }],
        protocol: Some("vc".to_owned()),
        source: "cell1/test.abf".to_owned(),
        import_warnings: Vec::new(),
    };
    let mut recording = crate::state::ElectrophysiologyDataset::load(data);
    recording.metadata.cell_id = "cell-42".to_owned();
    recording.processing.cutoff_hz = 750.0;
    let mut legacy_metadata = serde_json::to_value(&recording).unwrap();
    legacy_metadata
        .as_object_mut()
        .unwrap()
        .remove("resource_id");
    let legacy_recording: crate::state::ElectrophysiologyDataset =
        serde_json::from_value(legacy_metadata).unwrap();
    assert!(!legacy_recording.resource_id.to_string().is_empty());
    let mut app = PlotxApp::new();
    app.doc
        .datasets
        .push(Dataset::Electrophysiology(Box::new(recording)));

    save_project(&app, &path, false).unwrap();
    let loaded = load_project(&path).unwrap();
    let recording = loaded.doc.datasets[0].as_electrophysiology().unwrap();
    assert_eq!(
        recording.data.sweeps[0].channels[0],
        vec![1.0, -2.0, -4.0, 1.0]
    );
    assert_eq!(recording.data.sweeps[0].commands[0].samples[1], -90.0);
    assert_eq!(recording.metadata.cell_id, "cell-42");
    assert_eq!(recording.processing.cutoff_hz, 750.0);
    std::fs::remove_file(path).unwrap();
}
