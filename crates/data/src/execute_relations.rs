use crate::{
    AggregateFunction, AggregateMeasure, ColumnId, DataError, Diagnostic, ExecutionInput,
    JoinCardinality, JoinKey, JoinKind, MaterializedColumn, MaterializedTable, OperationId,
    Relation, Result, RevisionId, RowId, ScalarValue, TableId, TableSchema,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::atomic::AtomicBool,
};

pub(crate) fn union(
    relations: &[Relation],
    operation: OperationId,
    inputs: &BTreeMap<(TableId, RevisionId), ExecutionInput>,
    cancel: &AtomicBool,
    diagnostics: &mut Vec<Diagnostic>,
) -> Result<MaterializedTable> {
    let mut tables = relations
        .iter()
        .map(|relation| {
            crate::execute::eval_relation(relation, operation, inputs, cancel, diagnostics)
        })
        .collect::<Result<Vec<_>>>()?;
    let mut output = tables.remove(0);
    let source = output.table_id;
    output
        .row_ids
        .iter_mut()
        .for_each(|row| *row = RowId::namespaced(source, *row));
    for table in tables {
        if table.schema != output.schema {
            return Err(DataError::InvalidPlan(
                "union inputs have different schemas".into(),
            ));
        }
        output.row_ids.extend(
            table
                .row_ids
                .iter()
                .map(|row| RowId::namespaced(table.table_id, *row)),
        );
        for (target, source) in output.columns.iter_mut().zip(table.columns) {
            target.values.extend(source.values);
        }
    }
    Ok(output)
}

pub(crate) fn join(
    left: MaterializedTable,
    right: MaterializedTable,
    kind: JoinKind,
    keys: &[JoinKey],
    cardinality: JoinCardinality,
    operation: OperationId,
    cancel: &AtomicBool,
) -> Result<MaterializedTable> {
    let right_ids: BTreeSet<_> = right
        .schema
        .columns
        .iter()
        .map(|column| column.id)
        .collect();
    if left
        .schema
        .columns
        .iter()
        .any(|column| right_ids.contains(&column.id))
    {
        return Err(DataError::InvalidPlan(
            "join inputs contain colliding column ids".into(),
        ));
    }
    let left_key_columns: Vec<_> = keys.iter().map(|key| key.left).collect();
    let right_key_columns: Vec<_> = keys.iter().map(|key| key.right).collect();
    let left_keys = build_key_index(&left, &left_key_columns)?;
    let right_keys = build_key_index(&right, &right_key_columns)?;
    validate_cardinality(&left_keys, &right_keys, cardinality)?;

    let mut pairs = Vec::new();
    let mut matched_right = BTreeSet::new();
    for left_row in 0..left.row_ids.len() {
        crate::execute::check_periodic(cancel, left_row)?;
        let key = row_key(&left, left_row, &left_key_columns, false)?;
        if let Some(matches) = key.as_ref().and_then(|key| right_keys.get(key)) {
            for right_row in matches {
                pairs.push((Some(left_row), Some(*right_row)));
                matched_right.insert(*right_row);
            }
        } else if kind != JoinKind::Inner {
            pairs.push((Some(left_row), None));
        }
    }
    if kind == JoinKind::Full {
        for right_row in 0..right.row_ids.len() {
            if !matched_right.contains(&right_row) {
                pairs.push((None, Some(right_row)));
            }
        }
    }

    let mut schemas = left.schema.columns.clone();
    schemas.extend(right.schema.columns.clone());
    for schema in &mut schemas {
        schema.nullable |=
            kind == JoinKind::Full || (right_ids.contains(&schema.id) && kind == JoinKind::Left);
    }
    let schema = TableSchema::new(schemas)?;
    let mut columns = Vec::new();
    for column in left.columns.iter().chain(&right.columns) {
        let from_left = left.schema.column(column.schema.id).is_some();
        let values = pairs
            .iter()
            .map(|(left_row, right_row)| {
                if from_left {
                    left_row
                        .map(|row| column.values[row].clone())
                        .unwrap_or(ScalarValue::Null)
                } else {
                    right_row
                        .map(|row| column.values[row].clone())
                        .unwrap_or(ScalarValue::Null)
                }
            })
            .collect();
        columns.push(MaterializedColumn {
            schema: schema
                .column(column.schema.id)
                .expect("joined schema contains every source column")
                .clone(),
            values,
        });
    }
    let row_ids = pairs
        .iter()
        .enumerate()
        .map(|(index, (left_row, right_row))| {
            let ids: Vec<RowId> = left_row
                .map(|row| left.row_ids[row])
                .into_iter()
                .chain(right_row.map(|row| right.row_ids[row]))
                .collect();
            RowId::derived(operation, &ids, &(index as u64).to_le_bytes())
        })
        .collect();
    Ok(MaterializedTable {
        table_id: left.table_id,
        schema,
        row_ids,
        columns,
    })
}

pub(crate) fn aggregate(
    input: MaterializedTable,
    groups: &[ColumnId],
    measures: &[AggregateMeasure],
    operation: OperationId,
    diagnostics: &mut Vec<Diagnostic>,
    cancel: &AtomicBool,
) -> Result<MaterializedTable> {
    let mut grouped: BTreeMap<RowKey, Vec<usize>> = BTreeMap::new();
    if groups.is_empty() {
        grouped.insert(Vec::new(), (0..input.row_ids.len()).collect());
    } else {
        for row in 0..input.row_ids.len() {
            crate::execute::check_periodic(cancel, row)?;
            let key = row_key(&input, row, groups, true)?
                .expect("null-equal grouping always returns a key");
            grouped.entry(key).or_default().push(row);
        }
    }
    let mut schemas = groups
        .iter()
        .map(|id| input.column(*id).map(|column| column.schema.clone()))
        .collect::<Result<Vec<_>>>()?;
    schemas.extend(measures.iter().map(|measure| measure.output.clone()));
    let schema = TableSchema::new(schemas)?;
    let mut columns: Vec<MaterializedColumn> = schema
        .columns
        .iter()
        .cloned()
        .map(|schema| MaterializedColumn {
            schema,
            values: Vec::with_capacity(grouped.len()),
        })
        .collect();
    let mut row_ids = Vec::with_capacity(grouped.len());
    for (group_index, rows) in grouped.values().enumerate() {
        for (output, id) in columns.iter_mut().take(groups.len()).zip(groups) {
            output
                .values
                .push(input.column(*id)?.values[rows[0]].clone());
        }
        for (measure_index, measure) in measures.iter().enumerate() {
            let values = if let Some(expression) = &measure.input {
                rows.iter()
                    .map(|row| crate::execute_expr::eval_expression(expression, &input, *row))
                    .collect::<Result<Vec<_>>>()?
            } else {
                Vec::new()
            };
            let nulls = values
                .iter()
                .filter(|value| matches!(value, ScalarValue::Null))
                .count() as u64;
            let result = aggregate_value(measure.function, &values, rows.len())?;
            crate::execute::validate_scalar(&result, &measure.output)?;
            columns[groups.len() + measure_index].values.push(result);
            crate::execute::diagnostic_count(
                diagnostics,
                "aggregate.null_excluded",
                "Aggregate excluded null values",
                nulls,
            );
        }
        let ids: Vec<_> = rows.iter().map(|row| input.row_ids[*row]).collect();
        row_ids.push(RowId::derived(
            operation,
            &ids,
            &(group_index as u64).to_le_bytes(),
        ));
    }
    Ok(MaterializedTable {
        table_id: input.table_id,
        schema,
        row_ids,
        columns,
    })
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
#[doc(hidden)]
pub enum Key {
    Null,
    Bool(bool),
    Int(i64),
    Float(u64),
    Text(String),
    UInt(u32),
    Time(i64),
    Date(i32),
}

#[doc(hidden)]
pub type RowKey = Vec<Key>;

#[doc(hidden)]
pub fn row_key(
    table: &MaterializedTable,
    row: usize,
    columns: &[ColumnId],
    null_equal: bool,
) -> Result<Option<RowKey>> {
    columns
        .iter()
        .map(|id| match &table.column(*id)?.values[row] {
            ScalarValue::Null => Ok(null_equal.then_some(Key::Null)),
            ScalarValue::Boolean(value) => Ok(Some(Key::Bool(*value))),
            ScalarValue::Int64(value) => Ok(Some(Key::Int(*value))),
            ScalarValue::Float64(value) => Ok(Some(Key::Float(float_key(*value)))),
            ScalarValue::Utf8(value) => Ok(Some(Key::Text(value.clone()))),
            ScalarValue::Categorical(value) => Ok(Some(Key::UInt(*value))),
            ScalarValue::Date(value) => Ok(Some(Key::Date(*value))),
            ScalarValue::Time(value)
            | ScalarValue::Timestamp(value)
            | ScalarValue::Duration(value) => Ok(Some(Key::Time(*value))),
            ScalarValue::Extension { .. } => Err(DataError::Unsupported(
                "extension join keys require a registered comparison".into(),
            )),
        })
        .collect::<Result<Option<Vec<_>>>>()
}

fn build_key_index(
    table: &MaterializedTable,
    columns: &[ColumnId],
) -> Result<BTreeMap<RowKey, Vec<usize>>> {
    let mut index = BTreeMap::new();
    for row in 0..table.row_ids.len() {
        if let Some(key) = row_key(table, row, columns, false)? {
            index.entry(key).or_insert_with(Vec::new).push(row);
        }
    }
    Ok(index)
}

fn validate_cardinality(
    left: &BTreeMap<RowKey, Vec<usize>>,
    right: &BTreeMap<RowKey, Vec<usize>>,
    cardinality: JoinCardinality,
) -> Result<()> {
    let left_unique = matches!(
        cardinality,
        JoinCardinality::OneToOne | JoinCardinality::OneToMany
    );
    let right_unique = matches!(
        cardinality,
        JoinCardinality::OneToOne | JoinCardinality::ManyToOne
    );
    if left_unique && left.values().any(|rows| rows.len() > 1) {
        return Err(DataError::InvalidPlan(
            "join violates left-key uniqueness".into(),
        ));
    }
    if right_unique && right.values().any(|rows| rows.len() > 1) {
        return Err(DataError::InvalidPlan(
            "join violates right-key uniqueness".into(),
        ));
    }
    Ok(())
}

fn aggregate_value(
    function: AggregateFunction,
    values: &[ScalarValue],
    row_count: usize,
) -> Result<ScalarValue> {
    if function == AggregateFunction::CountAll {
        return Ok(ScalarValue::Int64(row_count as i64));
    }
    let numbers: Vec<f64> = values
        .iter()
        .filter_map(|value| match value {
            ScalarValue::Float64(value) => Some(*value),
            ScalarValue::Null => None,
            _ => None,
        })
        .collect();
    if function == AggregateFunction::Count {
        return Ok(ScalarValue::Int64(numbers.len() as i64));
    }
    if values
        .iter()
        .any(|value| !matches!(value, ScalarValue::Float64(_) | ScalarValue::Null))
    {
        return Err(DataError::InvalidPlan(
            "numeric aggregate requires Float64".into(),
        ));
    }
    if numbers.is_empty() {
        return Ok(ScalarValue::Null);
    }
    let value = match function {
        AggregateFunction::SumV1 => numbers.iter().sum(),
        AggregateFunction::MeanV1 => numbers.iter().sum::<f64>() / numbers.len() as f64,
        AggregateFunction::MinimumV1 => numbers
            .iter()
            .copied()
            .min_by(|left, right| crate::execute_expr::float_order(*left, *right))
            .expect("non-empty aggregate"),
        AggregateFunction::MaximumV1 => numbers
            .iter()
            .copied()
            .max_by(|left, right| crate::execute_expr::float_order(*left, *right))
            .expect("non-empty aggregate"),
        _ => unreachable!(),
    };
    Ok(ScalarValue::Float64(value))
}

fn float_key(value: f64) -> u64 {
    if value.is_nan() {
        f64::NAN.to_bits()
    } else if value == 0.0 {
        0
    } else {
        value.to_bits()
    }
}
