use super::reader::{FramedBlock, Reader, checked_add, checked_mul};
use super::{OriginError, OriginLimits};
use std::mem::size_of;

#[test]
fn reads_checked_little_endian_primitives() {
    let mut bytes = Vec::new();
    bytes.push(0xa5);
    bytes.extend_from_slice(&0x1234_u16.to_le_bytes());
    bytes.extend_from_slice(&0x89ab_cdef_u32.to_le_bytes());
    bytes.extend_from_slice(&(-12_345_i16).to_le_bytes());
    bytes.extend_from_slice(&(-123_456_789_i32).to_le_bytes());
    bytes.extend_from_slice(&12.5_f32.to_le_bytes());
    bytes.extend_from_slice(&(-98.25_f64).to_le_bytes());
    let limits = OriginLimits::default();
    let mut reader = Reader::new(&bytes, &limits).unwrap();

    assert_eq!(reader.read_u8().unwrap(), 0xa5);
    assert_eq!(reader.read_u16_le().unwrap(), 0x1234);
    assert_eq!(reader.read_u32_le().unwrap(), 0x89ab_cdef);
    assert_eq!(reader.read_i16_le().unwrap(), -12_345);
    assert_eq!(reader.read_i32_le().unwrap(), -123_456_789);
    assert_eq!(reader.read_f32_le().unwrap(), 12.5);
    assert_eq!(reader.read_f64_le().unwrap(), -98.25);
    assert_eq!(reader.offset(), bytes.len());
}

#[test]
fn checked_slice_accepts_exact_end_and_rejects_one_byte_past_end() {
    let limits = OriginLimits::default();
    let mut reader = Reader::new(b"abc", &limits).unwrap();
    assert_eq!(reader.read_slice(3).unwrap(), b"abc");

    assert!(matches!(
        reader.read_slice(1),
        Err(OriginError::Truncated {
            offset: 3,
            needed: 1,
            have: 0,
        })
    ));
}

#[test]
fn checked_arithmetic_reports_overflow() {
    assert!(matches!(
        checked_add(usize::MAX, 1, "test offset"),
        Err(OriginError::ArithmeticOverflow {
            resource: "test offset"
        })
    ));
    assert!(matches!(
        checked_mul(usize::MAX, 2, "test capacity"),
        Err(OriginError::ArithmeticOverflow {
            resource: "test capacity"
        })
    ));
}

#[test]
fn reads_data_and_null_block_framing() {
    let bytes = [
        3, 0, 0, 0, b'\n', b'a', b'b', b'c', b'\n', 0, 0, 0, 0, b'\n',
    ];
    let limits = OriginLimits::default();
    let mut reader = Reader::new(&bytes, &limits).unwrap();

    assert!(matches!(
        reader.read_block().unwrap(),
        FramedBlock::Data { offset: 0, payload } if payload == b"abc"
    ));
    assert!(matches!(
        reader.read_block().unwrap(),
        FramedBlock::Null { offset: 9 }
    ));
    assert_eq!(reader.offset(), bytes.len());
}

#[test]
fn rejects_bad_block_delimiters_at_their_offsets() {
    let limits = OriginLimits::default();
    let mut bad_size = Reader::new(&[1, 0, 0, 0, b'!', b'a', b'\n'], &limits).unwrap();
    assert!(matches!(
        bad_size.read_block(),
        Err(OriginError::CorruptStructure { offset: 4, .. })
    ));

    let mut bad_payload = Reader::new(&[1, 0, 0, 0, b'\n', b'a', b'!'], &limits).unwrap();
    assert!(matches!(
        bad_payload.read_block(),
        Err(OriginError::CorruptStructure { offset: 6, .. })
    ));
}

#[test]
fn rejects_oversized_declared_block_before_slicing() {
    let limits = OriginLimits {
        max_block_bytes: 2,
        ..OriginLimits::default()
    };
    let mut reader = Reader::new(&[3, 0, 0, 0, b'\n'], &limits).unwrap();

    assert!(matches!(
        reader.read_block(),
        Err(OriginError::LimitExceeded {
            resource: "block bytes",
            limit: 2,
            actual: 3,
        })
    ));
}

#[test]
fn parser_budget_is_checked_before_vec_reserve() {
    let limits = OriginLimits {
        max_parser_bytes: 3,
        ..OriginLimits::default()
    };
    let mut reader = Reader::new(&[], &limits).unwrap();
    let mut values = Vec::<u16>::new();

    assert!(matches!(
        reader.try_reserve(&mut values, 2, "test values"),
        Err(OriginError::LimitExceeded {
            resource: "parser bytes",
            limit: 3,
            actual: 4,
        })
    ));
    assert_eq!(values.capacity(), 0);
}

#[test]
fn repeated_single_item_reservations_grow_logarithmically_and_charge_capacity() {
    let limits = OriginLimits::default();
    let mut reader = Reader::new(&[], &limits).unwrap();
    let mut values = Vec::<u64>::new();
    let mut capacity_changes = 0_usize;

    for value in 0..4096_u64 {
        let previous_capacity = values.capacity();
        reader
            .try_reserve(&mut values, 1, "test reader values")
            .unwrap();
        if values.capacity() != previous_capacity {
            capacity_changes += 1;
        }
        values.push(value);
    }

    let usage = reader.into_usage();
    assert!(
        capacity_changes <= 16,
        "single-item appends reallocated {capacity_changes} times"
    );
    assert_eq!(usage.parser_bytes, values.capacity() * size_of::<u64>());
    assert_eq!(usage.total_owned_bytes, usage.parser_bytes);
}

#[test]
fn spare_vector_capacity_is_not_charged_as_a_new_reader_allocation() {
    let limits = OriginLimits::default();
    let mut reader = Reader::new(&[], &limits).unwrap();
    let mut values = Vec::<u64>::with_capacity(8);
    let original_capacity = values.capacity();

    reader
        .try_reserve(&mut values, 1, "test reader values")
        .unwrap();

    let usage = reader.into_usage();
    assert_eq!(values.capacity(), original_capacity);
    assert_eq!(usage.parser_bytes, 0);
    assert_eq!(usage.total_owned_bytes, 0);
}

#[test]
fn reader_charges_the_actual_capacity_delta_with_an_unbounded_budget() {
    let limits = OriginLimits {
        max_parser_bytes: usize::MAX,
        max_total_owned_bytes: usize::MAX,
        ..OriginLimits::default()
    };
    let mut reader = Reader::new(&[], &limits).unwrap();
    let mut values = vec![0_u8; 8];
    let original_capacity = values.capacity();

    reader
        .try_reserve(&mut values, 1, "test reader values")
        .unwrap();

    let usage = reader.into_usage();
    assert!(values.capacity() > original_capacity);
    assert_eq!(usage.parser_bytes, values.capacity() - original_capacity);
    assert_eq!(usage.total_owned_bytes, usage.parser_bytes);
}

#[test]
fn decoded_text_budget_is_checked_before_string_reserve() {
    let limits = OriginLimits {
        max_decoded_text_bytes: 3,
        ..OriginLimits::default()
    };
    let mut reader = Reader::new(b"text", &limits).unwrap();

    assert!(matches!(
        reader.read_fixed_ascii(4),
        Err(OriginError::LimitExceeded {
            resource: "decoded text bytes",
            limit: 3,
            actual: 4,
        })
    ));
}

#[test]
fn impossible_capacity_requests_return_errors_without_panicking() {
    let limits = OriginLimits {
        max_parser_bytes: usize::MAX,
        max_total_owned_bytes: usize::MAX,
        ..OriginLimits::default()
    };
    let result = std::panic::catch_unwind(|| {
        let mut reader = Reader::new(&[], &limits)?;
        let mut wide = Vec::<u16>::new();
        reader.try_reserve(&mut wide, usize::MAX, "wide values")?;
        Ok::<(), OriginError>(())
    });
    assert!(result.is_ok());
    assert!(matches!(
        result.unwrap(),
        Err(OriginError::ArithmeticOverflow { .. })
            | Err(OriginError::LimitExceeded { .. })
            | Err(OriginError::AllocationFailed { .. })
    ));

    let allocation = std::panic::catch_unwind(|| {
        let mut reader = Reader::new(&[], &limits)?;
        let mut bytes = Vec::<u8>::new();
        reader.try_reserve(&mut bytes, isize::MAX as usize, "huge byte buffer")?;
        Ok::<(), OriginError>(())
    });
    assert!(allocation.is_ok());
    assert!(matches!(
        allocation.unwrap(),
        Err(OriginError::AllocationFailed { .. }) | Err(OriginError::ArithmeticOverflow { .. })
    ));
}

#[test]
fn every_truncated_block_prefix_returns_a_structured_error() {
    let complete = [1, 0, 0, 0, b'\n', b'x', b'\n'];
    let limits = OriginLimits::default();

    for prefix_len in 0..complete.len() {
        let result = std::panic::catch_unwind(|| {
            let mut reader = Reader::new(&complete[..prefix_len], &limits)?;
            reader.read_block()
        });
        assert!(result.is_ok(), "prefix {prefix_len} panicked");
        assert!(matches!(
            result.unwrap(),
            Err(OriginError::Truncated { .. })
        ));
    }
}

#[test]
fn fixed_ascii_trims_only_in_field_nul_padding() {
    let limits = OriginLimits::default();
    let mut reader = Reader::new(b"abc\0\0next", &limits).unwrap();

    assert_eq!(reader.read_fixed_ascii(5).unwrap(), "abc");
    assert_eq!(reader.offset(), 5);
}

#[test]
fn fixed_ascii_rejects_non_ascii_before_nul_but_ignores_bytes_after_nul() {
    let limits = OriginLimits::default();
    let mut invalid = Reader::new(&[b'a', 0xff, 0, b'x'], &limits).unwrap();
    assert!(matches!(
        invalid.read_fixed_ascii(4),
        Err(OriginError::UnsupportedEncoding { offset: 1, .. })
    ));

    let mut padded = Reader::new(&[b'o', b'k', 0, 0xff], &limits).unwrap();
    assert_eq!(padded.read_fixed_ascii(4).unwrap(), "ok");
}

#[test]
fn reader_rejects_zero_custom_limits() {
    let limits = OriginLimits {
        max_block_bytes: 0,
        ..OriginLimits::default()
    };
    assert!(matches!(
        Reader::new(&[], &limits),
        Err(OriginError::InvalidLimit {
            name: "max_block_bytes",
            ..
        })
    ));
}
