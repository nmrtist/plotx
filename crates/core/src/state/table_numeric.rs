use super::TableDataset;
use plotx_data::{CodecRegistry, ColumnId, LogicalType, RevisionId, RowId, ScalarValue};
use std::collections::BTreeSet;

/// Contiguous numerical selection built on demand for algorithms that require
/// slices. Identity and null state remain explicit; unrelated typed columns
/// are never decoded.
pub(super) struct NumericAnalysisTable {
    pub revision_id: RevisionId,
    pub row_ids: Vec<RowId>,
    pub columns: Vec<NumericAnalysisColumn>,
}

pub(super) struct NumericAnalysisColumn {
    pub id: ColumnId,
    pub name: String,
    /// `None` is Null. Valid NaN and infinities remain `Some` values.
    pub values: Vec<Option<f64>>,
}

impl NumericAnalysisTable {
    pub fn resolve_draft(
        &self,
        draft: &super::StatDraft,
    ) -> Result<super::ResolvedStatDraft, String> {
        let resolve = |id: ColumnId| {
            self.columns
                .iter()
                .position(|column| column.id == id)
                .ok_or_else(|| format!("Selected table column {id} is no longer available."))
        };
        Ok(super::ResolvedStatDraft {
            question: draft.question,
            columns: draft
                .columns
                .iter()
                .copied()
                .map(resolve)
                .collect::<Result<_, _>>()?,
            column_a: resolve(draft.column_a)?,
            column_b: resolve(draft.column_b)?,
            group_columns: draft
                .group_columns
                .iter()
                .copied()
                .map(resolve)
                .collect::<Result<_, _>>()?,
            value_column: resolve(draft.value_column)?,
            factor_a_column: resolve(draft.factor_a_column)?,
            factor_b_column: resolve(draft.factor_b_column)?,
            variance: draft.variance,
            direction: draft.direction,
            correlation: draft.correlation,
            reference_value: draft.reference_value,
            confidence: draft.confidence,
            run_tukey: draft.run_tukey,
        })
    }
}

impl TableDataset {
    pub fn numeric_analysis_columns(&self) -> Vec<(ColumnId, String)> {
        numeric_column_ids(self)
            .into_iter()
            .filter_map(|id| {
                self.typed_state
                    .envelope
                    .revision
                    .snapshot
                    .schema
                    .column(id)
                    .map(|schema| (id, schema.name.clone()))
            })
            .collect()
    }

    pub(super) fn numeric_analysis_view(&self) -> Result<NumericAnalysisTable, String> {
        let snapshot = &self.typed_state.envelope.revision.snapshot;
        let projection = numeric_column_ids(self);
        let mut table = NumericAnalysisTable {
            revision_id: self.typed_state.envelope.revision.id,
            row_ids: Vec::with_capacity(usize::try_from(snapshot.row_count).unwrap_or(usize::MAX)),
            columns: projection
                .iter()
                .map(|id| NumericAnalysisColumn {
                    id: *id,
                    name: snapshot
                        .schema
                        .column(*id)
                        .map(|column| column.name.clone())
                        .unwrap_or_default(),
                    values: Vec::with_capacity(
                        usize::try_from(snapshot.row_count).unwrap_or(usize::MAX),
                    ),
                })
                .collect(),
        };
        let codecs = CodecRegistry::with_arrow_ipc();
        let reader =
            plotx_data::SnapshotReader::new(snapshot, self.typed_state.store.as_ref(), &codecs)
                .map_err(|error| error.to_string())?;
        for batch_index in 0..snapshot.batch_count() {
            let batch = reader
                .read_batch(batch_index, &projection)
                .map_err(|error| error.to_string())?;
            table.row_ids.extend(batch.row_ids);
            for (target, (_, chunk)) in table.columns.iter_mut().zip(batch.columns) {
                for row in 0..chunk.len() {
                    target.values.push(match chunk.value(row) {
                        Some(ScalarValue::Null) | None => None,
                        Some(ScalarValue::Int64(value)) => Some(value as f64),
                        Some(ScalarValue::Float64(value)) => Some(value),
                        Some(value) => {
                            return Err(format!(
                                "Column {:?} changed to non-numeric type {:?}.",
                                target.name,
                                value.logical_type()
                            ));
                        }
                    });
                }
            }
        }
        Ok(table)
    }
}

fn numeric_column_ids(table: &TableDataset) -> Vec<ColumnId> {
    let snapshot = &table.typed_state.envelope.revision.snapshot;
    let uncertainty = snapshot
        .uncertainty
        .iter()
        .flat_map(|relation| match relation.kind {
            plotx_data::UncertaintyKind::Symmetric { column, .. } => vec![column],
            plotx_data::UncertaintyKind::Asymmetric { lower, upper, .. }
            | plotx_data::UncertaintyKind::ConfidenceInterval { lower, upper, .. } => {
                vec![lower, upper]
            }
        })
        .collect::<BTreeSet<_>>();
    let candidates = if table.series_bindings.is_empty() {
        snapshot
            .schema
            .columns
            .iter()
            .map(|column| column.id)
            .collect::<Vec<_>>()
    } else {
        table
            .series_bindings
            .iter()
            .map(|binding| binding.value_column)
            .collect()
    };
    let mut seen = BTreeSet::new();
    candidates
        .into_iter()
        .filter(|id| seen.insert(*id) && !uncertainty.contains(id))
        .filter(|id| {
            snapshot.schema.column(*id).is_some_and(|column| {
                matches!(
                    column.logical_type,
                    LogicalType::Int64 | LogicalType::Float64
                )
            })
        })
        .collect()
}
