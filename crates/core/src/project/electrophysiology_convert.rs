use super::*;
use crate::state::ElectrophysiologyDataset;

/// Binary payload tag: bulk samples live in `payload.blob` as length-prefixed
/// little-endian `f64`, while the light structure and metadata are JSON in the
/// object's extensions.
const STORAGE_ELECTROPHYSIOLOGY_BIN: &str = "electrophysiology-bin-v1";
pub(super) const VALUES_PER_CHUNK: usize = 4096;

pub(super) fn electrophysiology_to_objects(
    recording: &ElectrophysiologyDataset,
    data_id: &str,
    recipe_id: &str,
) -> Result<(DataObject, RecipeObject)> {
    // Build the metadata skeleton first and release its sample memory before the
    // blob is allocated, so peak usage stays near one extra copy rather than two.
    // Cloning (not field-mapping) guarantees a future field can never be silently
    // dropped from the persisted record.
    let mut skeleton = recording.clone();
    for samples in sample_vectors_mut(&mut skeleton.data) {
        *samples = Vec::new();
    }
    let skeleton_value = serde_json::to_value(&skeleton)?;

    let data = DataObject {
        id: data_id.to_owned(),
        role: "data".to_owned(),
        classification: Classification {
            domain: "electrophysiology".to_owned(),
            technique: Some("patch_clamp".to_owned()),
            object: "recording".to_owned(),
        },
        label: recording.name.clone(),
        dimensions: vec![Dimension {
            id: "time".to_owned(),
            role: "direct".to_owned(),
            size: recording
                .data
                .sweeps
                .iter()
                .filter_map(|s| s.channels.first())
                .map(Vec::len)
                .max()
                .unwrap_or(0),
            storage_axis: 2,
            quantity: "time".to_owned(),
            display_quantity: None,
            unit: Some("s".to_owned()),
            nucleus: None,
            spectral_width_hz: None,
            observe_freq_mhz: None,
            carrier_ppm: None,
            group_delay: None,
        }],
        payload: Payload {
            storage: STORAGE_ELECTROPHYSIOLOGY_BIN.to_owned(),
            blob: format!("objects/{data_id}/data.bin"),
            shape: vec![recording.data.sweeps.len(), recording.data.channels.len()],
            domain: "time".to_owned(),
        },
        extensions: serde_json::json!({ "plotx.electrophysiology": skeleton_value }),
    };
    let recipe = RecipeObject {
        id: recipe_id.to_owned(),
        role: "recipe".to_owned(),
        classification: Classification {
            domain: "electrophysiology".to_owned(),
            technique: Some("patch_clamp".to_owned()),
            object: "processing".to_owned(),
        },
        input: data_id.to_owned(),
        parameters: RecipeParameters::default(),
        extensions: serde_json::Value::Null,
    };
    Ok((data, recipe))
}

pub(super) fn write_electrophysiology_blob(
    output: &mut impl std::io::Write,
    recording: &ElectrophysiologyDataset,
) -> Result<()> {
    let mut buffer = Vec::with_capacity(VALUES_PER_CHUNK * std::mem::size_of::<f64>());
    for samples in sample_vectors(&recording.data) {
        let len = u64::try_from(samples.len()).map_err(|_| {
            ProjectError::Invalid("electrophysiology sample count exceeds u64".to_owned())
        })?;
        output.write_all(&len.to_le_bytes())?;
        for chunk in samples.chunks(VALUES_PER_CHUNK) {
            buffer.clear();
            for &value in chunk {
                buffer.extend_from_slice(&value.to_le_bytes());
            }
            output.write_all(&buffer)?;
        }
    }
    Ok(())
}

pub(super) fn electrophysiology_from_object(
    zip: &mut zip::ZipArchive<File>,
    data: &DataObject,
) -> Result<Dataset> {
    let mut recording = match data.payload.storage.as_str() {
        STORAGE_ELECTROPHYSIOLOGY_BIN => {
            let value = data
                .extensions
                .get("plotx.electrophysiology")
                .ok_or_else(|| {
                    ProjectError::Invalid(
                        "electrophysiology data missing plotx.electrophysiology extension"
                            .to_owned(),
                    )
                })?;
            let mut recording: ElectrophysiologyDataset = serde_json::from_value(value.clone())
                .map_err(|error| {
                    ProjectError::Invalid(format!("invalid electrophysiology metadata: {error}"))
                })?;
            read_entry(
                zip,
                &data.payload.blob,
                "electrophysiology sample payload",
                ProjectLoadLimits::default().max_entry_bytes,
                |reader| {
                    for samples in sample_vectors_mut(&mut recording.data) {
                        *samples = read_f64_vec(reader)?;
                    }
                    Ok(())
                },
            )?;
            recording
        }
        other => {
            return Err(ProjectError::Unsupported(format!(
                "electrophysiology payload storage {other}"
            )));
        }
    };
    recording.region_analysis.validate().map_err(|error| {
        ProjectError::Invalid(format!("invalid region analysis state: {error}"))
    })?;
    recording.scientific_identity = super::convert::read_scientific_identity(data)?;
    let dataset = Dataset::Electrophysiology(Box::new(recording));
    dataset
        .validate_field_catalog()
        .map_err(ProjectError::Invalid)?;
    Ok(dataset)
}

/// Traversal order shared by save and load so the length-prefixed blob is filled
/// and drained identically: every sweep's recorded channels, then its command
/// waveforms, in stored order.
fn sample_vectors(data: &plotx_io::ElectrophysiologyData) -> impl Iterator<Item = &Vec<f64>> {
    data.sweeps.iter().flat_map(|sweep| {
        sweep
            .channels
            .iter()
            .chain(sweep.commands.iter().map(|command| &command.samples))
    })
}

fn sample_vectors_mut(
    data: &mut plotx_io::ElectrophysiologyData,
) -> impl Iterator<Item = &mut Vec<f64>> {
    data.sweeps.iter_mut().flat_map(|sweep| {
        sweep.channels.iter_mut().chain(
            sweep
                .commands
                .iter_mut()
                .map(|command| &mut command.samples),
        )
    })
}

fn read_f64_vec<R: std::io::Read>(reader: &mut EntryReader<'_, R>) -> Result<Vec<f64>> {
    reader.require_bytes(8, "electrophysiology vector length")?;
    let mut len_bytes = [0_u8; 8];
    reader.read_exact(&mut len_bytes).map_err(|error| {
        reader.invalid(format!(
            "electrophysiology blob truncated before a length: {error}"
        ))
    })?;
    let len = usize::try_from(u64::from_le_bytes(len_bytes))
        .map_err(|_| ProjectError::Invalid("electrophysiology vector is too large".to_owned()))?;
    let byte_len = len.checked_mul(8).ok_or_else(|| {
        ProjectError::Invalid("electrophysiology sample count overflows".to_owned())
    })?;
    reader.require_bytes(byte_len, "electrophysiology sample vector")?;
    let mut values = Vec::new();
    values
        .try_reserve_exact(len)
        .map_err(|_| reader.invalid("could not reserve electrophysiology samples"))?;
    let mut bytes = [0_u8; 8];
    for _ in 0..len {
        reader.read_exact(&mut bytes).map_err(|error| {
            reader.invalid(format!(
                "electrophysiology blob truncated inside a vector: {error}"
            ))
        })?;
        values.push(f64::from_le_bytes(bytes));
    }
    Ok(values)
}
