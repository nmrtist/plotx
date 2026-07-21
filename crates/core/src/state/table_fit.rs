use super::{TableDataset, TableMeta};
use plotx_data::{ColumnId, RowId, ScalarValue};

pub(super) struct FitAnalysisTable {
    pub revision_id: plotx_data::RevisionId,
    pub row_ids: Vec<RowId>,
    pub x: FitAnalysisColumn,
    pub series: Vec<FitAnalysisSeries>,
    pub meta: TableMeta,
}

pub(super) fn stejskal_tanner_binding_profile(
    table: &FitAnalysisTable,
) -> Result<std::collections::BTreeMap<&'static str, (f64, &'static str)>, String> {
    let meta = table.meta.diffusion.ok_or_else(|| {
        "Stejskal–Tanner needs diffusion parameters from the source DOSY data.".to_owned()
    })?;
    let effective_delay = meta.big_delta - meta.shape_factor * meta.delta - 0.5 * meta.tau;
    if !meta.gamma.is_finite()
        || !meta.delta.is_finite()
        || !meta.big_delta.is_finite()
        || !meta.tau.is_finite()
        || !meta.shape_factor.is_finite()
        || meta.gamma == 0.0
        || meta.delta <= 0.0
        || effective_delay <= 0.0
    {
        return Err(
            "Diffusion parameters must be finite and define a positive encoding duration.".into(),
        );
    }
    Ok(std::collections::BTreeMap::from([
        ("gamma", (meta.gamma, "diffusion.gamma")),
        ("delta", (meta.delta, "diffusion.delta")),
        ("big_delta", (meta.big_delta, "diffusion.big_delta")),
        ("tau", (meta.tau, "diffusion.tau")),
        (
            "shape_factor",
            (meta.shape_factor, "diffusion.shape_factor"),
        ),
    ]))
}

pub(super) struct FitAnalysisColumn {
    pub id: ColumnId,
    pub values: Vec<Option<f64>>,
}

pub(super) struct FitAnalysisSeries {
    pub value: FitAnalysisColumn,
    pub uncertainty: Option<FitAnalysisColumn>,
}

impl FitAnalysisTable {
    pub fn cell_is_null(&self, row: usize, column: ColumnId) -> bool {
        if self.x.id == column {
            return self.x.values.get(row).is_none_or(Option::is_none);
        }
        self.series.iter().any(|series| {
            (series.value.id == column && series.value.values.get(row).is_none_or(Option::is_none))
                || series.uncertainty.as_ref().is_some_and(|uncertainty| {
                    uncertainty.id == column
                        && uncertainty.values.get(row).is_none_or(Option::is_none)
                })
        })
    }
}

impl TableDataset {
    pub(super) fn fit_analysis_view(&self) -> Result<FitAnalysisTable, String> {
        let snapshot = &self.typed_state.envelope.revision.snapshot;
        let x = self
            .x_binding
            .ok_or_else(|| "Curve fitting needs an explicit x-column binding.".to_owned())?;
        let mut projection = vec![x];
        for binding in &self.series_bindings {
            projection.push(binding.value_column);
            if let Some(uncertainty) = binding.uncertainty_column {
                projection.push(uncertainty);
            }
        }
        let row_count = usize::try_from(snapshot.row_count)
            .map_err(|_| "The table is too large for this in-memory fitting backend.".to_owned())?;
        let rows = self.typed_rows(row_count, &projection)?;
        let mut columns = rows.columns.into_iter();
        let x = columns
            .next()
            .ok_or_else(|| "The fitted table omitted its x column.".to_owned())?;
        let x = numeric_column(x.schema.id, x.schema.name, x.values)?;
        let mut series = Vec::with_capacity(self.series_bindings.len());
        for binding in &self.series_bindings {
            let value = columns
                .next()
                .ok_or_else(|| "The fitted table omitted a response column.".to_owned())?;
            let value = numeric_column(value.schema.id, value.schema.name, value.values)?;
            let uncertainty = if binding.uncertainty_column.is_some() {
                let column = columns
                    .next()
                    .ok_or_else(|| "The fitted table omitted an uncertainty column.".to_owned())?;
                Some(numeric_column(
                    column.schema.id,
                    column.schema.name,
                    column.values,
                )?)
            } else {
                None
            };
            series.push(FitAnalysisSeries { value, uncertainty });
        }
        Ok(FitAnalysisTable {
            revision_id: self.typed_state.envelope.revision.id,
            row_ids: rows.row_ids,
            x,
            series,
            meta: self.meta,
        })
    }
}

fn numeric_column(
    id: ColumnId,
    name: String,
    values: Vec<ScalarValue>,
) -> Result<FitAnalysisColumn, String> {
    let values = values
        .into_iter()
        .map(|value| match value {
            ScalarValue::Null => Ok(None),
            ScalarValue::Int64(value) => Ok(Some(value as f64)),
            ScalarValue::Float64(value) => Ok(Some(value)),
            value => Err(format!(
                "Fit column {name:?} is not numeric ({:?}).",
                value.logical_type()
            )),
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(FitAnalysisColumn { id, values })
}

pub(super) fn backend_values(values: &[Option<f64>]) -> Vec<f64> {
    values
        .iter()
        .map(|value| value.unwrap_or(f64::NAN))
        .collect()
}
