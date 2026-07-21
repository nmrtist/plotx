use super::*;
use crate::state::ElectrophysiologyDataset;

/// Binary payload tag: bulk samples live in `payload.blob` as length-prefixed
/// little-endian `f64`, while the light structure and metadata are JSON in the
/// object's extensions. Superseded the earlier all-JSON `-json-v1` layout, whose
/// numeric arrays made large recordings slow to parse and many times larger on
/// disk. Loading of the legacy tag is retained below.
const STORAGE_ELECTROPHYSIOLOGY_BIN: &str = "electrophysiology-bin-v1";
const STORAGE_ELECTROPHYSIOLOGY_JSON: &str = "electrophysiology-json-v1";

pub(super) fn electrophysiology_to_objects(
    recording: &ElectrophysiologyDataset,
    data_id: &str,
    recipe_id: &str,
) -> Result<(DataObject, Vec<u8>, RecipeObject)> {
    // Build the metadata skeleton first and release its sample memory before the
    // blob is allocated, so peak usage stays near one extra copy rather than two.
    // Cloning (not field-mapping) guarantees a future field can never be silently
    // dropped from the persisted record.
    let mut skeleton = recording.clone();
    for samples in sample_vectors_mut(&mut skeleton.data) {
        *samples = Vec::new();
    }
    let skeleton_value = serde_json::to_value(&skeleton)?;

    let mut blob = Vec::new();
    for samples in sample_vectors(&recording.data) {
        blob.extend_from_slice(&(samples.len() as u64).to_le_bytes());
        for &value in samples {
            blob.extend_from_slice(&value.to_le_bytes());
        }
    }

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
    Ok((data, blob, recipe))
}

pub(super) fn electrophysiology_from_object(
    zip: &mut zip::ZipArchive<File>,
    data: &DataObject,
) -> Result<Dataset> {
    let blob = read_bytes(zip, &data.payload.blob)?;
    let recording = match data.payload.storage.as_str() {
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
            let mut cursor = 0usize;
            for samples in sample_vectors_mut(&mut recording.data) {
                *samples = read_f64_vec(&blob, &mut cursor)?;
            }
            if cursor != blob.len() {
                return Err(ProjectError::Invalid(
                    "electrophysiology sample blob has trailing bytes".to_owned(),
                ));
            }
            recording
        }
        // Legacy layout: the blob is the whole dataset serialized as JSON.
        STORAGE_ELECTROPHYSIOLOGY_JSON => serde_json::from_slice(&blob).map_err(|error| {
            ProjectError::Invalid(format!("invalid electrophysiology payload: {error}"))
        })?,
        other => {
            return Err(ProjectError::Unsupported(format!(
                "electrophysiology payload storage {other}"
            )));
        }
    };
    Ok(Dataset::Electrophysiology(Box::new(recording)))
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

fn read_f64_vec(blob: &[u8], cursor: &mut usize) -> Result<Vec<f64>> {
    let len_end = cursor.checked_add(8).ok_or_else(|| {
        ProjectError::Invalid("electrophysiology blob length overflow".to_owned())
    })?;
    let len_bytes = blob.get(*cursor..len_end).ok_or_else(|| {
        ProjectError::Invalid("electrophysiology blob truncated before a length".to_owned())
    })?;
    let len = usize::try_from(u64::from_le_bytes(len_bytes.try_into().unwrap()))
        .map_err(|_| ProjectError::Invalid("electrophysiology vector is too large".to_owned()))?;
    let byte_len = len.checked_mul(8).ok_or_else(|| {
        ProjectError::Invalid("electrophysiology sample count overflows".to_owned())
    })?;
    let data_end = len_end.checked_add(byte_len).ok_or_else(|| {
        ProjectError::Invalid("electrophysiology blob length overflow".to_owned())
    })?;
    let data = blob.get(len_end..data_end).ok_or_else(|| {
        ProjectError::Invalid("electrophysiology blob truncated inside a vector".to_owned())
    })?;
    *cursor = data_end;
    Ok(data
        .chunks_exact(8)
        .map(|chunk| f64::from_le_bytes(chunk.try_into().unwrap()))
        .collect())
}
