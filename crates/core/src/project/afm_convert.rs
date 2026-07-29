use super::{ProjectError, Result};
use plotx_io::AfmData;
use std::sync::Arc;

const MAGIC: &[u8; 8] = b"PXAFM1\0\0";
const VALUES_PER_CHUNK: usize = 4096;

pub(super) fn write_afm(output: &mut impl std::io::Write, data: &AfmData) -> Result<()> {
    let mut metadata = data.clone();
    for image in &mut metadata.images {
        image.raw = Arc::from([]);
    }
    if let Some(forces) = &mut metadata.forces {
        forces.raw = Arc::from([]);
        forces.display_order = Arc::from([]);
        if forces.z_positions.is_some() {
            forces.z_positions = Some(Arc::from([]));
        }
    }

    let json = serde_json::to_vec(&metadata)?;
    output.write_all(MAGIC)?;
    write_len(output, json.len())?;
    output.write_all(&json)?;
    for image in &data.images {
        write_i32s(output, &image.raw)?;
    }
    if let Some(forces) = &data.forces {
        write_i32s(output, &forces.raw)?;
        write_usizes(output, &forces.display_order)?;
        if let Some(z) = &forces.z_positions {
            write_f64s(output, z)?;
        }
    }
    Ok(())
}

#[cfg(test)]
pub(super) fn encode_afm(data: &AfmData) -> Result<Vec<u8>> {
    let mut output = Vec::new();
    write_afm(&mut output, data)?;
    Ok(output)
}

pub(super) fn decode_afm(bytes: &[u8]) -> Result<AfmData> {
    let mut reader = Reader::new(bytes);
    if reader.take(MAGIC.len())? != MAGIC {
        return Err(ProjectError::Invalid(
            "AFM payload has an invalid signature".to_owned(),
        ));
    }
    let metadata_len = reader.read_len()?;
    let mut data: AfmData = serde_json::from_slice(reader.take(metadata_len)?)?;
    for image in &mut data.images {
        image.raw = reader.read_i32s()?;
        let expected = image
            .width
            .checked_mul(image.height)
            .ok_or_else(|| ProjectError::Invalid("AFM image dimensions overflow".to_owned()))?;
        require_len("AFM image", image.raw.len(), expected)?;
    }
    if let Some(forces) = &mut data.forces {
        forces.raw = reader.read_i32s()?;
        forces.display_order = reader.read_usizes()?;
        if forces.z_positions.is_some() {
            forces.z_positions = Some(reader.read_f64s()?);
        }
        let curves = forces
            .grid_width
            .checked_mul(forces.grid_height)
            .and_then(|value| value.checked_mul(forces.samples_per_curve))
            .ok_or_else(|| ProjectError::Invalid("AFM force dimensions overflow".to_owned()))?;
        require_len("AFM force data", forces.raw.len(), curves)?;
        require_len(
            "AFM display order",
            forces.display_order.len(),
            forces.samples_per_curve,
        )?;
        if forces
            .display_order
            .iter()
            .any(|&index| index >= forces.samples_per_curve)
        {
            return Err(ProjectError::Invalid(
                "AFM display order contains an out-of-range sample".to_owned(),
            ));
        }
        if let Some(z) = &forces.z_positions {
            require_len("AFM Z positions", z.len(), forces.samples_per_curve)?;
        }
    }
    if !reader.is_empty() {
        return Err(ProjectError::Invalid(
            "AFM payload contains trailing data".to_owned(),
        ));
    }
    Ok(data)
}

fn require_len(label: &str, actual: usize, expected: usize) -> Result<()> {
    if actual != expected {
        return Err(ProjectError::Invalid(format!(
            "{label} length {actual} does not match expected length {expected}"
        )));
    }
    Ok(())
}

fn write_len(output: &mut impl std::io::Write, len: usize) -> Result<()> {
    let len = u64::try_from(len)
        .map_err(|_| ProjectError::Invalid("AFM array length exceeds u64".to_owned()))?;
    output.write_all(&len.to_le_bytes())?;
    Ok(())
}

fn write_i32s(output: &mut impl std::io::Write, values: &[i32]) -> Result<()> {
    write_scalars(output, values, |value| Ok(value.to_le_bytes()))
}

fn write_usizes(output: &mut impl std::io::Write, values: &[usize]) -> Result<()> {
    write_scalars(output, values, |&value| {
        let encoded = u64::try_from(value)
            .map_err(|_| ProjectError::Invalid("AFM sample index exceeds u64".to_owned()))?;
        Ok(encoded.to_le_bytes())
    })
}

fn write_f64s(output: &mut impl std::io::Write, values: &[f64]) -> Result<()> {
    write_scalars(output, values, |value| Ok(value.to_le_bytes()))
}

fn write_scalars<T, const WIDTH: usize>(
    output: &mut impl std::io::Write,
    values: &[T],
    mut encode: impl FnMut(&T) -> Result<[u8; WIDTH]>,
) -> Result<()> {
    write_len(output, values.len())?;
    let mut buffer = Vec::with_capacity(VALUES_PER_CHUNK * WIDTH);
    for chunk in values.chunks(VALUES_PER_CHUNK) {
        buffer.clear();
        for value in chunk {
            buffer.extend_from_slice(&encode(value)?);
        }
        output.write_all(&buffer)?;
    }
    Ok(())
}

struct Reader<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl<'a> Reader<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, offset: 0 }
    }

    fn take(&mut self, len: usize) -> Result<&'a [u8]> {
        let end = self
            .offset
            .checked_add(len)
            .ok_or_else(|| ProjectError::Invalid("AFM payload offset overflow".to_owned()))?;
        let result = self
            .bytes
            .get(self.offset..end)
            .ok_or_else(|| ProjectError::Invalid("AFM payload is truncated".to_owned()))?;
        self.offset = end;
        Ok(result)
    }

    fn read_len(&mut self) -> Result<usize> {
        let bytes: [u8; 8] = self
            .take(8)?
            .try_into()
            .map_err(|_| ProjectError::Invalid("invalid AFM length".to_owned()))?;
        usize::try_from(u64::from_le_bytes(bytes))
            .map_err(|_| ProjectError::Invalid("AFM length exceeds usize".to_owned()))
    }

    fn read_i32s(&mut self) -> Result<Arc<[i32]>> {
        let len = self.read_len()?;
        let byte_len = len
            .checked_mul(4)
            .ok_or_else(|| ProjectError::Invalid("AFM i32 array size overflow".to_owned()))?;
        let bytes = self.take(byte_len)?;
        Ok(bytes
            .chunks_exact(4)
            .map(|chunk| i32::from_le_bytes(chunk.try_into().expect("four-byte chunk")))
            .collect::<Vec<_>>()
            .into())
    }

    fn read_usizes(&mut self) -> Result<Arc<[usize]>> {
        let len = self.read_len()?;
        let byte_len = len
            .checked_mul(8)
            .ok_or_else(|| ProjectError::Invalid("AFM index array size overflow".to_owned()))?;
        let bytes = self.take(byte_len)?;
        let mut values = Vec::with_capacity(len);
        for chunk in bytes.chunks_exact(8) {
            let value = u64::from_le_bytes(chunk.try_into().expect("eight-byte chunk"));
            values.push(
                usize::try_from(value).map_err(|_| {
                    ProjectError::Invalid("AFM sample index exceeds usize".to_owned())
                })?,
            );
        }
        Ok(values.into())
    }

    fn read_f64s(&mut self) -> Result<Arc<[f64]>> {
        let len = self.read_len()?;
        let byte_len = len
            .checked_mul(8)
            .ok_or_else(|| ProjectError::Invalid("AFM f64 array size overflow".to_owned()))?;
        let bytes = self.take(byte_len)?;
        Ok(bytes
            .chunks_exact(8)
            .map(|chunk| f64::from_le_bytes(chunk.try_into().expect("eight-byte chunk")))
            .collect::<Vec<_>>()
            .into())
    }

    fn is_empty(&self) -> bool {
        self.offset == self.bytes.len()
    }
}

#[cfg(test)]
#[path = "afm_convert_tests.rs"]
mod tests;
