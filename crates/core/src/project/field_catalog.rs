use super::*;

pub(super) fn read(data: &DataObject) -> Result<crate::state::FieldCatalog> {
    let value = data
        .extensions
        .get("plotx.fields")
        .cloned()
        .ok_or_else(|| {
            ProjectError::Invalid(format!(
                "dataset {} is missing its mandatory plotx.fields identity catalog",
                data.id
            ))
        })?;
    serde_json::from_value(value).map_err(|error| {
        ProjectError::Invalid(format!(
            "dataset {} has an invalid plotx.fields identity catalog: {error}",
            data.id
        ))
    })
}

pub(super) fn validate_series(
    dataset: &Dataset,
    field: crate::state::FieldId,
    encoding: &plotx_figure::SeriesEncoding,
    context: &str,
) -> Result<()> {
    if !dataset.has_field(field) {
        return Err(ProjectError::Invalid(format!(
            "{context} references missing field {field}"
        )));
    }
    if !dataset.supports_encoding(field, encoding) {
        return Err(ProjectError::Invalid(format!(
            "{context} uses an encoding not supported by field {field}"
        )));
    }
    Ok(())
}

pub(super) fn validate_series_source(
    dataset: &Dataset,
    field: crate::state::FieldId,
    item: Option<plotx_data::TraceItemId>,
    encoding: &plotx_figure::SeriesEncoding,
    context: &str,
) -> Result<()> {
    validate_series(dataset, field, encoding, context)?;
    match (item, dataset.trace_collection(field)) {
        (Some(item), Some(collection)) if collection.item(item).is_some() => Ok(()),
        (Some(item), _) => Err(ProjectError::Invalid(format!(
            "{context} references unknown trace item {item}"
        ))),
        (None, Some(_)) => Err(ProjectError::Invalid(format!(
            "{context} addresses a trace collection as a scalar field"
        ))),
        (None, None) => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stale_field_or_inapplicable_encoding_is_rejected_before_serialization() {
        let source = plotx_io::NmrData {
            points: vec![num_complex::Complex64::new(1.0, 0.0); 4],
            domain: plotx_io::Domain::Frequency,
            spectral_width_hz: 4_000.0,
            observe_freq_mhz: 400.0,
            carrier_ppm: 0.0,
            nucleus: "1H".to_owned(),
            source: "field validation".to_owned(),
            group_delay: 0.0,
        };
        let dataset = Dataset::Nmr(Box::new(crate::state::NmrDataset::load(source)));
        let field = dataset.default_field_id().unwrap();
        let error = validate_series(
            &dataset,
            field,
            &plotx_figure::SeriesEncoding::Heatmap(plotx_figure::HeatmapSpec::default()),
            "test series",
        )
        .unwrap_err();
        assert!(error.to_string().contains("encoding not supported"));

        let error = validate_series(
            &dataset,
            crate::state::FieldId::new(field.get() + 1),
            &plotx_figure::SeriesEncoding::default(),
            "test series",
        )
        .unwrap_err();
        assert!(error.to_string().contains("missing field"));
    }
}
