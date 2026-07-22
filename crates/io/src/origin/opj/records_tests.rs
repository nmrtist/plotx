use std::mem::size_of;
use std::panic::catch_unwind;

use super::{DecodedColumnRecord, decode_column_record};
use crate::origin::{OriginCell, OriginColumnType, OriginError, OriginLimits, OriginResourceUsage};

const HEADER_LEN: usize = 123;
const TYPE_OFFSET: usize = 0x16;
const SECONDARY_TYPE_OFFSET: usize = 0x18;
const TOTAL_ROWS_OFFSET: usize = 0x19;
const FIRST_ROW_OFFSET: usize = 0x1d;
const LAST_ROW_OFFSET: usize = 0x21;
const WIDTH_OFFSET: usize = 0x3d;
const UNSIGNED_FLAG_OFFSET: usize = 0x3f;
const NAME_OFFSET: usize = 0x58;
const NAME_WIDTH: usize = 25;
const TERTIARY_TYPE_OFFSET: usize = 0x71;

const TYPE_F64: u16 = 0x6001;
const TYPE_F32: u16 = 0x6003;
const TYPE_I32: u16 = 0x6801;
const TYPE_I16: u16 = 0x6803;
const TYPE_TEXT: u16 = 0x6021;
const TYPE_MIXED: u16 = 0x6121;

const SECONDARY: u8 = 0x01;
const TERTIARY_NUMERIC: u16 = 0x10ca;
const TERTIARY_FLOAT_OR_TEXT: u16 = 0x10e8;
const EMPTY_F64: f64 = -1.23456789E-300;
const FIXTURE_F32: f32 = f32::from_bits(0x43ac_cccd);
const FIXTURE_MIXED_F64: f64 = f64::from_bits(0x4009_1eb8_51eb_851f);

#[derive(Clone)]
struct RecordBytes {
    header: Vec<u8>,
    content: Vec<u8>,
}

#[derive(Clone, Copy)]
struct HeaderSpec {
    data_type: u16,
    secondary: u8,
    total_rows: u32,
    first_row: u32,
    last_row: u32,
    width: u8,
    unsigned_flag: u8,
    tertiary: u16,
}

impl HeaderSpec {
    fn fixture_type(
        data_type: u16,
        total_rows: u32,
        first_row: u32,
        last_row: u32,
        width: u8,
        tertiary: u16,
    ) -> Self {
        Self {
            data_type,
            secondary: SECONDARY,
            total_rows,
            first_row,
            last_row,
            width,
            unsigned_flag: 0,
            tertiary,
        }
    }
}

fn header(spec: HeaderSpec, name: &str) -> Vec<u8> {
    let mut bytes = vec![0_u8; HEADER_LEN];
    bytes[TYPE_OFFSET..TYPE_OFFSET + 2].copy_from_slice(&spec.data_type.to_le_bytes());
    bytes[SECONDARY_TYPE_OFFSET] = spec.secondary;
    bytes[TOTAL_ROWS_OFFSET..TOTAL_ROWS_OFFSET + 4].copy_from_slice(&spec.total_rows.to_le_bytes());
    bytes[FIRST_ROW_OFFSET..FIRST_ROW_OFFSET + 4].copy_from_slice(&spec.first_row.to_le_bytes());
    bytes[LAST_ROW_OFFSET..LAST_ROW_OFFSET + 4].copy_from_slice(&spec.last_row.to_le_bytes());
    bytes[WIDTH_OFFSET] = spec.width;
    bytes[UNSIGNED_FLAG_OFFSET] = spec.unsigned_flag;
    bytes[TERTIARY_TYPE_OFFSET..TERTIARY_TYPE_OFFSET + 2]
        .copy_from_slice(&spec.tertiary.to_le_bytes());
    let name = name.as_bytes();
    assert!(name.len() < NAME_WIDTH);
    bytes[NAME_OFFSET..NAME_OFFSET + name.len()].copy_from_slice(name);
    bytes
}

fn f64_record(values: &[f64]) -> RecordBytes {
    let row_count = u32::try_from(values.len()).unwrap();
    RecordBytes {
        header: header(
            HeaderSpec::fixture_type(TYPE_F64, row_count, 0, row_count, 8, TERTIARY_NUMERIC),
            "Data1_INJV",
        ),
        content: values
            .iter()
            .flat_map(|value| value.to_le_bytes())
            .collect(),
    }
}

fn f32_record(values: &[f32]) -> RecordBytes {
    let row_count = u32::try_from(values.len()).unwrap();
    RecordBytes {
        header: header(
            HeaderSpec::fixture_type(TYPE_F32, row_count, 0, row_count, 4, TERTIARY_FLOAT_OR_TEXT),
            "TestW_Float",
        ),
        content: values
            .iter()
            .flat_map(|value| value.to_le_bytes())
            .collect(),
    }
}

fn i32_record(values: &[i32]) -> RecordBytes {
    let row_count = u32::try_from(values.len()).unwrap();
    RecordBytes {
        header: header(
            HeaderSpec::fixture_type(TYPE_I32, row_count, 0, row_count, 4, TERTIARY_NUMERIC),
            "TestW_Long",
        ),
        content: values
            .iter()
            .flat_map(|value| value.to_le_bytes())
            .collect(),
    }
}

fn i16_record(values: &[i16]) -> RecordBytes {
    let row_count = u32::try_from(values.len()).unwrap();
    RecordBytes {
        header: header(
            HeaderSpec::fixture_type(TYPE_I16, row_count, 0, row_count, 2, TERTIARY_NUMERIC),
            "TestW_Integer",
        ),
        content: values
            .iter()
            .flat_map(|value| value.to_le_bytes())
            .collect(),
    }
}

fn text_record(value: &str) -> RecordBytes {
    const WIDTH: usize = 25;
    let mut content = vec![0_u8; WIDTH];
    content[..value.len()].copy_from_slice(value.as_bytes());
    content[value.len() + 1..].fill(0xff);
    RecordBytes {
        header: header(
            HeaderSpec::fixture_type(TYPE_TEXT, 1, 0, 1, WIDTH as u8, TERTIARY_FLOAT_OR_TEXT),
            "TestW_Text",
        ),
        content,
    }
}

fn mixed_record() -> RecordBytes {
    let mut content = Vec::new();
    content.extend_from_slice(&[1, 0]);
    content.extend_from_slice(b"text\0\xff\xfe\xfd");
    content.extend_from_slice(&[0, 0]);
    content.extend_from_slice(&FIXTURE_MIXED_F64.to_le_bytes());
    RecordBytes {
        header: header(
            HeaderSpec::fixture_type(TYPE_MIXED, 2, 0, 2, 10, TERTIARY_NUMERIC),
            "TestW_TextNumeric",
        ),
        content,
    }
}

fn decode(record: &RecordBytes) -> Result<DecodedColumnRecord, OriginError> {
    let mut usage = OriginResourceUsage::default();
    decode_with(record, OriginLimits::default(), &mut usage)
}

fn decode_with(
    record: &RecordBytes,
    limits: OriginLimits,
    usage: &mut OriginResourceUsage,
) -> Result<DecodedColumnRecord, OriginError> {
    decode_column_record(&record.header, Some(&record.content), &limits, usage)
}

fn clear_dataset_name(record: &mut RecordBytes) {
    record.header[NAME_OFFSET..NAME_OFFSET + NAME_WIDTH].fill(0);
}

fn assert_limit(error: OriginError, resource: &'static str, limit: usize, actual: usize) {
    assert_eq!(
        error,
        OriginError::LimitExceeded {
            resource,
            limit,
            actual,
        }
    );
}

#[test]
fn decodes_fixture_backed_f64_and_missing_sentinel() {
    let decoded = decode(&f64_record(&[0.4, EMPTY_F64])).unwrap();
    assert_eq!(decoded.dataset_name, "Data1_INJV");
    assert_eq!(decoded.column_type, OriginColumnType::Float);
    assert_eq!(
        decoded.cells,
        vec![OriginCell::Float(0.4), OriginCell::Null]
    );
}

#[test]
fn decodes_fixture_backed_f32_losslessly_into_f64() {
    let decoded = decode(&f32_record(&[FIXTURE_F32])).unwrap();
    assert_eq!(
        decoded.cells,
        vec![OriginCell::Float(f64::from(FIXTURE_F32))]
    );
}

#[test]
fn decodes_fixture_backed_signed_i32() {
    let decoded = decode(&i32_record(&[345, -100_000])).unwrap();
    assert_eq!(decoded.column_type, OriginColumnType::Integer);
    assert_eq!(
        decoded.cells,
        vec![OriginCell::Integer(345), OriginCell::Integer(-100_000)]
    );
}

#[test]
fn decodes_fixture_backed_signed_i16() {
    let decoded = decode(&i16_record(&[34, -1000])).unwrap();
    assert_eq!(
        decoded.cells,
        vec![OriginCell::Integer(34), OriginCell::Integer(-1000)]
    );
}

#[test]
fn decodes_fixture_backed_fixed_ascii_text() {
    let decoded = decode(&text_record("test string 123")).unwrap();
    assert_eq!(decoded.column_type, OriginColumnType::Text);
    assert_eq!(
        decoded.cells,
        vec![OriginCell::Text("test string 123".to_owned())]
    );
}

#[test]
fn decodes_fixture_backed_mixed_text_and_number() {
    let decoded = decode(&mixed_record()).unwrap();
    assert_eq!(decoded.column_type, OriginColumnType::Mixed);
    assert_eq!(
        decoded.cells,
        vec![
            OriginCell::Text("text".to_owned()),
            OriginCell::Float(FIXTURE_MIXED_F64)
        ]
    );
}

#[test]
fn fixture_exclusive_last_row_preserves_verified_leading_null_slot() {
    let mut content = Vec::new();
    content.extend_from_slice(&EMPTY_F64.to_le_bytes());
    content.extend_from_slice(&5.23_f64.to_le_bytes());
    content.extend_from_slice(&(-7.0_f64).to_le_bytes());
    let record = RecordBytes {
        header: header(
            HeaderSpec::fixture_type(TYPE_F64, 3, 1, 3, 8, TERTIARY_NUMERIC),
            "TestW_firstRow",
        ),
        content,
    };

    let decoded = decode(&record).unwrap();
    assert_eq!(decoded.first_row, 1);
    assert_eq!(decoded.last_row_exclusive, 3);
    assert_eq!(
        decoded.cells,
        vec![
            OriginCell::Null,
            OriginCell::Float(5.23),
            OriginCell::Float(-7.0)
        ]
    );
}

#[test]
fn rejects_nonnull_payload_slots_before_first_row() {
    let mut content = Vec::new();
    content.extend_from_slice(&999.0_f64.to_le_bytes());
    content.extend_from_slice(&5.23_f64.to_le_bytes());
    let record = RecordBytes {
        header: header(
            HeaderSpec::fixture_type(TYPE_F64, 2, 1, 2, 8, TERTIARY_NUMERIC),
            "InvalidFirstRow",
        ),
        content,
    };

    assert!(matches!(
        decode(&record),
        Err(OriginError::CorruptStructure { .. })
    ));
}

#[test]
fn rejects_width_and_fixture_type_mismatches() {
    let cases = [
        (TYPE_F64, 4, TERTIARY_NUMERIC),
        (TYPE_F32, 8, TERTIARY_FLOAT_OR_TEXT),
        (TYPE_I32, 2, TERTIARY_NUMERIC),
        (TYPE_I16, 4, TERTIARY_NUMERIC),
        (TYPE_TEXT, 8, TERTIARY_FLOAT_OR_TEXT),
        (TYPE_TEXT, 24, TERTIARY_FLOAT_OR_TEXT),
        (TYPE_TEXT, 26, TERTIARY_FLOAT_OR_TEXT),
        (TYPE_MIXED, 9, TERTIARY_NUMERIC),
    ];
    for (data_type, width, tertiary) in cases {
        let record = RecordBytes {
            header: header(
                HeaderSpec::fixture_type(data_type, 1, 0, 1, width, tertiary),
                "Mismatch",
            ),
            content: vec![0; usize::from(width)],
        };
        assert!(matches!(
            decode(&record),
            Err(OriginError::UnsupportedFeature { .. })
        ));
    }
}

#[test]
fn rejects_geometry_outside_total_rows_or_signed_range() {
    let beyond_total = RecordBytes {
        header: header(
            HeaderSpec::fixture_type(TYPE_F64, 1, 0, 2, 8, TERTIARY_NUMERIC),
            "BeyondTotal",
        ),
        content: vec![0; 8],
    };
    assert!(matches!(
        decode(&beyond_total),
        Err(OriginError::CorruptStructure { .. })
    ));

    let reversed = RecordBytes {
        header: header(
            HeaderSpec::fixture_type(TYPE_F64, 3, 2, 1, 8, TERTIARY_NUMERIC),
            "Reversed",
        ),
        content: vec![0; 24],
    };
    assert!(matches!(
        decode(&reversed),
        Err(OriginError::CorruptStructure { .. })
    ));

    let signed_negative = RecordBytes {
        header: header(
            HeaderSpec::fixture_type(TYPE_F64, u32::MAX, 0, 1, 8, TERTIARY_NUMERIC),
            "NegativeGeometry",
        ),
        content: Vec::new(),
    };
    assert!(matches!(
        decode(&signed_negative),
        Err(OriginError::CorruptStructure { .. })
    ));
}

#[test]
fn rejects_incomplete_fixed_text_and_mixed_numeric_payloads() {
    let mut fixed = text_record("test string 123");
    fixed.content.pop();
    assert!(matches!(decode(&fixed), Err(OriginError::Truncated { .. })));

    let mut mixed = mixed_record();
    mixed.header = header(
        HeaderSpec::fixture_type(TYPE_MIXED, 1, 0, 1, 10, TERTIARY_NUMERIC),
        "MixedNumeric",
    );
    mixed.content = vec![0, 0, 1, 2, 3, 4, 5, 6, 7];
    assert!(matches!(decode(&mixed), Err(OriginError::Truncated { .. })));
}

#[test]
fn rejects_content_one_byte_larger_than_declared_geometry() {
    let mut oversized = f64_record(&[0.4]);
    oversized.content.push(0);

    assert!(matches!(
        decode(&oversized),
        Err(OriginError::CorruptStructure { .. })
    ));
}

#[test]
fn enforces_row_column_and_cell_limits_with_exact_counts() {
    let record = f64_record(&[0.4, EMPTY_F64]);
    let mut usage = OriginResourceUsage::default();
    let row_limits = OriginLimits {
        max_rows_per_column: 1,
        ..OriginLimits::default()
    };
    assert_limit(
        decode_with(&record, row_limits, &mut usage).unwrap_err(),
        "rows per column",
        1,
        2,
    );

    let mut usage = OriginResourceUsage {
        columns: 1,
        ..OriginResourceUsage::default()
    };
    let column_limits = OriginLimits {
        max_columns: 1,
        ..OriginLimits::default()
    };
    assert_limit(
        decode_with(&record, column_limits, &mut usage).unwrap_err(),
        "columns",
        1,
        2,
    );

    let mut usage = OriginResourceUsage {
        cells: 1,
        ..OriginResourceUsage::default()
    };
    let cell_limits = OriginLimits {
        max_cells: 2,
        ..OriginLimits::default()
    };
    assert_limit(
        decode_with(&record, cell_limits, &mut usage).unwrap_err(),
        "cells",
        2,
        3,
    );
}

#[test]
fn enforces_string_and_cumulative_decoded_text_limits() {
    let named_record = f64_record(&[0.4]);
    let mut usage = OriginResourceUsage::default();
    let dataset_name_limits = OriginLimits {
        max_string_bytes: 9,
        ..OriginLimits::default()
    };
    assert_limit(
        decode_with(&named_record, dataset_name_limits, &mut usage).unwrap_err(),
        "string bytes",
        9,
        10,
    );

    let mut record = text_record("test string 123");
    clear_dataset_name(&mut record);

    let mut usage = OriginResourceUsage::default();
    let string_limits = OriginLimits {
        max_string_bytes: 14,
        ..OriginLimits::default()
    };
    assert_limit(
        decode_with(&record, string_limits, &mut usage).unwrap_err(),
        "string bytes",
        14,
        15,
    );

    let mut usage = OriginResourceUsage {
        decoded_text_bytes: 1,
        ..OriginResourceUsage::default()
    };
    let text_limits = OriginLimits {
        max_decoded_text_bytes: 15,
        ..OriginLimits::default()
    };
    assert_limit(
        decode_with(&record, text_limits, &mut usage).unwrap_err(),
        "decoded text bytes",
        15,
        16,
    );
}

#[test]
fn enforces_parser_and_total_owned_limits_before_cell_vector_allocation() {
    let mut record = f64_record(&[0.4, EMPTY_F64]);
    clear_dataset_name(&mut record);
    let cell_bytes = 2 * size_of::<OriginCell>();

    let mut usage = OriginResourceUsage {
        parser_bytes: 1,
        ..OriginResourceUsage::default()
    };
    let parser_limits = OriginLimits {
        max_parser_bytes: cell_bytes,
        ..OriginLimits::default()
    };
    assert_limit(
        decode_with(&record, parser_limits, &mut usage).unwrap_err(),
        "parser bytes",
        cell_bytes,
        cell_bytes + 1,
    );

    let mut usage = OriginResourceUsage {
        total_owned_bytes: 1,
        ..OriginResourceUsage::default()
    };
    let total_limits = OriginLimits {
        max_total_owned_bytes: cell_bytes,
        ..OriginLimits::default()
    };
    assert_limit(
        decode_with(&record, total_limits, &mut usage).unwrap_err(),
        "total owned bytes",
        cell_bytes,
        cell_bytes + 1,
    );
}

#[test]
fn rejects_invalid_mixed_prefix_and_nonzero_reserved_prefix() {
    let mut invalid = mixed_record();
    invalid.content[0] = 2;
    assert!(matches!(
        decode(&invalid),
        Err(OriginError::CorruptStructure { .. })
    ));

    let mut reserved = mixed_record();
    reserved.content[1] = 1;
    assert!(matches!(
        decode(&reserved),
        Err(OriginError::CorruptStructure { .. })
    ));
}

#[test]
fn rejects_non_ascii_fixed_and_mixed_text() {
    let mut fixed = text_record("ascii");
    fixed.content[0] = 0xff;
    assert!(matches!(
        decode(&fixed),
        Err(OriginError::UnsupportedEncoding { .. })
    ));

    let mut mixed = mixed_record();
    mixed.content[2] = 0xff;
    assert!(matches!(
        decode(&mixed),
        Err(OriginError::UnsupportedEncoding { .. })
    ));
}

#[test]
fn rejects_unsupported_eight_bit_integer_and_unsigned_flag() {
    let eight_bit = RecordBytes {
        header: header(
            HeaderSpec::fixture_type(TYPE_I32, 1, 0, 1, 1, TERTIARY_NUMERIC),
            "EightBit",
        ),
        content: vec![1],
    };
    assert!(matches!(
        decode(&eight_bit),
        Err(OriginError::UnsupportedFeature { .. })
    ));

    let mut unsigned_spec = HeaderSpec::fixture_type(TYPE_I32, 1, 0, 1, 4, TERTIARY_NUMERIC);
    unsigned_spec.unsigned_flag = 8;
    let unsigned = RecordBytes {
        header: header(unsigned_spec, "Unsigned"),
        content: 345_i32.to_le_bytes().to_vec(),
    };
    assert!(matches!(
        decode(&unsigned),
        Err(OriginError::UnsupportedFeature { .. })
    ));
}

#[test]
fn rejects_unverified_secondary_and_tertiary_type_fields() {
    let mut secondary = HeaderSpec::fixture_type(TYPE_F64, 1, 0, 1, 8, TERTIARY_NUMERIC);
    secondary.secondary = 0x91;
    let secondary = RecordBytes {
        header: header(secondary, "Secondary"),
        content: 0.4_f64.to_le_bytes().to_vec(),
    };
    assert!(matches!(
        decode(&secondary),
        Err(OriginError::UnsupportedFeature { .. })
    ));

    let tertiary = RecordBytes {
        header: header(
            HeaderSpec::fixture_type(TYPE_F64, 1, 0, 1, 8, 0x50ca),
            "Tertiary",
        ),
        content: 0.4_f64.to_le_bytes().to_vec(),
    };
    assert!(matches!(
        decode(&tertiary),
        Err(OriginError::UnsupportedFeature { .. })
    ));
}

#[test]
fn every_truncated_minimal_record_returns_an_error_without_panicking() {
    let records = [
        f64_record(&[0.4, EMPTY_F64]),
        f32_record(&[FIXTURE_F32]),
        i32_record(&[345, -100_000]),
        i16_record(&[34, -1000]),
        text_record("test string 123"),
        mixed_record(),
    ];

    for record in records {
        for end in 0..record.header.len() {
            let outcome = catch_unwind(|| {
                let mut usage = OriginResourceUsage::default();
                decode_column_record(
                    &record.header[..end],
                    Some(&record.content),
                    &OriginLimits::default(),
                    &mut usage,
                )
            });
            assert!(outcome.is_ok(), "header prefix {end} panicked");
            assert!(outcome.unwrap().is_err(), "header prefix {end} succeeded");
        }

        for end in 0..record.content.len() {
            let outcome = catch_unwind(|| {
                let mut usage = OriginResourceUsage::default();
                decode_column_record(
                    &record.header,
                    Some(&record.content[..end]),
                    &OriginLimits::default(),
                    &mut usage,
                )
            });
            assert!(outcome.is_ok(), "content prefix {end} panicked");
            assert!(outcome.unwrap().is_err(), "content prefix {end} succeeded");
        }
    }
}
