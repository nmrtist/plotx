use std::mem::size_of;

use crate::origin::{OriginError, OriginLimits};

use super::super::super::reader::checked_add;

const LF: u8 = b'\n';
const PARAMETER_VALUE_LEN: usize = size_of::<f64>() + 1;

pub(super) enum MetadataBlock<'a> {
    Null { offset: usize },
    Data { offset: usize, payload: &'a [u8] },
}

pub(super) struct MetadataCursor<'a> {
    bytes: &'a [u8],
    offset: usize,
    base_offset: usize,
    pub(super) limits: &'a OriginLimits,
}

impl<'a> MetadataCursor<'a> {
    pub(super) fn new(bytes: &'a [u8], base_offset: usize, limits: &'a OriginLimits) -> Self {
        Self {
            bytes,
            offset: 0,
            base_offset,
            limits,
        }
    }

    pub(super) fn read_block(&mut self) -> Result<MetadataBlock<'a>, OriginError> {
        let start = self.offset;
        let size_end = checked_add(start, size_of::<u32>(), "metadata block size")?;
        let size_bytes = self.bytes.get(start..size_end).ok_or_else(|| {
            self.truncated(
                start,
                size_of::<u32>(),
                self.bytes.len().saturating_sub(start),
            )
        })?;
        let size_array: [u8; 4] = size_bytes
            .try_into()
            .map_err(|_| self.truncated(start, size_of::<u32>(), size_bytes.len()))?;
        let size = usize::try_from(u32::from_le_bytes(size_array)).map_err(|_| {
            OriginError::ArithmeticOverflow {
                resource: "metadata block size",
            }
        })?;
        enforce_limit("block bytes", size, self.limits.max_block_bytes)?;

        let size_lf = size_end;
        self.require_lf(size_lf, "metadata block size delimiter")?;
        let payload_start = checked_add(size_lf, 1, "metadata block payload")?;
        if size == 0 {
            self.offset = payload_start;
            return Ok(MetadataBlock::Null {
                offset: self.absolute(start)?,
            });
        }

        let payload_end = checked_add(payload_start, size, "metadata block payload")?;
        let payload = self.bytes.get(payload_start..payload_end).ok_or_else(|| {
            self.truncated(
                payload_start,
                size,
                self.bytes.len().saturating_sub(payload_start),
            )
        })?;
        self.require_lf(payload_end, "metadata block payload delimiter")?;
        self.offset = checked_add(payload_end, 1, "metadata block end")?;
        Ok(MetadataBlock::Data {
            offset: self.absolute(start)?,
            payload,
        })
    }

    pub(super) fn read_line(&mut self) -> Result<(usize, &'a [u8]), OriginError> {
        let start = self.offset;
        let available = self
            .bytes
            .get(start..)
            .ok_or_else(|| self.truncated(start, 1, self.bytes.len().saturating_sub(start)))?;
        let oversize = checked_add(self.limits.max_string_bytes, 1, "metadata line bound")?;
        let scan_len = available.len().min(oversize);
        let scan = available
            .get(..scan_len)
            .ok_or(OriginError::ArithmeticOverflow {
                resource: "metadata line scan",
            })?;
        let Some(relative_lf) = scan.iter().position(|byte| *byte == LF) else {
            if available.len() > self.limits.max_string_bytes {
                return Err(OriginError::LimitExceeded {
                    resource: "string bytes",
                    limit: self.limits.max_string_bytes,
                    actual: oversize,
                });
            }
            return Err(self.truncated(
                checked_add(start, available.len(), "metadata line end")?,
                1,
                0,
            ));
        };
        enforce_limit("string bytes", relative_lf, self.limits.max_string_bytes)?;
        let line = available
            .get(..relative_lf)
            .ok_or(OriginError::ArithmeticOverflow {
                resource: "metadata line",
            })?;
        self.offset = checked_add(
            start,
            checked_add(relative_lf, 1, "metadata line length")?,
            "metadata line end",
        )?;
        Ok((self.absolute(start)?, line))
    }

    pub(super) fn read_parameter_value(&mut self) -> Result<[u8; 8], OriginError> {
        let start = self.offset;
        let end = checked_add(start, PARAMETER_VALUE_LEN, "parameter value")?;
        let bytes = self.bytes.get(start..end).ok_or_else(|| {
            self.truncated(
                start,
                PARAMETER_VALUE_LEN,
                self.bytes.len().saturating_sub(start),
            )
        })?;
        let value = bytes
            .get(..size_of::<f64>())
            .ok_or_else(|| self.truncated(start, size_of::<f64>(), bytes.len()))?;
        let delimiter = bytes
            .get(size_of::<f64>())
            .copied()
            .ok_or_else(|| self.truncated(start, PARAMETER_VALUE_LEN, bytes.len()))?;
        if delimiter != LF {
            return Err(OriginError::CorruptStructure {
                offset: self.absolute(checked_add(start, size_of::<f64>(), "parameter LF")?)?,
                detail: "an Origin parameter value must end with LF".to_owned(),
            });
        }
        let value: [u8; 8] = value
            .try_into()
            .map_err(|_| self.truncated(start, size_of::<f64>(), value.len()))?;
        self.offset = end;
        Ok(value)
    }

    pub(super) fn read_exact(&mut self, length: usize) -> Result<&'a [u8], OriginError> {
        let start = self.offset;
        let end = checked_add(start, length, "terminal OPJ record")?;
        let bytes = self
            .bytes
            .get(start..end)
            .ok_or_else(|| self.truncated(start, length, self.bytes.len().saturating_sub(start)))?;
        self.offset = end;
        Ok(bytes)
    }

    pub(super) fn skip_exact(&mut self, length: usize) -> Result<(), OriginError> {
        let _ = self.read_exact(length)?;
        Ok(())
    }

    pub(super) fn relative_offset(&self) -> usize {
        self.offset
    }

    pub(super) fn absolute_offset(&self) -> Result<usize, OriginError> {
        self.absolute(self.offset)
    }

    pub(super) fn remaining(&self) -> usize {
        self.bytes.len().saturating_sub(self.offset)
    }

    fn require_lf(&self, offset: usize, field: &'static str) -> Result<(), OriginError> {
        let byte =
            self.bytes.get(offset).copied().ok_or_else(|| {
                self.truncated(offset, 1, self.bytes.len().saturating_sub(offset))
            })?;
        if byte != LF {
            return Err(OriginError::CorruptStructure {
                offset: self.absolute(offset)?,
                detail: format!("{field} must be LF"),
            });
        }
        Ok(())
    }

    fn absolute(&self, relative: usize) -> Result<usize, OriginError> {
        checked_add(self.base_offset, relative, "metadata file offset")
    }

    fn truncated(&self, offset: usize, needed: usize, have: usize) -> OriginError {
        OriginError::Truncated {
            offset: self.base_offset.saturating_add(offset),
            needed,
            have,
        }
    }
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
