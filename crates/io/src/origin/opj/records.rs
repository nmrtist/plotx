use std::mem::size_of;

use crate::origin::{OriginCell, OriginColumnType, OriginError, OriginLimits, OriginResourceUsage};

use super::super::reader::{checked_add, checked_mul};

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
const FIXED_TEXT_WIDTH: u8 = 25;
const SECONDARY: u8 = 0x01;
const TERTIARY_NUMERIC: u16 = 0x10ca;
const TERTIARY_FLOAT_OR_TEXT: u16 = 0x10e8;
const EMPTY_F64_BITS: u64 = 0x81aa_74fe_1c13_2c0e;

#[derive(Debug, PartialEq)]
pub(super) struct DecodedColumnRecord {
    pub(super) dataset_name: String,
    pub(super) column_type: OriginColumnType,
    pub(super) cells: Vec<OriginCell>,
    pub(super) first_row: usize,
    pub(super) last_row_exclusive: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ValueKind {
    F64,
    F32,
    I32,
    I16,
    FixedText,
    Mixed,
}

pub(super) fn decode_column_record(
    header: &[u8],
    content: Option<&[u8]>,
    limits: &OriginLimits,
    usage: &mut OriginResourceUsage,
) -> Result<DecodedColumnRecord, OriginError> {
    require_header_length(header)?;

    // This layout and the exact type combinations below are reimplemented
    // from the pinned MIT-licensed OpenOPJ Origin 7.0552 description and are
    // cross-checked against its redistributable test fixture:
    // https://github.com/jgonera/openopj/blob/42ddcf1eb3a490744c54fca0a4ed6fe7a5e723ca/docs/opj_format.markdown
    // https://github.com/jgonera/openopj/blob/42ddcf1eb3a490744c54fca0a4ed6fe7a5e723ca/lib/OpenOPJ/DataSection.php
    let data_type = read_u16_at(header, TYPE_OFFSET, "dataset type")?;
    let secondary = read_u8_at(header, SECONDARY_TYPE_OFFSET, "secondary dataset type")?;
    let total_rows = read_u32_at(header, TOTAL_ROWS_OFFSET, "total rows")?;
    let first_row = read_u32_at(header, FIRST_ROW_OFFSET, "first row")?;
    let last_row = read_u32_at(header, LAST_ROW_OFFSET, "last row")?;
    let width = read_u8_at(header, WIDTH_OFFSET, "value width")?;
    let unsigned_flag = read_u8_at(header, UNSIGNED_FLAG_OFFSET, "unsigned flag")?;
    let tertiary = read_u16_at(header, TERTIARY_TYPE_OFFSET, "tertiary dataset type")?;

    let kind = classify_type(data_type, secondary, width, unsigned_flag, tertiary)?;
    let (total_rows, first_row, last_row_exclusive) =
        validate_geometry(total_rows, first_row, last_row, limits)?;
    let width = usize::from(width);
    let expected_bytes = checked_mul(total_rows, width, "OPJ column content bytes")?;
    let content = require_content(content, expected_bytes)?;
    let dataset_name = decode_ascii(
        field(header, NAME_OFFSET, NAME_WIDTH, "dataset name")?,
        NAME_OFFSET,
        limits,
        usage,
    )?;

    charge_column_and_cells(last_row_exclusive, limits, usage)?;
    let cell_bytes = checked_mul(
        last_row_exclusive,
        size_of::<OriginCell>(),
        "decoded OPJ cells",
    )?;
    charge_parser(cell_bytes, limits, usage)?;
    let mut cells = Vec::new();
    cells
        .try_reserve_exact(last_row_exclusive)
        .map_err(|_| OriginError::AllocationFailed {
            resource: "decoded OPJ cells",
            requested: cell_bytes,
        })?;

    // The public fixture proves that lastRow is exclusive: TestW_Float stores
    // lastRow=2 for two values, while TestW_firstRow stores firstRow=1 and
    // lastRow=3 for [missing, 5.23, -7]. Decode existing payload slots rather
    // than prepending firstRow synthetic nulls, which would shift the data.
    for row in 0..last_row_exclusive {
        let start = checked_mul(row, width, "OPJ cell offset")?;
        let end = checked_add(start, width, "OPJ cell range")?;
        let slot = content
            .get(start..end)
            .ok_or_else(|| truncated_range(start, width, content.len().saturating_sub(start)))?;
        let cell = decode_cell(kind, slot, start, limits, usage)?;
        if row < first_row && cell != OriginCell::Null {
            return Err(OriginError::CorruptStructure {
                offset: start,
                detail: "a payload slot before firstRow is not the verified missing-value sentinel"
                    .to_owned(),
            });
        }
        cells.push(cell);
    }

    Ok(DecodedColumnRecord {
        dataset_name,
        column_type: column_type(kind),
        cells,
        first_row,
        last_row_exclusive,
    })
}

fn require_header_length(header: &[u8]) -> Result<(), OriginError> {
    if header.len() < HEADER_LEN {
        return Err(OriginError::Truncated {
            offset: header.len(),
            needed: HEADER_LEN - header.len(),
            have: 0,
        });
    }
    if header.len() > HEADER_LEN {
        return Err(OriginError::CorruptStructure {
            offset: HEADER_LEN,
            detail: format!(
                "Origin7V552 data header must be exactly {HEADER_LEN} bytes, not {}",
                header.len()
            ),
        });
    }
    Ok(())
}

fn classify_type(
    data_type: u16,
    secondary: u8,
    width: u8,
    unsigned_flag: u8,
    tertiary: u16,
) -> Result<ValueKind, OriginError> {
    if unsigned_flag != 0 {
        return unsupported("unsigned Origin integers are not verified for Origin7V552");
    }
    if secondary != SECONDARY {
        return unsupported("the secondary Origin dataset type is not verified for Origin7V552");
    }

    match (data_type, width, tertiary) {
        (TYPE_F64, 8, TERTIARY_NUMERIC) => Ok(ValueKind::F64),
        (TYPE_F32, 4, TERTIARY_FLOAT_OR_TEXT) => Ok(ValueKind::F32),
        (TYPE_I32, 4, TERTIARY_NUMERIC) => Ok(ValueKind::I32),
        (TYPE_I16, 2, TERTIARY_NUMERIC) => Ok(ValueKind::I16),
        (TYPE_TEXT, FIXED_TEXT_WIDTH, TERTIARY_FLOAT_OR_TEXT) => Ok(ValueKind::FixedText),
        (TYPE_MIXED, 10, TERTIARY_NUMERIC) => Ok(ValueKind::Mixed),
        _ => unsupported("the Origin dataset type and value width combination is not verified"),
    }
}

fn validate_geometry(
    total_rows: u32,
    first_row: u32,
    last_row: u32,
    limits: &OriginLimits,
) -> Result<(usize, usize, usize), OriginError> {
    if [total_rows, first_row, last_row]
        .into_iter()
        .any(|value| value > i32::MAX as u32)
    {
        return Err(OriginError::CorruptStructure {
            offset: TOTAL_ROWS_OFFSET,
            detail: "Origin7V552 row geometry contains a negative or unverified high-bit value"
                .to_owned(),
        });
    }
    if first_row > last_row || last_row > total_rows {
        return Err(OriginError::CorruptStructure {
            offset: FIRST_ROW_OFFSET,
            detail: "Origin7V552 rows must satisfy firstRow <= lastRow <= totalRows".to_owned(),
        });
    }

    let total_rows = usize::try_from(total_rows).map_err(|_| OriginError::ArithmeticOverflow {
        resource: "OPJ total rows",
    })?;
    let first_row = usize::try_from(first_row).map_err(|_| OriginError::ArithmeticOverflow {
        resource: "OPJ first row",
    })?;
    let last_row = usize::try_from(last_row).map_err(|_| OriginError::ArithmeticOverflow {
        resource: "OPJ last row",
    })?;
    enforce_limit("rows per column", total_rows, limits.max_rows_per_column)?;
    Ok((total_rows, first_row, last_row))
}

fn require_content(content: Option<&[u8]>, expected: usize) -> Result<&[u8], OriginError> {
    let content = content.unwrap_or_default();
    if content.len() < expected {
        return Err(OriginError::Truncated {
            offset: 0,
            needed: expected,
            have: content.len(),
        });
    }
    if content.len() > expected {
        return Err(OriginError::CorruptStructure {
            offset: expected,
            detail: format!(
                "Origin column content has {} bytes but its geometry requires {expected}",
                content.len()
            ),
        });
    }
    Ok(content)
}

fn decode_cell(
    kind: ValueKind,
    slot: &[u8],
    offset: usize,
    limits: &OriginLimits,
    usage: &mut OriginResourceUsage,
) -> Result<OriginCell, OriginError> {
    match kind {
        ValueKind::F64 => decode_f64(slot, offset),
        ValueKind::F32 => Ok(OriginCell::Float(f64::from(f32::from_le_bytes(
            read_array(slot, offset)?,
        )))),
        ValueKind::I32 => Ok(OriginCell::Integer(i64::from(i32::from_le_bytes(
            read_array(slot, offset)?,
        )))),
        ValueKind::I16 => Ok(OriginCell::Integer(i64::from(i16::from_le_bytes(
            read_array(slot, offset)?,
        )))),
        ValueKind::FixedText => Ok(OriginCell::Text(decode_ascii(slot, offset, limits, usage)?)),
        ValueKind::Mixed => decode_mixed(slot, offset, limits, usage),
    }
}

fn decode_f64(slot: &[u8], offset: usize) -> Result<OriginCell, OriginError> {
    let value = f64::from_le_bytes(read_array(slot, offset)?);
    if value.to_bits() == EMPTY_F64_BITS {
        Ok(OriginCell::Null)
    } else {
        Ok(OriginCell::Float(value))
    }
}

fn decode_mixed(
    slot: &[u8],
    offset: usize,
    limits: &OriginLimits,
    usage: &mut OriginResourceUsage,
) -> Result<OriginCell, OriginError> {
    let prefix = *slot.first().ok_or_else(|| truncated_range(offset, 1, 0))?;
    let reserved = *slot.get(1).ok_or_else(|| truncated_range(offset, 2, 1))?;
    if reserved != 0 {
        return Err(OriginError::CorruptStructure {
            offset: checked_add(offset, 1, "mixed prefix offset")?,
            detail: "the reserved mixed-cell prefix byte must be zero".to_owned(),
        });
    }
    let payload = slot
        .get(2..)
        .ok_or_else(|| truncated_range(offset, 2, slot.len()))?;
    match prefix {
        0 => decode_f64(payload, checked_add(offset, 2, "mixed value offset")?),
        1 => Ok(OriginCell::Text(decode_ascii(
            payload,
            checked_add(offset, 2, "mixed text offset")?,
            limits,
            usage,
        )?)),
        _ => Err(OriginError::CorruptStructure {
            offset,
            detail: "mixed Origin cells require a numeric prefix 0 or text prefix 1".to_owned(),
        }),
    }
}

fn decode_ascii(
    field: &[u8],
    offset: usize,
    limits: &OriginLimits,
    usage: &mut OriginResourceUsage,
) -> Result<String, OriginError> {
    let length = field
        .iter()
        .position(|byte| *byte == 0)
        .unwrap_or(field.len());
    let text = field.get(..length).ok_or(OriginError::ArithmeticOverflow {
        resource: "bounded ASCII field",
    })?;
    if let Some(relative) = text.iter().position(|byte| !byte.is_ascii()) {
        return Err(OriginError::UnsupportedEncoding {
            offset: checked_add(offset, relative, "ASCII byte offset")?,
            encoding: "non-ASCII byte in Origin7V552 text".to_owned(),
        });
    }
    enforce_limit("string bytes", length, limits.max_string_bytes)?;
    charge_text(length, limits, usage)?;

    let mut decoded = String::new();
    decoded
        .try_reserve_exact(length)
        .map_err(|_| OriginError::AllocationFailed {
            resource: "decoded Origin text",
            requested: length,
        })?;
    let text = std::str::from_utf8(text).map_err(|_| OriginError::UnsupportedEncoding {
        offset,
        encoding: "non-ASCII byte in Origin7V552 text".to_owned(),
    })?;
    decoded.push_str(text);
    Ok(decoded)
}

fn column_type(kind: ValueKind) -> OriginColumnType {
    match kind {
        ValueKind::F64 | ValueKind::F32 => OriginColumnType::Float,
        ValueKind::I32 | ValueKind::I16 => OriginColumnType::Integer,
        ValueKind::FixedText => OriginColumnType::Text,
        ValueKind::Mixed => OriginColumnType::Mixed,
    }
}

fn field<'a>(
    bytes: &'a [u8],
    offset: usize,
    length: usize,
    resource: &'static str,
) -> Result<&'a [u8], OriginError> {
    let end = checked_add(offset, length, resource)?;
    bytes
        .get(offset..end)
        .ok_or_else(|| truncated_range(offset, length, bytes.len().saturating_sub(offset)))
}

fn read_u8_at(bytes: &[u8], offset: usize, resource: &'static str) -> Result<u8, OriginError> {
    field(bytes, offset, 1, resource)?
        .first()
        .copied()
        .ok_or_else(|| truncated_range(offset, 1, 0))
}

fn read_u16_at(bytes: &[u8], offset: usize, resource: &'static str) -> Result<u16, OriginError> {
    Ok(u16::from_le_bytes(read_array(
        field(bytes, offset, size_of::<u16>(), resource)?,
        offset,
    )?))
}

fn read_u32_at(bytes: &[u8], offset: usize, resource: &'static str) -> Result<u32, OriginError> {
    Ok(u32::from_le_bytes(read_array(
        field(bytes, offset, size_of::<u32>(), resource)?,
        offset,
    )?))
}

fn read_array<const N: usize>(bytes: &[u8], offset: usize) -> Result<[u8; N], OriginError> {
    bytes.try_into().map_err(|_| OriginError::Truncated {
        offset,
        needed: N,
        have: bytes.len(),
    })
}

fn charge_column_and_cells(
    cells: usize,
    limits: &OriginLimits,
    usage: &mut OriginResourceUsage,
) -> Result<(), OriginError> {
    let columns = checked_add(usage.columns, 1, "decoded OPJ columns")?;
    enforce_limit("columns", columns, limits.max_columns)?;
    let total_cells = checked_add(usage.cells, cells, "decoded OPJ cells")?;
    enforce_limit("cells", total_cells, limits.max_cells)?;
    usage.columns = columns;
    usage.cells = total_cells;
    Ok(())
}

fn charge_text(
    bytes: usize,
    limits: &OriginLimits,
    usage: &mut OriginResourceUsage,
) -> Result<(), OriginError> {
    let decoded = checked_add(usage.decoded_text_bytes, bytes, "decoded text bytes")?;
    enforce_limit("decoded text bytes", decoded, limits.max_decoded_text_bytes)?;
    charge_parser(bytes, limits, usage)?;
    usage.decoded_text_bytes = decoded;
    Ok(())
}

fn charge_parser(
    bytes: usize,
    limits: &OriginLimits,
    usage: &mut OriginResourceUsage,
) -> Result<(), OriginError> {
    let parser = checked_add(usage.parser_bytes, bytes, "parser bytes")?;
    enforce_limit("parser bytes", parser, limits.max_parser_bytes)?;
    let total = checked_add(usage.total_owned_bytes, bytes, "total owned bytes")?;
    enforce_limit("total owned bytes", total, limits.max_total_owned_bytes)?;
    usage.parser_bytes = parser;
    usage.total_owned_bytes = total;
    Ok(())
}

fn enforce_limit(resource: &'static str, actual: usize, limit: usize) -> Result<(), OriginError> {
    if actual > limit {
        return Err(OriginError::LimitExceeded {
            resource,
            limit,
            actual,
        });
    }
    Ok(())
}

fn unsupported<T>(feature: &'static str) -> Result<T, OriginError> {
    Err(OriginError::UnsupportedFeature {
        feature: feature.to_owned(),
    })
}

fn truncated_range(offset: usize, needed: usize, have: usize) -> OriginError {
    OriginError::Truncated {
        offset,
        needed,
        have,
    }
}

#[cfg(test)]
#[path = "records_tests.rs"]
mod records_tests;
