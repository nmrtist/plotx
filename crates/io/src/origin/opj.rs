use std::mem::size_of;

use super::reader::{FramedBlock, Reader, checked_add};
use super::{
    OriginColumn, OriginDiagnosticCode, OriginError, OriginLimits, OriginProbe, OriginProject,
    OriginResourceUsage, OriginWorkbook, OriginWorksheet,
};

mod metadata;
mod records;

const SIGNATURE: &[u8] = b"CPYA 4.2673 552#\n";
const ORIGIN_HEADER_LEN: usize = 39;
const ORIGIN_VERSION_OFFSET: usize = 0x1b;
const DATA_HEADER_LEN: usize = 123;
const BLOCK_PREFIX_LEN: usize = 5;
const EXPECTED_ORIGIN_VERSION: f64 = 7.0552;

#[derive(Debug)]
pub(super) struct RawOpjDataSection<'a> {
    pub(super) header: &'a [u8],
    pub(super) content: Option<&'a [u8]>,
}

#[derive(Debug)]
pub(super) struct RawOpjProject<'a> {
    pub(super) origin_header: &'a [u8],
    pub(super) data_sections: Vec<RawOpjDataSection<'a>>,
    pub(super) remaining: &'a [u8],
    pub(super) resource_usage: OriginResourceUsage,
}

pub(super) fn read(
    bytes: &[u8],
    limits: &OriginLimits,
    probe: OriginProbe,
    retained_probe_bytes: usize,
) -> Result<OriginProject, OriginError> {
    let raw = parse_raw(bytes, limits, retained_probe_bytes)?;
    if raw.data_sections.is_empty() && raw.remaining.is_empty() {
        return Err(OriginError::NoSupportedWorksheet);
    }

    let _validated_header_len = raw.origin_header.len();
    let mut usage = raw.resource_usage;
    let metadata_offset =
        bytes
            .len()
            .checked_sub(raw.remaining.len())
            .ok_or(OriginError::ArithmeticOverflow {
                resource: "OPJ metadata offset",
            })?;
    let mut parsed = metadata::parse(raw.remaining, metadata_offset, limits, &mut usage)?;
    let mut fallback_columns = Vec::new();
    let mut unsupported_columns = 0_usize;

    for section in &raw.data_sections {
        match records::decode_column_record(section.header, section.content, limits, &mut usage) {
            Ok(decoded) => {
                match associate_dataset(&decoded.dataset_name, &parsed.windows) {
                    DatasetAssociation::Window {
                        index,
                        prefix_bytes,
                    } => {
                        let column = make_column(decoded, Some(prefix_bytes))?;
                        let window = parsed.windows.get_mut(index).ok_or(
                            OriginError::ArithmeticOverflow {
                                resource: "associated Origin window",
                            },
                        )?;
                        metadata::try_reserve(
                            &mut window.columns,
                            1,
                            "Origin worksheet columns",
                            limits,
                            &mut usage,
                        )?;
                        window.columns.push(column);
                    }
                    DatasetAssociation::ExactWindow => {
                        unsupported_columns =
                            checked_add(unsupported_columns, 1, "unsupported Origin columns")?;
                    }
                    DatasetAssociation::Fallback => {
                        metadata::push_diagnostic(
                            &mut parsed.diagnostics,
                            OriginDiagnosticCode::DecodingWarning,
                            "A worksheet column had no unambiguous Origin window; PlotX kept it in Unmatched Origin data.",
                            None,
                            limits,
                            &mut usage,
                        )?;
                        let column = make_column(decoded, None)?;
                        metadata::try_reserve(
                            &mut fallback_columns,
                            1,
                            "unmatched Origin worksheet columns",
                            limits,
                            &mut usage,
                        )?;
                        fallback_columns.push(column);
                    }
                }
            }
            Err(OriginError::UnsupportedFeature { .. }) => {
                // The Task 4 decoder emits UnsupportedFeature only while
                // classifying a fully framed column header, before it reads or
                // interprets that column's payload. The outer data-section
                // bounds therefore make this individual skip deterministic.
                unsupported_columns =
                    checked_add(unsupported_columns, 1, "unsupported Origin columns")?;
            }
            Err(error) => return Err(error),
        }
    }

    if unsupported_columns > 0 {
        metadata::push_summary(
            &mut parsed.unsupported_objects,
            "worksheet columns",
            unsupported_columns,
            limits,
            &mut usage,
        )?;
        metadata::push_diagnostic(
            &mut parsed.diagnostics,
            OriginDiagnosticCode::UnsupportedColumnSkipped,
            "PlotX skipped independently framed Origin columns whose value layouts are not verified.",
            None,
            limits,
            &mut usage,
        )?;
    }

    let unused_windows = parsed
        .windows
        .iter()
        .filter(|window| window.columns.is_empty())
        .count();
    if unused_windows > 0 {
        metadata::push_summary(
            &mut parsed.unsupported_objects,
            "unsupported window records",
            unused_windows,
            limits,
            &mut usage,
        )?;
        metadata::push_diagnostic(
            &mut parsed.diagnostics,
            OriginDiagnosticCode::UnsupportedObjectSkipped,
            "PlotX skipped Origin window records that had no supported worksheet columns.",
            None,
            limits,
            &mut usage,
        )?;
    }

    let workbooks = assemble_workbooks(parsed.windows, fallback_columns, limits, &mut usage)?;
    Ok(OriginProject {
        probe,
        parameters: parsed.parameters,
        notes: parsed.notes,
        workbooks,
        diagnostics: parsed.diagnostics,
        unsupported_objects: parsed.unsupported_objects,
        resource_usage: usage,
    })
}

enum DatasetAssociation {
    Window { index: usize, prefix_bytes: usize },
    ExactWindow,
    Fallback,
}

fn associate_dataset(dataset_name: &str, windows: &[metadata::WindowInfo]) -> DatasetAssociation {
    let mut best = None;
    let mut best_len = 0_usize;
    let mut ambiguous = false;
    let mut exact = false;

    for (index, window) in windows.iter().enumerate() {
        let Some(window_name) = window.name.as_deref() else {
            continue;
        };
        if dataset_name == window_name {
            exact = true;
            continue;
        }
        let Some(suffix) = dataset_name
            .strip_prefix(window_name)
            .and_then(|rest| rest.strip_prefix('_'))
        else {
            continue;
        };
        if suffix.is_empty() {
            continue;
        }

        match window_name.len().cmp(&best_len) {
            std::cmp::Ordering::Greater => {
                best = Some(index);
                best_len = window_name.len();
                ambiguous = false;
            }
            std::cmp::Ordering::Equal => ambiguous = true,
            std::cmp::Ordering::Less => {}
        }
    }

    if exact {
        return DatasetAssociation::ExactWindow;
    }
    match (best, ambiguous) {
        (Some(index), false) => DatasetAssociation::Window {
            index,
            prefix_bytes: best_len,
        },
        _ => DatasetAssociation::Fallback,
    }
}

fn make_column(
    decoded: records::DecodedColumnRecord,
    prefix_bytes: Option<usize>,
) -> Result<OriginColumn, OriginError> {
    let mut name = decoded.dataset_name;
    if let Some(prefix_bytes) = prefix_bytes {
        if !name.is_char_boundary(prefix_bytes) || name.get(prefix_bytes..).is_none() {
            return Err(OriginError::CorruptStructure {
                offset: 0,
                detail: "an associated Origin dataset prefix is not a text boundary".to_owned(),
            });
        }
        name.replace_range(..prefix_bytes, "");
        if name.as_bytes().first() == Some(&b'_') {
            name.remove(0);
        }
    }
    Ok(OriginColumn {
        name,
        long_name: None,
        role: None,
        units: None,
        comments: None,
        column_type: decoded.column_type,
        cells: decoded.cells,
    })
}

fn assemble_workbooks(
    windows: Vec<metadata::WindowInfo>,
    fallback_columns: Vec<OriginColumn>,
    limits: &OriginLimits,
    usage: &mut OriginResourceUsage,
) -> Result<Vec<OriginWorkbook>, OriginError> {
    let named_workbooks = windows
        .iter()
        .filter(|window| !window.columns.is_empty())
        .count();
    let workbook_count = checked_add(
        named_workbooks,
        usize::from(!fallback_columns.is_empty()),
        "workbooks",
    )?;
    if workbook_count == 0 {
        return Err(OriginError::NoSupportedWorksheet);
    }
    enforce_count("workbooks", workbook_count, limits.max_workbooks)?;
    enforce_count(
        "worksheets per workbook",
        1,
        limits.max_worksheets_per_workbook,
    )?;
    let has_supported_rows = windows
        .iter()
        .any(|window| window.columns.iter().any(|column| !column.cells.is_empty()))
        || fallback_columns
            .iter()
            .any(|column| !column.cells.is_empty());
    if !has_supported_rows {
        return Err(OriginError::NoSupportedWorksheet);
    }

    let mut workbooks = Vec::new();
    metadata::try_reserve(
        &mut workbooks,
        workbook_count,
        "Origin workbooks",
        limits,
        usage,
    )?;
    for window in windows {
        if window.columns.is_empty() {
            continue;
        }
        let name = window.name.ok_or(OriginError::CorruptStructure {
            offset: 0,
            detail: "a supported Origin worksheet lost its validated window name".to_owned(),
        })?;
        push_workbook(&mut workbooks, name, window.columns, limits, usage)?;
    }
    if !fallback_columns.is_empty() {
        let name = metadata::copy_generated_text("Unmatched Origin data", limits, usage)?;
        push_workbook(&mut workbooks, name, fallback_columns, limits, usage)?;
    }

    usage.workbooks = workbook_count;
    usage.worksheets = workbook_count;
    Ok(workbooks)
}

fn push_workbook(
    workbooks: &mut Vec<OriginWorkbook>,
    name: String,
    columns: Vec<OriginColumn>,
    limits: &OriginLimits,
    usage: &mut OriginResourceUsage,
) -> Result<(), OriginError> {
    let row_count = columns
        .iter()
        .map(|column| column.cells.len())
        .max()
        .unwrap_or(0);
    let worksheet_name = metadata::copy_generated_text("Sheet1", limits, usage)?;
    let mut worksheets = Vec::new();
    metadata::try_reserve(&mut worksheets, 1, "Origin worksheets", limits, usage)?;
    worksheets.push(OriginWorksheet {
        name: worksheet_name,
        columns,
        row_count,
        metadata: Vec::new(),
    });
    workbooks.push(OriginWorkbook { name, worksheets });
    Ok(())
}

fn enforce_count(resource: &'static str, actual: usize, limit: usize) -> Result<(), OriginError> {
    if actual > limit {
        return Err(OriginError::LimitExceeded {
            resource,
            limit,
            actual,
        });
    }
    Ok(())
}

pub(super) fn parse_raw<'a>(
    bytes: &'a [u8],
    limits: &OriginLimits,
    initial_parser_bytes: usize,
) -> Result<RawOpjProject<'a>, OriginError> {
    let mut reader = Reader::new_with_parser_bytes(bytes, limits, initial_parser_bytes)?;
    let signature = reader.read_slice(SIGNATURE.len())?;
    if signature != SIGNATURE {
        return Err(OriginError::UnsupportedVersion {
            raw_version: "classic OPJ signature does not match Origin7V552".to_owned(),
        });
    }

    // OpenOPJ documents the Origin header as one data block followed by a null
    // block, with the Origin version f64 at payload offset 0x1b:
    // https://github.com/jgonera/openopj/blob/42ddcf1eb3a490744c54fca0a4ed6fe7a5e723ca/docs/opj_format.markdown
    // https://github.com/jgonera/openopj/blob/42ddcf1eb3a490744c54fca0a4ed6fe7a5e723ca/lib/OpenOPJ/OPJFile.php
    let origin_header_block = reader.read_block()?;
    let (origin_header_offset, origin_header) = require_data_block(
        origin_header_block,
        "the Origin header must be a data block",
    )?;
    require_exact_length(
        origin_header_offset,
        origin_header,
        ORIGIN_HEADER_LEN,
        "Origin header payload",
    )?;
    validate_embedded_origin_version(origin_header_offset, origin_header)?;

    let header_terminator = reader.read_block()?;
    require_null_block(
        header_terminator,
        "the Origin header must end with a null block",
    )?;

    let mut data_sections = Vec::new();
    loop {
        // The verified Origin7V552 data list is a sequence of
        // <123-byte header block><content block or null><null block>, followed
        // by a consumed list null. This is the bounded portion described by the
        // pinned MIT OpenOPJ sources; parsing stops at that exact boundary.
        // https://github.com/jgonera/openopj/blob/42ddcf1eb3a490744c54fca0a4ed6fe7a5e723ca/docs/opj_format.markdown
        // https://github.com/jgonera/openopj/blob/42ddcf1eb3a490744c54fca0a4ed6fe7a5e723ca/lib/OpenOPJ/OPJFile.php
        let header_block = reader.read_block()?;
        let (header_offset, header) = match header_block {
            FramedBlock::Null { .. } => break,
            FramedBlock::Data { offset, payload } => (offset, payload),
        };
        require_exact_length(
            header_offset,
            header,
            DATA_HEADER_LEN,
            "Origin7V552 data-header payload",
        )?;

        let content = match reader.read_block()? {
            FramedBlock::Null { .. } => None,
            FramedBlock::Data { payload, .. } => Some(payload),
        };
        let section_terminator = reader.read_block()?;
        require_null_block(
            section_terminator,
            "an Origin data section must end with a null block",
        )?;

        let section_count = checked_add(data_sections.len(), 1, "raw OPJ data sections")?;
        if section_count > limits.max_columns {
            return Err(OriginError::LimitExceeded {
                resource: "data sections",
                limit: limits.max_columns,
                actual: section_count,
            });
        }
        reader.try_reserve(&mut data_sections, 1, "raw OPJ data sections")?;
        data_sections.push(RawOpjDataSection { header, content });
    }

    let remaining = bytes
        .get(reader.offset()..)
        .ok_or(OriginError::ArithmeticOverflow {
            resource: "remaining OPJ structure",
        })?;
    let resource_usage = reader.into_usage();
    Ok(RawOpjProject {
        origin_header,
        data_sections,
        remaining,
        resource_usage,
    })
}

fn validate_embedded_origin_version(
    header_block_offset: usize,
    header: &[u8],
) -> Result<(), OriginError> {
    let version_payload_delta = checked_add(
        BLOCK_PREFIX_LEN,
        ORIGIN_VERSION_OFFSET,
        "embedded Origin version offset",
    )?;
    let version_offset_in_file = checked_add(
        header_block_offset,
        version_payload_delta,
        "embedded Origin version offset",
    )?;
    let version_end = checked_add(
        ORIGIN_VERSION_OFFSET,
        size_of::<f64>(),
        "embedded Origin version range",
    )?;
    let available_version_bytes =
        header
            .len()
            .checked_sub(ORIGIN_VERSION_OFFSET)
            .ok_or(OriginError::ArithmeticOverflow {
                resource: "embedded Origin version bytes",
            })?;
    let version_bytes =
        header
            .get(ORIGIN_VERSION_OFFSET..version_end)
            .ok_or(OriginError::Truncated {
                offset: version_offset_in_file,
                needed: size_of::<f64>(),
                have: available_version_bytes,
            })?;
    let version_array: [u8; 8] = version_bytes
        .try_into()
        .map_err(|_| OriginError::Truncated {
            offset: version_offset_in_file,
            needed: size_of::<f64>(),
            have: version_bytes.len(),
        })?;
    let version = f64::from_le_bytes(version_array);
    if version.to_bits() != EXPECTED_ORIGIN_VERSION.to_bits() {
        return Err(OriginError::UnsupportedVersion {
            raw_version: format!("4.2673 552 header embeds Origin {version}"),
        });
    }
    Ok(())
}

fn require_data_block<'a>(
    block: FramedBlock<'a>,
    detail: &'static str,
) -> Result<(usize, &'a [u8]), OriginError> {
    match block {
        FramedBlock::Data { offset, payload } => Ok((offset, payload)),
        FramedBlock::Null { offset } => Err(OriginError::CorruptStructure {
            offset,
            detail: detail.to_owned(),
        }),
    }
}

fn require_null_block(block: FramedBlock<'_>, detail: &'static str) -> Result<(), OriginError> {
    match block {
        FramedBlock::Null { .. } => Ok(()),
        FramedBlock::Data { .. } => Err(OriginError::CorruptStructure {
            offset: block.offset(),
            detail: detail.to_owned(),
        }),
    }
}

fn require_exact_length(
    block_offset: usize,
    payload: &[u8],
    expected: usize,
    field: &'static str,
) -> Result<(), OriginError> {
    if payload.len() != expected {
        return Err(OriginError::CorruptStructure {
            offset: block_offset,
            detail: format!("{field} must be exactly {expected} bytes"),
        });
    }
    Ok(())
}
