use std::mem::size_of;

use super::{OriginError, OriginLimits, OriginResourceUsage};

const LF: u8 = b'\n';

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum FramedBlock<'a> {
    Null { offset: usize },
    Data { offset: usize, payload: &'a [u8] },
}

impl FramedBlock<'_> {
    pub(super) fn offset(&self) -> usize {
        match self {
            Self::Null { offset } | Self::Data { offset, .. } => *offset,
        }
    }
}

pub(super) struct Reader<'bytes, 'limits> {
    bytes: &'bytes [u8],
    offset: usize,
    limits: &'limits OriginLimits,
    usage: OriginResourceUsage,
}

impl<'bytes, 'limits> Reader<'bytes, 'limits> {
    pub(super) fn new(
        bytes: &'bytes [u8],
        limits: &'limits OriginLimits,
    ) -> Result<Self, OriginError> {
        limits.validate()?;
        enforce_limit("input bytes", bytes.len(), limits.max_input_bytes)?;
        enforce_limit(
            "total owned bytes",
            bytes.len(),
            limits.max_total_owned_bytes,
        )?;

        Ok(Self {
            bytes,
            offset: 0,
            limits,
            usage: OriginResourceUsage {
                input_bytes: bytes.len(),
                total_owned_bytes: bytes.len(),
                ..OriginResourceUsage::default()
            },
        })
    }

    pub(super) fn offset(&self) -> usize {
        self.offset
    }

    pub(super) fn into_usage(self) -> OriginResourceUsage {
        self.usage
    }

    pub(super) fn read_slice(&mut self, length: usize) -> Result<&'bytes [u8], OriginError> {
        let start = self.offset;
        let end = checked_add(start, length, "reader offset")?;
        let available =
            self.bytes
                .len()
                .checked_sub(start)
                .ok_or(OriginError::ArithmeticOverflow {
                    resource: "remaining input bytes",
                })?;
        let slice = self.bytes.get(start..end).ok_or(OriginError::Truncated {
            offset: start,
            needed: length,
            have: available,
        })?;
        self.offset = end;
        Ok(slice)
    }

    pub(super) fn read_u8(&mut self) -> Result<u8, OriginError> {
        let start = self.offset;
        let byte = self
            .bytes
            .get(start)
            .copied()
            .ok_or(OriginError::Truncated {
                offset: start,
                needed: 1,
                have: 0,
            })?;
        self.offset = checked_add(start, 1, "reader offset")?;
        Ok(byte)
    }

    // Task 3 establishes these checked primitives before record decoding uses
    // every width in production; their focused tests keep the staged API live.
    #[cfg_attr(not(test), allow(dead_code))]
    pub(super) fn read_u16_le(&mut self) -> Result<u16, OriginError> {
        Ok(u16::from_le_bytes(self.read_array()?))
    }

    pub(super) fn read_u32_le(&mut self) -> Result<u32, OriginError> {
        Ok(u32::from_le_bytes(self.read_array()?))
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub(super) fn read_i16_le(&mut self) -> Result<i16, OriginError> {
        Ok(i16::from_le_bytes(self.read_array()?))
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub(super) fn read_i32_le(&mut self) -> Result<i32, OriginError> {
        Ok(i32::from_le_bytes(self.read_array()?))
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub(super) fn read_f32_le(&mut self) -> Result<f32, OriginError> {
        Ok(f32::from_le_bytes(self.read_array()?))
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub(super) fn read_f64_le(&mut self) -> Result<f64, OriginError> {
        Ok(f64::from_le_bytes(self.read_array()?))
    }

    pub(super) fn read_block(&mut self) -> Result<FramedBlock<'bytes>, OriginError> {
        // OpenOPJ's MIT-licensed reader defines a block as little-endian u32
        // size plus LF, followed (only for nonzero size) by payload plus LF.
        // https://github.com/jgonera/openopj/blob/42ddcf1eb3a490744c54fca0a4ed6fe7a5e723ca/lib/OpenOPJ/common.php
        let block_offset = self.offset;
        let payload_len = self.read_u32_le()?;
        self.expect_lf("block-size delimiter")?;
        if payload_len == 0 {
            return Ok(FramedBlock::Null {
                offset: block_offset,
            });
        }

        let payload_len =
            usize::try_from(payload_len).map_err(|_| OriginError::ArithmeticOverflow {
                resource: "block bytes",
            })?;
        enforce_limit("block bytes", payload_len, self.limits.max_block_bytes)?;
        let payload = self.read_slice(payload_len)?;
        self.expect_lf("block-payload delimiter")?;
        Ok(FramedBlock::Data {
            offset: block_offset,
            payload,
        })
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub(super) fn read_fixed_ascii(&mut self, width: usize) -> Result<String, OriginError> {
        let field_offset = self.offset;
        let field = self.read_slice(width)?;
        let text_len = field
            .iter()
            .position(|byte| *byte == 0)
            .unwrap_or(field.len());
        let text = field
            .get(..text_len)
            .ok_or(OriginError::ArithmeticOverflow {
                resource: "fixed ASCII field",
            })?;
        if let Some(relative) = text.iter().position(|byte| !byte.is_ascii()) {
            return Err(OriginError::UnsupportedEncoding {
                offset: checked_add(field_offset, relative, "text byte offset")?,
                encoding: "non-ASCII byte in fixed-width text".to_owned(),
            });
        }
        enforce_limit("string bytes", text_len, self.limits.max_string_bytes)?;
        self.charge_text(text_len)?;

        let mut decoded = String::new();
        decoded
            .try_reserve_exact(text_len)
            .map_err(|_| OriginError::AllocationFailed {
                resource: "decoded text",
                requested: text_len,
            })?;
        let text = std::str::from_utf8(text).map_err(|_| OriginError::UnsupportedEncoding {
            offset: field_offset,
            encoding: "non-ASCII byte in fixed-width text".to_owned(),
        })?;
        decoded.push_str(text);
        Ok(decoded)
    }

    pub(super) fn try_reserve<T>(
        &mut self,
        values: &mut Vec<T>,
        additional: usize,
        resource: &'static str,
    ) -> Result<(), OriginError> {
        let _requested_elements = checked_add(values.len(), additional, resource)?;
        let requested_bytes = checked_mul(additional, size_of::<T>(), resource)?;
        self.charge_parser(requested_bytes)?;
        values
            .try_reserve_exact(additional)
            .map_err(|_| OriginError::AllocationFailed {
                resource,
                requested: requested_bytes,
            })
    }

    fn read_array<const N: usize>(&mut self) -> Result<[u8; N], OriginError> {
        let start = self.offset;
        let bytes = self.read_slice(N)?;
        bytes.try_into().map_err(|_| OriginError::Truncated {
            offset: start,
            needed: N,
            have: bytes.len(),
        })
    }

    fn expect_lf(&mut self, field: &'static str) -> Result<(), OriginError> {
        let offset = self.offset;
        let delimiter = self.read_u8()?;
        if delimiter != LF {
            return Err(OriginError::CorruptStructure {
                offset,
                detail: format!("{field} must be LF"),
            });
        }
        Ok(())
    }

    #[cfg_attr(not(test), allow(dead_code))]
    fn charge_text(&mut self, bytes: usize) -> Result<(), OriginError> {
        let decoded_text_bytes =
            checked_add(self.usage.decoded_text_bytes, bytes, "decoded text bytes")?;
        enforce_limit(
            "decoded text bytes",
            decoded_text_bytes,
            self.limits.max_decoded_text_bytes,
        )?;
        self.charge_parser(bytes)?;
        self.usage.decoded_text_bytes = decoded_text_bytes;
        Ok(())
    }

    fn charge_parser(&mut self, bytes: usize) -> Result<(), OriginError> {
        let parser_bytes = checked_add(self.usage.parser_bytes, bytes, "parser bytes")?;
        enforce_limit("parser bytes", parser_bytes, self.limits.max_parser_bytes)?;
        let total_owned_bytes =
            checked_add(self.usage.total_owned_bytes, bytes, "total owned bytes")?;
        enforce_limit(
            "total owned bytes",
            total_owned_bytes,
            self.limits.max_total_owned_bytes,
        )?;

        self.usage.parser_bytes = parser_bytes;
        self.usage.total_owned_bytes = total_owned_bytes;
        Ok(())
    }
}

pub(super) fn checked_add(
    left: usize,
    right: usize,
    resource: &'static str,
) -> Result<usize, OriginError> {
    left.checked_add(right)
        .ok_or(OriginError::ArithmeticOverflow { resource })
}

pub(super) fn checked_mul(
    left: usize,
    right: usize,
    resource: &'static str,
) -> Result<usize, OriginError> {
    left.checked_mul(right)
        .ok_or(OriginError::ArithmeticOverflow { resource })
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
