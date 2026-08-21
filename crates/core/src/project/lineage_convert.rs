use super::*;
use crate::state::{DatasetLineage, DerivationKind};

pub(super) fn derivation_kind_to_str(kind: DerivationKind) -> &'static str {
    match kind {
        DerivationKind::Slice => "slice",
        DerivationKind::Projection => "projection",
        DerivationKind::SpectrumArithmetic => "spectrum_arithmetic",
        DerivationKind::LiveRegionTable => "live_region_table",
        DerivationKind::FrozenRegionTable => "frozen_region_table",
        DerivationKind::LineFitTable => "line_fit_table",
        DerivationKind::MultipletTable => "multiplet_table",
        DerivationKind::CraftComponentTable => "craft_component_table",
        DerivationKind::WindowStatisticsTable => "window_statistics_table",
        DerivationKind::IvTable => "iv_table",
        DerivationKind::StatisticsTable => "statistics_table",
        DerivationKind::RelationalTransform => "relational_transform",
    }
}

fn derivation_kind_from_str(value: &str) -> Result<DerivationKind> {
    match value {
        "slice" => Ok(DerivationKind::Slice),
        "projection" => Ok(DerivationKind::Projection),
        "spectrum_arithmetic" => Ok(DerivationKind::SpectrumArithmetic),
        "live_region_table" => Ok(DerivationKind::LiveRegionTable),
        "frozen_region_table" => Ok(DerivationKind::FrozenRegionTable),
        "line_fit_table" => Ok(DerivationKind::LineFitTable),
        "multiplet_table" => Ok(DerivationKind::MultipletTable),
        "craft_component_table" => Ok(DerivationKind::CraftComponentTable),
        "window_statistics_table" => Ok(DerivationKind::WindowStatisticsTable),
        "iv_table" => Ok(DerivationKind::IvTable),
        "statistics_table" => Ok(DerivationKind::StatisticsTable),
        "relational_transform" => Ok(DerivationKind::RelationalTransform),
        other => Err(ProjectError::Invalid(format!(
            "unknown dataset derivation kind {other}"
        ))),
    }
}

pub(super) fn resolve_dataset_lineage(
    datasets: &mut [Dataset],
    bindings: &[DatasetBinding],
    data_to_dataset: &HashMap<String, usize>,
) -> Result<()> {
    for (di, binding) in bindings.iter().enumerate() {
        if let Some(dto) = &binding.derivation {
            if dto.sources.is_empty() {
                return Err(ProjectError::Invalid(format!(
                    "dataset {} has a derivation with no sources",
                    binding.data
                )));
            }
            let mut sources = Vec::with_capacity(dto.sources.len());
            for source_id in &dto.sources {
                let source_index = data_to_dataset.get(source_id).copied().ok_or_else(|| {
                    ProjectError::Invalid(format!(
                        "dataset {} references missing lineage source {source_id}",
                        binding.data
                    ))
                })?;
                if source_index == di {
                    return Err(ProjectError::Invalid(format!(
                        "dataset {} cannot derive from itself",
                        binding.data
                    )));
                }
                let source = datasets[source_index].resource_id();
                if !sources.contains(&source) {
                    sources.push(source);
                }
            }
            datasets[di].set_lineage(Some(DatasetLineage::new(
                derivation_kind_from_str(&dto.kind)?,
                sources,
            )));
        }
    }

    validate_lineage_acyclic(datasets, bindings)
}

fn validate_lineage_acyclic(datasets: &[Dataset], bindings: &[DatasetBinding]) -> Result<()> {
    fn visit(
        di: usize,
        datasets: &[Dataset],
        state: &mut [u8],
        bindings: &[DatasetBinding],
    ) -> Result<()> {
        if state[di] == 1 {
            return Err(ProjectError::Invalid(format!(
                "dataset lineage contains a cycle at {}",
                bindings[di].data
            )));
        }
        if state[di] == 2 {
            return Ok(());
        }
        state[di] = 1;
        if let Some(lineage) = datasets[di].lineage() {
            for &source in &lineage.sources {
                let source_index = datasets
                    .iter()
                    .position(|dataset| dataset.resource_id() == source)
                    .ok_or_else(|| {
                        ProjectError::Invalid(format!(
                            "dataset {} references missing lineage source {source}",
                            bindings[di].data
                        ))
                    })?;
                visit(source_index, datasets, state, bindings)?;
            }
        }
        state[di] = 2;
        Ok(())
    }

    let mut state = vec![0; datasets.len()];
    for di in 0..datasets.len() {
        visit(di, datasets, &mut state, bindings)?;
    }
    Ok(())
}
