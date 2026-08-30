use super::*;
use plotx_io::{
    AcquisitionStream, ChromatogramChannel, ChromatogramChannelId, Polarity, SpectrumAcquisition,
    SpectrumRepresentation, SpectrumSummaryProvenance,
};

pub(crate) fn sample_mass_spec_run() -> MassSpecRun {
    let scan = |id, time, tic, polarity, mz: &[f64], intensity: &[f64]| MassSpectrum {
        id: SpectrumId::new(id),
        source_native_id: Some(id.to_string()),
        retention_time_min: time,
        ms_level: 1,
        polarity,
        representation: SpectrumRepresentation::Profile,
        acquisition: SpectrumAcquisition::default(),
        mz: mz.to_vec(),
        intensity: intensity.to_vec(),
        tic,
        tic_provenance: SpectrumSummaryProvenance::Derived,
        base_peak_mz: mz.first().copied(),
        base_peak_intensity: intensity.first().copied(),
        base_peak_provenance: SpectrumSummaryProvenance::Derived,
        precursor: None,
    };
    MassSpecRun {
        source: "synthetic.raw".to_owned(),
        metadata: [("Sample".to_owned(), "test".to_owned())]
            .into_iter()
            .collect(),
        instrument: Some("SQD2".to_owned()),
        streams: vec![
            AcquisitionStream {
                id: AcquisitionStreamId::new(3),
                source_native_id: Some("3".to_owned()),
                source_label: Some("Function 3".to_owned()),
                role: StreamRole::Primary,
                acquisition_range: Some([10.0, 500.0]),
                spectra: vec![
                    scan(11, 0.5, 2.0, Polarity::Positive, &[10.0], &[2.0]),
                    scan(12, 1.0, 9.0, Polarity::Positive, &[20.0, 30.0], &[9.0, 1.0]),
                ],
            },
            AcquisitionStream {
                id: AcquisitionStreamId::new(5),
                source_native_id: Some("5".to_owned()),
                source_label: Some("Function 5".to_owned()),
                role: StreamRole::Reference,
                acquisition_range: None,
                spectra: vec![],
            },
            AcquisitionStream {
                id: AcquisitionStreamId::new(7),
                source_native_id: Some("7".to_owned()),
                source_label: Some("Function 7".to_owned()),
                role: StreamRole::Primary,
                acquisition_range: Some([20.0, 800.0]),
                spectra: vec![
                    scan(101, 0.4, 4.0, Polarity::Negative, &[40.0], &[4.0]),
                    scan(105, 1.4, 3.0, Polarity::Negative, &[50.0], &[3.0]),
                ],
            },
        ],
        chromatograms: vec![
            channel(
                "stream:9:coordinate:217.5",
                ChromatogramKind::Optical,
                Some(217.5),
                "PDA 217.5 nm",
                "AU",
                &[0.5, 1.0],
                &[-1.0, 2.0],
            ),
            channel(
                "stream:9:coordinate:280",
                ChromatogramKind::Optical,
                Some(280.0),
                "PDA 280 nm",
                "AU",
                &[0.5, 1.0],
                &[3.0, 4.0],
            ),
            channel(
                "auxiliary:1",
                ChromatogramKind::Temperature,
                None,
                "Sample temperature",
                "°C",
                &[0.5],
                &[25.0],
            ),
        ],
        import_warnings: vec!["optional reference was unavailable".to_owned()],
    }
}

fn channel(
    id: &str,
    kind: ChromatogramKind,
    coordinate: Option<f64>,
    description: &str,
    unit: &str,
    time_min: &[f64],
    values: &[f64],
) -> ChromatogramChannel {
    ChromatogramChannel {
        id: ChromatogramChannelId(id.to_owned()),
        kind,
        polarity: Polarity::Unknown,
        transition: None,
        source_stream: None,
        coordinate,
        description: description.to_owned(),
        unit: unit.to_owned(),
        time_min: time_min.to_vec(),
        values: values.to_vec(),
    }
}
