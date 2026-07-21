use std::mem::size_of;

use super::reader::{FramedBlock, Reader, checked_add};
use super::{OriginError, OriginLimits, OriginProbe, OriginProject, OriginResourceUsage};

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
    _probe: OriginProbe,
) -> Result<OriginProject, OriginError> {
    let raw = parse_raw(bytes, limits)?;
    let has_data = !raw.data_sections.is_empty();
    let has_unparsed_structure = !raw.remaining.is_empty();

    // Keep the borrowed framing and its accounting live through dispatch. Later
    // decoding stages consume these exact slices instead of rediscovering bounds.
    let _header_len = raw.origin_header.len();
    let mut usage = raw.resource_usage;
    let _section_bytes = raw
        .data_sections
        .iter()
        .try_fold(0_usize, |total, section| {
            let with_header = checked_add(total, section.header.len(), "raw OPJ section bytes")?;
            checked_add(
                with_header,
                section.content.map_or(0, <[u8]>::len),
                "raw OPJ section bytes",
            )
        })?;

    for section in &raw.data_sections {
        let _decoded =
            records::decode_column_record(section.header, section.content, limits, &mut usage)?;
    }

    if has_data || has_unparsed_structure {
        return Err(OriginError::UnsupportedFeature {
            feature: "classic OPJ data records after validated framing are not decoded yet"
                .to_owned(),
        });
    }
    Err(OriginError::NoSupportedWorksheet)
}

pub(super) fn parse_raw<'a>(
    bytes: &'a [u8],
    limits: &OriginLimits,
) -> Result<RawOpjProject<'a>, OriginError> {
    let mut reader = Reader::new(bytes, limits)?;
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
