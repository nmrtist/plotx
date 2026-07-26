use super::{ProjectError, Result, STORAGE_DOSY_V1};
use crate::{DosyMethod, DosyResultProvenance, PseudoDisplay};
use plotx_analysis::diffusion::DiffusionMap;
use plotx_analysis::ilt::IltResult;
use serde::{Deserialize, Serialize};

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

pub(super) fn decode_dosy(bytes: &[u8], expected_shapes: &DosyShapes) -> Result<DecodedDosy> {
    let mut reader = Reader::new(bytes);
    if reader.take(MAGIC.len())? != MAGIC {
        return Err(ProjectError::Invalid(
            "DOSY payload has an invalid signature".to_owned(),
        ));
    }
    let metadata_len = reader.read_len()?;
    let header: DosyBlobHeader = serde_json::from_slice(reader.take(metadata_len)?)?;
    if &header.shapes != expected_shapes {
        return Err(ProjectError::Invalid(format!(
            "DOSY payload shapes {:?} do not match expected shapes {:?}",
            header.shapes, expected_shapes
        )));
    }

    let dosy_map = match &header.shapes.diffusion {
        Some(shape) => {
            let ppm = reader.read_f64s()?;
            require_len("DOSY ppm", ppm.len(), shape.len)?;
            let d = reader.read_f64s()?;
            require_len("DOSY diffusion", d.len(), shape.len)?;
            let amp = reader.read_f64s()?;
            require_len("DOSY amplitude", amp.len(), shape.len)?;
            Some(DiffusionMap { ppm, d, amp })
        }
        None => None,
    };
    let ilt_map = match &header.shapes.ilt {
        Some(shape) => {
            let ppm = reader.read_f64s()?;
            require_len("ILT ppm", ppm.len(), shape.ppm_len)?;
            let d_grid = reader.read_f64s()?;
            require_len("ILT diffusion grid", d_grid.len(), shape.d_grid_len)?;
            // Deliberately not `with_capacity(shape.ppm_len)`: the row count comes
            // from the file and nothing has yet proven the payload holds that many
            // rows, so reserving up front lets a corrupt header abort the process
            // on allocation failure. Growing per row caps the reservation at what
            // the bytes actually contain, because each `read_f64s` proves its own
            // extent first.
            let mut amp: Vec<Vec<f64>> = Vec::new();
            for row in 0..shape.ppm_len {
                let values = reader.read_f64s()?;
                require_len(
                    &format!("ILT amplitude row {row}"),
                    values.len(),
                    shape.d_grid_len,
                )?;
                amp.push(values);
            }
            Some(IltResult { ppm, d_grid, amp })
        }
        None => None,
    };
    if !reader.is_empty() {
        return Err(ProjectError::Invalid(
            "DOSY payload contains trailing data".to_owned(),
        ));
    }
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
            .ok_or_else(|| ProjectError::Invalid("DOSY payload offset overflow".to_owned()))?;
        let result = self
            .bytes
            .get(self.offset..end)
            .ok_or_else(|| ProjectError::Invalid("DOSY payload is truncated".to_owned()))?;
        self.offset = end;
        Ok(result)
    }

    fn read_len(&mut self) -> Result<usize> {
        let bytes: [u8; 8] = self
            .take(8)?
            .try_into()
            .map_err(|_| ProjectError::Invalid("invalid DOSY length".to_owned()))?;
        usize::try_from(u64::from_le_bytes(bytes))
            .map_err(|_| ProjectError::Invalid("DOSY length exceeds usize".to_owned()))
    }

    fn read_f64s(&mut self) -> Result<Vec<f64>> {
        let len = self.read_len()?;
        let byte_len = len
            .checked_mul(8)
            .ok_or_else(|| ProjectError::Invalid("DOSY f64 array size overflow".to_owned()))?;
        let bytes = self.take(byte_len)?;
        Ok(bytes
            .chunks_exact(8)
            .map(|chunk| {
                f64::from_bits(u64::from_le_bytes(
                    chunk.try_into().expect("eight-byte chunk"),
                ))
            })
            .collect())
    }

    fn is_empty(&self) -> bool {
        self.offset == self.bytes.len()
    }
}

#[cfg(test)]
#[path = "dosy_convert_tests.rs"]
mod tests;
