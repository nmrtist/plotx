use super::*;

#[test]
fn sample_vectors_are_written_in_bounded_chunks() {
    #[derive(Default)]
    struct WriteCounter {
        calls: usize,
        bytes: usize,
    }

    impl std::io::Write for WriteCounter {
        fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
            self.calls += 1;
            self.bytes += buffer.len();
            Ok(buffer.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    let samples = vec![1.25; super::electrophysiology_convert::VALUES_PER_CHUNK + 1];
    let recording = crate::state::ElectrophysiologyDataset::load(plotx_io::ElectrophysiologyData {
        abf_version: "2.9.0.0".to_owned(),
        sample_rate_hz: 10_000.0,
        channels: vec![plotx_io::RecordedChannel {
            name: "Current".to_owned(),
            unit: plotx_io::ElectricalUnit::from_symbol("pA"),
        }],
        sweeps: vec![plotx_io::Sweep {
            start_time_s: 0.0,
            channels: vec![samples],
            commands: Vec::new(),
        }],
        protocol: None,
        source: "synthetic.abf".to_owned(),
        import_warnings: Vec::new(),
    });
    let mut output = WriteCounter::default();

    super::electrophysiology_convert::write_electrophysiology_blob(&mut output, &recording)
        .unwrap();

    assert_eq!(output.calls, 3);
    assert_eq!(
        output.bytes,
        8 + (super::electrophysiology_convert::VALUES_PER_CHUNK + 1) * std::mem::size_of::<f64>()
    );
}

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
        sweeps: vec![
            plotx_io::Sweep {
                start_time_s: 0.0,
                channels: vec![vec![1.0, -2.0, -4.0, 1.0]],
                commands: vec![command.clone()],
            },
            plotx_io::Sweep {
                start_time_s: 1.0,
                channels: vec![vec![2.0, -3.0, -5.0, 2.0]],
                commands: vec![command],
            },
        ],
        protocol: Some("vc".to_owned()),
        source: "cell1/test.abf".to_owned(),
        import_warnings: Vec::new(),
    };
    let mut recording = crate::state::ElectrophysiologyDataset::load(data);
    recording.metadata.cell_id = "cell-42".to_owned();
    recording.processing.cutoff_hz = 750.0;
    let selected_item = recording.trace_items()[1].id;
    recording.invocation.analysis_selection = Some(vec![selected_item]);
    recording
        .region_analysis
        .regions
        .push(crate::state::Region {
            id: crate::state::RegionId::new(0),
            lo: 0.0001,
            hi: 0.0003,
            name: "transient".to_owned(),
            label_position: Some([0.2, 0.8]),
            color: crate::state::region_color(0),
            metric: Some(crate::state::RegionMetric::Area),
        });
    recording.region_analysis.next_region_id = crate::state::RegionId::new(1);
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
    assert!(recording.invocation.analysis_selection.is_none());
    assert_eq!(recording.selected_sweep_indices(), vec![0, 1]);
    assert_eq!(recording.region_analysis.regions.len(), 1);
    assert_eq!(recording.region_analysis.regions[0].name, "transient");
    assert_eq!(
        recording.region_analysis.regions[0].label_position,
        Some([0.2, 0.8])
    );
    assert_eq!(
        recording.region_analysis.default_metric,
        crate::state::RegionMetric::Height
    );
    std::fs::remove_file(path).unwrap();
}
