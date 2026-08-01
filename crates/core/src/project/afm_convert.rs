use super::{EntryReader, ProjectError, ProjectLoadLimits, Result};
use plotx_io::AfmData;
use std::io::Read;
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

pub(super) fn decode_afm<R: Read>(input: &mut EntryReader<'_, R>) -> Result<AfmData> {
    let mut reader = Reader::new(input);
    if reader.read_array::<8>()? != *MAGIC {
        return Err(ProjectError::Invalid(
            "AFM payload has an invalid signature".to_owned(),
        ));
    }
    let metadata_len = reader.read_len()?;
    if metadata_len > ProjectLoadLimits::default().max_metadata_bytes as usize {
        return Err(reader
            .input
            .invalid("AFM metadata exceeds the configured limit"));
    }
    let metadata = reader.read_bytes(metadata_len, "AFM metadata")?;
    let mut data: AfmData = serde_json::from_slice(&metadata)?;
    for image in &mut data.images {
        let expected = image
            .width
            .checked_mul(image.height)
            .ok_or_else(|| ProjectError::Invalid("AFM image dimensions overflow".to_owned()))?;
        image.raw = reader.read_i32s(expected, "AFM image")?;
    }
    if let Some(forces) = &mut data.forces {
        let curves = forces
            .grid_width
            .checked_mul(forces.grid_height)
            .and_then(|value| value.checked_mul(forces.samples_per_curve))
            .ok_or_else(|| ProjectError::Invalid("AFM force dimensions overflow".to_owned()))?;
        forces.raw = reader.read_i32s(curves, "AFM force data")?;
        forces.display_order = reader.read_usizes(forces.samples_per_curve, "AFM display order")?;
        if forces.z_positions.is_some() {
            forces.z_positions =
                Some(reader.read_f64s(forces.samples_per_curve, "AFM Z positions")?);
        }
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
    Ok(data)
}

#[cfg(test)]
fn decode_afm_bytes(bytes: &[u8]) -> Result<AfmData> {
    let mut reader = EntryReader::new(
        std::io::Cursor::new(bytes),
        "test.bin",
        "AFM",
        bytes.len() as u64,
        bytes.len() as u64,
    )?;
    let value = decode_afm(&mut reader)?;
    reader.finish()?;
    Ok(value)
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

struct Reader<'a, 'p, R: Read> {
    input: &'a mut EntryReader<'p, R>,
}

impl<'a, 'p, R: Read> Reader<'a, 'p, R> {
    fn new(input: &'a mut EntryReader<'p, R>) -> Self {
        Self { input }
    }

    fn read_array<const N: usize>(&mut self) -> Result<[u8; N]> {
        self.input.require_bytes(N, "AFM field")?;
        let mut bytes = [0_u8; N];
        self.input.read_exact(&mut bytes).map_err(|error| {
            self.input
                .invalid(format!("AFM payload is truncated: {error}"))
        })?;
        Ok(bytes)
    }

    fn read_bytes(&mut self, len: usize, label: &str) -> Result<Vec<u8>> {
        self.input.require_bytes(len, label)?;
        let mut bytes = Vec::new();
        bytes
            .try_reserve_exact(len)
            .map_err(|_| self.input.invalid(format!("could not reserve {label}")))?;
        bytes.resize(len, 0);
        self.input.read_exact(&mut bytes).map_err(|error| {
            self.input
                .invalid(format!("AFM payload is truncated: {error}"))
        })?;
        Ok(bytes)
    }

    fn read_len(&mut self) -> Result<usize> {
        let bytes = self.read_array::<8>()?;
        usize::try_from(u64::from_le_bytes(bytes))
            .map_err(|_| ProjectError::Invalid("AFM length exceeds usize".to_owned()))
    }

    fn read_i32s(&mut self, expected: usize, label: &str) -> Result<Arc<[i32]>> {
        let len = self.read_len()?;
        require_len(label, len, expected)?;
        let byte_len = len
            .checked_mul(4)
            .ok_or_else(|| ProjectError::Invalid("AFM i32 array size overflow".to_owned()))?;
        self.input.require_bytes(byte_len, "AFM i32 array")?;
        let mut values = Vec::new();
        values
            .try_reserve_exact(len)
            .map_err(|_| self.input.invalid("could not reserve AFM i32 array"))?;
        for _ in 0..len {
            values.push(i32::from_le_bytes(self.read_array()?));
        }
        Ok(values.into())
    }

    fn read_usizes(&mut self, expected: usize, label: &str) -> Result<Arc<[usize]>> {
        let len = self.read_len()?;
        require_len(label, len, expected)?;
        let byte_len = len
            .checked_mul(8)
            .ok_or_else(|| ProjectError::Invalid("AFM index array size overflow".to_owned()))?;
        self.input.require_bytes(byte_len, "AFM index array")?;
        let mut values = Vec::new();
        values
            .try_reserve_exact(len)
            .map_err(|_| self.input.invalid("could not reserve AFM index array"))?;
        for _ in 0..len {
            let value = u64::from_le_bytes(self.read_array()?);
            values.push(
                usize::try_from(value).map_err(|_| {
                    ProjectError::Invalid("AFM sample index exceeds usize".to_owned())
                })?,
            );
        }
        Ok(values.into())
    }

    fn read_f64s(&mut self, expected: usize, label: &str) -> Result<Arc<[f64]>> {
        let len = self.read_len()?;
        require_len(label, len, expected)?;
        let byte_len = len
            .checked_mul(8)
            .ok_or_else(|| ProjectError::Invalid("AFM f64 array size overflow".to_owned()))?;
        self.input.require_bytes(byte_len, "AFM f64 array")?;
        let mut values = Vec::new();
        values
            .try_reserve_exact(len)
            .map_err(|_| self.input.invalid("could not reserve AFM f64 array"))?;
        for _ in 0..len {
            values.push(f64::from_le_bytes(self.read_array()?));
        }
        Ok(values.into())
    }
}

#[cfg(test)]
#[path = "afm_convert_tests.rs"]
mod tests;
