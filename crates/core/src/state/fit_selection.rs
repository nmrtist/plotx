use super::*;
use plotx_analysis::fit_model::NonFinitePolicy;
use std::collections::{BTreeMap, BTreeSet};

pub(super) fn snapshot(
    table: &super::table_fit::FitAnalysisTable,
    bindings: &[ModelInstanceBinding],
    result: &plotx_analysis::fit_model::FitResult,
) -> Result<FitSelectionSnapshot, String> {
    let rule = match result.options.non_finite {
        NonFinitePolicy::Reject => FitSelectionRule::RejectNonFinite,
        NonFinitePolicy::ExcludeRows => FitSelectionRule::ExcludeNonFinite,
    };
    let row_ids = &table.row_ids;
    let binding_by_dataset: BTreeMap<&str, &ModelInstanceBinding> = bindings
        .iter()
        .map(|binding| (binding.dataset_id.as_str(), binding))
        .collect();
    let mut input_columns = BTreeSet::new();
    for binding in bindings {
        for source in binding.variables.values() {
            if let FitDataBinding::Column { column } = source {
                input_columns.insert(*column);
            }
        }
    }

    let mut instances = Vec::with_capacity(result.datasets.len());
    for dataset in &result.datasets {
        let binding = binding_by_dataset
            .get(dataset.id.as_str())
            .ok_or_else(|| format!("fit dataset '{}' has no source binding", dataset.id))?;
        let response = result
            .model
            .responses
            .first()
            .ok_or_else(|| "fit result has no response definition".to_owned())?;
        let response_column = match binding.responses.get(&response.id) {
            Some(FitDataBinding::Column { column }) => *column,
            _ => {
                return Err(format!(
                    "fit response '{}' has no source column binding",
                    response.id
                ));
            }
        };
        let included_indexes: BTreeSet<usize> = result
            .points
            .iter()
            .filter(|point| point.dataset_id == dataset.id)
            .map(|point| point.row)
            .collect();
        let row_count = result
            .model
            .independent_variables
            .first()
            .and_then(|variable| dataset.inputs.get(&variable.id))
            .map(Vec::len)
            .ok_or_else(|| format!("fit dataset '{}' has no input values", dataset.id))?;
        if row_count > row_ids.len() {
            return Err(format!(
                "fit dataset '{}' has more rows than the source identity map",
                dataset.id
            ));
        }
        let included_rows = included_indexes
            .iter()
            .map(|index| {
                row_ids.get(*index).copied().ok_or_else(|| {
                    format!(
                        "fit dataset '{}' references missing row {}",
                        dataset.id,
                        index + 1
                    )
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        let mut excluded_rows = Vec::new();
        for (index, row_id) in row_ids.iter().copied().take(row_count).enumerate() {
            if included_indexes.contains(&index) {
                continue;
            }
            let (reason, quantities, columns) =
                exclusion_causes(table, row_id, dataset, binding, result, index);
            excluded_rows.push(FitRowExclusion {
                row: row_id,
                reason,
                quantities,
                columns,
            });
        }
        instances.push(FitInstanceSelection {
            dataset_id: dataset.id.clone(),
            response_column,
            included_rows,
            excluded_rows,
        });
    }
    Ok(FitSelectionSnapshot {
        source_revision: table.revision_id,
        input_columns: input_columns.into_iter().collect(),
        instances,
        rule,
    })
}

fn exclusion_causes(
    table: &super::table_fit::FitAnalysisTable,
    _row_id: plotx_data::RowId,
    dataset: &plotx_analysis::fit_model::FitDataset,
    binding: &ModelInstanceBinding,
    result: &plotx_analysis::fit_model::FitResult,
    index: usize,
) -> (
    FitRowExclusionReason,
    Vec<String>,
    Vec<plotx_data::ColumnId>,
) {
    let mut quantities = Vec::new();
    let mut columns = BTreeSet::new();
    let mut has_null = false;
    let mut has_non_finite = false;
    for variable in &result.model.independent_variables {
        let value = dataset.inputs[&variable.id][index];
        if !value.is_finite() {
            quantities.push(variable.id.clone());
            if let Some(FitDataBinding::Column { column }) = binding.variables.get(&variable.id) {
                columns.insert(*column);
                has_null |= table.cell_is_null(index, *column);
                has_non_finite |= !table.cell_is_null(index, *column);
            } else {
                has_non_finite = true;
            }
        }
    }
    for constant in &result.model.constants {
        if dataset
            .constants
            .get(&constant.id)
            .is_some_and(|value| !value.is_finite())
        {
            quantities.push(constant.id.clone());
            has_non_finite = true;
        }
    }
    for response in &result.model.responses {
        let value = dataset.responses[&response.id][index];
        if !value.is_finite() {
            quantities.push(response.id.clone());
            if let Some(FitDataBinding::Column { column }) = binding.responses.get(&response.id) {
                columns.insert(*column);
                has_null |= table.cell_is_null(index, *column);
                has_non_finite |= !table.cell_is_null(index, *column);
            } else {
                has_non_finite = true;
            }
        }
    }
    let reason = match (has_null, has_non_finite) {
        (true, true) => FitRowExclusionReason::NullAndNonFiniteRequiredValues,
        (true, false) => FitRowExclusionReason::NullRequiredValue,
        _ => FitRowExclusionReason::NonFiniteRequiredValue,
    };
    (reason, quantities, columns.into_iter().collect())
}
