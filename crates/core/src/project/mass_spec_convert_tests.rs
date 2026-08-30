use super::*;

fn minimal_run_prefix() -> Vec<u8> {
    let mut bytes = MAGIC.to_vec();
    bytes.extend_from_slice(&VERSION.to_le_bytes());
    bytes.extend_from_slice(&0_u64.to_le_bytes());
    bytes.push(0);
    bytes.extend_from_slice(&0_u64.to_le_bytes());
    bytes
}

fn assert_count_rejected_without_payload(bytes: &[u8], label: &str) {
    let message = decode_bytes(bytes).unwrap_err().to_string();
    assert!(
        message.contains(label) && (message.contains("remaining") || message.contains("truncated")),
        "{message}"
    );
}

#[test]
fn rejects_unknown_future_version_precisely() {
    let mut bytes = MAGIC.to_vec();
    bytes.extend_from_slice(&2_u16.to_le_bytes());
    let error = decode_bytes(&bytes).unwrap_err();
    assert!(error.to_string().contains("LC–MS payload version 2"));
}

#[test]
fn rejects_truncated_header_invalid_tag_and_huge_length_before_allocation() {
    assert!(
        decode_bytes(MAGIC)
            .unwrap_err()
            .to_string()
            .contains("truncated")
    );

    let mut invalid_tag = MAGIC.to_vec();
    invalid_tag.extend_from_slice(&VERSION.to_le_bytes());
    invalid_tag.extend_from_slice(&0_u64.to_le_bytes());
    invalid_tag.push(2);
    assert!(
        decode_bytes(&invalid_tag)
            .unwrap_err()
            .to_string()
            .contains("invalid option tag 2")
    );

    let mut huge = MAGIC.to_vec();
    huge.extend_from_slice(&VERSION.to_le_bytes());
    huge.extend_from_slice(&u64::MAX.to_le_bytes());
    let message = decode_bytes(&huge).unwrap_err().to_string();
    assert!(
        message.contains("string exceeds") || message.contains("length exceeds"),
        "{message}"
    );
}

#[test]
fn rejects_large_structural_counts_without_reserving_the_claimed_collection() {
    const LARGE_COUNT: u64 = 10_000_000;
    let mut warnings = minimal_run_prefix();
    warnings.extend_from_slice(&LARGE_COUNT.to_le_bytes());
    assert_count_rejected_without_payload(&warnings, "warning count");

    let mut streams = minimal_run_prefix();
    streams.extend_from_slice(&0_u64.to_le_bytes());
    streams.extend_from_slice(&LARGE_COUNT.to_le_bytes());
    assert_count_rejected_without_payload(&streams, "stream count");

    let mut spectra = minimal_run_prefix();
    spectra.extend_from_slice(&0_u64.to_le_bytes());
    spectra.extend_from_slice(&1_u64.to_le_bytes());
    spectra.extend_from_slice(&7_u64.to_le_bytes());
    spectra.extend_from_slice(&[0, 0, 0, 0]);
    spectra.extend_from_slice(&LARGE_COUNT.to_le_bytes());
    assert_count_rejected_without_payload(&spectra, "spectrum count");
}

#[test]
fn v1_channel_wire_layout_remains_fixed_and_defaults_provenance_to_source() {
    let channel = ChromatogramChannel {
        id: ChromatogramChannelId("legacy-tic".to_owned()),
        kind: ChromatogramKind::TotalIonCurrent,
        provenance: ChromatogramProvenance::PeakArrays,
        polarity: Polarity::Unknown,
        transition: None,
        source_stream: None,
        coordinate: None,
        description: "Legacy TIC".to_owned(),
        unit: "count".to_owned(),
        time_min: vec![0.5],
        values: vec![10.0],
    };
    let run = MassSpecRun {
        source: String::new(),
        metadata: BTreeMap::new(),
        instrument: None,
        streams: Vec::new(),
        chromatograms: vec![channel],
        import_warnings: Vec::new(),
    };

    let mut v1 = minimal_run_prefix();
    v1.extend_from_slice(&0_u64.to_le_bytes()); // import warnings
    v1.extend_from_slice(&0_u64.to_le_bytes()); // streams
    v1.extend_from_slice(&1_u64.to_le_bytes()); // chromatograms
    write_string(&mut v1, "legacy-tic").unwrap();
    v1.push(0); // TIC
    v1.push(2); // unknown polarity
    v1.push(0); // no transition
    v1.push(0); // no source stream
    v1.push(0); // no coordinate
    write_string(&mut v1, "Legacy TIC").unwrap();
    write_string(&mut v1, "count").unwrap();
    write_f64s(&mut v1, &[0.5]).unwrap();
    write_f64s(&mut v1, &[10.0]).unwrap();
    v1.extend_from_slice(&0_u64.to_le_bytes()); // no active stream
    v1.extend_from_slice(&0_u64.to_le_bytes()); // extracted spectra
    v1.extend_from_slice(&1_u64.to_le_bytes()); // next extraction ID
    v1.extend_from_slice(&0_u64.to_le_bytes()); // extracted-ion chromatograms
    v1.extend_from_slice(&1_u64.to_le_bytes()); // next chromatogram ID

    assert_eq!(encode(&run).unwrap(), v1);
    let decoded = decode_bytes(&v1).unwrap();
    assert_eq!(decoded.chromatograms[0].polarity, Polarity::Unknown);
    assert_eq!(
        decoded.chromatograms[0].provenance,
        ChromatogramProvenance::Source
    );
}

#[test]
fn payload_round_trips_spectra_channels_precursors_and_transitions() {
    let mut run = crate::state::sample_mass_spec_run();
    run.instrument = Some("QTOF".to_owned());
    run.streams[0].spectra[1].acquisition = SpectrumAcquisition {
        instrument_configuration_id: Some("IC2".to_owned()),
        source_event_id: Some(7),
        filter_string: Some("ITMS MS2".to_owned()),
    };
    run.streams[0].spectra[1].tic_provenance = SpectrumSummaryProvenance::Source;
    run.streams[0].spectra[1].base_peak_provenance = SpectrumSummaryProvenance::Source;
    run.streams[0].spectra[1].precursor = Some(Precursor {
        source_spectrum_native_id: Some("scan=10".to_owned()),
        selected_mz: Some(445.2),
        selected_intensity: Some(1_200.0),
        charge: Some(2),
        isolation_window_target_mz: Some(445.0),
        isolation_window_lower_offset: Some(0.5),
        isolation_window_upper_offset: Some(0.75),
        collision_energy: Some(20.0),
        activation_method: Some("CID".to_owned()),
    });
    run.chromatograms[0].kind = ChromatogramKind::SelectedReactionMonitoring;
    run.chromatograms[0].provenance = ChromatogramProvenance::SpectrumSummary;
    run.chromatograms[0].polarity = Polarity::Positive;
    run.chromatograms[0].transition = Some(plotx_io::MassTransition {
        precursor_mz: Some(445.2),
        product_mz: Some(220.1),
        collision_energy: Some(20.0),
        activation_method: Some("CID".to_owned()),
    });
    let decoded = decode_bytes(&encode(&run).unwrap()).unwrap();
    assert_eq!(decoded.source, run.source);
    assert_eq!(decoded.instrument, run.instrument);
    assert_eq!(decoded.metadata, run.metadata);
    assert_eq!(decoded.import_warnings, run.import_warnings);
    assert_eq!(decoded.streams.len(), 3);
    assert_eq!(decoded.streams[0].role, StreamRole::Primary);
    assert_eq!(decoded.streams[0].spectra[1].id, SpectrumId::new(12));
    assert_eq!(decoded.streams[0].spectra[1].mz, [20.0, 30.0]);
    assert_eq!(
        decoded.streams[0].spectra[1]
            .acquisition
            .instrument_configuration_id
            .as_deref(),
        Some("IC2")
    );
    assert_eq!(
        decoded.streams[0].spectra[1].acquisition.source_event_id,
        Some(7)
    );
    assert_eq!(
        decoded.streams[0].spectra[1].tic_provenance,
        SpectrumSummaryProvenance::Source
    );
    assert_eq!(
        decoded.streams[0].spectra[1].base_peak_provenance,
        SpectrumSummaryProvenance::Source
    );
    let precursor = decoded.streams[0].spectra[1].precursor.as_ref().unwrap();
    assert_eq!(
        precursor.source_spectrum_native_id.as_deref(),
        Some("scan=10")
    );
    assert_eq!(precursor.selected_mz, Some(445.2));
    assert_eq!(precursor.selected_intensity, Some(1_200.0));
    assert_eq!(precursor.charge, Some(2));
    assert_eq!(precursor.isolation_window_target_mz, Some(445.0));
    assert_eq!(precursor.activation_method.as_deref(), Some("CID"));
    let channel = &decoded.chromatograms[0];
    assert_eq!(channel.kind, ChromatogramKind::SelectedReactionMonitoring);
    assert_eq!(channel.provenance, ChromatogramProvenance::Source);
    assert_eq!(channel.polarity, Polarity::Positive);
    let transition = channel.transition.as_ref().unwrap();
    assert_eq!(transition.precursor_mz, Some(445.2));
    assert_eq!(transition.product_mz, Some(220.1));
    assert_eq!(transition.activation_method.as_deref(), Some("CID"));
}

#[test]
fn rejects_truncated_and_trailing_payloads() {
    let bytes = encode(&crate::state::sample_mass_spec_run()).unwrap();
    assert!(
        decode_bytes(&bytes[..bytes.len() - 1])
            .unwrap_err()
            .to_string()
            .contains("truncated")
    );
    let mut trailing = bytes;
    trailing.push(0);
    assert!(
        decode_bytes(&trailing)
            .unwrap_err()
            .to_string()
            .contains("trailing data")
    );
}

#[test]
fn chromatogram_only_payload_round_trips_with_no_active_stream() {
    let mut run = crate::state::sample_mass_spec_run();
    run.streams.clear();
    run.chromatograms.truncate(1);
    let decoded = decode_bytes(&encode(&run).unwrap()).unwrap();
    assert!(decoded.streams.is_empty());
    assert_eq!(decoded.chromatograms.len(), 1);
    assert_eq!(
        decoded.chromatograms[0].provenance,
        ChromatogramProvenance::Source
    );
}
