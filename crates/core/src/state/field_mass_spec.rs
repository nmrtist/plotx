use super::*;
use crate::automation::{CAP_FIELD_MASS_CHROMATOGRAM, CAP_FIELD_MASS_SPECTRUM, CapabilityId};
use crate::state::{FieldMetadata, MassSpecDataset, readable_ms_stream};

pub(super) fn default_field_id(dataset: &MassSpecDataset) -> Option<FieldId> {
    dataset
        .run
        .streams
        .iter()
        .find(|stream| readable_ms_stream(stream))
        .and_then(|stream| dataset.field_catalog.id_for_key(&stream_tic_key(stream.id)))
        .or_else(|| {
            dataset
                .run
                .chromatograms
                .iter()
                .find(|channel| channel.source_stream.is_none() && channel.kind.is_signal())
                .and_then(|channel| {
                    dataset
                        .field_catalog
                        .id_for_key(&channel_key(&channel.id.0))
                })
        })
}

pub(super) fn descriptor(dataset: &MassSpecDataset, id: FieldId) -> Option<FieldDescriptor> {
    let key = dataset.field_catalog.key_for_id(id)?;
    if let Some(channel_id) = key.strip_prefix("mass_spec.channel.") {
        let channel = dataset
            .run
            .chromatograms
            .iter()
            .find(|channel| channel.id.0 == channel_id)?;
        return Some(build(
            dataset,
            id,
            key,
            &channel.description,
            CAP_FIELD_MASS_CHROMATOGRAM,
            channel.values.len(),
            vec!["min".to_owned(), channel.unit.clone()],
        ));
    }
    for stream in dataset
        .run
        .streams
        .iter()
        .filter(|stream| readable_ms_stream(stream))
    {
        let stream_label = stream_display_label(stream);
        if key == stream_tic_key(stream.id) {
            return Some(build(
                dataset,
                id,
                key,
                &format!("{stream_label} TIC"),
                CAP_FIELD_MASS_CHROMATOGRAM,
                stream.spectra.len(),
                vec!["min".to_owned()],
            ));
        }
        if key == stream_bpi_key(stream.id) {
            return Some(build(
                dataset,
                id,
                key,
                &format!("{stream_label} BPI"),
                CAP_FIELD_MASS_CHROMATOGRAM,
                stream.spectra.len(),
                vec!["min".to_owned()],
            ));
        }
        if key == stream_spectrum_key(stream.id) {
            let length = stream
                .spectra
                .iter()
                .map(|scan| scan.mz.len())
                .max()
                .unwrap_or(0);
            return Some(build(
                dataset,
                id,
                key,
                &format!("{stream_label} current spectrum"),
                CAP_FIELD_MASS_SPECTRUM,
                length,
                vec!["m/z".to_owned()],
            ));
        }
    }
    if let Some(extraction) = dataset
        .extracted_spectra
        .iter()
        .find(|item| key == extracted_stream_spectrum_key(item.id))
    {
        return Some(build(
            dataset,
            id,
            key,
            &extraction_title(&dataset.run, extraction),
            CAP_FIELD_MASS_SPECTRUM,
            0,
            vec!["m/z".to_owned()],
        ));
    }
    let xic = dataset
        .extracted_ion_chromatograms
        .iter()
        .find(|item| key == xic_key(item.id))?;
    Some(build(
        dataset,
        id,
        key,
        &xic_title(&dataset.run, xic),
        CAP_FIELD_MASS_CHROMATOGRAM,
        xic.intensity.len(),
        vec!["min".to_owned()],
    ))
}

fn build(
    dataset: &MassSpecDataset,
    id: FieldId,
    key: &str,
    name: &str,
    capability: &str,
    length: usize,
    units: Vec<String>,
) -> FieldDescriptor {
    let x_unit = units.first().cloned().unwrap_or_default();
    let intrinsic = dataset
        .field_representation(id)
        .map(FieldRepresentation::intrinsic_capabilities)
        .unwrap_or_default();
    FieldDescriptor {
        id,
        local_id: key.to_owned(),
        name: name.to_owned(),
        scientific_observation: SummaryPart::new(format!("field:{key}"), name),
        capabilities: FieldCapabilities::new(
            intrinsic
                .iter()
                .cloned()
                .chain(std::iter::once(CapabilityId::new(capability))),
        ),
        dimensions: vec![length],
        units,
        metadata: FieldMetadata(BTreeMap::from([
            ("recommended_encoding".to_owned(), "line".to_owned()),
            (LINE_X_UNIT_METADATA_KEY.to_owned(), x_unit),
        ])),
    }
}
