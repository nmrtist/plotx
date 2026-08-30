use super::*;
use crate::SpectrumSummaryProvenance;
use std::collections::BTreeMap;
use std::io::Write;
use std::path::PathBuf;

fn source_spectrum() -> SpectrumRecord {
    SpectrumRecord {
        index: 4,
        scan_number: 5,
        native_id: "file=fixture scan=5".to_owned(),
        ms_level: 2,
        polarity: Some(SourcePolarity::Positive),
        scan_mode: Some(ScanMode::Centroid),
        analyzer: Some(Analyzer::TOFMS),
        acquisition_event_id: None,
        filter: None,
        retention_time_sec: 90.0,
        total_ion_current: None,
        base_peak_mz: None,
        base_peak_intensity: None,
        low_mz: None,
        high_mz: None,
        ion_injection_time_ms: None,
        inv_mobility: None,
        faims_cv: None,
        precursor: Some(PrecursorInfo {
            selected_mz: Some(445.34),
            target_mz: Some(445.35),
            isolation_width: Some(2.0),
            charge: Some(2),
            collision_energy: Some(30.0),
            activation: Some(Activation::CID),
        }),
        mz: vec![100.0, 250.0, 600.0],
        intensity: vec![3.0, 11.0, 7.0],
        inv_mobility_per_peak: None,
        extra: BTreeMap::new(),
    }
}

fn write_synthetic_pair(path: &Path, samples: &[(&str, u32, f32, f64)]) -> PathBuf {
    let file = std::fs::File::create(path).unwrap();
    let mut compound = cfb::CompoundFile::create(file).unwrap();
    compound.create_storage("SampleSubtree").unwrap();
    let mut scan = vec![0_u8; samples.len() * 100];
    for (index, (sample, ms_level, time_min, tic)) in samples.iter().enumerate() {
        compound
            .create_storage(format!("SampleSubtree/{sample}"))
            .unwrap();
        let offset = index * 100;
        let mut idx = vec![0_u8; 32 + 54];
        idx[32..36].copy_from_slice(&u32::try_from(offset).unwrap().to_le_bytes());
        idx[36..40].copy_from_slice(&100_u32.to_le_bytes());
        idx[44..48].copy_from_slice(&time_min.to_le_bytes());
        idx[48..50].copy_from_slice(&u16::try_from(*ms_level).unwrap().to_le_bytes());
        idx[50..58].copy_from_slice(&tic.to_le_bytes());
        compound
            .create_stream(format!("SampleSubtree/{sample}/Idx"))
            .unwrap()
            .write_all(&idx)
            .unwrap();
        scan[offset + 56..offset + 64]
            .copy_from_slice(&[100, 0x85, 10, 0x89, 0xff, 0xff, 0xff, 0xff]);
    }
    compound.flush().unwrap();
    drop(compound);

    let mut scan_path = path.to_owned();
    let mut name = path.file_name().unwrap().to_os_string();
    name.push(".scan");
    scan_path.set_file_name(name);
    std::fs::write(&scan_path, scan).unwrap();
    scan_path
}

#[test]
fn detects_wiff_extension_case_insensitively() {
    for path in ["run.wiff", "run.WIFF", "run.WiFf"] {
        assert_eq!(
            crate::detect_format(path).unwrap(),
            DataFormat::MassSpectrometry(MassSpectrometryFormat::SciexWiff)
        );
    }
    assert_eq!(
        DataFormat::MassSpectrometry(MassSpectrometryFormat::SciexWiff).as_str(),
        "sciex-wiff"
    );
}

#[test]
fn rejects_wiff2_and_timeseries_with_conversion_guidance() {
    for path in ["run.wiff2", "run.WIFF2", "run.timeseries.data"] {
        let error = crate::detect_format(path).unwrap_err().to_string();
        assert!(error.contains("not currently supported"), "{error}");
        assert!(error.contains("mzML"), "{error}");
    }
}

#[test]
fn requires_the_paired_scan_file() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("missing.wiff");
    std::fs::write(&path, b"not inspected before companion check").unwrap();

    let error = load(&path).unwrap_err().to_string();

    assert!(
        error.contains("paired .wiff.scan file is missing"),
        "{error}"
    );
    assert!(error.contains("missing.wiff.scan"), "{error}");
}

#[test]
fn reports_a_corrupt_wiff_container() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("corrupt.wiff");
    std::fs::write(&path, b"not an OLE container").unwrap();
    std::fs::write(directory.path().join("corrupt.wiff.scan"), b"scan").unwrap();

    let error = load(&path).unwrap_err().to_string();

    assert!(
        error.starts_with("invalid or unsupported SCIEX WIFF:"),
        "{error}"
    );
}

#[test]
fn rejects_an_empty_sample_container() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("samples.wiff");
    let file = std::fs::File::create(&path).unwrap();
    let mut compound = cfb::CompoundFile::create(file).unwrap();
    compound.create_storage("SampleSubtree").unwrap();
    compound.flush().unwrap();
    drop(compound);
    std::fs::write(directory.path().join("samples.wiff.scan"), b"scan").unwrap();

    let error = load(&path).unwrap_err().to_string();

    assert!(error.contains("contains no samples"), "{error}");
}

#[test]
fn loads_a_synthetic_single_sample_wiff_pair_end_to_end() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("single.wiff");
    let scan_path = write_synthetic_pair(&path, &[("Sample1", 1, 1.25, 14.0)]);

    let loaded = crate::load_path(&path).unwrap();

    assert_eq!(
        loaded.format,
        DataFormat::MassSpectrometry(MassSpectrometryFormat::SciexWiff)
    );
    assert_eq!(loaded.provenance.companion_paths, vec![scan_path]);
    let Acquisition::MassSpec(run) = loaded.acquisition else {
        panic!("WIFF should produce a mass-spectrometry run");
    };
    assert_eq!(run.streams.len(), 1);
    assert_eq!(
        run.streams[0].source_label.as_deref(),
        Some("Sample1 - MS1 unknown")
    );
    assert_eq!(
        run.metadata.get("sample count").map(String::as_str),
        Some("1")
    );
    let spectrum = &run.streams[0].spectra[0];
    assert_eq!(
        spectrum.source_native_id.as_deref(),
        Some("file=single scan=1")
    );
    assert_eq!(spectrum.retention_time_min, 1.25);
    assert_eq!(spectrum.mz, vec![100.0, 110.0]);
    assert_eq!(spectrum.intensity, vec![5.0, 9.0]);
    assert_eq!(spectrum.tic, 14.0);
    assert_eq!(run.chromatograms.len(), 1);
    assert_eq!(run.chromatograms[0].time_min, vec![1.25]);
    assert_eq!(run.chromatograms[0].values, vec![14.0]);
}

#[test]
fn loads_all_samples_as_distinct_streams_and_chromatograms() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("multi.wiff");
    let scan_path = write_synthetic_pair(
        &path,
        &[("Sample1", 1, 1.25, 14.0), ("Sample2", 2, 2.5, 28.0)],
    );

    let loaded = crate::load_path(&path).unwrap();

    assert_eq!(loaded.provenance.companion_paths, vec![scan_path]);
    assert_eq!(loaded.acquisition_identity.subject, None);
    assert_eq!(
        loaded.acquisition_identity.acquisition.as_deref(),
        Some("2 samples")
    );
    let Acquisition::MassSpec(run) = loaded.acquisition else {
        panic!("WIFF should produce a mass-spectrometry run");
    };
    assert_eq!(
        run.metadata.get("sample count").map(String::as_str),
        Some("2")
    );
    assert_eq!(
        run.metadata.get("samples").map(String::as_str),
        Some("Sample1, Sample2")
    );
    assert_eq!(run.streams.len(), 2);
    assert_eq!(run.streams[0].id, AcquisitionStreamId::new(1));
    assert_eq!(run.streams[1].id, AcquisitionStreamId::new(2));
    assert_eq!(
        run.streams[0].source_label.as_deref(),
        Some("Sample1 - MS1 unknown")
    );
    assert_eq!(
        run.streams[1].source_label.as_deref(),
        Some("Sample2 - MS2 unknown")
    );
    assert_eq!(run.streams[0].spectra[0].retention_time_min, 1.25);
    assert_eq!(run.streams[1].spectra[0].retention_time_min, 2.5);
    assert_eq!(
        run.chromatograms
            .iter()
            .map(|channel| channel.id.0.as_str())
            .collect::<Vec<_>>(),
        vec!["Sample1:TIC", "Sample2:TIC"]
    );
    assert_eq!(run.chromatograms[0].values, vec![14.0]);
    assert_eq!(run.chromatograms[1].values, vec![28.0]);
}

#[test]
fn maps_spectrum_identity_time_polarity_precursor_and_summaries() {
    let mut source = source_spectrum();
    source.total_ion_current = Some(19.0);
    source.base_peak_mz = Some(100.0);
    source.base_peak_intensity = Some(4.0);
    source.acquisition_event_id = Some(2);
    source.filter = Some("TOF MS2".to_owned());
    let streams = build_streams(vec![source], "Sample1", &mut 1, &mut Vec::new()).unwrap();
    let stream = &streams[0];
    let spectrum = &stream.spectra[0];

    assert_eq!(stream.acquisition_range, Some([100.0, 600.0]));
    assert_eq!(
        stream.source_label.as_deref(),
        Some("Sample1 - MS2 positive")
    );
    assert_eq!(spectrum.id, SpectrumId::new(5));
    assert_eq!(
        spectrum.source_native_id.as_deref(),
        Some("file=fixture scan=5")
    );
    assert_eq!(spectrum.retention_time_min, 1.5);
    assert_eq!(spectrum.ms_level, 2);
    assert_eq!(spectrum.polarity, Polarity::Positive);
    assert_eq!(spectrum.representation, SpectrumRepresentation::Centroid);
    assert_eq!(spectrum.mz.len(), spectrum.intensity.len());
    assert_eq!(spectrum.acquisition.source_event_id, Some(2));
    assert_eq!(
        spectrum.acquisition.filter_string.as_deref(),
        Some("TOF MS2")
    );
    assert_eq!(spectrum.tic, 19.0);
    assert_eq!(spectrum.tic_provenance, SpectrumSummaryProvenance::Source);
    assert_eq!(spectrum.base_peak_mz, Some(100.0));
    assert_eq!(spectrum.base_peak_intensity, Some(4.0));
    assert_eq!(
        spectrum.base_peak_provenance,
        SpectrumSummaryProvenance::Source
    );
    let precursor = spectrum.precursor.as_ref().unwrap();
    assert_eq!(precursor.source_spectrum_native_id, None);
    assert_eq!(precursor.selected_mz, Some(445.34));
    assert_eq!(precursor.selected_intensity, None);
    assert_eq!(precursor.charge, Some(2));
    assert_eq!(precursor.isolation_window_target_mz, Some(445.35));
    assert_eq!(precursor.isolation_window_lower_offset, Some(1.0));
    assert_eq!(precursor.isolation_window_upper_offset, Some(1.0));
    assert_eq!(precursor.collision_energy, Some(30.0));
    assert_eq!(precursor.activation_method.as_deref(), Some("CID"));
}

#[test]
fn rejects_a_sample_with_no_decoded_spectra() {
    let mut record = source_spectrum();
    record.mz.clear();
    record.intensity.clear();

    let mut warnings = Vec::new();
    let error = build_streams(vec![record], "Sample1", &mut 1, &mut warnings)
        .unwrap_err()
        .to_string();

    assert!(error.contains("Sample1"), "{error}");
    assert!(error.contains("contains no decoded spectra"), "{error}");
    assert_eq!(warnings.len(), 1);
    assert!(warnings[0].contains("file=fixture scan=5"));
    assert!(warnings[0].contains("was skipped"));
}

#[test]
fn skips_an_empty_scan_when_the_sample_has_readable_spectra() {
    let mut empty = source_spectrum();
    empty.native_id = "file=fixture scan=4".to_owned();
    empty.scan_number = 4;
    empty.mz.clear();
    empty.intensity.clear();
    let mut warnings = Vec::new();

    let streams = build_streams(
        vec![empty, source_spectrum()],
        "Sample1",
        &mut 1,
        &mut warnings,
    )
    .unwrap();

    assert_eq!(streams.len(), 1);
    assert_eq!(streams[0].spectra.len(), 1);
    assert_eq!(streams[0].spectra[0].id, SpectrumId::new(5));
    assert_eq!(warnings.len(), 1);
    assert!(warnings[0].contains("file=fixture scan=4"));
}

#[test]
fn local_wiff_fixture_imports_validated_multi_sample_layout_when_present() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(".tmp/WIFF/20250305.wiff");
    if !path.is_file() {
        return;
    }

    let loaded = load(&path).expect("local legacy WIFF fixture should import every sample");
    assert!(
        loaded.warnings.is_empty(),
        "valid empty scan headers are not import warnings"
    );
    assert_eq!(
        loaded.format,
        DataFormat::MassSpectrometry(MassSpectrometryFormat::SciexWiff)
    );
    let Acquisition::MassSpec(run) = loaded.acquisition else {
        panic!("WIFF should produce a mass-spectrometry run");
    };
    assert_eq!(run.metadata["sample count"].parse::<usize>().unwrap(), 2);
    assert_eq!(run.metadata["samples"], "yjs_10ppm #1, yjs_10ppm #2");
    assert_eq!(run.streams.len(), 22);
    assert_eq!(run.chromatograms.len(), 22);
    let sample0: usize = run.streams[..11]
        .iter()
        .flat_map(|stream| &stream.spectra)
        .filter(|spectrum| spectrum.tic > 0.0)
        .count();
    let sample1: usize = run.streams[11..]
        .iter()
        .flat_map(|stream| &stream.spectra)
        .filter(|spectrum| spectrum.tic > 0.0)
        .count();
    assert_eq!((sample0, sample1), (3141, 3072));
    assert!(
        run.streams
            .iter()
            .flat_map(|stream| &stream.spectra)
            .all(|spectrum| {
                spectrum.mz.len() == spectrum.intensity.len()
                    && spectrum.retention_time_min.is_finite()
                    && spectrum.mz.iter().all(|value| value.is_finite())
            })
    );
    let tic = &run.chromatograms[0];
    assert_eq!(tic.time_min.len(), 3905);
    assert!(tic.time_min.windows(2).all(|pair| pair[1] > pair[0]));
    assert!((tic.time_min[0] - 0.002533333333333333).abs() < 1e-9);
    assert!((tic.time_min[3904] - 13.49435).abs() < 1e-9);
    let ms1 = &run.streams[0];
    let peak = ms1
        .spectra
        .iter()
        .max_by(|left, right| left.tic.total_cmp(&right.tic))
        .unwrap();
    assert!((peak.retention_time_min - 0.9720333333333334).abs() < 1e-9);
    assert_eq!(peak.tic, 5374726.0);
    assert_eq!(loaded.provenance.companion_paths.len(), 1);
}
