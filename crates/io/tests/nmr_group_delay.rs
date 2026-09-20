//! Original valid vendor-delay test inputs through the unified public reader.
use nmr::raw::GroupDelayState;
use plotx_io::nmr_bridge;
use std::{path::PathBuf, sync::Arc};

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/nmr")
        .join(name)
}

fn delay(input: &nmr::Dataset) -> f64 {
    match input.as_raw().unwrap().descriptor().axes()[0].group_delay() {
        GroupDelayState::Pending(value) => value.delay_points(),
        other => panic!("expected known delay, got {other:?}"),
    }
}

#[test]
fn group_delay_prefers_the_original_explicit_grpdly() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::copy(fixture("bruker-1d/fid"), dir.path().join("fid")).unwrap();
    let text = std::fs::read_to_string(fixture("bruker-1d/acqus"))
        .unwrap()
        .replace(
            "##$GRPDLY= 0",
            "##$GRPDLY= 67.98\n##$DSPFVS= 21\n##$DECIM= 2080",
        );
    std::fs::write(dir.path().join("acqus"), text).unwrap();
    let input = nmr_bridge::read(dir.path(), &mut nmr::ExecutionContext::default()).unwrap();
    assert!((delay(&input) - 67.98).abs() < 1e-9);
}

#[test]
fn group_delay_falls_back_to_table() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::copy(fixture("bruker-1d/fid"), dir.path().join("fid")).unwrap();
    let text = std::fs::read_to_string(fixture("bruker-1d/acqus"))
        .unwrap()
        .replace("##$GRPDLY= 0", "##$GRPDLY= -1\n##$DSPFVS= 12\n##$DECIM= 16");
    std::fs::write(dir.path().join("acqus"), text).unwrap();
    let input = nmr_bridge::read(dir.path(), &mut nmr::ExecutionContext::default()).unwrap();
    // Retain the original Bruker parser test's input and numerical contract.
    // The reader owns hardware lookup; PlotX must not implement a second table.
    let actual = input.as_raw().unwrap().descriptor().axes()[0].group_delay();
    assert!(
        matches!(actual, GroupDelayState::Pending(value) if (value.delay_points() - 71.625).abs() < 1e-9),
        "GRPDLY=-1 / DSPFVS=12 / DECIM=16 must resolve to 71.625 points; got {actual:?}"
    );
}

fn jeol_filter(parameters: &[(&str, &str)]) -> Arc<nmr::Dataset> {
    let original = std::fs::read(fixture("jeol-complex.jdf")).unwrap();
    let old_start = u32::from_be_bytes(original[1284..1288].try_into().unwrap()) as usize;
    let parameter_bytes = 16 + 64 * parameters.len();
    let data_start = 1360 + parameter_bytes;
    let mut bytes = vec![0; data_start];
    bytes[..1360].copy_from_slice(&original[..1360]);
    bytes[1212..1216].copy_from_slice(&1360u32.to_be_bytes());
    bytes[1216..1220].copy_from_slice(&(parameter_bytes as u32).to_be_bytes());
    bytes[1284..1288].copy_from_slice(&(data_start as u32).to_be_bytes());
    for (offset, value) in [
        (1360, 64),
        (1364, 0),
        (1368, parameters.len()),
        (1372, parameter_bytes),
    ] {
        bytes[offset..offset + 4].copy_from_slice(&(value as u32).to_le_bytes());
    }
    for (index, (name, value)) in parameters.iter().enumerate() {
        let start = 1376 + 64 * index;
        bytes[start + 16..start + 16 + value.len()].copy_from_slice(value.as_bytes());
        bytes[start + 36..start + 36 + name.len()].copy_from_slice(name.as_bytes());
    }
    bytes.extend_from_slice(&original[old_start..]);
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("filter.jdf");
    std::fs::write(&path, bytes).unwrap();
    nmr_bridge::read(&path, &mut nmr::ExecutionContext::default()).unwrap()
}

#[test]
fn group_delay_from_the_original_jeol_fir_cascades() {
    for (orders, factors, expected) in [
        ("2 41 74", "6  2", 239.0 / 12.0),
        ("2 15 73", "2  2", 19.75),
    ] {
        let input = jeol_filter(&[
            ("digital_filter", "TRUE"),
            ("orders", orders),
            ("factors", factors),
        ]);
        let g = delay(&input);
        assert!((g - expected).abs() < 1e-9, "got {g}");
    }
    let disabled = jeol_filter(&[
        ("digital_filter", "FALSE"),
        ("orders", "2 41 74"),
        ("factors", "6  2"),
    ]);
    assert_eq!(
        disabled.as_raw().unwrap().descriptor().axes()[0].group_delay(),
        &GroupDelayState::NotApplicable
    );
}
