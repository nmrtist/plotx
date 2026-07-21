use crate::{
    ColumnId, DataError, Diagnostic, ExecutionInput, Expression, JoinCardinality,
    MaterializedColumn, MaterializedTable, OperationId, Relation, Result, RevisionId, ScalarValue,
    TableId, TableSchema,
};
use std::{
    collections::BTreeMap,
    sync::atomic::{AtomicBool, Ordering},
};

struct AggregateIdentity {
    group_values: Vec<ScalarValue>,
    deriver: crate::id::RowIdDeriver,
}

struct PivotIdentity {
    group_values: Vec<ScalarValue>,
    rows_by_name: BTreeMap<String, Vec<crate::RowId>>,
}

pub(super) fn supports_large(relation: &Relation) -> bool {
    if super::runtime::preserves_row_identity(relation) {
        return true;
    }
    matches!(relation, Relation::Aggregate { input, .. } if matches!(input.as_ref(), Relation::SnapshotRead(_)))
        || matches!(relation, Relation::Join { left, right, .. }
            if matches!(left.as_ref(), Relation::SnapshotRead(_))
                && matches!(right.as_ref(), Relation::SnapshotRead(_)))
        || matches!(relation, Relation::Unpivot { input, .. }
            if matches!(input.as_ref(), Relation::SnapshotRead(_)))
        || matches!(relation, Relation::Pivot { input, .. }
            if matches!(input.as_ref(), Relation::SnapshotRead(_)))
}

pub(super) fn preflight_large(
    relation: &Relation,
    inputs: &BTreeMap<(TableId, RevisionId), ExecutionInput>,
    cancel: &AtomicBool,
) -> Result<()> {
    let Relation::Join {
        left,
        right,
        keys,
        cardinality,
        ..
    } = relation
    else {
        return Ok(());
    };
    let Relation::SnapshotRead(left_read) = left.as_ref() else {
        return Ok(());
    };
    let Relation::SnapshotRead(right_read) = right.as_ref() else {
        return Ok(());
    };
    let left = inputs
        .get(&(left_read.table, left_read.revision))
        .ok_or_else(|| DataError::InvalidPlan("left join snapshot is unavailable".into()))?;
    let right = inputs
        .get(&(right_read.table, right_read.revision))
        .ok_or_else(|| DataError::InvalidPlan("right join snapshot is unavailable".into()))?;
    let left_columns = keys.iter().map(|key| key.left).collect::<Vec<_>>();
    let right_columns = keys.iter().map(|key| key.right).collect::<Vec<_>>();
    let left_counts = key_counts(left, &left_columns, cancel)?;
    let right_counts = key_counts(right, &right_columns, cancel)?;
    let left_unique = matches!(
        cardinality,
        JoinCardinality::OneToOne | JoinCardinality::OneToMany
    );
    let right_unique = matches!(
        cardinality,
        JoinCardinality::OneToOne | JoinCardinality::ManyToOne
    );
    if left_unique && left_counts.values().any(|count| *count > 1) {
        return Err(DataError::InvalidPlan(
            "join violates left-key uniqueness".into(),
        ));
    }
    if right_unique && right_counts.values().any(|count| *count > 1) {
        return Err(DataError::InvalidPlan(
            "join violates right-key uniqueness".into(),
        ));
    }
    Ok(())
}

fn key_counts(
    input: &ExecutionInput,
    columns: &[ColumnId],
    cancel: &AtomicBool,
) -> Result<BTreeMap<crate::execute_relations::RowKey, usize>> {
    let mut counts = BTreeMap::new();
    input.visit_materialized_batches(|table| {
        for row in 0..table.row_ids.len() {
            if row % 1_024 == 0 && cancel.load(Ordering::Relaxed) {
                return Err(DataError::Cancelled);
            }
            if let Some(key) = crate::execute_relations::row_key(table, row, columns, false)? {
                *counts.entry(key).or_insert(0) += 1;
            }
        }
        Ok(())
    })?;
    Ok(counts)
}

pub(super) fn expected_output(
    relation: &Relation,
    inputs: &BTreeMap<(TableId, RevisionId), ExecutionInput>,
    schema: TableSchema,
    operation: OperationId,
    cancel: &AtomicBool,
) -> Result<MaterializedTable> {
    if let Relation::Pivot {
        input,
        groups,
        names_from,
        values_from,
        aggregate,
    } = relation
    {
        let Relation::SnapshotRead(read) = input.as_ref() else {
            return Err(DataError::Unsupported(
                "large pivot identity currently requires a pinned snapshot input".into(),
            ));
        };
        let input = inputs
            .get(&(read.table, read.revision))
            .ok_or_else(|| DataError::InvalidPlan("snapshot input is unavailable".into()))?;
        return pivot_expected(
            input,
            groups,
            *names_from,
            *values_from,
            *aggregate,
            operation,
            cancel,
        );
    }
    let Relation::Aggregate {
        input,
        groups,
        measures: _,
    } = relation
    else {
        return Ok(MaterializedTable {
            table_id: super::runtime::first_source_table(relation)?,
            schema,
            row_ids: Vec::new(),
            columns: Vec::new(),
        });
    };
    let Relation::SnapshotRead(read) = input.as_ref() else {
        return Err(DataError::Unsupported(
            "large aggregate identity currently requires a pinned snapshot input".into(),
        ));
    };
    let input = inputs
        .get(&(read.table, read.revision))
        .ok_or_else(|| DataError::InvalidPlan("snapshot input is unavailable".into()))?;
    let mut grouped: BTreeMap<crate::execute_relations::RowKey, AggregateIdentity> =
        BTreeMap::new();
    if groups.is_empty() && input.row_count() == 0 {
        grouped.insert(
            Vec::new(),
            AggregateIdentity {
                group_values: Vec::new(),
                deriver: crate::id::RowIdDeriver::new(operation),
            },
        );
    }
    input.visit_materialized_batches(|table| {
        for row in 0..table.row_ids.len() {
            crate::execute::check_periodic(cancel, row)?;
            let key = if groups.is_empty() {
                Vec::new()
            } else {
                crate::execute_relations::row_key(table, row, groups, true)?
                    .expect("null-equal grouping always returns a key")
            };
            let group = grouped.entry(key).or_insert_with(|| AggregateIdentity {
                group_values: groups
                    .iter()
                    .map(|id| {
                        table
                            .column(*id)
                            .expect("type checking resolved aggregate group")
                            .values[row]
                            .clone()
                    })
                    .collect(),
                deriver: crate::id::RowIdDeriver::new(operation),
            });
            group.deriver.push(table.row_ids[row]);
        }
        Ok(())
    })?;
    let mut columns = groups
        .iter()
        .map(|id| {
            let index = groups
                .iter()
                .position(|candidate| candidate == id)
                .expect("group column comes from the group list");
            let source = input
                .schema()
                .column(*id)
                .ok_or(DataError::MissingColumn(*id))?;
            Ok(MaterializedColumn {
                schema: source.clone(),
                values: grouped
                    .values()
                    .map(|group| group.group_values[index].clone())
                    .collect(),
            })
        })
        .collect::<Result<Vec<_>>>()?;
    // Alignment needs only the group prefix; result columns are decoded from
    // DataFusion after the frozen group order is established.
    columns.truncate(groups.len());
    let row_ids = grouped
        .into_values()
        .enumerate()
        .map(|(group_index, group)| group.deriver.finish(&(group_index as u64).to_le_bytes()))
        .collect();
    Ok(MaterializedTable {
        table_id: input.table_id(),
        schema,
        row_ids,
        columns,
    })
}

#[allow(clippy::too_many_arguments)]
fn pivot_expected(
    input: &ExecutionInput,
    groups: &[ColumnId],
    names_from: ColumnId,
    values_from: ColumnId,
    aggregate: crate::AggregateFunction,
    operation: OperationId,
    cancel: &AtomicBool,
) -> Result<MaterializedTable> {
    let name_schema = input
        .schema()
        .column(names_from)
        .ok_or(DataError::MissingColumn(names_from))?;
    let value_schema = input
        .schema()
        .column(values_from)
        .ok_or(DataError::MissingColumn(values_from))?;
    let names = pivot_names(input, names_from)?;
    let mut grouped: BTreeMap<crate::execute_relations::RowKey, PivotIdentity> = BTreeMap::new();
    input.visit_materialized_batches(|table| {
        let name_column = table.column(names_from)?;
        for row in 0..table.row_ids.len() {
            crate::execute::check_periodic(cancel, row)?;
            let Some(name) = pivot_name(&name_column.values[row], &name_schema.logical_type)?
            else {
                continue;
            };
            let key = crate::execute_relations::row_key(table, row, groups, true)?
                .expect("null-equal grouping always returns a key");
            grouped
                .entry(key)
                .or_insert_with(|| PivotIdentity {
                    group_values: groups
                        .iter()
                        .map(|id| {
                            table
                                .column(*id)
                                .expect("type checking resolved pivot group")
                                .values[row]
                                .clone()
                        })
                        .collect(),
                    rows_by_name: BTreeMap::new(),
                })
                .rows_by_name
                .entry(name)
                .or_default()
                .push(table.row_ids[row]);
        }
        Ok(())
    })?;
    let mut schemas = groups
        .iter()
        .map(|id| {
            input
                .schema()
                .column(*id)
                .cloned()
                .ok_or(DataError::MissingColumn(*id))
        })
        .collect::<Result<Vec<_>>>()?;
    for name in &names {
        let logical_type = if matches!(
            aggregate,
            crate::AggregateFunction::CountAll | crate::AggregateFunction::Count
        ) {
            crate::LogicalType::Int64
        } else {
            crate::LogicalType::Float64
        };
        let mut column = crate::ColumnSchema::new(name, logical_type);
        column.id = crate::ColumnId::derived(operation, name.as_bytes());
        if !matches!(
            aggregate,
            crate::AggregateFunction::CountAll | crate::AggregateFunction::Count
        ) {
            column.unit = value_schema.unit.clone();
        }
        schemas.push(column);
    }
    let schema = TableSchema::new(schemas)?;
    let columns = groups
        .iter()
        .map(|id| {
            let index = groups
                .iter()
                .position(|candidate| candidate == id)
                .expect("group column comes from the group list");
            let source = input
                .schema()
                .column(*id)
                .ok_or(DataError::MissingColumn(*id))?;
            Ok(MaterializedColumn {
                schema: source.clone(),
                values: grouped
                    .values()
                    .map(|group| group.group_values[index].clone())
                    .collect(),
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let row_ids = grouped
        .into_values()
        .enumerate()
        .map(|(group_index, group)| {
            let sources = group
                .rows_by_name
                .into_values()
                .flatten()
                .collect::<Vec<_>>();
            crate::RowId::derived(operation, &sources, &(group_index as u64).to_le_bytes())
        })
        .collect();
    Ok(MaterializedTable {
        table_id: input.table_id(),
        schema,
        row_ids,
        columns,
    })
}

fn pivot_name(value: &ScalarValue, logical_type: &crate::LogicalType) -> Result<Option<String>> {
    match value {
        ScalarValue::Null => Ok(None),
        ScalarValue::Utf8(value) => Ok(Some(value.clone())),
        ScalarValue::Categorical(index) => {
            let crate::LogicalType::Categorical { levels } = logical_type else {
                return Err(DataError::InvalidArray(
                    "categorical pivot value has non-categorical schema".into(),
                ));
            };
            levels
                .get(*index as usize)
                .map(|level| Some(level.value.clone()))
                .ok_or_else(|| DataError::InvalidArray("categorical level is out of range".into()))
        }
        _ => Err(DataError::InvalidPlan(
            "pivot name value is not text/categorical".into(),
        )),
    }
}

pub(super) fn pivot_names(input: &ExecutionInput, names_from: ColumnId) -> Result<Vec<String>> {
    let logical_type = &input
        .schema()
        .column(names_from)
        .ok_or(DataError::MissingColumn(names_from))?
        .logical_type;
    let mut names = std::collections::BTreeSet::new();
    input.visit_materialized_batches(|table| {
        let column = table.column(names_from)?;
        for value in &column.values {
            if let Some(name) = pivot_name(value, logical_type)? {
                names.insert(name);
            }
        }
        Ok(())
    })?;
    Ok(names.into_iter().collect())
}

pub(super) fn aggregate_diagnostics(
    relation: &Relation,
    inputs: &BTreeMap<(TableId, RevisionId), ExecutionInput>,
    cancel: &AtomicBool,
) -> Result<Vec<Diagnostic>> {
    if let Relation::Pivot {
        input,
        names_from,
        values_from,
        ..
    } = relation
    {
        let Relation::SnapshotRead(read) = input.as_ref() else {
            return Ok(Vec::new());
        };
        let input = inputs
            .get(&(read.table, read.revision))
            .ok_or_else(|| DataError::InvalidPlan("snapshot input is unavailable".into()))?;
        let mut nulls = 0;
        input.visit_materialized_batches(|table| {
            let names = table.column(*names_from)?;
            let values = table.column(*values_from)?;
            for row in 0..table.row_ids.len() {
                crate::execute::check_periodic(cancel, row)?;
                if !matches!(names.values[row], ScalarValue::Null)
                    && matches!(values.values[row], ScalarValue::Null)
                {
                    nulls += 1;
                }
            }
            Ok(())
        })?;
        let mut diagnostics = Vec::new();
        crate::execute::diagnostic_count(
            &mut diagnostics,
            "pivot.null_excluded",
            "Pivot aggregate excluded null values",
            nulls,
        );
        return Ok(diagnostics);
    }
    let Relation::Aggregate {
        input, measures, ..
    } = relation
    else {
        return Ok(Vec::new());
    };
    let Relation::SnapshotRead(read) = input.as_ref() else {
        return Ok(Vec::new());
    };
    let input = inputs
        .get(&(read.table, read.revision))
        .ok_or_else(|| DataError::InvalidPlan("snapshot input is unavailable".into()))?;
    let mut diagnostics = Vec::new();
    for measure in measures {
        let Some(expression) = &measure.input else {
            continue;
        };
        let nulls = count_nulls(expression, input, cancel)?;
        crate::execute::diagnostic_count(
            &mut diagnostics,
            "aggregate.null_excluded",
            "Aggregate excluded null values",
            nulls,
        );
    }
    Ok(diagnostics)
}

fn count_nulls(
    expression: &Expression,
    input: &ExecutionInput,
    cancel: &AtomicBool,
) -> Result<u64> {
    let mut nulls = 0;
    input.visit_materialized_batches(|table| {
        for row in 0..table.row_ids.len() {
            crate::execute::check_periodic(cancel, row)?;
            if matches!(
                crate::execute_expr::eval_expression(expression, table, row)?,
                ScalarValue::Null
            ) {
                nulls += 1;
            }
        }
        Ok(())
    })?;
    Ok(nulls)
}
