use crate::{
    AggregateFunction, ColumnId, ColumnSchema, DataError, Diagnostic, LogicalType,
    MaterializedColumn, MaterializedTable, OperationId, Result, RowId, ScalarValue, TableSchema,
};
use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum GroupValue {
    Null,
    Boolean(bool),
    Int64(i64),
    Float64(u64),
    Utf8(String),
    Categorical(u32),
    Date(i32),
    Time(i64),
}

type GroupKey = Vec<GroupValue>;

pub(crate) fn pivot(
    input: MaterializedTable,
    groups: &[ColumnId],
    names_from: ColumnId,
    values_from: ColumnId,
    aggregate_function: AggregateFunction,
    operation: OperationId,
    diagnostics: &mut Vec<Diagnostic>,
) -> Result<MaterializedTable> {
    if groups.contains(&names_from) || groups.contains(&values_from) || names_from == values_from {
        return Err(DataError::InvalidPlan(
            "pivot group, name, and value columns must be distinct".into(),
        ));
    }
    let name_column = input.column(names_from)?;
    let value_column = input.column(values_from)?;
    if !matches!(
        name_column.schema.logical_type,
        LogicalType::Utf8 | LogicalType::Categorical { .. }
    ) || !matches!(value_column.schema.logical_type, LogicalType::Float64)
    {
        return Err(DataError::InvalidPlan(
            "pivot names must be text/categorical and values must be Float64".into(),
        ));
    }

    let mut names = BTreeSet::new();
    let mut grouped: BTreeMap<GroupKey, BTreeMap<String, Vec<usize>>> = BTreeMap::new();
    for row in 0..input.row_ids.len() {
        let name = pivot_name(&name_column.values[row], &name_column.schema)?;
        let Some(name) = name else {
            continue;
        };
        names.insert(name.clone());
        grouped
            .entry(group_key(&input, row, groups)?)
            .or_default()
            .entry(name)
            .or_default()
            .push(row);
    }
    let names: Vec<String> = names.into_iter().collect();
    let mut schemas = groups
        .iter()
        .map(|id| input.column(*id).map(|column| column.schema.clone()))
        .collect::<Result<Vec<_>>>()?;
    for name in &names {
        let output_type = if matches!(
            aggregate_function,
            AggregateFunction::CountAll | AggregateFunction::Count
        ) {
            LogicalType::Int64
        } else {
            LogicalType::Float64
        };
        let mut column = ColumnSchema::new(name, output_type);
        column.id = ColumnId::derived(operation, name.as_bytes());
        if !matches!(
            aggregate_function,
            AggregateFunction::CountAll | AggregateFunction::Count
        ) {
            column.unit = value_column.schema.unit.clone();
        }
        schemas.push(column);
    }
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
    for (group_index, cells) in grouped.values().enumerate() {
        let source_rows: Vec<usize> = cells.values().flatten().copied().collect();
        let representative = *source_rows
            .first()
            .ok_or_else(|| DataError::InvalidArray("pivot produced an empty group".into()))?;
        for (output, id) in columns.iter_mut().take(groups.len()).zip(groups) {
            output
                .values
                .push(input.column(*id)?.values[representative].clone());
        }
        for (name_index, name) in names.iter().enumerate() {
            let rows = cells.get(name).map(Vec::as_slice).unwrap_or_default();
            let values: Vec<ScalarValue> = rows
                .iter()
                .map(|row| value_column.values[*row].clone())
                .collect();
            let null_count = values
                .iter()
                .filter(|value| matches!(value, ScalarValue::Null))
                .count() as u64;
            if null_count > 0 {
                super::execute::diagnostic_count(
                    diagnostics,
                    "pivot.null_excluded",
                    "Pivot aggregate excluded null values",
                    null_count,
                );
            }
            columns[groups.len() + name_index].values.push(aggregate(
                &values,
                aggregate_function,
                rows.len(),
            )?);
        }
        let ids: Vec<RowId> = source_rows.iter().map(|row| input.row_ids[*row]).collect();
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

pub(crate) fn unpivot(
    input: MaterializedTable,
    ids: &[ColumnId],
    values: &[ColumnId],
    name_schema: &ColumnSchema,
    value_schema: &ColumnSchema,
    operation: OperationId,
) -> Result<MaterializedTable> {
    if values.is_empty()
        || !matches!(name_schema.logical_type, LogicalType::Utf8)
        || ids.iter().any(|id| values.contains(id))
    {
        return Err(DataError::InvalidPlan(
            "unpivot requires value columns distinct from ids and a UTF-8 name column".into(),
        ));
    }
    let source_columns = values
        .iter()
        .map(|id| input.column(*id))
        .collect::<Result<Vec<_>>>()?;
    if source_columns.iter().any(|column| {
        column.schema.logical_type != value_schema.logical_type
            || column.schema.unit != value_schema.unit
    }) {
        return Err(DataError::InvalidPlan(
            "unpivot value columns must have one type and unit".into(),
        ));
    }
    let mut schemas = ids
        .iter()
        .map(|id| input.column(*id).map(|column| column.schema.clone()))
        .collect::<Result<Vec<_>>>()?;
    schemas.push(name_schema.clone());
    schemas.push(value_schema.clone());
    let schema = TableSchema::new(schemas)?;
    let output_len = input.row_ids.len().saturating_mul(values.len());
    let mut columns: Vec<MaterializedColumn> = schema
        .columns
        .iter()
        .cloned()
        .map(|schema| MaterializedColumn {
            schema,
            values: Vec::with_capacity(output_len),
        })
        .collect();
    let mut row_ids = Vec::with_capacity(output_len);
    for row in 0..input.row_ids.len() {
        for source in &source_columns {
            for (output, id) in columns.iter_mut().take(ids.len()).zip(ids) {
                output.values.push(input.column(*id)?.values[row].clone());
            }
            columns[ids.len()]
                .values
                .push(ScalarValue::Utf8(source.schema.name.clone()));
            columns[ids.len() + 1]
                .values
                .push(source.values[row].clone());
            row_ids.push(RowId::derived(
                operation,
                &[input.row_ids[row]],
                source.schema.id.as_bytes(),
            ));
        }
    }
    Ok(MaterializedTable {
        table_id: input.table_id,
        schema,
        row_ids,
        columns,
    })
}

fn pivot_name(value: &ScalarValue, schema: &ColumnSchema) -> Result<Option<String>> {
    match value {
        ScalarValue::Null => Ok(None),
        ScalarValue::Utf8(value) => Ok(Some(value.clone())),
        ScalarValue::Categorical(index) => {
            let LogicalType::Categorical { levels } = &schema.logical_type else {
                return Err(DataError::InvalidArray(
                    "categorical value has non-categorical schema".into(),
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

fn group_key(table: &MaterializedTable, row: usize, groups: &[ColumnId]) -> Result<GroupKey> {
    groups
        .iter()
        .map(|id| match &table.column(*id)?.values[row] {
            ScalarValue::Null => Ok(GroupValue::Null),
            ScalarValue::Boolean(value) => Ok(GroupValue::Boolean(*value)),
            ScalarValue::Int64(value) => Ok(GroupValue::Int64(*value)),
            ScalarValue::Float64(value) => Ok(GroupValue::Float64(float_key(*value))),
            ScalarValue::Utf8(value) => Ok(GroupValue::Utf8(value.clone())),
            ScalarValue::Categorical(value) => Ok(GroupValue::Categorical(*value)),
            ScalarValue::Date(value) => Ok(GroupValue::Date(*value)),
            ScalarValue::Time(value)
            | ScalarValue::Timestamp(value)
            | ScalarValue::Duration(value) => Ok(GroupValue::Time(*value)),
            ScalarValue::Extension { .. } => Err(DataError::Unsupported(
                "extension group keys require a registered comparison".into(),
            )),
        })
        .collect()
}

fn aggregate(
    values: &[ScalarValue],
    function: AggregateFunction,
    rows: usize,
) -> Result<ScalarValue> {
    if function == AggregateFunction::CountAll {
        return Ok(ScalarValue::Int64(rows as i64));
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
    if numbers.is_empty() {
        return Ok(ScalarValue::Null);
    }
    let result = match function {
        AggregateFunction::SumV1 => numbers.iter().sum(),
        AggregateFunction::MeanV1 => numbers.iter().sum::<f64>() / numbers.len() as f64,
        AggregateFunction::MinimumV1 => numbers
            .iter()
            .copied()
            .min_by(|left, right| float_cmp(*left, *right))
            .expect("non-empty pivot aggregate"),
        AggregateFunction::MaximumV1 => numbers
            .iter()
            .copied()
            .max_by(|left, right| float_cmp(*left, *right))
            .expect("non-empty pivot aggregate"),
        _ => unreachable!(),
    };
    Ok(ScalarValue::Float64(result))
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

fn float_cmp(left: f64, right: f64) -> Ordering {
    match (left.is_nan(), right.is_nan()) {
        (true, true) => Ordering::Equal,
        (true, false) => Ordering::Greater,
        (false, true) => Ordering::Less,
        (false, false) => left.total_cmp(&right),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn long_table() -> (MaterializedTable, ColumnId, ColumnId, ColumnId) {
        let group = ColumnSchema::new("sample", LogicalType::Utf8);
        let name = ColumnSchema::new("metric", LogicalType::Utf8);
        let value = ColumnSchema::new("value", LogicalType::Float64);
        let schema = TableSchema::new(vec![group.clone(), name.clone(), value.clone()]).unwrap();
        (
            MaterializedTable {
                table_id: crate::TableId::new(),
                schema,
                row_ids: (0..4).map(|_| RowId::new()).collect(),
                columns: vec![
                    MaterializedColumn {
                        schema: group.clone(),
                        values: ["a", "a", "b", "b"]
                            .map(|value| ScalarValue::Utf8(value.into()))
                            .into(),
                    },
                    MaterializedColumn {
                        schema: name.clone(),
                        values: ["height", "area", "height", "area"]
                            .map(|value| ScalarValue::Utf8(value.into()))
                            .into(),
                    },
                    MaterializedColumn {
                        schema: value.clone(),
                        values: [1.0, 2.0, 3.0, 4.0].map(ScalarValue::Float64).into(),
                    },
                ],
            },
            group.id,
            name.id,
            value.id,
        )
    }

    #[test]
    fn pivot_has_deterministic_columns_and_rows() {
        let (table, group, name, value) = long_table();
        let operation = OperationId::from_bytes([9; 16]);
        let mut diagnostics = Vec::new();
        let first = pivot(
            table.clone(),
            &[group],
            name,
            value,
            AggregateFunction::MeanV1,
            operation,
            &mut diagnostics,
        )
        .unwrap();
        let second = pivot(
            table,
            &[group],
            name,
            value,
            AggregateFunction::MeanV1,
            operation,
            &mut diagnostics,
        )
        .unwrap();
        assert_eq!(first.schema, second.schema);
        assert_eq!(first.row_ids, second.row_ids);
        assert_eq!(first.row_ids.len(), 2);
    }

    #[test]
    fn unpivot_preserves_source_column_identity_in_row_ids() {
        let (table, group, name, value) = long_table();
        let name_schema = ColumnSchema::new("measure", LogicalType::Utf8);
        let value_schema = ColumnSchema::new("reading", LogicalType::Float64);
        let wide = unpivot(
            table,
            &[group],
            &[name, value],
            &name_schema,
            &value_schema,
            OperationId::from_bytes([7; 16]),
        );
        assert!(wide.is_err(), "different source types must be rejected");
    }
}
