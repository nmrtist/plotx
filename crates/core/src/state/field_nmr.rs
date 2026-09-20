//! Stable NMR fields remain valid binding targets while another result is active.

use super::*;

pub(super) fn inactive_descriptor(
    dataset: &super::super::Dataset,
    id: FieldId,
) -> Option<FieldDescriptor> {
    let super::super::Dataset::Nmr2D(nmr) = dataset else {
        return None;
    };
    let key = nmr.field_catalog.key_for_id(id)?;
    let (name, recommended, capabilities) = match key {
        "nmr.stack" => (
            "Stack",
            "line",
            vec![CAP_FIELD_CURVE_1D, CAP_FIELD_TRACE_COLLECTION],
        ),
        "nmr.observations" if nmr.data.nus.is_some() => (
            "Acquired NUS observations",
            "line",
            vec![CAP_FIELD_CURVE_1D, CAP_FIELD_TRACE_COLLECTION],
        ),
        "nmr.real" => (
            "Real",
            "contour",
            vec![CAP_FIELD_BOUNDED, CAP_FIELD_SCALAR_GRID_2D_REGULAR],
        ),
        "nmr.magnitude" => (
            "Magnitude",
            "heatmap",
            vec![CAP_FIELD_BOUNDED, CAP_FIELD_SCALAR_GRID_2D_REGULAR],
        ),
        "nmr.dosy_map" => (
            "DOSY map",
            "contour",
            vec![CAP_FIELD_BOUNDED, CAP_FIELD_SCALAR_GRID_2D_REGULAR],
        ),
        "nmr.ilt_map" => (
            "ILT map",
            "contour",
            vec![CAP_FIELD_BOUNDED, CAP_FIELD_SCALAR_GRID_2D_REGULAR],
        ),
        _ => return None,
    };
    // These are the provider's declared rendering types, not a fabricated payload.
    // Active field discovery and payload access still require an actual result.
    Some(FieldDescriptor {
        id,
        local_id: key.to_owned(),
        name: name.to_owned(),
        scientific_observation: SummaryPart::new(format!("field:{key}"), name),
        capabilities: FieldCapabilities::new(capabilities.into_iter().map(CapabilityId::new)),
        dimensions: vec![],
        units: vec![],
        metadata: FieldMetadata(BTreeMap::from([
            ("recommended_encoding".into(), recommended.into()),
            ("availability".into(), "inactive".into()),
        ])),
    })
}
