use super::*;

pub(super) fn descriptor(
    dataset: &crate::state::MassSpecDataset,
) -> (Vec<usize>, Vec<String>, Vec<ResourceRef>) {
    (
        vec![
            dataset.run.functions.len(),
            dataset
                .run
                .functions
                .iter()
                .map(|function| function.scans.len())
                .sum(),
            dataset.run.chromatograms.len(),
        ],
        vec!["min".to_owned(), "m/z".to_owned()],
        Vec::new(),
    )
}

pub(super) fn preview(
    dataset: &crate::state::MassSpecDataset,
    target: &ResourceRef,
    limit: usize,
    statistics: &mut BTreeMap<String, f64>,
) -> (Vec<usize>, serde_json::Value, usize) {
    if target.kind.0 == KIND_FIELD
        && let Some(local_id) = target.local_id.as_deref()
        && let Some(field) = dataset.field_catalog.id_for_key(local_id)
        && let Some((_, _, _, points, _)) = dataset.field_values(field)
    {
        let values = points.iter().map(|point| point[1]).collect::<Vec<_>>();
        add_statistics(statistics, &values);
        return (
            vec![values.len()],
            serde_json::json!(finite_slice(&values, limit)),
            values.len(),
        );
    }
    let scans = dataset
        .run
        .functions
        .iter()
        .map(|function| function.scans.len())
        .sum();
    (
        vec![
            dataset.run.functions.len(),
            scans,
            dataset.run.chromatograms.len(),
        ],
        serde_json::json!({"summary": format!(
            "{} MS functions · {scans} scans · {} detector channels",
            dataset.supported_ms_functions().count(), dataset.run.chromatograms.len()
        )}),
        scans,
    )
}
