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
    assert_eq!(limits.max_window_records, 1024);
    assert_eq!(limits.max_worksheets_per_workbook, 128);
    assert_eq!(limits.max_columns, 4096);
    assert_eq!(limits.max_metadata_records, 65_536);
    assert_eq!(limits.max_rows_per_column, 1_000_000);
    assert_eq!(limits.max_cells, 2_000_000);
    assert_eq!(limits.max_metadata_depth, 32);
}

#[test]
fn rejects_a_zero_metadata_record_limit() {
    let limits = OriginLimits {
        max_metadata_records: 0,
        ..OriginLimits::default()
    };
    assert!(matches!(
        read_origin(b"CPYUA 4.3668 178\n", limits),
        Err(OriginError::InvalidLimit {
            name: "max_metadata_records",
            value: 0,
            ..
        })
    ));
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

mod origin7_profile {
    use super::*;

    const SIGNATURE: &[u8] = b"CPYA 4.2673 552#\n";
    const ORIGIN_HEADER_LEN: usize = 39;
    const DATA_HEADER_LEN: usize = 123;
    const SIZE_PREFIX_LEN: usize = 5;
    const NULL_BLOCK_LEN: usize = 5;

    fn push_block(bytes: &mut Vec<u8>, payload: Option<&[u8]>) {
        let payload = payload.unwrap_or_default();
        let length = u32::try_from(payload.len()).unwrap();
        bytes.extend_from_slice(&length.to_le_bytes());
        bytes.push(b'\n');
        if !payload.is_empty() {
            bytes.extend_from_slice(payload);
            bytes.push(b'\n');
        }
    }

    fn origin_header() -> [u8; ORIGIN_HEADER_LEN] {
        let mut header = [0; ORIGIN_HEADER_LEN];
        header[0x1b..0x23].copy_from_slice(&7.0552_f64.to_le_bytes());
        header
    }

    fn data_header() -> [u8; DATA_HEADER_LEN] {
        [0; DATA_HEADER_LEN]
    }

    fn synthetic_project(contents: &[Option<&[u8]>]) -> Vec<u8> {
        let mut bytes = SIGNATURE.to_vec();
        push_block(&mut bytes, Some(&origin_header()));
        push_block(&mut bytes, None);
        for content in contents {
            push_block(&mut bytes, Some(&data_header()));
            push_block(&mut bytes, *content);
            push_block(&mut bytes, None);
        }
        push_block(&mut bytes, None);
        bytes
    }

    fn first_data_header_offset() -> usize {
        SIGNATURE.len() + SIZE_PREFIX_LEN + ORIGIN_HEADER_LEN + 1 + NULL_BLOCK_LEN
    }

    #[test]
    fn accepts_exact_header_and_empty_data_list_framing() {
        let bytes = synthetic_project(&[]);
        let probe = probe_origin(&bytes).unwrap();
        assert_eq!(probe.profile, Some(OriginProfile::Origin7V552));
        assert!(matches!(
            read_origin(&bytes, OriginLimits::default()),
            Err(OriginError::NoSupportedWorksheet)
        ));
    }

    #[test]
    fn framed_data_without_required_metadata_is_truncated() {
        let bytes = synthetic_project(&[Some(b"values"), None]);
        assert!(matches!(
            read_origin(&bytes, OriginLimits::default()),
            Err(OriginError::Truncated { .. })
        ));
    }

    #[test]
    fn rejects_other_producer_version() {
        let mut bytes = synthetic_project(&[]);
        bytes[14] = b'1';
        assert!(matches!(
            read_origin(&bytes, OriginLimits::default()),
            Err(OriginError::UnsupportedVersion { .. })
        ));
    }

    #[test]
    fn rejects_wrong_origin_header_length() {
        let mut bytes = synthetic_project(&[]);
        bytes[SIGNATURE.len()..SIGNATURE.len() + 4]
            .copy_from_slice(&(ORIGIN_HEADER_LEN as u32 - 1).to_le_bytes());
        let final_header_byte = SIGNATURE.len() + SIZE_PREFIX_LEN + ORIGIN_HEADER_LEN - 1;
        bytes.remove(final_header_byte);
        assert!(matches!(
            read_origin(&bytes, OriginLimits::default()),
            Err(OriginError::CorruptStructure {
                offset,
                ..
            }) if offset == SIGNATURE.len()
        ));
    }

    #[test]
    fn rejects_wrong_embedded_origin_version() {
        let mut bytes = synthetic_project(&[]);
        let version_offset = SIGNATURE.len() + SIZE_PREFIX_LEN + 0x1b;
        bytes[version_offset..version_offset + 8].copy_from_slice(&7.0551_f64.to_le_bytes());
        assert!(matches!(
            read_origin(&bytes, OriginLimits::default()),
            Err(OriginError::UnsupportedVersion { .. })
        ));
    }

    #[test]
    fn rejects_bad_origin_header_size_delimiter_with_offset() {
        let mut bytes = synthetic_project(&[]);
        let delimiter = SIGNATURE.len() + 4;
        bytes[delimiter] = b'!';
        assert!(matches!(
            read_origin(&bytes, OriginLimits::default()),
            Err(OriginError::CorruptStructure { offset, .. }) if offset == delimiter
        ));
    }

    #[test]
    fn rejects_bad_origin_header_payload_delimiter_with_offset() {
        let mut bytes = synthetic_project(&[]);
        let delimiter = SIGNATURE.len() + SIZE_PREFIX_LEN + ORIGIN_HEADER_LEN;
        bytes[delimiter] = b'!';
        assert!(matches!(
            read_origin(&bytes, OriginLimits::default()),
            Err(OriginError::CorruptStructure { offset, .. }) if offset == delimiter
        ));
    }

    #[test]
    fn rejects_declared_oversized_block_before_payload_access() {
        let mut bytes = synthetic_project(&[]);
        let limits = OriginLimits {
            max_block_bytes: ORIGIN_HEADER_LEN,
            ..OriginLimits::default()
        };
        bytes[SIGNATURE.len()..SIGNATURE.len() + 4]
            .copy_from_slice(&(ORIGIN_HEADER_LEN as u32 + 1).to_le_bytes());
        assert!(matches!(
            read_origin(&bytes, limits),
            Err(OriginError::LimitExceeded {
                resource: "block bytes",
                limit: ORIGIN_HEADER_LEN,
                actual,
            }) if actual == ORIGIN_HEADER_LEN + 1
        ));
    }

    #[test]
    fn rejects_missing_origin_header_null_block() {
        let mut bytes = synthetic_project(&[Some(b"value")]);
        let header_null = first_data_header_offset() - NULL_BLOCK_LEN;
        bytes.drain(header_null..header_null + NULL_BLOCK_LEN);
        assert!(matches!(
            read_origin(&bytes, OriginLimits::default()),
            Err(OriginError::CorruptStructure { offset, .. }) if offset == header_null
        ));
    }

    #[test]
    fn rejects_wrong_data_header_length() {
        let mut bytes = synthetic_project(&[Some(b"value")]);
        let data_header = first_data_header_offset();
        bytes[data_header..data_header + 4]
            .copy_from_slice(&(DATA_HEADER_LEN as u32 - 1).to_le_bytes());
        bytes.remove(data_header + SIZE_PREFIX_LEN + DATA_HEADER_LEN - 1);
        assert!(matches!(
            read_origin(&bytes, OriginLimits::default()),
            Err(OriginError::CorruptStructure { offset, .. }) if offset == data_header
        ));
    }

    #[test]
    fn rejects_missing_data_content_block_as_truncated() {
        let mut bytes = synthetic_project(&[Some(b"value")]);
        let content = first_data_header_offset() + SIZE_PREFIX_LEN + DATA_HEADER_LEN + 1;
        let content_len = SIZE_PREFIX_LEN + b"value".len() + 1;
        bytes.drain(content..content + content_len);
        assert!(matches!(
            read_origin(&bytes, OriginLimits::default()),
            Err(OriginError::Truncated { .. })
        ));
    }

    #[test]
    fn rejects_missing_per_section_null_as_truncated() {
        let mut bytes = synthetic_project(&[Some(b"value")]);
        let section_null = first_data_header_offset()
            + SIZE_PREFIX_LEN
            + DATA_HEADER_LEN
            + 1
            + SIZE_PREFIX_LEN
            + b"value".len()
            + 1;
        bytes.drain(section_null..section_null + NULL_BLOCK_LEN);
        assert!(matches!(
            read_origin(&bytes, OriginLimits::default()),
            Err(OriginError::Truncated { .. })
        ));
    }

    #[test]
    fn rejects_missing_data_list_terminator_as_truncated() {
        let mut bytes = synthetic_project(&[]);
        bytes.truncate(bytes.len() - NULL_BLOCK_LEN);
        assert!(matches!(
            read_origin(&bytes, OriginLimits::default()),
            Err(OriginError::Truncated { .. })
        ));
    }

    #[test]
    fn rejects_the_4097th_raw_data_section() {
        let contents = vec![None::<&[u8]>; 4097];
        let bytes = synthetic_project(&contents);

        assert!(matches!(
            read_origin(&bytes, OriginLimits::default()),
            Err(OriginError::LimitExceeded {
                resource: "data sections",
                limit: 4096,
                actual: 4097,
            })
        ));
    }

    #[test]
    fn every_truncated_project_prefix_returns_a_structured_error() {
        let complete = synthetic_project(&[]);
        for prefix_len in 0..complete.len() {
            let result = std::panic::catch_unwind(|| {
                read_origin(&complete[..prefix_len], OriginLimits::default())
            });
            assert!(result.is_ok(), "prefix {prefix_len} panicked");
            assert!(matches!(
                result.unwrap(),
                Err(OriginError::Truncated { .. })
            ));
        }
    }
}
