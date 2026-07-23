use super::*;

fn synthetic(word: usize) -> Vec<u8> {
    let mut header = format!(
        "\\*File list\n\\Version: 0x09000000\n\\*Ciao scan list\n\\Samps/line: 2\n\\Lines: 2\n\\Scan size: 4 6 nm\n\\*Ciao image list\n\\Data offset: {{OFFSET}}\n\\Data length: {}\n\\Bytes/pixel: {word}\n\\Samps/line: 2\n\\Number of lines: 2\n\\@2:Image Data: S [Height] \"Height\"\n\\@2:Z scale: 0.5 nm/LSB\n\\Frame direction: Retrace\n\\*File list end\n\u{1a}",
        word * 4
    );
    let offset = 512;
    header = header.replace("{OFFSET}", &offset.to_string());
    let mut bytes = header.into_bytes();
    bytes.resize(offset, 0);
    for value in [1_i32, 2, 3, 4] {
        if word == 2 {
            bytes.extend_from_slice(&(value as i16).to_le_bytes());
        } else {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
    }
    bytes
}

#[test]
fn reads_16_and_32_bit_images_and_normalizes_direction() {
    for word in [2, 4] {
        let (data, warnings) = parse(
            &synthetic(word),
            Path::new("image.spm"),
            DataFormat::BrukerNanoScopeSpm,
        )
        .unwrap();
        assert!(warnings.is_empty());
        assert_eq!(&*data.images[0].raw, &[4, 3, 2, 1]);
        assert_eq!(data.images[0].scale.apply(2), 1.0);
    }
}

#[test]
fn deflection_error_raster_remains_an_image_channel() {
    let bytes = String::from_utf8_lossy(&synthetic(2))
        .replace("Height", "Deflection Error")
        .into_bytes();
    let (data, warnings) = parse(
        &bytes,
        Path::new("image.spm"),
        DataFormat::BrukerNanoScopeSpm,
    )
    .unwrap();
    assert!(warnings.is_empty());
    assert_eq!(data.images.len(), 1);
    assert_eq!(data.images[0].name, "Deflection Error");
    assert!(data.forces.is_none());
}

#[test]
fn malformed_optional_force_block_does_not_abort_valid_images() {
    let mut header = "\\*File list
\\*Ciao scan list
\\Samps/line: 2
\\Lines: 2
\\Scan size: 4 6 nm
\\*Ciao image list
\\Data offset: 768
\\Data length: 8
\\Bytes/pixel: 2
\\Samps/line: 2
\\Number of lines: 2
\\@2:Image Data: S [Height] \"Height\"
\\@2:Z scale: 1 nm/LSB
\\*Ciao force image list
\\Data offset: 776
\\Data length: 3
\\Bytes/pixel: 2
\\Samps/line: 2
\\Number of lines: 2
\\@2:Image Data: S [Deflection] \"Deflection\"
\\@2:Z scale: 1 nm/LSB
\\*File list end
\u{1a}"
        .as_bytes()
        .to_vec();
    header.resize(768, 0);
    for value in [1_i16, 2, 3, 4] {
        header.extend_from_slice(&value.to_le_bytes());
    }
    header.extend_from_slice(&[1, 2, 3]);

    let (data, warnings) = parse(
        &header,
        Path::new("mixed.spm"),
        DataFormat::BrukerNanoScopeSpm,
    )
    .unwrap();
    assert_eq!(data.images.len(), 1);
    assert!(data.forces.is_none());
    assert_eq!(warnings.len(), 1);
    assert_eq!(warnings[0].code, LoadWarningCode::OptionalChannelSkipped);
}

#[test]
fn rejects_truncated_blocks() {
    let mut bytes = synthetic(2);
    bytes.pop();
    let error = parse(
        &bytes,
        Path::new("image.spm"),
        DataFormat::BrukerNanoScopeSpm,
    )
    .unwrap_err();
    assert!(error.to_string().contains("no readable"));
}

#[test]
fn peakforce_order_is_a_permutation() {
    let (order, approach, z) = force_axis(
        512,
        DataFormat::BrukerPeakForceCapture,
        Some(2000.0),
        Some(100.0),
        0.25,
    );
    let mut sorted = order.clone();
    sorted.sort_unstable();
    assert_eq!(sorted, (0..512).collect::<Vec<_>>());
    assert_eq!(approach, 256);
    assert_eq!(z.unwrap().len(), 512);
}

#[test]
fn peakforce_sync_distance_uses_two_microsecond_steps() {
    let (order, _, _) = force_axis(
        512,
        DataFormat::BrukerPeakForceCapture,
        Some(2.0),
        Some(100.0),
        141.15,
    );
    assert_eq!(order[0], 223);
}

#[test]
fn nanoscope_micro_symbol_is_normalized() {
    assert_eq!(normalize_afm_unit("~m"), "µm");
    assert_eq!(normalize_afm_unit("um"), "µm");
}

#[test]
fn force_cycle_sentinel_is_replaced_from_neighbours() {
    let mut curve = [10, 20, i32::MIN, 40, 50];
    replace_force_sentinels(&mut curve, 5);
    assert_eq!(curve, [10, 20, 30, 40, 50]);
}

#[test]
fn consecutive_force_sentinels_are_interpolated_without_wraparound() {
    let mut curve = [10, i32::MIN, i32::MIN, 40];
    replace_force_sentinels(&mut curve, 4);
    assert_eq!(curve, [10, 20, 30, 40]);
}

#[test]
fn force_grid_is_normalized_like_image_rows() {
    let mut curves = [10, 11, 20, 21, 30, 31, 40, 41];
    normalize_force_grid(&mut curves, 2, 2, 2, AfmFrameDirection::Retrace);
    assert_eq!(curves, [40, 41, 30, 31, 20, 21, 10, 11]);
}

#[test]
fn file_list_marker_wins_over_ctrl_z_byte() {
    let bytes = b"\\*File list\n\x1a\n\\*File list end\nbinary";
    assert_eq!(
        header_end(bytes).unwrap(),
        b"\\*File list\n\x1a\n\\*File list end".len()
    );
}

#[test]
fn single_force_curve_does_not_use_scan_image_dimensions() {
    let section = Section {
        name: "Ciao force image list".to_owned(),
        entries: vec![
            Entry {
                key: "Samps/line".to_owned(),
                value: "512".to_owned(),
            },
            Entry {
                key: "Number of lines".to_owned(),
                value: "2".to_owned(),
            },
            Entry {
                key: "Z scale".to_owned(),
                value: "1 nm/LSB".to_owned(),
            },
        ],
    };
    let globals = HashMap::from([
        ("samps/line".to_owned(), "256".to_owned()),
        ("lines".to_owned(), "256".to_owned()),
    ]);
    let bytes = vec![0_u8; 1024 * 2];
    let force = parse_force(
        &bytes,
        (&section, 0, bytes.len(), 512, 2, 2),
        &globals,
        DataFormat::BrukerNanoScopeSpm,
    )
    .unwrap();
    assert_eq!([force.grid_width, force.grid_height], [1, 1]);
    assert_eq!(force.samples_per_curve, 1024);
    assert_eq!(force.approach_samples, 512);
}

#[test]
fn companion_names_ignore_peakforce_suffix_and_separators() {
    assert_eq!(
        companion::normalize_companion_stem("sample_01-PeakForceCapture"),
        "sample01peakforcecapture"
    );
    assert!(companion::common_prefix_len("sample01", "sample01all") >= 8);
}

#[test]
fn soft_scale_uses_parenthesized_physical_unit() {
    let section = Section {
        name: "Ciao image list".to_owned(),
        entries: vec![Entry {
            key: "Z scale".to_owned(),
            value: "V [Sens. DeflSens] (98.5 nm/LSB)".to_owned(),
        }],
    };
    let scale = scale(&section, &HashMap::new(), 2).unwrap();
    assert_eq!(scale.unit, "nm/LSB");
    assert!(is_physical_deflection_unit(&scale.unit));
    assert_eq!(scale.multiplier, 98.5);
}

#[test]
fn referenced_sensitivity_is_combined_with_volts_per_lsb() {
    let section = Section {
        name: "Ciao image list".to_owned(),
        entries: vec![Entry {
            key: "Z scale".to_owned(),
            value: "V [Sens. Zsens] (0.006714 V/LSB)".to_owned(),
        }],
    };
    let globals = HashMap::from([("sens. zsens".to_owned(), "V 14.979 nm/V".to_owned())]);
    let scale = scale(&section, &globals, 2).unwrap();
    assert_eq!(scale.unit, "nm");
    assert!((scale.multiplier - 0.006714 * 14.979).abs() < 1e-12);
}

#[test]
fn deflection_sensitivity_converts_nanometres_per_volt_to_si() {
    assert_eq!(calibrated_si_value("V 105.0 nm/V", "m/v"), Some(105.0e-9));
}

#[test]
fn unnumbered_ciao_sensitivity_key_drops_at_prefix() {
    let sections = parse_header(
        b"\\*Scanner list\n\\@Sens. DeflSens: V 12.91464 nm/V\n\\*File list end\n\x1a",
    );
    let globals = global_values(&sections);
    assert_eq!(
        global_value(&globals, &["Sens. DeflSens"]),
        Some("V 12.91464 nm/V")
    );
}
