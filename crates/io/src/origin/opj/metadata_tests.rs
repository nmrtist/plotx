use crate::origin::{
    OriginCell, OriginDiagnosticCode, OriginError, OriginLimits, OriginMetadataEntry, OriginNote,
    OriginResourceUsage, read_origin,
};

use std::mem::size_of;

const OPENOPJ_FIXTURE: &[u8] =
    include_bytes!("../../../tests/fixtures/origin/test-origin-7.0552.opj");

#[test]
fn imports_real_fixture_parameters_and_notes() {
    let project = read_origin(OPENOPJ_FIXTURE, OriginLimits::default())
        .expect("the licensed OpenOPJ fixture should import");

    assert!(
        project
            .parameters
            .iter()
            .any(|entry| entry.key == "ERR" && entry.value == "1")
    );
    assert!(project.notes.iter().any(|note| {
        note.name == "Results" && note.content == "Data1 Temperature:\t25.10242\r\n\r\n"
    }));
}

const SIGNATURE: &[u8] = b"CPYA 4.2673 552#\n";
const ORIGIN_HEADER_LEN: usize = 39;
const DATA_HEADER_LEN: usize = 123;

fn push_block(bytes: &mut Vec<u8>, payload: Option<&[u8]>) {
    let payload = payload.unwrap_or_default();
    bytes.extend_from_slice(&u32::try_from(payload.len()).unwrap().to_le_bytes());
    bytes.push(b'\n');
    if !payload.is_empty() {
        bytes.extend_from_slice(payload);
        bytes.push(b'\n');
    }
}

fn data_header(name: &str, supported: bool, row_count: u32) -> [u8; DATA_HEADER_LEN] {
    let mut header = [0_u8; DATA_HEADER_LEN];
    header[0x16..0x18].copy_from_slice(&0x6001_u16.to_le_bytes());
    header[0x18] = 1;
    header[0x19..0x1d].copy_from_slice(&row_count.to_le_bytes());
    header[0x1d..0x21].copy_from_slice(&0_u32.to_le_bytes());
    header[0x21..0x25].copy_from_slice(&row_count.to_le_bytes());
    header[0x3d] = 8;
    header[0x3f] = u8::from(!supported);
    let name = name.as_bytes();
    header[0x58..0x58 + name.len()].copy_from_slice(name);
    header[0x71..0x73].copy_from_slice(&0x10ca_u16.to_le_bytes());
    header
}

fn push_window(bytes: &mut Vec<u8>, name: &[u8]) {
    let mut header = [0_u8; 27];
    header[2..2 + name.len()].copy_from_slice(name);
    push_block(bytes, Some(&header));
    push_block(bytes, None);
}

fn synthetic_project_with_parameters(
    records: &[(&str, bool, u32)],
    windows: &[&[u8]],
    parameters: &[(&[u8], f64)],
) -> Vec<u8> {
    let mut bytes = SIGNATURE.to_vec();
    let mut origin_header = [0_u8; ORIGIN_HEADER_LEN];
    origin_header[0x1b..0x23].copy_from_slice(&7.0552_f64.to_le_bytes());
    push_block(&mut bytes, Some(&origin_header));
    push_block(&mut bytes, None);

    for (name, supported, row_count) in records {
        push_block(&mut bytes, Some(&data_header(name, *supported, *row_count)));
        let mut content = Vec::new();
        for _ in 0..*row_count {
            content.extend_from_slice(&1.5_f64.to_le_bytes());
        }
        push_block(&mut bytes, Some(&content));
        push_block(&mut bytes, None);
    }
    push_block(&mut bytes, None);

    for name in windows {
        push_window(&mut bytes, name);
    }
    push_block(&mut bytes, None);

    for (name, value) in parameters {
        bytes.extend_from_slice(name);
        bytes.push(b'\n');
        bytes.extend_from_slice(&value.to_le_bytes());
        bytes.push(b'\n');
    }
    bytes.extend_from_slice(b"\0\n");
    push_block(&mut bytes, None);

    push_block(&mut bytes, Some(&[0_u8; 4]));
    push_block(&mut bytes, Some(b"Results\0"));
    push_block(&mut bytes, Some(b"ok\0"));
    push_block(&mut bytes, None);
    push_block(&mut bytes, Some(&10_u32.to_le_bytes()));
    bytes
}

fn synthetic_project(records: &[(&str, bool)], windows: &[&[u8]]) -> Vec<u8> {
    let records = records
        .iter()
        .map(|(name, supported)| (*name, *supported, 1))
        .collect::<Vec<_>>();
    synthetic_project_with_parameters(&records, windows, &[(b"ERR".as_slice(), 1.0)])
}

fn insert_layer_with_two_nested_lists(bytes: &mut Vec<u8>) {
    let marker = [27_u32.to_le_bytes().as_slice(), b"\n\0\0Book"].concat();
    let window = bytes
        .windows(marker.len())
        .position(|candidate| candidate == marker)
        .expect("synthetic window framing");
    let layer_list = window + 5 + 27 + 1;
    let mut layer = Vec::new();
    push_block(&mut layer, Some(&[0_u8]));
    push_block(&mut layer, Some(&[1_u8]));
    for _ in 1..4 {
        push_block(&mut layer, None);
    }
    push_block(&mut layer, None);
    push_block(&mut layer, Some(&[1_u8]));
    push_block(&mut layer, None);
    for _ in 0..5 {
        push_block(&mut layer, None);
    }
    bytes.splice(layer_list..layer_list, layer);
}

fn only_column(project: &crate::origin::OriginProject) -> &crate::origin::OriginColumn {
    &project.workbooks[0].worksheets[0].columns[0]
}

#[test]
fn chooses_the_longest_validated_window_prefix() {
    let bytes = synthetic_project(&[("BookLong_Value", true)], &[b"Book", b"BookLong"]);
    let project = read_origin(&bytes, OriginLimits::default()).unwrap();

    assert_eq!(project.workbooks.len(), 1);
    assert_eq!(project.workbooks[0].name, "BookLong");
    assert_eq!(only_column(&project).name, "Value");
    assert_eq!(only_column(&project).cells, [OriginCell::Float(1.5)]);
}

#[test]
fn requires_an_underscore_between_window_and_column_names() {
    let bytes = synthetic_project(&[("BookValue", true)], &[b"Book"]);
    let project = read_origin(&bytes, OriginLimits::default()).unwrap();

    assert_eq!(project.workbooks[0].name, "Unmatched Origin data");
    assert_eq!(only_column(&project).name, "BookValue");
    assert!(
        project
            .diagnostics
            .iter()
            .any(|diagnostic| { diagnostic.code == OriginDiagnosticCode::DecodingWarning })
    );
}

#[test]
fn preserves_non_identifier_column_suffixes_after_an_exact_window_prefix() {
    let bytes = synthetic_project(&[("Book_1", true), ("Book_A-B", true)], &[b"Book"]);
    let project = read_origin(&bytes, OriginLimits::default()).unwrap();

    assert_eq!(project.workbooks.len(), 1);
    assert_eq!(project.workbooks[0].name, "Book");
    assert_eq!(
        project.workbooks[0].worksheets[0]
            .columns
            .iter()
            .map(|column| column.name.as_str())
            .collect::<Vec<_>>(),
        ["1", "A-B"]
    );
}

#[test]
fn ambiguous_or_missing_window_associations_use_a_stable_fallback() {
    for windows in [
        &[b"Other".as_slice()][..],
        &[b"Book".as_slice(), b"Book".as_slice()][..],
    ] {
        let dataset = if windows.len() == 1 {
            "Orphan_Value"
        } else {
            "Book_Value"
        };
        let bytes = synthetic_project(&[(dataset, true)], windows);
        let project = read_origin(&bytes, OriginLimits::default()).unwrap();

        assert_eq!(project.workbooks[0].name, "Unmatched Origin data");
        assert_eq!(only_column(&project).name, dataset);
        assert!(project.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == OriginDiagnosticCode::DecodingWarning
                && diagnostic.message == "A worksheet column had no unambiguous Origin window; PlotX kept it in Unmatched Origin data."
        }));
    }
}

#[test]
fn skips_an_independently_framed_unsupported_column() {
    let bytes = synthetic_project(&[("Book_Good", true), ("Book_Unknown", false)], &[b"Book"]);
    let project = read_origin(&bytes, OriginLimits::default()).unwrap();

    assert_eq!(project.workbooks[0].worksheets[0].columns.len(), 1);
    assert_eq!(only_column(&project).name, "Good");
    assert!(
        project.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == OriginDiagnosticCode::UnsupportedColumnSkipped
        })
    );
    assert!(
        project
            .unsupported_objects
            .iter()
            .any(|summary| { summary.kind == "worksheet columns" && summary.count == 1 })
    );
}

#[test]
fn does_not_treat_an_exact_window_name_as_a_worksheet_column() {
    let bytes = synthetic_project(&[("Matrix", true)], &[b"Matrix"]);
    assert!(matches!(
        read_origin(&bytes, OriginLimits::default()),
        Err(OriginError::NoSupportedWorksheet)
    ));
}

#[test]
fn rejects_project_with_only_zero_row_supported_columns() {
    let bytes = synthetic_project_with_parameters(
        &[("Book_Empty", true, 0)],
        &[b"Book"],
        &[(b"ERR".as_slice(), 1.0)],
    );

    assert!(matches!(
        read_origin(&bytes, OriginLimits::default()),
        Err(OriginError::NoSupportedWorksheet)
    ));
}

#[test]
fn keeps_workbook_with_a_nonempty_supported_column() {
    let bytes = synthetic_project_with_parameters(
        &[("Book_Empty", true, 0), ("Book_Value", true, 1)],
        &[b"Book"],
        &[(b"ERR".as_slice(), 1.0)],
    );

    let project = read_origin(&bytes, OriginLimits::default()).unwrap();

    assert_eq!(project.workbooks.len(), 1);
    assert_eq!(project.workbooks[0].worksheets[0].row_count, 1);
    assert_eq!(project.workbooks[0].worksheets[0].columns.len(), 2);
    assert_eq!(project.resource_usage.workbooks, 1);
    assert_eq!(project.resource_usage.worksheets, 1);
    assert_eq!(project.resource_usage.columns, 2);
    assert_eq!(project.resource_usage.cells, 1);
}

#[test]
fn skips_bounded_non_ascii_note_metadata_with_a_warning() {
    let mut bytes = synthetic_project(&[("Book_A", true)], &[b"Book"]);
    let note_content = bytes
        .windows(3)
        .rposition(|window| window == b"ok\0")
        .expect("synthetic note content");
    bytes[note_content] = 0x80;

    let project = read_origin(&bytes, OriginLimits::default()).unwrap();
    assert!(project.notes.is_empty());
    assert!(
        project
            .diagnostics
            .iter()
            .any(|diagnostic| { diagnostic.code == OriginDiagnosticCode::MetadataSkipped })
    );
}

#[test]
fn enforces_workbook_and_metadata_limits() {
    let bytes = synthetic_project(&[("One_A", true), ("Two_B", true)], &[b"One", b"Two"]);
    let workbook_limits = OriginLimits {
        max_workbooks: 1,
        ..OriginLimits::default()
    };
    assert!(matches!(
        read_origin(&bytes, workbook_limits),
        Err(OriginError::LimitExceeded {
            resource: "workbooks",
            limit: 1,
            actual: 2,
        })
    ));

    let string_limits = OriginLimits {
        max_string_bytes: 2,
        ..OriginLimits::default()
    };
    assert!(matches!(
        read_origin(&bytes, string_limits),
        Err(OriginError::LimitExceeded {
            resource: "string bytes",
            limit: 2,
            ..
        })
    ));
}

#[test]
fn repeated_single_item_reservations_grow_logarithmically_and_charge_capacity() {
    let limits = OriginLimits::default();
    let mut usage = OriginResourceUsage::default();
    let mut values = Vec::<u64>::new();
    let mut capacity_changes = 0_usize;

    for value in 0..4096_u64 {
        let previous_capacity = values.capacity();
        super::try_reserve(&mut values, 1, "test metadata", &limits, &mut usage).unwrap();
        if values.capacity() != previous_capacity {
            capacity_changes += 1;
        }
        values.push(value);
    }

    assert!(
        capacity_changes <= 16,
        "single-item appends reallocated {capacity_changes} times"
    );
    assert_eq!(usage.parser_bytes, values.capacity() * size_of::<u64>());
    assert_eq!(usage.total_owned_bytes, usage.parser_bytes);
}

#[test]
fn spare_vector_capacity_is_not_charged_as_a_new_allocation() {
    let limits = OriginLimits::default();
    let mut usage = OriginResourceUsage::default();
    let mut values = Vec::<u64>::with_capacity(8);
    let original_capacity = values.capacity();

    super::try_reserve(&mut values, 1, "test metadata", &limits, &mut usage).unwrap();

    assert_eq!(values.capacity(), original_capacity);
    assert_eq!(usage.parser_bytes, 0);
    assert_eq!(usage.total_owned_bytes, 0);
}

#[test]
fn existing_capacity_does_not_overflow_an_unbounded_custom_budget() {
    let limits = OriginLimits {
        max_parser_bytes: usize::MAX,
        max_total_owned_bytes: usize::MAX,
        ..OriginLimits::default()
    };
    let mut usage = OriginResourceUsage::default();
    let mut values = vec![0_u8; 8];
    let original_capacity = values.capacity();

    super::try_reserve(&mut values, 1, "test metadata", &limits, &mut usage).unwrap();

    assert!(values.capacity() > original_capacity);
    assert_eq!(usage.parser_bytes, values.capacity() - original_capacity);
    assert_eq!(usage.total_owned_bytes, usage.parser_bytes);
}

#[test]
fn rejects_excess_window_records_before_dataset_association() {
    let window_names = (0..1025)
        .map(|index| format!("W{index:04}"))
        .collect::<Vec<_>>();
    let window_name_bytes = window_names
        .iter()
        .map(|name| name.as_bytes())
        .collect::<Vec<_>>();
    let bytes = synthetic_project(&[("W0000_A", true)], &window_name_bytes);

    assert!(matches!(
        read_origin(&bytes, OriginLimits::default()),
        Err(OriginError::LimitExceeded {
            resource: "window records",
            limit: 1024,
            actual: 1025,
        })
    ));
}

#[test]
fn metadata_records_do_not_consume_the_data_column_limit() {
    let bytes = synthetic_project_with_parameters(
        &[("Book_A", true, 1)],
        &[b"Book"],
        &[
            (b"ERR".as_slice(), 1.0),
            (b"ALPHA".as_slice(), 2.0),
            (b"BETA".as_slice(), 3.0),
        ],
    );
    let limits = OriginLimits {
        max_columns: 1,
        ..OriginLimits::default()
    };
    let project = read_origin(&bytes, limits).unwrap();

    assert_eq!(project.workbooks[0].worksheets[0].columns.len(), 1);
    assert_eq!(project.parameters.len(), 3);
}

#[test]
fn metadata_record_limit_is_cumulative_across_nested_lists() {
    let mut bytes = synthetic_project(&[("Book_A", true)], &[b"Book"]);
    insert_layer_with_two_nested_lists(&mut bytes);
    let limits = OriginLimits {
        // Window, layer, and the first nested item consume the full budget;
        // the first item in the second independent list is record four.
        max_metadata_records: 3,
        ..OriginLimits::default()
    };

    assert!(matches!(
        read_origin(&bytes, limits),
        Err(OriginError::LimitExceeded {
            resource: "metadata records",
            limit: 3,
            actual: 4,
        })
    ));
}

#[test]
fn skipped_parameters_still_consume_the_metadata_record_budget() {
    let cases: &[(&[u8], f64)] = &[(&[0x80], 2.0), (b"NAN", f64::NAN)];
    for &(name, value) in cases {
        let bytes = synthetic_project_with_parameters(
            &[("Book_A", true, 1)],
            &[b"Book"],
            &[(b"ERR", 1.0), (name, value)],
        );
        let limits = OriginLimits {
            max_metadata_records: 2,
            ..OriginLimits::default()
        };
        assert!(matches!(
            read_origin(&bytes, limits),
            Err(OriginError::LimitExceeded {
                resource: "metadata records",
                limit: 2,
                actual: 3,
            })
        ));
    }
}

#[test]
fn metadata_record_count_equal_to_the_limit_succeeds() {
    let bytes = synthetic_project(&[("Book_A", true)], &[b"Book"]);
    let limits = OriginLimits {
        max_metadata_records: 4,
        ..OriginLimits::default()
    };
    let project = read_origin(&bytes, limits).unwrap();

    assert_eq!(project.resource_usage.metadata_records, 4);
}

#[test]
fn enforces_metadata_nesting_depth_before_walking_nested_lists() {
    let mut bytes = synthetic_project(&[("Book_A", true)], &[b"Book"]);
    let marker = [27_u32.to_le_bytes().as_slice(), b"\n\0\0Book"].concat();
    let window = bytes
        .windows(marker.len())
        .position(|candidate| candidate == marker)
        .expect("synthetic window framing");
    let layer_list = window + 5 + 27 + 1;
    let mut nested_layer = Vec::new();
    push_block(&mut nested_layer, Some(&[0_u8]));
    for _ in 0..6 {
        push_block(&mut nested_layer, None);
    }
    bytes.splice(layer_list..layer_list, nested_layer);

    let limits = OriginLimits {
        max_metadata_depth: 2,
        ..OriginLimits::default()
    };
    assert!(matches!(
        read_origin(&bytes, limits),
        Err(OriginError::LimitExceeded {
            resource: "metadata nesting depth",
            limit: 2,
            actual: 3,
        })
    ));
}

#[test]
fn accepts_framed_null_components_but_rejects_a_missing_component() {
    let mut complete = synthetic_project(&[("Book_A", true)], &[b"Book"]);
    let marker = [27_u32.to_le_bytes().as_slice(), b"\n\0\0Book"].concat();
    let window = complete
        .windows(marker.len())
        .position(|candidate| candidate == marker)
        .expect("synthetic window framing");
    let layer_list = window + 5 + 27 + 1;
    let mut layer = Vec::new();
    push_block(&mut layer, Some(&[0_u8]));
    push_block(&mut layer, Some(&[1_u8]));
    for _ in 0..9 {
        push_block(&mut layer, None);
    }
    let missing_component = layer_list + 14;
    complete.splice(layer_list..layer_list, layer);

    let project = read_origin(&complete, OriginLimits::default()).unwrap();
    assert!(
        project
            .unsupported_objects
            .iter()
            .any(|summary| { summary.kind == "window presentation records" && summary.count == 2 })
    );

    let mut truncated_item = complete;
    truncated_item.drain(missing_component..missing_component + 5);
    assert!(read_origin(&truncated_item, OriginLimits::default()).is_err());
}

#[test]
fn retains_validated_parameter_and_note_values() {
    let bytes = synthetic_project(&[("Book_A", true)], &[b"Book"]);
    let project = read_origin(&bytes, OriginLimits::default()).unwrap();

    assert_eq!(
        project.parameters,
        [OriginMetadataEntry {
            key: "ERR".to_owned(),
            value: "1".to_owned(),
        }]
    );
    assert_eq!(
        project.notes,
        [OriginNote {
            name: "Results".to_owned(),
            content: "ok".to_owned(),
        }]
    );
}

#[test]
fn truncated_metadata_never_panics_or_returns_partial_output() {
    let complete = synthetic_project(&[("Book_A", true)], &[b"Book"]);
    let metadata_start = complete
        .windows(27)
        .position(|window| window.get(2..6) == Some(b"Book"))
        .expect("synthetic window header");

    for prefix in metadata_start..complete.len() {
        let result =
            std::panic::catch_unwind(|| read_origin(&complete[..prefix], OriginLimits::default()));
        assert!(result.is_ok(), "metadata prefix {prefix} panicked");
        assert!(
            result.unwrap().is_err(),
            "metadata prefix {prefix} succeeded"
        );
    }
}

#[test]
fn rejects_corrupt_real_fixture_project_tree_and_attachment_bounds() {
    for end in [
        0x43602,
        0x43607,
        0x43700,
        0x4377f,
        0x43790,
        OPENOPJ_FIXTURE.len() - 1,
    ] {
        let result = std::panic::catch_unwind(|| {
            read_origin(&OPENOPJ_FIXTURE[..end], OriginLimits::default())
        });
        assert!(result.is_ok(), "real fixture prefix {end:#x} panicked");
        assert!(
            result.unwrap().is_err(),
            "real fixture prefix {end:#x} succeeded"
        );
    }

    let tree_only = read_origin(&OPENOPJ_FIXTURE[..0x4377e], OriginLimits::default())
        .expect("an exact project-tree EOF is a valid no-attachment project");
    assert!(
        tree_only
            .unsupported_objects
            .iter()
            .any(|summary| { summary.kind == "project tree" && summary.count == 1 })
    );
    assert!(
        !tree_only
            .unsupported_objects
            .iter()
            .any(|summary| { summary.kind == "embedded attachments" })
    );

    let mut tree = OPENOPJ_FIXTURE.to_vec();
    tree[0x43607..0x4360b].copy_from_slice(&381_u32.to_le_bytes());
    assert!(read_origin(&tree, OriginLimits::default()).is_err());

    let mut attachment = OPENOPJ_FIXTURE.to_vec();
    attachment[0x43786..0x4378a].copy_from_slice(&5633_u32.to_le_bytes());
    assert!(read_origin(&attachment, OriginLimits::default()).is_err());
}
