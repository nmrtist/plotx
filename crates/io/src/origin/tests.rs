use super::*;

#[test]
fn probes_opj_and_opju_by_content() {
    let opj = probe_origin(b"CPYA 4.2673 552#\n").unwrap();
    assert_eq!(opj.format, OriginFormat::Opj);
    assert_eq!(opj.profile, Some(OriginProfile::Origin7V552));
    assert_eq!(opj.support, OriginSupport::Supported);

    let opju = probe_origin(b"CPYUA 4.3668 178\n").unwrap();
    assert_eq!(opju.format, OriginFormat::Opju);
    assert_eq!(opju.profile, None);
    assert_eq!(opju.support, OriginSupport::RecognizedUnsupported);
}

#[test]
fn parses_header_components_without_floating_point() {
    let opj = probe_origin(b"CPYA 4.2673 552#\n").unwrap();
    assert_eq!(opj.raw_version, "4.2673 552");
    assert_eq!(opj.version.major, 4);
    assert_eq!(opj.version.minor, 2673);
    assert_eq!(opj.version.build, 552);
    assert_eq!(opj.byte_order, OriginByteOrder::LittleEndian);

    let opju = probe_origin(b"CPYUA 4.3668 178\n").unwrap();
    assert_eq!(opju.raw_version, "4.3668 178");
    assert_eq!(opju.version.major, 4);
    assert_eq!(opju.version.minor, 3668);
    assert_eq!(opju.version.build, 178);
    assert_eq!(opju.byte_order, OriginByteOrder::LittleEndian);
}

#[test]
fn opju_is_recognized_but_not_partially_imported() {
    let error = read_origin(b"CPYUA 4.3668 178\nrest", OriginLimits::default()).unwrap_err();
    assert!(matches!(error, OriginError::UnsupportedOpjuVariant { .. }));
}

#[test]
fn rejects_unknown_or_truncated_headers() {
    assert!(matches!(
        probe_origin(b"CP"),
        Err(OriginError::Truncated { .. })
    ));
    assert!(matches!(
        probe_origin(b"not an origin file"),
        Err(OriginError::UnrecognizedFormat)
    ));
}

#[test]
fn rejects_malformed_classic_version_lines() {
    for bytes in [
        b"CPYA 4.2673#\n".as_slice(),
        b"CPYA 4.x 552#\n".as_slice(),
        b"CPYA 4.2673 build#\n".as_slice(),
        b"CPYA 4.2673 552\n".as_slice(),
        b"CPYA 4.2673 552##\n".as_slice(),
        b"CPYA 4.2673 552#\r\n".as_slice(),
    ] {
        assert!(matches!(
            probe_origin(bytes),
            Err(OriginError::MalformedHeader { .. })
        ));
    }
}

#[test]
fn rejects_headers_over_the_default_limit() {
    let mut bytes = b"CPYA ".to_vec();
    bytes.resize(129, b'1');
    bytes.push(b'\n');

    assert!(matches!(
        probe_origin(&bytes),
        Err(OriginError::HeaderTooLong { limit: 128 })
    ));
}

#[test]
fn rejects_input_one_byte_over_a_custom_limit() {
    let bytes = b"CPYUA 4.3668 178\n";
    let limit = bytes.len() - 1;
    let limits = OriginLimits {
        max_input_bytes: limit,
        ..OriginLimits::default()
    };

    assert!(matches!(
        read_origin(bytes, limits),
        Err(OriginError::LimitExceeded {
            resource: "input bytes",
            limit: found_limit,
            actual,
        }) if found_limit == limit && actual == bytes.len()
    ));
}

#[test]
fn opju_requires_the_exact_verified_header_grammar() {
    for bytes in [
        b"CPYUA  178\n".as_slice(),
        b"CPYUA 4.3668\n".as_slice(),
        b"CPYUA 4.3668 178#\n".as_slice(),
        b"CPYUA four.3668 178\n".as_slice(),
        b"CPYUA 4.minor 178\n".as_slice(),
        b"CPYUA 4.3668 build\n".as_slice(),
    ] {
        assert!(matches!(
            probe_origin(bytes),
            Err(OriginError::MalformedHeader { .. })
        ));
    }
}

#[test]
fn unsupported_classic_versions_are_not_claimed_as_supported() {
    assert!(matches!(
        probe_origin(b"CPYA 4.2673 551#\n"),
        Err(OriginError::UnsupportedVersion { .. })
    ));
}

#[test]
fn default_limits_match_the_public_contract() {
    let limits = OriginLimits::default();
    assert_eq!(limits.max_input_bytes, 128 * 1024 * 1024);
    assert_eq!(limits.max_header_bytes, 128);
    assert_eq!(limits.max_block_bytes, 32 * 1024 * 1024);
    assert_eq!(limits.max_string_bytes, 1024 * 1024);
    assert_eq!(limits.max_decoded_text_bytes, 32 * 1024 * 1024);
    assert_eq!(limits.max_parser_bytes, 128 * 1024 * 1024);
    assert_eq!(limits.max_total_owned_bytes, 384 * 1024 * 1024);
    assert_eq!(limits.max_workbooks, 256);
    assert_eq!(limits.max_worksheets_per_workbook, 128);
    assert_eq!(limits.max_columns, 4096);
    assert_eq!(limits.max_rows_per_column, 1_000_000);
    assert_eq!(limits.max_cells, 2_000_000);
    assert_eq!(limits.max_metadata_depth, 32);
}

#[test]
fn invalid_custom_limits_return_an_error_without_panicking() {
    let limits = OriginLimits {
        max_header_bytes: 0,
        ..OriginLimits::default()
    };
    let result = std::panic::catch_unwind(|| read_origin(b"CPYUA 4.3668 178\n", limits));

    assert!(result.is_ok());
    assert!(matches!(
        result.unwrap(),
        Err(OriginError::InvalidLimit {
            name: "max_header_bytes",
            ..
        })
    ));
}

#[test]
fn project_notes_keep_distinct_names_and_content() {
    let project = OriginProject {
        probe: probe_origin(b"CPYA 4.2673 552#\n").unwrap(),
        parameters: Vec::new(),
        notes: vec![
            OriginNote {
                name: "Methods".to_owned(),
                content: "Prepared under nitrogen.".to_owned(),
            },
            OriginNote {
                name: "Observations".to_owned(),
                content: "The solution remained clear.".to_owned(),
            },
        ],
        workbooks: Vec::new(),
        diagnostics: Vec::new(),
        unsupported_objects: Vec::new(),
        resource_usage: OriginResourceUsage::default(),
    };

    assert_eq!(project.notes.len(), 2);
    assert_eq!(project.notes[0].name, "Methods");
    assert_eq!(project.notes[0].content, "Prepared under nitrogen.");
    assert_eq!(project.notes[1].name, "Observations");
    assert_eq!(project.notes[1].content, "The solution remained clear.");
}

#[test]
fn enforces_the_header_limit_at_the_lf_byte_boundary() {
    let mut accepted = b"CPYUA ".to_vec();
    accepted.extend(std::iter::repeat_n(b'0', 111));
    accepted.extend_from_slice(b"4.3668 178\n");
    assert_eq!(accepted.len(), 128);

    let probe = probe_origin(&accepted).unwrap();
    assert_eq!(probe.format, OriginFormat::Opju);
    assert_eq!(probe.version.major, 4);
    assert_eq!(probe.version.minor, 3668);
    assert_eq!(probe.version.build, 178);

    let mut too_long = b"CPYUA ".to_vec();
    too_long.extend(std::iter::repeat_n(b'0', 112));
    too_long.extend_from_slice(b"4.3668 178\n");
    assert_eq!(too_long.len(), 129);
    assert!(matches!(
        probe_origin(&too_long),
        Err(OriginError::HeaderTooLong { limit: 128 })
    ));
}

#[test]
fn rejects_complete_magic_with_an_invalid_following_byte() {
    for bytes in [
        b"CPYAB 4.2673 552#\n".as_slice(),
        b"CPYUAX 4.3668 178\n".as_slice(),
    ] {
        assert!(matches!(
            probe_origin(bytes),
            Err(OriginError::MalformedHeader { .. })
        ));
    }
}

#[test]
fn parses_numeric_maxima_and_rejects_integer_overflow() {
    let opju = probe_origin(b"CPYUA 65535.65535 4294967295\n").unwrap();
    assert_eq!(opju.version.major, u16::MAX);
    assert_eq!(opju.version.minor, u16::MAX);
    assert_eq!(opju.version.build, u32::MAX);

    assert!(matches!(
        probe_origin(b"CPYA 65535.65535 4294967295#\n"),
        Err(OriginError::UnsupportedVersion { .. })
    ));

    for bytes in [
        b"CPYUA 65536.1 1\n".as_slice(),
        b"CPYUA 1.65536 1\n".as_slice(),
        b"CPYUA 1.1 4294967296\n".as_slice(),
    ] {
        let result = std::panic::catch_unwind(|| probe_origin(bytes));
        assert!(result.is_ok());
        assert!(matches!(
            result.unwrap(),
            Err(OriginError::MalformedHeader { .. })
        ));
    }
}
