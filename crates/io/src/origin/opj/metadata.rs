use std::fmt::{self, Write as _};
use std::mem::size_of;

use crate::origin::{
    OriginColumn, OriginDiagnostic, OriginDiagnosticCode, OriginDiagnosticSeverity, OriginError,
    OriginLimits, OriginMetadataEntry, OriginNote, OriginObjectLocation, OriginResourceUsage,
    OriginUnsupportedObjectSummary,
};

use super::super::reader::{checked_add, checked_mul};
use cursor::{MetadataBlock, MetadataCursor};

mod cursor;
mod tail;

const BLOCK_PREFIX_LEN: usize = 5;
const WINDOW_NAME_OFFSET: usize = 2;
const WINDOW_NAME_WIDTH: usize = 25;
const WINDOW_HEADER_MIN_LEN: usize = WINDOW_NAME_OFFSET + WINDOW_NAME_WIDTH;
const AXIS_PARAMETER_LISTS: usize = 3;
const FORMATTED_F64_CAPACITY: usize = 32;

pub(super) struct WindowInfo {
    pub(super) name: Option<String>,
    pub(super) columns: Vec<OriginColumn>,
}

pub(super) struct ParsedMetadata {
    pub(super) windows: Vec<WindowInfo>,
    pub(super) parameters: Vec<OriginMetadataEntry>,
    pub(super) notes: Vec<OriginNote>,
    pub(super) diagnostics: Vec<OriginDiagnostic>,
    pub(super) unsupported_objects: Vec<OriginUnsupportedObjectSummary>,
}

pub(super) fn parse(
    bytes: &[u8],
    base_offset: usize,
    limits: &OriginLimits,
    usage: &mut OriginResourceUsage,
) -> Result<ParsedMetadata, OriginError> {
    let mut cursor = MetadataCursor::new(bytes, base_offset, limits);
    let mut diagnostics = Vec::new();
    let mut unsupported_objects = Vec::new();

    let (windows, presentation_records) = parse_windows(&mut cursor, usage, &mut diagnostics)?;
    if presentation_records > 0 {
        push_summary(
            &mut unsupported_objects,
            "window presentation records",
            presentation_records,
            limits,
            usage,
        )?;
        push_diagnostic(
            &mut diagnostics,
            OriginDiagnosticCode::UnsupportedObjectSkipped,
            "PlotX imported worksheet values but skipped bounded Origin window presentation records.",
            None,
            limits,
            usage,
        )?;
    }
    let parameters = parse_parameters(&mut cursor, usage, &mut diagnostics)?;
    let (notes, note_properties) = parse_notes(&mut cursor, usage, &mut diagnostics)?;
    if note_properties > 0 {
        push_summary(
            &mut unsupported_objects,
            "note properties",
            note_properties,
            limits,
            usage,
        )?;
        push_diagnostic(
            &mut diagnostics,
            OriginDiagnosticCode::MetadataSkipped,
            "PlotX imported note names and text but skipped bounded Origin note properties.",
            None,
            limits,
            usage,
        )?;
    }

    tail::parse(
        &mut cursor,
        &mut diagnostics,
        &mut unsupported_objects,
        usage,
    )?;

    Ok(ParsedMetadata {
        windows,
        parameters,
        notes,
        diagnostics,
        unsupported_objects,
    })
}

fn parse_windows(
    cursor: &mut MetadataCursor<'_>,
    usage: &mut OriginResourceUsage,
    diagnostics: &mut Vec<OriginDiagnostic>,
) -> Result<(Vec<WindowInfo>, usize), OriginError> {
    enforce_depth(1, cursor.limits)?;
    let mut windows = Vec::new();
    let mut presentation_records = 0_usize;

    loop {
        let header = match cursor.read_block()? {
            MetadataBlock::Null { .. } => break,
            MetadataBlock::Data { offset, payload } => (offset, payload),
        };
        let window_count = checked_add(windows.len(), 1, "Origin window records")?;
        enforce_limit(
            "Origin window records",
            window_count,
            cursor.limits.max_columns,
        )?;

        // This exact header-plus-layer-list traversal is reimplemented from
        // the pinned MIT OpenOPJ Origin 7.0552 WindowList description:
        // https://github.com/jgonera/openopj/blob/42ddcf1eb3a490744c54fca0a4ed6fe7a5e723ca/lib/OpenOPJ/WindowList.php
        let name = match decode_window_name(header.1, header.0, cursor.limits, usage) {
            Ok(name) if !name.is_empty() => Some(name),
            Ok(_) => {
                push_diagnostic(
                    diagnostics,
                    OriginDiagnosticCode::MetadataSkipped,
                    "PlotX skipped an empty Origin window name after validating its boundaries.",
                    Some(header.0),
                    cursor.limits,
                    usage,
                )?;
                None
            }
            Err(OriginError::UnsupportedEncoding { .. }) => {
                push_diagnostic(
                    diagnostics,
                    OriginDiagnosticCode::MetadataSkipped,
                    "PlotX skipped a non-ASCII Origin window name after validating its boundaries.",
                    Some(header.0),
                    cursor.limits,
                    usage,
                )?;
                None
            }
            Err(error) => return Err(error),
        };

        presentation_records = checked_add(
            presentation_records,
            walk_layer_list(cursor, 2)?,
            "Origin window presentation records",
        )?;
        try_reserve(
            &mut windows,
            1,
            "Origin window records",
            cursor.limits,
            usage,
        )?;
        windows.push(WindowInfo {
            name,
            columns: Vec::new(),
        });
    }
    Ok((windows, presentation_records))
}

fn walk_layer_list(cursor: &mut MetadataCursor<'_>, depth: usize) -> Result<usize, OriginError> {
    enforce_depth(depth, cursor.limits)?;
    let mut layers = 0_usize;
    let mut records = 0_usize;
    loop {
        match cursor.read_block()? {
            MetadataBlock::Null { .. } => break,
            MetadataBlock::Data { .. } => {}
        }
        layers = checked_add(layers, 1, "Origin layer records")?;
        enforce_limit("Origin layer records", layers, cursor.limits.max_columns)?;
        records = checked_add(records, 1, "Origin window presentation records")?;

        records = checked_add(
            records,
            walk_fixed_block_list(cursor, checked_add(depth, 1, "metadata nesting depth")?, 4)?,
            "Origin window presentation records",
        )?;
        records = checked_add(
            records,
            walk_fixed_block_list(cursor, checked_add(depth, 1, "metadata nesting depth")?, 2)?,
            "Origin window presentation records",
        )?;
        records = checked_add(
            records,
            walk_fixed_block_list(cursor, checked_add(depth, 1, "metadata nesting depth")?, 1)?,
            "Origin window presentation records",
        )?;
        for _ in 0..AXIS_PARAMETER_LISTS {
            records = checked_add(
                records,
                walk_fixed_block_list(cursor, checked_add(depth, 1, "metadata nesting depth")?, 1)?,
                "Origin window presentation records",
            )?;
        }
    }
    Ok(records)
}

fn walk_fixed_block_list(
    cursor: &mut MetadataCursor<'_>,
    depth: usize,
    blocks_per_item: usize,
) -> Result<usize, OriginError> {
    enforce_depth(depth, cursor.limits)?;
    let mut items = 0_usize;
    loop {
        match cursor.read_block()? {
            MetadataBlock::Null { .. } => break,
            MetadataBlock::Data { .. } => {}
        }
        items = checked_add(items, 1, "Origin nested records")?;
        enforce_limit("Origin nested records", items, cursor.limits.max_columns)?;
        for _ in 1..blocks_per_item {
            let _ = cursor.read_block()?;
        }
    }
    Ok(items)
}

fn parse_parameters(
    cursor: &mut MetadataCursor<'_>,
    usage: &mut OriginResourceUsage,
    diagnostics: &mut Vec<OriginDiagnostic>,
) -> Result<Vec<OriginMetadataEntry>, OriginError> {
    // Origin7V552 parameters use an LF-terminated name, one little-endian f64
    // plus LF, and a NUL-name terminator as described by pinned MIT OpenOPJ:
    // https://github.com/jgonera/openopj/blob/42ddcf1eb3a490744c54fca0a4ed6fe7a5e723ca/lib/OpenOPJ/ParametersSection.php
    let mut parameters = Vec::new();
    loop {
        let (name_offset, name_bytes) = cursor.read_line()?;
        if name_bytes == [0] {
            break;
        }
        if name_bytes.is_empty() {
            return Err(OriginError::CorruptStructure {
                offset: name_offset,
                detail: "an Origin parameter name cannot be empty".to_owned(),
            });
        }

        let value_offset = cursor.absolute_offset()?;
        let value_bytes = cursor.read_parameter_value()?;
        let value = f64::from_le_bytes(value_bytes);
        let parameter_count = checked_add(parameters.len(), 1, "Origin project parameters")?;
        enforce_limit(
            "Origin project parameters",
            parameter_count,
            cursor.limits.max_columns,
        )?;

        let name = match validate_ascii(name_bytes, name_offset, "Origin parameter name") {
            Ok(name) => name,
            Err(OriginError::UnsupportedEncoding { .. }) => {
                push_diagnostic(
                    diagnostics,
                    OriginDiagnosticCode::MetadataSkipped,
                    "PlotX skipped a non-ASCII Origin parameter after validating its value boundary.",
                    Some(name_offset),
                    cursor.limits,
                    usage,
                )?;
                continue;
            }
            Err(error) => return Err(error),
        };
        if !value.is_finite() {
            push_diagnostic(
                diagnostics,
                OriginDiagnosticCode::MetadataSkipped,
                "PlotX skipped a non-finite Origin parameter value.",
                Some(value_offset),
                cursor.limits,
                usage,
            )?;
            continue;
        }

        let key = copy_decoded_text(name, cursor.limits, usage)?;
        let formatted = format_f64(value, value_offset)?;
        let value = copy_parser_text(formatted, cursor.limits, usage)?;
        try_reserve(
            &mut parameters,
            1,
            "Origin project parameters",
            cursor.limits,
            usage,
        )?;
        parameters.push(OriginMetadataEntry { key, value });
    }

    require_null_block(
        cursor.read_block()?,
        "the Origin parameter section must end with a null block",
    )?;
    Ok(parameters)
}

fn parse_notes(
    cursor: &mut MetadataCursor<'_>,
    usage: &mut OriginResourceUsage,
    diagnostics: &mut Vec<OriginDiagnostic>,
) -> Result<(Vec<OriginNote>, usize), OriginError> {
    // Each note is exactly header/name/content framed blocks. PlotX retains
    // only bounded ASCII name/content and reports the opaque header properties:
    // https://github.com/jgonera/openopj/blob/42ddcf1eb3a490744c54fca0a4ed6fe7a5e723ca/lib/OpenOPJ/NoteSection.php
    let mut notes = Vec::new();
    let mut note_properties = 0_usize;
    loop {
        let header_offset = match cursor.read_block()? {
            MetadataBlock::Null { .. } => break,
            MetadataBlock::Data { offset, .. } => offset,
        };
        note_properties = checked_add(note_properties, 1, "Origin note properties")?;
        enforce_limit(
            "Origin note properties",
            note_properties,
            cursor.limits.max_columns,
        )?;
        let (name_offset, name_block) = require_data_block(
            cursor.read_block()?,
            "an Origin note name must be a data block",
        )?;
        let (content_offset, content_block) = require_data_block(
            cursor.read_block()?,
            "an Origin note content must be a data block",
        )?;

        let name = validate_nul_terminated_ascii(
            name_block,
            name_offset,
            cursor.limits,
            "Origin note name",
        );
        let content = validate_nul_terminated_ascii(
            content_block,
            content_offset,
            cursor.limits,
            "Origin note content",
        );
        let (name, content) = match (name, content) {
            (Ok(name), Ok(content)) => (name, content),
            (Err(OriginError::UnsupportedEncoding { .. }), _)
            | (_, Err(OriginError::UnsupportedEncoding { .. })) => {
                push_diagnostic(
                    diagnostics,
                    OriginDiagnosticCode::MetadataSkipped,
                    "PlotX skipped a non-ASCII Origin note after validating its block boundaries.",
                    Some(header_offset),
                    cursor.limits,
                    usage,
                )?;
                continue;
            }
            (Err(error), _) | (_, Err(error)) => return Err(error),
        };

        let note_count = checked_add(notes.len(), 1, "Origin project notes")?;
        enforce_limit(
            "Origin project notes",
            note_count,
            cursor.limits.max_columns,
        )?;
        let name = copy_decoded_text(name, cursor.limits, usage)?;
        let content = copy_decoded_text(content, cursor.limits, usage)?;
        try_reserve(&mut notes, 1, "Origin project notes", cursor.limits, usage)?;
        notes.push(OriginNote { name, content });
    }
    Ok((notes, note_properties))
}

fn decode_window_name(
    header: &[u8],
    block_offset: usize,
    limits: &OriginLimits,
    usage: &mut OriginResourceUsage,
) -> Result<String, OriginError> {
    if header.len() < WINDOW_HEADER_MIN_LEN {
        return Err(OriginError::CorruptStructure {
            offset: block_offset,
            detail: format!(
                "an Origin7V552 window header must contain at least {WINDOW_HEADER_MIN_LEN} bytes"
            ),
        });
    }
    let end = checked_add(
        WINDOW_NAME_OFFSET,
        WINDOW_NAME_WIDTH,
        "Origin window name range",
    )?;
    let field = header
        .get(WINDOW_NAME_OFFSET..end)
        .ok_or(OriginError::Truncated {
            offset: block_offset,
            needed: WINDOW_HEADER_MIN_LEN,
            have: header.len(),
        })?;
    let length = field
        .iter()
        .position(|byte| *byte == 0)
        .unwrap_or(field.len());
    let text = field.get(..length).ok_or(OriginError::ArithmeticOverflow {
        resource: "Origin window name",
    })?;
    let text_offset = checked_add(
        block_offset,
        checked_add(
            BLOCK_PREFIX_LEN,
            WINDOW_NAME_OFFSET,
            "Origin window name offset",
        )?,
        "Origin window name offset",
    )?;
    let text = validate_ascii(text, text_offset, "Origin window name")?;
    copy_decoded_text(text, limits, usage)
}

fn validate_nul_terminated_ascii<'a>(
    bytes: &'a [u8],
    offset: usize,
    limits: &OriginLimits,
    field: &'static str,
) -> Result<&'a str, OriginError> {
    let Some(text) = bytes.strip_suffix(&[0]) else {
        return Err(OriginError::CorruptStructure {
            offset,
            detail: format!("{field} must end with a NUL byte inside its framed block"),
        });
    };
    if let Some(relative) = text.iter().position(|byte| *byte == 0) {
        return Err(OriginError::CorruptStructure {
            offset: checked_add(offset, relative, "embedded metadata NUL offset")?,
            detail: format!("{field} contains an embedded NUL byte"),
        });
    }
    enforce_limit("string bytes", text.len(), limits.max_string_bytes)?;
    validate_ascii(text, offset, field)
}

fn validate_ascii<'a>(
    bytes: &'a [u8],
    offset: usize,
    field: &'static str,
) -> Result<&'a str, OriginError> {
    if let Some(relative) = bytes.iter().position(|byte| !byte.is_ascii()) {
        return Err(OriginError::UnsupportedEncoding {
            offset: checked_add(offset, relative, "metadata ASCII offset")?,
            encoding: format!("non-ASCII byte in {field}"),
        });
    }
    std::str::from_utf8(bytes).map_err(|_| OriginError::UnsupportedEncoding {
        offset,
        encoding: format!("non-ASCII byte in {field}"),
    })
}

fn format_f64(value: f64, offset: usize) -> Result<FixedText, OriginError> {
    let mut output = FixedText::default();
    write!(&mut output, "{value}").map_err(|_| OriginError::CorruptStructure {
        offset,
        detail: "an Origin parameter value could not be represented as bounded text".to_owned(),
    })?;
    Ok(output)
}

#[derive(Default)]
struct FixedText {
    bytes: [u8; FORMATTED_F64_CAPACITY],
    len: usize,
}

impl FixedText {
    fn as_str(&self) -> Result<&str, OriginError> {
        std::str::from_utf8(
            self.bytes
                .get(..self.len)
                .ok_or(OriginError::ArithmeticOverflow {
                    resource: "formatted Origin parameter",
                })?,
        )
        .map_err(|_| OriginError::CorruptStructure {
            offset: 0,
            detail: "formatted Origin parameter text is not ASCII".to_owned(),
        })
    }
}

impl fmt::Write for FixedText {
    fn write_str(&mut self, text: &str) -> fmt::Result {
        let end = self.len.checked_add(text.len()).ok_or(fmt::Error)?;
        let destination = self.bytes.get_mut(self.len..end).ok_or(fmt::Error)?;
        destination.copy_from_slice(text.as_bytes());
        self.len = end;
        Ok(())
    }
}

fn copy_decoded_text(
    text: &str,
    limits: &OriginLimits,
    usage: &mut OriginResourceUsage,
) -> Result<String, OriginError> {
    enforce_limit("string bytes", text.len(), limits.max_string_bytes)?;
    charge_text(text.len(), limits, usage)?;
    copy_after_charge(text, "decoded Origin metadata")
}

fn copy_parser_text(
    text: FixedText,
    limits: &OriginLimits,
    usage: &mut OriginResourceUsage,
) -> Result<String, OriginError> {
    let text = text.as_str()?;
    enforce_limit("string bytes", text.len(), limits.max_string_bytes)?;
    charge_parser(text.len(), limits, usage)?;
    copy_after_charge(text, "formatted Origin parameter")
}

fn copy_static_text(
    text: &'static str,
    limits: &OriginLimits,
    usage: &mut OriginResourceUsage,
) -> Result<String, OriginError> {
    charge_parser(text.len(), limits, usage)?;
    copy_after_charge(text, "Origin diagnostic text")
}

pub(super) fn copy_generated_text(
    text: &'static str,
    limits: &OriginLimits,
    usage: &mut OriginResourceUsage,
) -> Result<String, OriginError> {
    copy_static_text(text, limits, usage)
}

fn copy_after_charge(text: &str, resource: &'static str) -> Result<String, OriginError> {
    let mut output = String::new();
    output
        .try_reserve_exact(text.len())
        .map_err(|_| OriginError::AllocationFailed {
            resource,
            requested: text.len(),
        })?;
    output.push_str(text);
    Ok(output)
}

pub(super) fn try_reserve<T>(
    values: &mut Vec<T>,
    additional: usize,
    resource: &'static str,
    limits: &OriginLimits,
    usage: &mut OriginResourceUsage,
) -> Result<(), OriginError> {
    let _requested = checked_add(values.len(), additional, resource)?;
    let bytes = checked_mul(additional, size_of::<T>(), resource)?;
    charge_parser(bytes, limits, usage)?;
    values
        .try_reserve_exact(additional)
        .map_err(|_| OriginError::AllocationFailed {
            resource,
            requested: bytes,
        })
}

pub(super) fn push_diagnostic(
    diagnostics: &mut Vec<OriginDiagnostic>,
    code: OriginDiagnosticCode,
    message: &'static str,
    byte_offset: Option<usize>,
    limits: &OriginLimits,
    usage: &mut OriginResourceUsage,
) -> Result<(), OriginError> {
    let message = copy_static_text(message, limits, usage)?;
    try_reserve(diagnostics, 1, "Origin diagnostics", limits, usage)?;
    diagnostics.push(OriginDiagnostic {
        code,
        severity: OriginDiagnosticSeverity::Warning,
        location: byte_offset.map(|byte_offset| OriginObjectLocation {
            workbook: None,
            worksheet: None,
            column: None,
            byte_offset: Some(byte_offset),
        }),
        message,
    });
    Ok(())
}

pub(super) fn push_summary(
    summaries: &mut Vec<OriginUnsupportedObjectSummary>,
    kind: &'static str,
    count: usize,
    limits: &OriginLimits,
    usage: &mut OriginResourceUsage,
) -> Result<(), OriginError> {
    let kind = copy_static_text(kind, limits, usage)?;
    try_reserve(
        summaries,
        1,
        "Origin unsupported-object summaries",
        limits,
        usage,
    )?;
    summaries.push(OriginUnsupportedObjectSummary { kind, count });
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

fn enforce_depth(depth: usize, limits: &OriginLimits) -> Result<(), OriginError> {
    enforce_limit("metadata nesting depth", depth, limits.max_metadata_depth)
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

fn require_data_block<'a>(
    block: MetadataBlock<'a>,
    detail: &'static str,
) -> Result<(usize, &'a [u8]), OriginError> {
    match block {
        MetadataBlock::Data { offset, payload } => Ok((offset, payload)),
        MetadataBlock::Null { offset } => Err(OriginError::CorruptStructure {
            offset,
            detail: detail.to_owned(),
        }),
    }
}

fn require_null_block(block: MetadataBlock<'_>, detail: &'static str) -> Result<(), OriginError> {
    match block {
        MetadataBlock::Null { .. } => Ok(()),
        MetadataBlock::Data { offset, .. } => Err(OriginError::CorruptStructure {
            offset,
            detail: detail.to_owned(),
        }),
    }
}

#[cfg(test)]
#[path = "metadata_tests.rs"]
mod metadata_tests;
