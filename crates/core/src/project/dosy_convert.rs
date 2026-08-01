use super::{EntryReader, ProjectError, ProjectLoadLimits, Result, STORAGE_DOSY_V1};
use crate::{DosyMethod, DosyResultProvenance, PseudoDisplay};
use plotx_analysis::diffusion::DiffusionMap;
use plotx_analysis::ilt::IltResult;
use serde::{Deserialize, Serialize};
use std::io::Read;

const MAGIC: &[u8; 8] = b"PXDOSY1\0";

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(super) struct DiffusionMapShape {
    pub len: usize,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(super) struct IltMapShape {
    pub ppm_len: usize,
    pub d_grid_len: usize,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub(super) struct DosyShapes {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub diffusion: Option<DiffusionMapShape>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ilt: Option<IltMapShape>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub(super) struct DosyProvenanceDto {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub diffusion: Option<DosyResultProvenance>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ilt: Option<DosyResultProvenance>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(super) struct DosyExtensionDto {
    pub display: PseudoDisplay,
    pub method: DosyMethod,
    pub provenance: DosyProvenanceDto,
    pub storage: String,
    pub blob: String,
    pub shapes: DosyShapes,
}

impl DosyExtensionDto {
    pub(super) fn new(
        display: PseudoDisplay,
        method: DosyMethod,
        provenance: DosyProvenanceDto,
        blob: String,
        shapes: DosyShapes,
    ) -> Self {
        Self {
            display,
            method,
            provenance,
            storage: STORAGE_DOSY_V1.to_owned(),
            blob,
            shapes,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct DosyBlobHeader {
    shapes: DosyShapes,
}

#[derive(Debug)]
pub(super) struct DecodedDosy {
    pub dosy_map: Option<DiffusionMap>,
    pub ilt_map: Option<IltResult>,
}

pub(super) fn encode_dosy(
    dosy_map: Option<&DiffusionMap>,
    ilt_map: Option<&IltResult>,
) -> Result<(Vec<u8>, DosyShapes)> {
    let shapes = DosyShapes {
        diffusion: dosy_map
            .map(validate_diffusion_map)
            .transpose()?
            .map(|len| DiffusionMapShape { len }),
        ilt: ilt_map
            .map(validate_ilt_map)
            .transpose()?
            .map(|(ppm_len, d_grid_len)| IltMapShape {
                ppm_len,
                d_grid_len,
            }),
    };
    let json = serde_json::to_vec(&DosyBlobHeader {
        shapes: shapes.clone(),
    })?;
    let mut output = Vec::with_capacity(json.len().saturating_add(128));
    output.extend_from_slice(MAGIC);
    write_len(&mut output, json.len())?;
    output.extend_from_slice(&json);
    if let Some(map) = dosy_map {
        write_f64s(&mut output, &map.ppm)?;
        write_f64s(&mut output, &map.d)?;
        write_f64s(&mut output, &map.amp)?;
    }
    if let Some(map) = ilt_map {
        write_f64s(&mut output, &map.ppm)?;
        write_f64s(&mut output, &map.d_grid)?;
        for row in &map.amp {
            write_f64s(&mut output, row)?;
        }
    }
    Ok((output, shapes))
}

pub(super) fn decode_dosy<R: Read>(
    input: &mut EntryReader<'_, R>,
    expected_shapes: &DosyShapes,
) -> Result<DecodedDosy> {
    let mut reader = Reader::new(input);
    if reader.read_array::<8>()? != *MAGIC {
        return Err(ProjectError::Invalid(
            "DOSY payload has an invalid signature".to_owned(),
        ));
    }
    let metadata_len = reader.read_len()?;
    if metadata_len > ProjectLoadLimits::default().max_metadata_bytes as usize {
        return Err(reader
            .input
            .invalid("DOSY metadata exceeds the configured limit"));
    }
    let metadata = reader.read_bytes(metadata_len, "DOSY metadata")?;
    let header: DosyBlobHeader = serde_json::from_slice(&metadata)?;
    if &header.shapes != expected_shapes {
        return Err(ProjectError::Invalid(format!(
            "DOSY payload shapes {:?} do not match expected shapes {:?}",
            header.shapes, expected_shapes
        )));
    }

    let dosy_map = match &header.shapes.diffusion {
        Some(shape) => {
            let ppm = reader.read_f64s(shape.len, "DOSY ppm")?;
            let d = reader.read_f64s(shape.len, "DOSY diffusion")?;
            let amp = reader.read_f64s(shape.len, "DOSY amplitude")?;
            Some(DiffusionMap { ppm, d, amp })
        }
        None => None,
    };
    let ilt_map = match &header.shapes.ilt {
        Some(shape) => {
            let ppm = reader.read_f64s(shape.ppm_len, "ILT ppm")?;
            let d_grid = reader.read_f64s(shape.d_grid_len, "ILT diffusion grid")?;
            // Deliberately not `with_capacity(shape.ppm_len)`: the row count comes
            // from the file and nothing has yet proven the payload holds that many
            // rows, so reserving up front lets a corrupt header abort the process
            // on allocation failure. Growing per row caps the reservation at what
            // the bytes actually contain, because each `read_f64s` proves its own
            // extent first.
            let mut amp: Vec<Vec<f64>> = Vec::new();
            for row in 0..shape.ppm_len {
                let values =
                    reader.read_f64s(shape.d_grid_len, &format!("ILT amplitude row {row}"))?;
                amp.push(values);
            }
            Some(IltResult { ppm, d_grid, amp })
        }
        None => None,
    };
    Ok(DecodedDosy { dosy_map, ilt_map })
}

fn validate_diffusion_map(map: &DiffusionMap) -> Result<usize> {
    let expected = map.ppm.len();
    require_len("DOSY diffusion", map.d.len(), expected)?;
    require_len("DOSY amplitude", map.amp.len(), expected)?;
    Ok(expected)
}

fn validate_ilt_map(map: &IltResult) -> Result<(usize, usize)> {
    let ppm_len = map.ppm.len();
    let d_grid_len = map.d_grid.len();
    require_len("ILT amplitude rows", map.amp.len(), ppm_len)?;
    for (row, values) in map.amp.iter().enumerate() {
        require_len(
            &format!("ILT amplitude row {row}"),
            values.len(),
            d_grid_len,
        )?;
    }
    Ok((ppm_len, d_grid_len))
}

fn require_len(label: &str, actual: usize, expected: usize) -> Result<()> {
    if actual != expected {
        return Err(ProjectError::Invalid(format!(
            "{label} length {actual} does not match expected length {expected}"
        )));
    }
    Ok(())
}

fn write_len(output: &mut Vec<u8>, len: usize) -> Result<()> {
    let len = u64::try_from(len)
        .map_err(|_| ProjectError::Invalid("DOSY array length exceeds u64".to_owned()))?;
    output.extend_from_slice(&len.to_le_bytes());
    Ok(())
}

fn write_f64s(output: &mut Vec<u8>, values: &[f64]) -> Result<()> {
    write_len(output, values.len())?;
    for value in values {
        output.extend_from_slice(&value.to_bits().to_le_bytes());
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
        self.input.require_bytes(N, "DOSY field")?;
        let mut bytes = [0_u8; N];
        self.input.read_exact(&mut bytes).map_err(|error| {
            self.input
                .invalid(format!("DOSY payload is truncated: {error}"))
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
                .invalid(format!("DOSY payload is truncated: {error}"))
        })?;
        Ok(bytes)
    }

    fn read_len(&mut self) -> Result<usize> {
        let bytes = self.read_array::<8>()?;
        usize::try_from(u64::from_le_bytes(bytes))
            .map_err(|_| ProjectError::Invalid("DOSY length exceeds usize".to_owned()))
    }

    fn read_f64s(&mut self, expected: usize, label: &str) -> Result<Vec<f64>> {
        let len = self.read_len()?;
        require_len(label, len, expected)?;
        let byte_len = len
            .checked_mul(8)
            .ok_or_else(|| ProjectError::Invalid("DOSY f64 array size overflow".to_owned()))?;
        self.input.require_bytes(byte_len, "DOSY f64 array")?;
        let mut values = Vec::new();
        values
            .try_reserve_exact(len)
            .map_err(|_| self.input.invalid("could not reserve DOSY f64 array"))?;
        for _ in 0..len {
            values.push(f64::from_bits(u64::from_le_bytes(self.read_array()?)));
        }
        Ok(values)
    }
}

#[cfg(test)]
fn decode_dosy_bytes(bytes: &[u8], expected_shapes: &DosyShapes) -> Result<DecodedDosy> {
    let mut reader = EntryReader::new(
        std::io::Cursor::new(bytes),
        "test.bin",
        "DOSY",
        bytes.len() as u64,
        bytes.len() as u64,
    )?;
    let value = decode_dosy(&mut reader, expected_shapes)?;
    reader.finish()?;
    Ok(value)
}

#[cfg(test)]
#[path = "dosy_convert_tests.rs"]
mod tests;
