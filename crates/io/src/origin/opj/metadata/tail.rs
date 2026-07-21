use std::mem::size_of;

use crate::origin::{
    OriginDiagnostic, OriginDiagnosticCode, OriginError, OriginResourceUsage,
    OriginUnsupportedObjectSummary,
};

use super::cursor::{MetadataBlock, MetadataCursor};
use super::{push_diagnostic, push_summary};
use crate::origin::reader::checked_add;

const TREE_SPAN_PAYLOAD_LEN: usize = size_of::<u32>();
const ATTACHMENT_HEADER_LEN: usize = 52;
const ATTACHMENT_TYPE_OLE: usize = 0x7fca_0459;
const OLE_COMPOUND_SIGNATURE: &[u8] = &[0xd0, 0xcf, 0x11, 0xe0, 0xa1, 0xb1, 0x1a, 0xe1];

pub(super) fn parse(
    cursor: &mut MetadataCursor<'_>,
    diagnostics: &mut Vec<OriginDiagnostic>,
    unsupported_objects: &mut Vec<OriginUnsupportedObjectSummary>,
    usage: &mut OriginResourceUsage,
) -> Result<(), OriginError> {
    parse_project_tree(cursor)?;
    push_summary(unsupported_objects, "project tree", 1, cursor.limits, usage)?;
    push_diagnostic(
        diagnostics,
        OriginDiagnosticCode::UnsupportedObjectSkipped,
        "PlotX preserved worksheet data but did not import the bounded Origin project tree.",
        None,
        cursor.limits,
        usage,
    )?;

    let attachment_count = parse_attachments(cursor)?;
    if attachment_count > 0 {
        push_summary(
            unsupported_objects,
            "embedded attachments",
            attachment_count,
            cursor.limits,
            usage,
        )?;
        push_diagnostic(
            diagnostics,
            OriginDiagnosticCode::UnsupportedObjectSkipped,
            "PlotX did not extract, open, or execute bounded embedded Origin attachments.",
            None,
            cursor.limits,
            usage,
        )?;
    }
    Ok(())
}

fn parse_project_tree(cursor: &mut MetadataCursor<'_>) -> Result<(), OriginError> {
    let tree_start = cursor.relative_offset();
    let (block_offset, span_payload) = match cursor.read_block()? {
        MetadataBlock::Data { offset, payload } => (offset, payload),
        MetadataBlock::Null { offset } => {
            return Err(OriginError::CorruptStructure {
                offset,
                detail: "the Origin project tree requires a bounded span block".to_owned(),
            });
        }
    };
    cursor.charge_record()?;
    if span_payload.len() != TREE_SPAN_PAYLOAD_LEN {
        return Err(OriginError::CorruptStructure {
            offset: block_offset,
            detail: "the Origin7V552 project-tree span must be a 4-byte value".to_owned(),
        });
    }
    let span_bytes: [u8; 4] =
        span_payload
            .try_into()
            .map_err(|_| OriginError::CorruptStructure {
                offset: block_offset,
                detail: "the Origin7V552 project-tree span is incomplete".to_owned(),
            })?;
    let declared_span = usize::try_from(u32::from_le_bytes(span_bytes)).map_err(|_| {
        OriginError::ArithmeticOverflow {
            resource: "Origin project-tree span",
        }
    })?;
    enforce_limit("block bytes", declared_span, cursor.limits.max_block_bytes)?;
    let consumed = cursor.relative_offset().checked_sub(tree_start).ok_or(
        OriginError::ArithmeticOverflow {
            resource: "Origin project-tree span",
        },
    )?;
    let remaining_span =
        declared_span
            .checked_sub(consumed)
            .ok_or_else(|| OriginError::CorruptStructure {
                offset: block_offset,
                detail: "the Origin project-tree span is shorter than its framing".to_owned(),
            })?;

    // OpenOPJ documents the project tree as the section following notes. The
    // public MIT fixture independently supplies this outer span, so PlotX can
    // skip exactly to the attachment header without interpreting tree names,
    // paths, or executable content.
    // https://github.com/jgonera/openopj/blob/42ddcf1eb3a490744c54fca0a4ed6fe7a5e723ca/docs/opj_format.markdown
    cursor.skip_exact(remaining_span)
}

fn parse_attachments(cursor: &mut MetadataCursor<'_>) -> Result<usize, OriginError> {
    let mut count = 0_usize;
    while cursor.remaining() > 0 {
        cursor.charge_record()?;
        let header_offset = cursor.absolute_offset()?;
        let header = cursor.read_exact(ATTACHMENT_HEADER_LEN)?;
        let header_len = read_u32(header, 0, header_offset, "attachment header size")?;
        if header_len != ATTACHMENT_HEADER_LEN {
            return Err(OriginError::UnsupportedFeature {
                feature: "an Origin attachment header layout is not verified".to_owned(),
            });
        }
        let attachment_type = read_u32(header, 4, header_offset, "attachment type")?;
        if attachment_type != ATTACHMENT_TYPE_OLE {
            return Err(OriginError::UnsupportedFeature {
                feature: "an embedded Origin attachment type is not supported".to_owned(),
            });
        }
        let payload_size = read_u32(header, 8, header_offset, "attachment payload size")?;
        enforce_limit("block bytes", payload_size, cursor.limits.max_block_bytes)?;
        let payload = cursor.read_exact(payload_size)?;
        if !payload.starts_with(OLE_COMPOUND_SIGNATURE) {
            return Err(OriginError::UnsupportedFeature {
                feature: "an embedded Origin attachment payload is not a verified OLE object"
                    .to_owned(),
            });
        }
        count = checked_add(count, 1, "embedded Origin attachments")?;
    }
    Ok(count)
}

fn read_u32(
    bytes: &[u8],
    offset: usize,
    file_offset: usize,
    resource: &'static str,
) -> Result<usize, OriginError> {
    let end = checked_add(offset, size_of::<u32>(), resource)?;
    let value = bytes.get(offset..end).ok_or(OriginError::Truncated {
        offset: checked_add(file_offset, offset, resource)?,
        needed: size_of::<u32>(),
        have: bytes.len().saturating_sub(offset),
    })?;
    let value_offset = checked_add(file_offset, offset, resource)?;
    let value: [u8; 4] = value.try_into().map_err(|_| OriginError::Truncated {
        offset: value_offset,
        needed: size_of::<u32>(),
        have: value.len(),
    })?;
    usize::try_from(u32::from_le_bytes(value))
        .map_err(|_| OriginError::ArithmeticOverflow { resource })
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
