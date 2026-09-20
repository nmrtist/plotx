use nmr::{ExecutionContext, axis::AxisUnit};
use plotx_io::{nmr_bridge, nmr_view::NmrSource};
use std::path::PathBuf;

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/nmr")
        .join(name)
}

#[test]
fn ppm_view_preserves_missing_metadata_and_scalar_components() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("spectrum.dx");
    let text = std::fs::read_to_string(fixture("jcamp-ppm.dx")).unwrap();
    let text = text
        .lines()
        .filter(|line| !line.starts_with("##.OBSERVE"))
        .collect::<Vec<_>>()
        .join("\n");
    std::fs::write(&path, text).unwrap();
    let source =
        NmrSource::new(nmr_bridge::read(&path, &mut ExecutionContext::default()).unwrap()).unwrap();
    assert_eq!(source.axes()[0].observe_frequency_mhz(), None);
    assert_eq!(source.axes()[0].unit, Some(AxisUnit::Ppm));
    assert_eq!(
        source.axes()[0].coordinate_values().unwrap(),
        [4., 3., 2., 1.]
    );
    assert!(!source.has_imaginary(0));
    assert_eq!(
        source
            .trace()
            .unwrap()
            .iter()
            .map(|v| v.re)
            .collect::<Vec<_>>(),
        [2., 4., 6., 8.]
    );
    assert!(source.craft_fid().is_err());
}

#[test]
fn craft_view_requires_delay_evidence_and_keeps_explicit_zero() {
    let dir = tempfile::tempdir().unwrap();
    let text = std::fs::read_to_string(fixture("bruker-1d/acqus")).unwrap();
    std::fs::copy(fixture("bruker-1d/fid"), dir.path().join("fid")).unwrap();
    std::fs::write(dir.path().join("acqus"), &text).unwrap();
    let source =
        NmrSource::new(nmr_bridge::read(dir.path(), &mut ExecutionContext::default()).unwrap())
            .unwrap();
    let fid = source.craft_fid().unwrap();
    assert_eq!(fid.group_delay, 0.0);
    assert_eq!(fid.observe_freq_mhz, 400.0);
    assert_eq!(fid.points, source.trace().unwrap());
    let text = text
        .lines()
        .filter(|line| !line.starts_with("##$GRPDLY"))
        .collect::<Vec<_>>()
        .join("\n");
    std::fs::write(dir.path().join("acqus"), text).unwrap();
    let source =
        NmrSource::new(nmr_bridge::read(dir.path(), &mut ExecutionContext::default()).unwrap())
            .unwrap();
    assert!(
        source
            .craft_fid()
            .unwrap_err()
            .to_string()
            .contains("delay evidence")
    );
}

#[test]
fn all_zero_imaginary_channel_is_still_present() {
    let source = NmrSource::new(
        nmr_bridge::read(
            &fixture("jeol-complex.jdf"),
            &mut ExecutionContext::default(),
        )
        .unwrap(),
    )
    .unwrap();
    assert!(source.has_imaginary(0));
    assert!(source.trace().unwrap().iter().all(|value| value.im == 0.0));
}

#[test]
fn reference_frequency_survives_binning_without_becoming_observe_frequency() {
    use nmr::axis::{AxisCoordinates, AxisDomain, AxisRole, FrequencyEvidence};
    use nmr::processed::{
        ComponentBasis, ProcessedAxis, ProcessedDataset, ProcessedOrigin, ProcessedProvenance,
    };
    use nmr::processing::{
        BinAggregation, FrequencyFrame, ProcessingOperation as Op, ProcessingPlan, ReferenceSource,
        SpectrumOperation,
    };
    let axis = ProcessedAxis::new(
        AxisRole::Signal,
        AxisDomain::Frequency,
        Some(AxisUnit::Hertz),
        8,
        AxisCoordinates::Uniform {
            start: -4.0,
            step: 1.0,
        },
        ComponentBasis::Cartesian,
    )
    .unwrap()
    .with_frequency_evidence(Some(FrequencyEvidence::new(Some(500.005), None).unwrap()))
    .unwrap();
    let input = ProcessedDataset::from_complex_trace(
        axis,
        vec![nmr::Complex64::new(1.0, 2.0); 8],
        ProcessedProvenance::new(ProcessedOrigin::Unknown, vec![]).unwrap(),
    )
    .unwrap();
    let output = ProcessingPlan::new(vec![
        Op::ResolveFrequencyFrame {
            axis: 0,
            frame: FrequencyFrame::Ppm(ReferenceSource::Explicit(
                nmr::raw::ChemicalShiftReference::user_constructed(10.0, 500.0).unwrap(),
            )),
        },
        Op::Spectrum {
            axis: 0,
            operation: SpectrumOperation::Bin {
                width: 0.004,
                aggregation: BinAggregation::Mean,
            },
        },
    ])
    .unwrap()
    .apply(&input.into())
    .unwrap();
    let source = NmrSource::new(std::sync::Arc::new(output)).unwrap();
    assert_eq!(source.axes()[0].observe_frequency_mhz(), Some(500.005));
    assert_eq!(source.reference_frequency_mhz(0), Some(500.0));
    assert_eq!(source.len(), 4);
    let coordinates = source.axes()[0].coordinate_values().unwrap();
    assert!((coordinates[1] - coordinates[0] - 0.004).abs() < 1e-12);
    let mut bytes = Vec::new();
    nmr_bridge::snapshot::write(
        source.dataset(),
        &mut bytes,
        Default::default(),
        &mut ExecutionContext::default(),
    )
    .unwrap();
    let restored = NmrSource::new(
        nmr_bridge::snapshot::read(
            &mut bytes.as_slice(),
            Default::default(),
            &mut ExecutionContext::default(),
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(restored.reference_frequency_mhz(0), Some(500.0));
    assert_eq!(restored.axes()[0].observe_frequency_mhz(), Some(500.005));
    assert_eq!(restored.axes()[0].coordinate_values().unwrap(), coordinates);
}

#[test]
fn processed_sf_is_reference_evidence_without_observe_carrier_or_filter_claims() {
    let source = NmrSource::new(
        nmr_bridge::read(
            &fixture("bruker-1d/pdata/1/1r"),
            &mut ExecutionContext::default(),
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(source.reference_frequency_mhz(0), Some(400.0));
    assert_eq!(source.axes()[0].observe_frequency_mhz(), None);
    assert_eq!(source.reference_frequency_mhz(1), None);
    let evidence = source
        .dataset()
        .as_processed()
        .unwrap()
        .axis_evidence(0)
        .unwrap();
    assert!(evidence.reference_evidence().is_some());
    assert!(evidence.chemical_shift_reference().is_none());
    assert_eq!(
        evidence.group_delay(),
        nmr::processed::ProcessedGroupDelay::Unknown
    );
    assert_eq!(
        source.axes()[0].coordinate_values().unwrap(),
        [10.0, 7.5, 5.0, 2.5]
    );
}
