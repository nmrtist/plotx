use nmr::{
    DatasetKind, ExecutionContext,
    axis::{AxisDomain, AxisUnit},
    raw::GroupDelayState,
};
use plotx_io::nmr_bridge;
use std::{path::PathBuf, sync::Arc};

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/nmr")
        .join(name)
}

fn read(name: &str) -> Arc<nmr::Dataset> {
    nmr_bridge::read(&fixture(name), &mut ExecutionContext::default()).unwrap()
}

#[test]
fn directory_prefers_raw_but_processed_file_selection_is_respected() {
    let raw = read("bruker-1d");
    assert_eq!(raw.kind(), DatasetKind::Raw);
    let direct = &raw.as_raw().unwrap().descriptor().axes()[0];
    assert!(
        matches!(direct.group_delay(), GroupDelayState::Pending(delay) if delay.delay_points() == 0.0)
    );
    assert_eq!(
        raw.as_raw().unwrap().read_trace(&[]).unwrap().samples(),
        &[nmr::Complex64::new(1.0, 2.0), nmr::Complex64::new(3.0, 4.0)]
    );
    let processed = read("bruker-1d/pdata/1/1r");
    assert_eq!(processed.kind(), DatasetKind::Processed);
    let processed = processed.as_processed().unwrap();
    assert_eq!(processed.descriptor().component_counts(), [1]);
    assert_eq!(processed.data().samples(), [2.0, 4.0, 6.0, 8.0]);
    assert_eq!(
        processed.descriptor().axes()[0]
            .coordinate_iter()
            .unwrap()
            .collect::<Vec<_>>(),
        [10.0, 7.5, 5.0, 2.5]
    );
}

#[test]
fn jcamp_keeps_explicit_coordinates_scale_and_scalar_descriptor() {
    let input = read("jcamp-hz.dx");
    let processed = input.as_processed().unwrap();
    let axis = &processed.descriptor().axes()[0];
    assert_eq!(axis.domain(), AxisDomain::Frequency);
    assert_eq!(axis.unit(), Some(AxisUnit::Hertz));
    assert_eq!(axis.component_count(), 1);
    assert_eq!(
        axis.coordinate_iter().unwrap().collect::<Vec<_>>(),
        [4.0, 3.0, 2.0, 1.0]
    );
    assert_eq!(processed.data().samples(), [2.0, 4.0, 6.0, 8.0]);
    assert_eq!(
        nmr_bridge::provenance(&input).unwrap().selected_path,
        fixture("jcamp-hz.dx")
    );
    assert_eq!(
        nmr_bridge::identity(&input).subject.as_deref(),
        input.identity().subject()
    );
    assert_eq!(nmr_bridge::identity(&input).source_label, "jcamp-hz");
}

#[test]
fn ppm_jcamp_keeps_coordinates_without_inventing_observe_frequency() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("no-observe.dx");
    let text = std::fs::read_to_string(fixture("jcamp-ppm.dx")).unwrap();
    let text = text
        .lines()
        .filter(|line| !line.starts_with("##.OBSERVE"))
        .collect::<Vec<_>>()
        .join("\n");
    std::fs::write(&path, text).unwrap();
    let input = nmr_bridge::read(&path, &mut ExecutionContext::default()).unwrap();
    let processed = input.as_processed().unwrap();
    let axis = &processed.descriptor().axes()[0];
    assert_eq!(axis.unit(), Some(AxisUnit::Ppm));
    assert_eq!(
        axis.frequency_evidence()
            .and_then(|e| e.observe_frequency_mhz()),
        None
    );
    assert_eq!(axis.nucleus(), None);
    assert_eq!(
        axis.coordinate_iter().unwrap().collect::<Vec<_>>(),
        [4.0, 3.0, 2.0, 1.0]
    );
    assert_eq!(processed.descriptor().component_counts(), [1]);
    assert_eq!(processed.data().samples(), [2.0, 4.0, 6.0, 8.0]);
}

#[test]
fn states_tppi_retains_component_lanes() {
    let input = read("bruker-states-tppi");
    let raw = input.as_raw().unwrap();
    assert_eq!(raw.descriptor().logical_shape(), [2, 2]);
    assert_eq!(raw.descriptor().component_lanes(), [2, 1]);
    let trace = raw.read_trace(&[1]).unwrap();
    assert_eq!(
        trace.samples(),
        &[
            nmr::Complex64::new(9., 10.),
            nmr::Complex64::new(11., 12.),
            nmr::Complex64::new(13., 14.),
            nmr::Complex64::new(15., 16.)
        ]
    );
}

#[test]
fn states_decoding_keeps_both_lanes_without_tppi_modulation() {
    use nmr::processing::{ProcessingOperation, ProcessingPlan};
    let input = read("bruker-states");
    let output = ProcessingPlan::new(vec![ProcessingOperation::ComponentTransform { axis: 0 }])
        .unwrap()
        .apply(&input)
        .unwrap();
    let data = output.as_dense_processed().unwrap();
    assert_eq!(data.component_counts(), [2, 2]);
    for row in 0..2 {
        for lane in 0..2 {
            for col in 0..2 {
                for channel in 0..2 {
                    let expected = (1 + row * 8 + lane * 4 + col * 2 + channel) as f64;
                    assert_eq!(data.get(&[row, col], &[lane, channel]).unwrap(), expected);
                }
            }
        }
    }
}

#[test]
fn jeol_policy_does_not_alert_and_zero_imaginary_values_remain_complex() {
    let input = read("jeol-complex.jdf");
    assert!(input.warnings().iter().any(|warning| matches!(
        warning,
        nmr::ReadWarning::ExperimentalVendorSemantics { .. }
    )));
    assert!(
        !nmr_bridge::warnings(&input)
            .iter()
            .any(|warning| warning.message.contains("ExperimentalVendorSemantics"))
    );
    let raw = input.as_raw().unwrap();
    assert!(matches!(
        raw.descriptor().axes()[0].kind(),
        nmr::raw::RawAxisKind::Direct(nmr::raw::DirectSamples::Complex)
    ));
    assert!(
        raw.read_trace(&[])
            .unwrap()
            .samples()
            .iter()
            .all(|sample| sample.im == 0.0)
    );
}

#[test]
fn snapshot_restores_after_source_removal_with_exact_context_and_corruption_checks() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("source.dx");
    std::fs::copy(fixture("jcamp-hz.dx"), &path).unwrap();
    let input = nmr_bridge::read(&path, &mut ExecutionContext::default()).unwrap();
    let limits = nmr::snapshot::SnapshotLimits::default();
    let mut bytes = Vec::new();
    nmr_bridge::snapshot::write(&input, &mut bytes, limits, &mut ExecutionContext::default())
        .unwrap();
    std::fs::remove_file(path).unwrap();
    let restored = nmr_bridge::snapshot::read(
        &mut bytes.as_slice(),
        limits,
        &mut ExecutionContext::default(),
    )
    .unwrap();
    assert_eq!(restored.canonical_digests(), input.canonical_digests());
    assert_eq!(restored.warnings(), input.warnings());
    assert_eq!(restored.sources(), input.sources());
    assert_eq!(restored.selected_path(), input.selected_path());
    assert!(restored.metadata().accepted_archive());
    let mut trailing = bytes.clone();
    trailing.push(0);
    assert!(
        nmr_bridge::snapshot::read(
            &mut trailing.as_slice(),
            limits,
            &mut ExecutionContext::default()
        )
        .is_err()
    );
    bytes[30] ^= 1;
    assert!(
        nmr_bridge::snapshot::read(
            &mut bytes.as_slice(),
            limits,
            &mut ExecutionContext::default()
        )
        .is_err()
    );
}

#[test]
fn cancelled_read_and_snapshot_budget_are_reported() {
    let token = nmr::CancellationToken::new();
    token.cancel();
    let error = nmr_bridge::read(
        &fixture("bruker-1d"),
        &mut ExecutionContext::default().with_cancellation(token),
    )
    .unwrap_err();
    assert!(
        matches!(error, plotx_io::IoError::Nmr(error) if error.kind() == nmr::ReadErrorKind::Cancelled)
    );
    let limits = nmr::snapshot::SnapshotLimits {
        max_bytes: 32,
        ..Default::default()
    };
    assert!(
        nmr_bridge::snapshot::write(
            &read("bruker-1d"),
            &mut Vec::new(),
            limits,
            &mut ExecutionContext::default()
        )
        .is_err()
    );
}

#[test]
fn varian_keeps_scaling_sign_and_rejects_the_old_simplified_test_header() {
    let input = read("varian.fid");
    assert_eq!(
        input.as_raw().unwrap().read_trace(&[]).unwrap().samples(),
        &[nmr::Complex64::new(2., -4.), nmr::Complex64::new(6., -8.)]
    );
    assert!(
        matches!(nmr_bridge::read(&fixture("varian-short-header.fid"), &mut ExecutionContext::default()),
        Err(plotx_io::IoError::Nmr(error)) if error.kind() == nmr::ReadErrorKind::InvalidMetadata)
    );
}

#[test]
fn sparse_snapshot_keeps_observation_order_and_missing_points() {
    let input = read("bruker-nus");
    assert!(
        !nmr_bridge::warnings(&input)
            .iter()
            .any(|warning| warning.message.contains("ExperimentalVendorSemantics"))
    );
    let limits = nmr::snapshot::SnapshotLimits::default();
    let mut bytes = Vec::new();
    nmr_bridge::snapshot::write(&input, &mut bytes, limits, &mut ExecutionContext::default())
        .unwrap();
    let restored = nmr_bridge::snapshot::read(
        &mut bytes.as_slice(),
        limits,
        &mut ExecutionContext::default(),
    )
    .unwrap();
    assert_eq!(restored.canonical_digests(), input.canonical_digests());
    let raw = restored.as_raw().unwrap();
    assert!(raw.data().is_sparse());
    assert_eq!(
        raw.sampling_schedule()
            .unwrap()
            .coordinates()
            .iter()
            .map(|coordinate| coordinate.as_slice()[0])
            .collect::<Vec<_>>(),
        [3, 1]
    );
    assert_eq!(
        raw.read_trace(&[0]).unwrap_err().kind(),
        nmr::ReadErrorKind::UnsampledCoordinate
    );
    assert_eq!(
        raw.read_trace(&[3]).unwrap().samples()[0],
        nmr::Complex64::new(1., 2.)
    );
}

#[test]
fn ambiguous_processed_directories_and_truncated_payloads_are_errors() {
    let temp = tempfile::tempdir().unwrap();
    for number in [1, 2] {
        let path = temp.path().join("pdata").join(number.to_string());
        std::fs::create_dir_all(&path).unwrap();
        for name in ["procs", "1r"] {
            std::fs::copy(
                fixture(&format!("bruker-1d/pdata/1/{name}")),
                path.join(name),
            )
            .unwrap();
        }
    }
    assert!(
        matches!(nmr_bridge::read(temp.path(), &mut ExecutionContext::default()), Err(plotx_io::IoError::Nmr(error))
        if error.kind() == nmr::ReadErrorKind::Ambiguous)
    );
    let path = temp.path().join("pdata/1/1r");
    std::fs::write(&path, [0u8; 3]).unwrap();
    assert!(nmr_bridge::read(&path, &mut ExecutionContext::default()).is_err());
}

#[test]
fn raw_snapshot_reprocesses_without_vendor_files_and_preserves_processed_history() {
    let temp = tempfile::tempdir().unwrap();
    for name in ["acqus", "fid"] {
        std::fs::copy(
            fixture(&format!("bruker-1d/{name}")),
            temp.path().join(name),
        )
        .unwrap();
    }
    let input = nmr_bridge::read(temp.path(), &mut ExecutionContext::default()).unwrap();
    let plan = nmr::processing::ProcessingPlan::new(vec![
        nmr::processing::ProcessingOperation::FourierTransform {
            axis: 0,
            transform: nmr::processing::FourierTransform::default(),
        },
    ])
    .unwrap();
    let expected = plan.apply(&input).unwrap();
    let limits = nmr::snapshot::SnapshotLimits::default();
    let mut bytes = Vec::new();
    nmr_bridge::snapshot::write(&input, &mut bytes, limits, &mut ExecutionContext::default())
        .unwrap();
    temp.close().unwrap();
    let restored = nmr_bridge::snapshot::read(
        &mut bytes.as_slice(),
        limits,
        &mut ExecutionContext::default(),
    )
    .unwrap();
    let processed = plan.apply(&restored).unwrap();
    assert_eq!(processed.canonical_digests(), expected.canonical_digests());
    assert_eq!(
        processed.as_dense_processed(),
        expected.as_dense_processed()
    );
    bytes.clear();
    nmr_bridge::snapshot::write(
        &processed,
        &mut bytes,
        limits,
        &mut ExecutionContext::default(),
    )
    .unwrap();
    let archived = nmr_bridge::snapshot::read(
        &mut bytes.as_slice(),
        limits,
        &mut ExecutionContext::default(),
    )
    .unwrap();
    let history = archived
        .as_processed()
        .unwrap()
        .provenance()
        .history()
        .unwrap();
    let replay = history
        .replay_raw(
            restored.as_raw().unwrap(),
            nmr::processing::ProcessingOptions::new(),
        )
        .unwrap();
    assert_eq!(replay.data(), processed.as_dense_processed().unwrap());
}

#[test]
fn supported_jcamp_versions_preserve_ppm_coordinates_and_scaling() {
    let dir = tempfile::tempdir().unwrap();
    let text = std::fs::read_to_string(fixture("jcamp-ppm.dx")).unwrap();
    for version in ["5.00", "5.01"] {
        let path = dir.path().join(format!("v{version}.dx"));
        std::fs::write(
            &path,
            text.replace("##JCAMP-DX=5.00", &format!("##JCAMP-DX={version}")),
        )
        .unwrap();
        let source = nmr_bridge::read(&path, &mut ExecutionContext::default()).unwrap();
        let processed = source.as_processed().unwrap();
        assert_eq!(
            processed.descriptor().axes()[0]
                .coordinate_iter()
                .unwrap()
                .collect::<Vec<_>>(),
            [4.0, 3.0, 2.0, 1.0]
        );
        assert_eq!(processed.data().samples(), [2.0, 4.0, 6.0, 8.0]);
    }
}
