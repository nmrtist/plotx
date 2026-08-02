use super::convert_recipes::{nmr2d_recipe_extensions, read_region_analysis};
use super::electrophysiology_convert::{
    electrophysiology_from_object, electrophysiology_to_objects,
};
use super::field_catalog::read as read_field_catalog;
use super::*;
use crate::{DosyMethod, PseudoDisplay};
use plotx_processing::Processed2D;

pub enum DatasetBlob<'a> {
    Complex(&'a [Complex64]),
    Electrophysiology(&'a crate::state::ElectrophysiologyDataset),
    Afm(&'a plotx_io::AfmData),
    MassSpec(&'a crate::state::MassSpecDataset),
}

pub struct DatasetObjects<'a> {
    pub data: DataObject,
    pub blob: DatasetBlob<'a>,
    pub recipe: RecipeObject,
    /// Extra blobs this dataset owns, as (zip path, bytes). Written verbatim.
    pub extra_blobs: Vec<(String, Vec<u8>)>,
}

impl<'a> DatasetObjects<'a> {
    fn primary(data: DataObject, blob: DatasetBlob<'a>, recipe: RecipeObject) -> Self {
        Self {
            data,
            blob,
            recipe,
            extra_blobs: Vec::new(),
        }
    }
}

pub fn dataset_to_objects<'a>(
    dataset: &'a Dataset,
    data_id: &str,
    recipe_id: &str,
) -> Result<DatasetObjects<'a>> {
    Ok(match dataset {
        Dataset::Nmr(n) => {
            let data = DataObject {
                id: data_id.to_owned(),
                role: "data".to_owned(),
                classification: nmr_acquisition_classification(),
                label: n.name.clone(),
                dimensions: vec![dimension_from_1d(&n.data)],
                payload: Payload {
                    storage: STORAGE_COMPLEX_F64_LE.to_owned(),
                    blob: format!("objects/{data_id}/data.bin"),
                    shape: vec![n.data.points.len()],
                    domain: domain_to_str(n.data.domain).to_owned(),
                },
                extensions: serde_json::json!({
                    "plotx.nmr": {
                        "source": &n.data.source
                    },
                    "plotx.fields": &n.field_catalog
                }),
            };
            let recipe = RecipeObject {
                id: recipe_id.to_owned(),
                role: "recipe".to_owned(),
                classification: nmr_recipe_classification(),
                input: data_id.to_owned(),
                parameters: RecipeParameters {
                    dimension_count: 1,
                    pipelines: vec![pipeline_to_dto(&n.pipeline)],
                    group_delay_correct: n.group_delay_correct,
                    ..RecipeParameters::default()
                },
                extensions: serde_json::json!({
                    "plotx.step_allocator": {
                        "next_id": n.next_step_id
                    },
                    "plotx.analysis": {
                        "peaks": &n.peaks,
                        "integrals": &n.integrals,
                        "line_fits": &n.line_fits,
                        "multiplets": &n.multiplets
                    }
                }),
            };
            DatasetObjects::primary(data, DatasetBlob::Complex(&n.data.points), recipe)
        }
        Dataset::Nmr2D(n) => {
            let data = DataObject {
                id: data_id.to_owned(),
                role: "data".to_owned(),
                classification: nmr_acquisition_classification(),
                label: n.name.clone(),
                dimensions: vec![
                    dimension_from_dim("f1", "indirect", 0, n.data.rows, &n.data.indirect),
                    dimension_from_dim("f2", "direct", 1, n.data.cols, &n.data.direct),
                ],
                payload: Payload {
                    storage: STORAGE_COMPLEX_F64_LE.to_owned(),
                    blob: format!("objects/{data_id}/data.bin"),
                    shape: vec![n.data.rows, n.data.cols],
                    domain: domain_to_str(n.data.domain).to_owned(),
                },
                extensions: serde_json::json!({
                    "plotx.nmr": {
                        "source": &n.data.source,
                        "quad": quad_to_str(n.data.quad),
                        "indirect_conjugate": n.data.indirect_conjugate,
                        "experiment_hint": &n.data.experiment,
                        "pseudo_axis": n.data.pseudo_axis.as_ref().map(pseudo_axis_to_dto),
                        "diffusion": n.data.diffusion.as_ref().map(diffusion_to_dto),
                    },
                    "plotx.fields": &n.field_catalog
                }),
            };
            let has_dosy_state = n.display != PseudoDisplay::Stack
                || n.dosy_method != DosyMethod::MonoExp
                || n.dosy_map.is_some()
                || n.ilt_map.is_some()
                || n.dosy_provenance.is_some()
                || n.ilt_provenance.is_some();
            let (dosy_extension, extra_blobs) = if has_dosy_state {
                // Only the map-without-provenance direction is refused: a stored
                // map nobody can attribute is the orphan this guard exists to
                // prevent, and every builder installs both together, so reaching
                // it means a bug rather than user state. The mirror case is
                // legitimate and reachable — a load whose blob failed to decode
                // keeps the provenance and drops the numbers — and refusing it
                // would make that project permanently unsaveable.
                if n.dosy_map.is_some() && n.dosy_provenance.is_none() {
                    return Err(ProjectError::Invalid(
                        "per-column DOSY map has no provenance to store with it".to_owned(),
                    ));
                }
                if n.ilt_map.is_some() && n.ilt_provenance.is_none() {
                    return Err(ProjectError::Invalid(
                        "ILT DOSY map has no provenance to store with it".to_owned(),
                    ));
                }
                let dosy_path = format!("objects/{data_id}/dosy.bin");
                let (bytes, shapes) =
                    super::dosy_convert::encode_dosy(n.dosy_map.as_ref(), n.ilt_map.as_ref())?;
                let extension = super::dosy_convert::DosyExtensionDto::new(
                    n.display,
                    n.dosy_method,
                    super::dosy_convert::DosyProvenanceDto {
                        diffusion: n.dosy_provenance.clone(),
                        ilt: n.ilt_provenance.clone(),
                    },
                    dosy_path.clone(),
                    shapes,
                );
                (
                    Some(serde_json::to_value(extension)?),
                    vec![(dosy_path, bytes)],
                )
            } else {
                (None, Vec::new())
            };
            let recipe = RecipeObject {
                id: recipe_id.to_owned(),
                role: "recipe".to_owned(),
                classification: nmr_recipe_classification(),
                input: data_id.to_owned(),
                parameters: RecipeParameters {
                    dimension_count: 2,
                    pipelines: vec![pipeline_to_dto(&n.params.f2), pipeline_to_dto(&n.params.f1)],
                    group_delay_correct: n.group_delay_correct,
                    layout: Some(layout_to_str(n.params.layout).to_owned()),
                    preset: Some(preset_to_str(n.preset).to_owned()),
                },
                extensions: nmr2d_recipe_extensions(n, dosy_extension),
            };
            DatasetObjects {
                data,
                blob: DatasetBlob::Complex(&n.data.data),
                recipe,
                extra_blobs,
            }
        }
        Dataset::Table(t) => {
            return Err(ProjectError::Invalid(format!(
                "typed table {} reached the generic object encoder",
                t.resource_id
            )));
        }
        Dataset::Electrophysiology(recording) => {
            let (data, recipe) = electrophysiology_to_objects(recording, data_id, recipe_id)?;
            DatasetObjects::primary(data, DatasetBlob::Electrophysiology(recording), recipe)
        }
        Dataset::Afm(afm) => {
            let data = DataObject {
                id: data_id.to_owned(),
                role: "data".to_owned(),
                classification: Classification {
                    domain: "afm".to_owned(),
                    technique: Some("nanoscope".to_owned()),
                    object: "acquisition".to_owned(),
                },
                label: afm.name.clone(),
                dimensions: Vec::new(),
                payload: Payload {
                    storage: STORAGE_AFM_V1.to_owned(),
                    blob: format!("objects/{data_id}/data.bin"),
                    shape: afm.data.forces.as_ref().map_or_else(Vec::new, |forces| {
                        vec![
                            forces.grid_height,
                            forces.grid_width,
                            forces.samples_per_curve,
                        ]
                    }),
                    domain: "afm".to_owned(),
                },
                extensions: serde_json::json!({
                    "plotx.afm": {
                        "selected_pixel": afm.selected_pixel
                    },
                    "plotx.fields": &afm.field_catalog
                }),
            };
            let recipe = RecipeObject {
                id: recipe_id.to_owned(),
                role: "recipe".to_owned(),
                classification: Classification {
                    domain: "afm".to_owned(),
                    technique: Some("nanoscope".to_owned()),
                    object: "display_recipe".to_owned(),
                },
                input: data_id.to_owned(),
                parameters: RecipeParameters::default(),
                extensions: serde_json::Value::Null,
            };
            DatasetObjects::primary(data, DatasetBlob::Afm(&afm.data), recipe)
        }
        Dataset::MassSpec(mass_spec) => {
            let scan_count = mass_spec
                .run
                .streams
                .iter()
                .map(|stream| stream.spectra.len())
                .sum();
            let point_count = mass_spec
                .run
                .streams
                .iter()
                .flat_map(|stream| &stream.spectra)
                .map(|scan| scan.mz.len())
                .sum();
            let data = DataObject {
                id: data_id.to_owned(),
                role: "data".to_owned(),
                classification: Classification {
                    domain: "mass_spectrometry".to_owned(),
                    technique: Some("lc_ms".to_owned()),
                    object: "acquisition".to_owned(),
                },
                label: mass_spec.name.clone(),
                dimensions: Vec::new(),
                payload: Payload {
                    storage: STORAGE_MASS_SPEC_V1.to_owned(),
                    blob: format!("objects/{data_id}/data.bin"),
                    shape: vec![mass_spec.run.streams.len(), scan_count, point_count],
                    domain: "mass_spectrometry".to_owned(),
                },
                extensions: serde_json::json!({
                    "plotx.fields": &mass_spec.field_catalog
                }),
            };
            let recipe = RecipeObject {
                id: recipe_id.to_owned(),
                role: "recipe".to_owned(),
                classification: Classification {
                    domain: "mass_spectrometry".to_owned(),
                    technique: Some("lc_ms".to_owned()),
                    object: "display_recipe".to_owned(),
                },
                input: data_id.to_owned(),
                parameters: RecipeParameters::default(),
                extensions: serde_json::Value::Null,
            };
            DatasetObjects::primary(data, DatasetBlob::MassSpec(mass_spec), recipe)
        }
    })
}
pub fn object_to_dataset(
    zip: &mut zip::ZipArchive<File>,
    data: &DataObject,
    recipe: &RecipeObject,
) -> Result<Dataset> {
    // Named generic decoder functions do not satisfy the higher-ranked lifetime
    // required by `ZipFile`; closures let the compiler reborrow each entry.
    #[allow(clippy::redundant_closure)]
    if data.classification.domain == "mass_spectrometry" {
        if data.payload.storage != STORAGE_MASS_SPEC_V1 {
            return Err(ProjectError::Unsupported(format!(
                "LC–MS payload storage {}",
                data.payload.storage
            )));
        }
        let mut dataset = read_entry(
            zip,
            &data.payload.blob,
            "LC–MS payload",
            ProjectLoadLimits::default().max_entry_bytes,
            |reader| super::mass_spec_convert::decode(reader),
        )?;
        dataset.field_catalog = read_field_catalog(data)?;
        dataset.name = data.label.clone();
        dataset.repair_selection().map_err(ProjectError::Invalid)?;
        let dataset = Dataset::MassSpec(Box::new(dataset));
        dataset
            .validate_field_catalog()
            .map_err(ProjectError::Invalid)?;
        return Ok(dataset);
    }
    if data.classification.domain == "afm" && data.payload.storage == STORAGE_AFM_V1 {
        #[allow(clippy::redundant_closure)]
        let decoded = read_entry(
            zip,
            &data.payload.blob,
            "AFM payload",
            ProjectLoadLimits::default().max_entry_bytes,
            |reader| super::afm_convert::decode_afm(reader),
        )?;
        let mut dataset = crate::state::AfmDataset::load(decoded);
        dataset.field_catalog = read_field_catalog(data)?;
        dataset.name = data.label.clone();
        if let Some(state) = data.extensions.get("plotx.afm")
            && let Some(pixel) = state
                .get("selected_pixel")
                .and_then(serde_json::Value::as_array)
            && let (Some(x), Some(y)) = (
                pixel.first().and_then(serde_json::Value::as_u64),
                pixel.get(1).and_then(serde_json::Value::as_u64),
            )
        {
            dataset.selected_pixel = [x as usize, y as usize];
        }
        let dataset = Dataset::Afm(Box::new(dataset));
        dataset
            .validate_field_catalog()
            .map_err(ProjectError::Invalid)?;
        return Ok(dataset);
    }
    if data.classification.domain == "electrophysiology"
        && data.classification.object == "recording"
    {
        return electrophysiology_from_object(zip, data);
    }
    if data.classification.object == "table" {
        if data.payload.storage != STORAGE_TABLE_V1 {
            return Err(ProjectError::Unsupported(
                "legacy DataTable payload; this project must be regenerated".to_owned(),
            ));
        }
        return table_dataset_from_v1(zip, data).map(|table| Dataset::Table(Box::new(table)));
    }
    if data.classification.domain != "spectroscopy"
        || data.classification.technique.as_deref() != Some("nmr")
        || data.classification.object != "acquisition"
    {
        return Err(ProjectError::Unsupported(format!(
            "data classification {}/{:?}/{}",
            data.classification.domain, data.classification.technique, data.classification.object
        )));
    }
    if data.payload.storage != STORAGE_COMPLEX_F64_LE {
        return Err(ProjectError::Unsupported(format!(
            "payload storage {}",
            data.payload.storage
        )));
    }
    let expected_values = match data.dimensions.len() {
        1 => data
            .payload
            .shape
            .first()
            .copied()
            .unwrap_or(data.dimensions[0].size),
        2 => data
            .payload
            .shape
            .first()
            .copied()
            .zip(data.payload.shape.get(1).copied())
            .ok_or_else(|| ProjectError::Invalid("2D payload shape is incomplete".to_owned()))?
            .0
            .checked_mul(data.payload.shape[1])
            .ok_or_else(|| ProjectError::Invalid("2D NMR shape overflows usize".to_owned()))?,
        n => {
            return Err(ProjectError::Unsupported(format!(
                "NMR acquisitions with {n} dimensions"
            )));
        }
    };
    let expected_bytes = expected_values.checked_mul(16).ok_or_else(|| {
        ProjectError::Invalid("NMR payload byte length overflows usize".to_owned())
    })?;
    let values = read_entry(
        zip,
        &data.payload.blob,
        "NMR complex-f64 payload",
        ProjectLoadLimits::default().max_entry_bytes,
        |reader| {
            if reader.remaining() != expected_bytes as u64 {
                return Err(reader.invalid(format!(
                    "complex payload has {} bytes but shape requires {expected_bytes}",
                    reader.remaining()
                )));
            }
            complex_from_reader(reader)
        },
    )?;
    match data.dimensions.len() {
        1 => {
            let dim = data.dimensions.first().unwrap();
            let expected = data.payload.shape.first().copied().unwrap_or(dim.size);
            if values.len() != expected {
                return Err(ProjectError::Invalid(format!(
                    "1D data length {} does not match shape {expected}",
                    values.len()
                )));
            }
            let mut dataset = NmrDataset::load(NmrData {
                points: values,
                domain: domain_from_str(&data.payload.domain),
                spectral_width_hz: required(dim.spectral_width_hz, "spectral_width_hz")?,
                observe_freq_mhz: required(dim.observe_freq_mhz, "observe_freq_mhz")?,
                carrier_ppm: required(dim.carrier_ppm, "carrier_ppm")?,
                nucleus: dim.nucleus.clone().unwrap_or_else(|| "X".to_owned()),
                source: nmr_source(data),
                group_delay: dim.group_delay.unwrap_or(0.0),
            });
            dataset.field_catalog = read_field_catalog(data)?;
            apply_1d_recipe(&mut dataset, recipe)?;
            dataset.name = data.label.clone();
            dataset.retransform();
            let dataset = Dataset::Nmr(Box::new(dataset));
            dataset
                .validate_field_catalog()
                .map_err(ProjectError::Invalid)?;
            Ok(dataset)
        }
        2 => {
            let rows = *data
                .payload
                .shape
                .first()
                .ok_or_else(|| ProjectError::Invalid("2D payload missing rows".to_owned()))?;
            let cols = *data
                .payload
                .shape
                .get(1)
                .ok_or_else(|| ProjectError::Invalid("2D payload missing cols".to_owned()))?;
            let expected_len = rows
                .checked_mul(cols)
                .ok_or_else(|| ProjectError::Invalid("2D NMR shape overflows usize".to_owned()))?;
            if values.len() != expected_len {
                return Err(ProjectError::Invalid(format!(
                    "2D data length {} does not match shape {}x{}",
                    values.len(),
                    rows,
                    cols
                )));
            }
            let direct = data
                .dimensions
                .iter()
                .find(|d| d.role == "direct")
                .or_else(|| data.dimensions.iter().find(|d| d.storage_axis == 1))
                .ok_or_else(|| {
                    ProjectError::Invalid("2D data missing direct dimension".to_owned())
                })?;
            let indirect = data
                .dimensions
                .iter()
                .find(|d| d.role == "indirect")
                .or_else(|| data.dimensions.iter().find(|d| d.storage_axis == 0))
                .ok_or_else(|| {
                    ProjectError::Invalid("2D data missing indirect dimension".to_owned())
                })?;
            let mut dataset = Nmr2DDataset::load(NmrData2D {
                data: values,
                rows,
                cols,
                domain: domain_from_str(&data.payload.domain),
                direct: dim_from_dimension(direct)?,
                indirect: dim_from_dimension(indirect)?,
                quad: quad_from_str(nmr_ext_str(data, "quad").unwrap_or("complex")),
                indirect_conjugate: nmr_ext_bool(data, "indirect_conjugate").unwrap_or(false),
                experiment: nmr_ext_str(data, "experiment_hint").map(str::to_owned),
                pseudo_axis: read_pseudo_axis(data),
                diffusion: read_diffusion(data),
                nus: None,
                source: nmr_source(data),
            });
            dataset.field_catalog = read_field_catalog(data)?;
            apply_2d_recipe(&mut dataset, recipe)?;
            read_region_analysis(&mut dataset, recipe)?;
            read_integrals_2d(&mut dataset, recipe)?;
            read_peaks_2d(&mut dataset, recipe)?;
            dataset.name = data.label.clone();
            dataset.retransform();
            // `retransform` deliberately invalidates every analysis map. Restore
            // auxiliary DOSY state only after it, or a load will silently erase
            // the stored result and serve the stack fallback instead.
            restore_dosy(zip, &mut dataset, recipe);
            if let Err(error) = dataset.recompute_integrals() {
                dataset.integral_error = Some(error.to_string());
            }
            let dataset = Dataset::Nmr2D(Box::new(dataset));
            dataset
                .validate_field_catalog()
                .map_err(ProjectError::Invalid)?;
            Ok(dataset)
        }
        n => Err(ProjectError::Unsupported(format!(
            "NMR acquisitions with {n} dimensions"
        ))),
    }
}

fn restore_dosy(
    zip: &mut zip::ZipArchive<File>,
    dataset: &mut Nmr2DDataset,
    recipe: &RecipeObject,
) {
    let Some(value) = recipe.extensions.get("plotx.dosy") else {
        return;
    };
    let extension: super::dosy_convert::DosyExtensionDto =
        match serde_json::from_value(value.clone()) {
            Ok(extension) => extension,
            Err(error) => {
                dataset.dosy_provenance_warning = Some(format!(
                    "The stored DOSY state is malformed ({error}), so PlotX is showing the \
                     reconstructed stack instead. Rebuild the DOSY map."
                ));
                return;
            }
        };
    dataset.display = extension.display;
    dataset.dosy_method = extension.method;
    dataset.dosy_provenance = extension.provenance.diffusion;
    dataset.ilt_provenance = extension.provenance.ilt;

    let decoded = if extension.storage != STORAGE_DOSY_V1 {
        Err(ProjectError::Unsupported(format!(
            "DOSY payload storage {}",
            extension.storage
        )))
    } else {
        read_entry(
            zip,
            &extension.blob,
            "DOSY payload",
            ProjectLoadLimits::default().max_entry_bytes,
            |reader| super::dosy_convert::decode_dosy(reader, &extension.shapes),
        )
    };
    match decoded {
        Ok(decoded) => {
            dataset.dosy_map = decoded.dosy_map;
            dataset.ilt_map = decoded.ilt_map;
        }
        Err(error) => {
            dataset.dosy_provenance_warning = Some(unavailable_dosy_warning(
                dataset.dosy_method,
                &error.to_string(),
            ));
            return;
        }
    }

    let mut warnings = Vec::new();
    let Processed2D::Stack(stack) = &dataset.processed else {
        // Do not claim the stored map is being shown: `figure()` selects on the
        // processed result before it consults `display`, so a true-2D result
        // always draws the contour spectrum and never a DOSY map.
        warnings.push(
            "The stored DOSY map belongs to a pseudo-2D stack, but the processing recipe now \
             reconstructs a true-2D spectrum, so the map is not shown. Return the dataset to its \
             pseudo-2D layout and rebuild the DOSY map."
                .to_owned(),
        );
        dataset.dosy_provenance_warning = Some(warnings.join(" "));
        return;
    };
    // Both methods hash the same inputs, so one reconstruction serves both.
    if let (Some(axis), Some(meta)) = (
        dataset.data.pseudo_axis.as_ref(),
        dataset.data.diffusion.as_ref(),
    ) {
        let reconstructed = crate::state::dosy_data_fingerprint(stack, &axis.values, meta);
        for (label, provenance) in [
            (
                "per-column DOSY",
                dataset
                    .dosy_map
                    .as_ref()
                    .and(dataset.dosy_provenance.as_ref()),
            ),
            (
                "ILT DOSY",
                dataset
                    .ilt_map
                    .as_ref()
                    .and(dataset.ilt_provenance.as_ref()),
            ),
        ] {
            let Some(provenance) = provenance else {
                continue;
            };
            if reconstructed != provenance.data_fingerprint {
                warnings.push(fingerprint_warning(
                    label,
                    &provenance.data_fingerprint,
                    &reconstructed,
                ));
            }
        }
    }
    // "The selected map is absent" is deliberately *not* stored here. It is a
    // pure function of the display, the method and which maps exist, and
    // `Nmr2DDataset::missing_selected_map_note` already derives it. A stored copy
    // would survive the user switching to a method that does have a map, and
    // would then keep claiming the stack is shown while the map is on screen —
    // and because readers prefer the stored warning, the stale copy would win.
    dataset.dosy_provenance_warning = (!warnings.is_empty()).then(|| warnings.join(" "));
}

fn unavailable_dosy_warning(method: DosyMethod, reason: &str) -> String {
    let (label, action) = match method {
        DosyMethod::MonoExp => ("per-column DOSY", "Build the per-column DOSY map"),
        DosyMethod::Ilt(_) => ("ILT DOSY", "Build the ILT DOSY map"),
    };
    format!(
        "The stored {label} map could not be loaded ({reason}), so PlotX is showing the stack \
         instead. {action} to replace it."
    )
}

fn fingerprint_warning(label: &str, stored: &str, reconstructed: &str) -> String {
    // A short prefix, not the full digest: two 64-character hashes bury the one
    // sentence the reader has to act on, and nothing downstream reconstructs the
    // input from them. The prefix still distinguishes one mismatch from another.
    format!(
        "The stored {label} map was produced from different data than the recipe now reconstructs \
         (saved data {}, rebuilt data {}). The stored map is being shown. Rebuild the {label} map \
         to update it.",
        short_fingerprint(stored),
        short_fingerprint(reconstructed),
    )
}

fn short_fingerprint(value: &str) -> &str {
    value.get(..12).unwrap_or(value)
}
