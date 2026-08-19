use super::convert::{DatasetBlob, DatasetObjects};
use super::{
    Classification, DataObject, Dataset, Payload, ProjectError, ProjectLoadLimits, RecipeObject,
    RecipeParameters, Result, STORAGE_XPS, read_entry,
};
use plotx_io::xps::XpsExperiment;
use std::fs::File;
use std::io::{Read, Write};

const MAGIC: &[u8; 8] = b"PLOTXXPS";
const MAX_METADATA_BYTES: usize = 16 * 1024 * 1024;
const MAX_VALUES: usize = 16 * 1024 * 1024;

pub(super) fn to_objects<'a>(
    xps: &'a crate::state::XpsDataset,
    data_id: &str,
    recipe_id: &str,
) -> DatasetObjects<'a> {
    let data = DataObject {
        id: data_id.to_owned(),
        role: "data".to_owned(),
        classification: Classification {
            domain: "spectroscopy".to_owned(),
            technique: Some("xps".to_owned()),
            object: "experiment".to_owned(),
        },
        label: xps.name.clone(),
        dimensions: Vec::new(),
        payload: Payload {
            storage: STORAGE_XPS.to_owned(),
            blob: format!("objects/{data_id}/data.bin"),
            shape: vec![
                xps.experiment.measurements.len(),
                xps.experiment.regions.len(),
            ],
            domain: "binding_energy".to_owned(),
        },
        extensions: serde_json::json!({ "plotx.fields": &xps.field_catalog }),
    };
    let recipe = RecipeObject {
        id: recipe_id.to_owned(),
        role: "recipe".to_owned(),
        classification: Classification {
            domain: "spectroscopy".to_owned(),
            technique: Some("xps".to_owned()),
            object: "processing_recipe".to_owned(),
        },
        input: data_id.to_owned(),
        parameters: RecipeParameters::default(),
        extensions: serde_json::json!({ "plotx.xps": {
            "active_region": xps.active_region.0,
            "measurement_shifts": &xps.measurement_shifts,
            "region_recipes": &xps.region_recipes,
            "fit_workspaces": &xps.fit_workspaces,
            "fits": &xps.fits,
            "next_step_id": xps.next_step_id
        }}),
    };
    DatasetObjects::primary(data, DatasetBlob::Xps(&xps.experiment), recipe)
}

pub(super) fn matches(data: &DataObject) -> bool {
    data.classification.domain == "spectroscopy"
        && data.classification.technique.as_deref() == Some("xps")
}

pub(super) fn from_objects(
    zip: &mut zip::ZipArchive<File>,
    data: &DataObject,
    recipe: &RecipeObject,
) -> Result<Dataset> {
    if data.payload.storage != STORAGE_XPS {
        return Err(ProjectError::Unsupported(format!(
            "XPS payload storage {}",
            data.payload.storage
        )));
    }
    let experiment: XpsExperiment = read_entry(
        zip,
        &data.payload.blob,
        "XPS payload",
        ProjectLoadLimits::default().max_entry_bytes,
        |reader| read(reader),
    )?;
    experiment.validate().map_err(ProjectError::Invalid)?;
    let mut dataset = crate::state::XpsDataset::load(experiment);
    dataset.scientific_identity = super::convert::read_scientific_identity(data)?;
    dataset.field_catalog = super::field_catalog::read(data)?;
    dataset.name = data.label.clone();
    let state = recipe
        .extensions
        .get("plotx.xps")
        .ok_or_else(|| ProjectError::Invalid("XPS recipe state is missing".into()))?;
    let active = state
        .get("active_region")
        .and_then(serde_json::Value::as_u64)
        .ok_or_else(|| ProjectError::Invalid("XPS active region is missing".into()))?;
    if !dataset.select_region(plotx_io::xps::XpsRegionId(active)) {
        return Err(ProjectError::Invalid(
            "XPS active region does not exist".into(),
        ));
    }
    dataset.measurement_shifts = serde_json::from_value(
        state
            .get("measurement_shifts")
            .cloned()
            .ok_or_else(|| ProjectError::Invalid("XPS measurement shifts are missing".into()))?,
    )?;
    dataset.region_recipes = serde_json::from_value(
        state
            .get("region_recipes")
            .cloned()
            .ok_or_else(|| ProjectError::Invalid("XPS region recipes are missing".into()))?,
    )?;
    dataset.fit_workspaces = serde_json::from_value(
        state
            .get("fit_workspaces")
            .cloned()
            .ok_or_else(|| ProjectError::Invalid("XPS fit workspaces are missing".into()))?,
    )?;
    dataset.fits = serde_json::from_value(
        state
            .get("fits")
            .cloned()
            .ok_or_else(|| ProjectError::Invalid("XPS fits are missing".into()))?,
    )?;
    dataset.next_step_id = state
        .get("next_step_id")
        .and_then(serde_json::Value::as_u64)
        .ok_or_else(|| ProjectError::Invalid("XPS step allocator is missing".into()))?;
    dataset
        .validate_and_rehydrate_fits()
        .map_err(ProjectError::Invalid)?;
    let mut dataset = Dataset::Xps(Box::new(dataset));
    let processing = crate::actions::DatasetProcessingState::from_dataset(&dataset);
    processing
        .apply_to(&mut dataset)
        .map_err(|error| ProjectError::Invalid(error.to_string()))?;
    dataset
        .validate_field_catalog()
        .map_err(ProjectError::Invalid)?;
    Ok(dataset)
}

pub(super) fn write(mut output: impl Write, experiment: &XpsExperiment) -> Result<()> {
    let mut metadata = experiment.clone();
    for region in &mut metadata.regions {
        region.native_energy_ev.clear();
        region.intensity_cps.clear();
        if let Some(values) = &mut region.binding_energy_ev {
            values.clear();
        }
        if let Some(values) = &mut region.counts {
            values.clear();
        }
        if let Some(fit) = &mut region.imported_fit {
            fit.background_cps.clear();
            fit.envelope_cps.clear();
            for component in &mut fit.components_cps {
                component.clear();
            }
        }
    }
    let metadata = serde_json::to_vec(&metadata)?;
    if metadata.len() > MAX_METADATA_BYTES {
        return Err(ProjectError::Invalid(
            "XPS metadata exceeds its binary payload limit".into(),
        ));
    }
    output.write_all(MAGIC)?;
    write_len(&mut output, metadata.len())?;
    output.write_all(&metadata)?;
    for region in &experiment.regions {
        write_values(&mut output, &region.native_energy_ev)?;
        if let Some(values) = &region.binding_energy_ev {
            write_values(&mut output, values)?;
        }
        write_values(&mut output, &region.intensity_cps)?;
        if let Some(values) = &region.counts {
            write_values(&mut output, values)?;
        }
        if let Some(fit) = &region.imported_fit {
            write_values(&mut output, &fit.background_cps)?;
            write_values(&mut output, &fit.envelope_cps)?;
            for component in &fit.components_cps {
                write_values(&mut output, component)?;
            }
        }
    }
    Ok(())
}

pub(super) fn read(mut input: impl Read) -> Result<XpsExperiment> {
    let mut magic = [0_u8; 8];
    input.read_exact(&mut magic)?;
    if &magic != MAGIC {
        return Err(ProjectError::Invalid("XPS payload magic is invalid".into()));
    }
    let metadata_len = read_len(&mut input, MAX_METADATA_BYTES, "metadata")?;
    let mut metadata = vec![0_u8; metadata_len];
    input.read_exact(&mut metadata)?;
    let mut experiment: XpsExperiment = serde_json::from_slice(&metadata)?;
    for region in &mut experiment.regions {
        region.native_energy_ev = read_values(&mut input, "native energy")?;
        if region.binding_energy_ev.is_some() {
            region.binding_energy_ev = Some(read_values(&mut input, "binding energy")?);
        }
        region.intensity_cps = read_values(&mut input, "intensity")?;
        if region.counts.is_some() {
            region.counts = Some(read_values(&mut input, "counts")?);
        }
        if let Some(fit) = &mut region.imported_fit {
            fit.background_cps = read_values(&mut input, "imported background")?;
            fit.envelope_cps = read_values(&mut input, "imported envelope")?;
            for component in &mut fit.components_cps {
                *component = read_values(&mut input, "imported component")?;
            }
        }
    }
    let mut trailing = [0_u8; 1];
    if input.read(&mut trailing)? != 0 {
        return Err(ProjectError::Invalid(
            "XPS payload has trailing bytes".into(),
        ));
    }
    experiment.validate().map_err(ProjectError::Invalid)?;
    Ok(experiment)
}

fn write_len(output: &mut impl Write, value: usize) -> Result<()> {
    let value = u64::try_from(value)
        .map_err(|_| ProjectError::Invalid("XPS payload length exceeds u64".into()))?;
    output.write_all(&value.to_le_bytes())?;
    Ok(())
}

fn read_len(input: &mut impl Read, maximum: usize, label: &str) -> Result<usize> {
    let mut bytes = [0_u8; 8];
    input.read_exact(&mut bytes)?;
    let value = usize::try_from(u64::from_le_bytes(bytes))
        .map_err(|_| ProjectError::Invalid(format!("XPS {label} length exceeds usize")))?;
    if value > maximum {
        return Err(ProjectError::Invalid(format!(
            "XPS {label} exceeds its payload limit"
        )));
    }
    Ok(value)
}

fn write_values(output: &mut impl Write, values: &[f64]) -> Result<()> {
    if values.len() > MAX_VALUES {
        return Err(ProjectError::Invalid(
            "XPS array exceeds its payload limit".into(),
        ));
    }
    write_len(output, values.len())?;
    for value in values {
        output.write_all(&value.to_le_bytes())?;
    }
    Ok(())
}

fn read_values(input: &mut impl Read, label: &str) -> Result<Vec<f64>> {
    let count = read_len(input, MAX_VALUES, label)?;
    let mut values = Vec::new();
    values
        .try_reserve_exact(count)
        .map_err(|_| ProjectError::Invalid(format!("could not allocate XPS {label} array")))?;
    let mut bytes = [0_u8; 8];
    for _ in 0..count {
        input.read_exact(&mut bytes)?;
        values.push(f64::from_le_bytes(bytes));
    }
    Ok(values)
}
