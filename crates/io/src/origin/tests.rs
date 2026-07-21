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
