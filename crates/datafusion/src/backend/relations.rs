use super::{
    LEFT_ROW_ID_FIELD, LEFT_ROW_POSITION_FIELD, RIGHT_ROW_ID_FIELD, RIGHT_ROW_POSITION_FIELD,
    ROW_ID_FIELD, ROW_POSITION_FIELD, UNPIVOT_SOURCE_ID_FIELD, UNPIVOT_SOURCE_POSITION_FIELD,
    column_field, compile_expression,
};
use crate::{
    AggregateFunction, AggregateMeasure, ColumnId, ColumnSchema, DataError, JoinKey, JoinKind,
    LogicalType, MaterializedTable, OperationId, Result, ScalarValue, TableSchema,
};
use datafusion::{
    dataframe::DataFrame,
    logical_expr::JoinType,
    logical_expr::expr_fn::when,
    prelude::{col, lit},
};
use std::collections::BTreeSet;

pub(super) fn aggregate(
    frame: DataFrame,
    groups: &[ColumnId],
    measures: &[AggregateMeasure],
) -> Result<DataFrame> {
    use datafusion::functions_aggregate::expr_fn::{avg, count, max, min, sum};

    let group_expr = groups
        .iter()
        .map(|column| col(column_field(*column)))
        .collect::<Vec<_>>();
    let measures = measures
        .iter()
        .map(|measure| {
            let expression = measure.input.as_ref().map(compile_expression).transpose()?;
            let aggregate = match (measure.function, expression) {
                (AggregateFunction::CountAll, None) => count(lit(1_i64)),
                (AggregateFunction::Count, Some(value)) => count(value),
                (AggregateFunction::SumV1, Some(value)) => sum(value),
                (AggregateFunction::MeanV1, Some(value)) => avg(value),
                (AggregateFunction::MinimumV1, Some(value)) => min(value),
                (AggregateFunction::MaximumV1, Some(value)) => max(value),
                _ => {
                    return Err(DataError::InvalidPlan(
                        "invalid aggregate input contract".into(),
                    ));
                }
            };
            Ok(aggregate.alias(column_field(measure.output.id)))
        })
        .collect::<Result<Vec<_>>>()?;
    frame
        .aggregate(group_expr, measures)
        .map_err(|error| DataError::Backend(error.to_string()))
}

pub(super) fn join(
    left: DataFrame,
    right: DataFrame,
    kind: JoinKind,
    keys: &[JoinKey],
) -> Result<DataFrame> {
    let left = left
        .with_column_renamed(ROW_ID_FIELD, LEFT_ROW_ID_FIELD)
        .and_then(|frame| frame.with_column_renamed(ROW_POSITION_FIELD, LEFT_ROW_POSITION_FIELD))
        .map_err(|error| DataError::Backend(error.to_string()))?;
    let right = right
        .with_column_renamed(ROW_ID_FIELD, RIGHT_ROW_ID_FIELD)
        .and_then(|frame| frame.with_column_renamed(ROW_POSITION_FIELD, RIGHT_ROW_POSITION_FIELD))
        .map_err(|error| DataError::Backend(error.to_string()))?;
    let output_fields = left
        .schema()
        .fields()
        .iter()
        .map(|field| field.name().to_owned())
        .filter(|field| field != LEFT_ROW_ID_FIELD && field != LEFT_ROW_POSITION_FIELD)
        .chain(
            right
                .schema()
                .fields()
                .iter()
                .map(|field| field.name().to_owned())
                .filter(|field| field != RIGHT_ROW_ID_FIELD && field != RIGHT_ROW_POSITION_FIELD),
        )
        .chain([
            LEFT_ROW_ID_FIELD.to_owned(),
            RIGHT_ROW_ID_FIELD.to_owned(),
            LEFT_ROW_POSITION_FIELD.to_owned(),
            RIGHT_ROW_POSITION_FIELD.to_owned(),
        ])
        .collect::<Vec<_>>();
    let left_keys = keys
        .iter()
        .map(|key| column_field(key.left))
        .collect::<Vec<_>>();
    let right_keys = keys
        .iter()
        .map(|key| column_field(key.right))
        .collect::<Vec<_>>();
    let left_refs = left_keys.iter().map(String::as_str).collect::<Vec<_>>();
    let right_refs = right_keys.iter().map(String::as_str).collect::<Vec<_>>();
    let kind = match kind {
        JoinKind::Inner => JoinType::Inner,
        JoinKind::Left => JoinType::Left,
        JoinKind::Full => JoinType::Full,
    };
    left.join(right, kind, &left_refs, &right_refs, None)
        .and_then(|frame| frame.select(output_fields.iter().map(col).collect::<Vec<_>>()))
        .and_then(|frame| {
            frame.sort(vec![
                col(LEFT_ROW_POSITION_FIELD).sort(true, false),
                col(RIGHT_ROW_POSITION_FIELD).sort(true, false),
            ])
        })
        .map_err(|error| DataError::Backend(error.to_string()))
}

pub(super) fn pivot_names(table: &MaterializedTable, names_from: ColumnId) -> Result<Vec<String>> {
    let column = table.column(names_from)?;
    let mut names = BTreeSet::new();
    for value in &column.values {
        match value {
            ScalarValue::Null => {}
            ScalarValue::Utf8(value) => {
                names.insert(value.clone());
            }
            ScalarValue::Categorical(index) => {
                let LogicalType::Categorical { levels } = &column.schema.logical_type else {
                    return Err(DataError::InvalidArray(
                        "categorical value has non-categorical schema".into(),
                    ));
                };
                let level = levels.get(*index as usize).ok_or_else(|| {
                    DataError::InvalidArray("categorical level is out of range".into())
                })?;
                names.insert(level.value.clone());
            }
            _ => {
                return Err(DataError::InvalidPlan(
                    "pivot name value is not text/categorical".into(),
                ));
            }
        }
    }
    Ok(names.into_iter().collect())
}

pub(super) fn pivot(
    input: DataFrame,
    groups: &[ColumnId],
    names_from: ColumnId,
    values_from: ColumnId,
    function: AggregateFunction,
    operation: OperationId,
    names: &[String],
) -> Result<DataFrame> {
    use datafusion::functions_aggregate::expr_fn::{avg, count, max, min, sum};

    let name = col(column_field(names_from));
    let input = input
        .filter(name.clone().is_not_null())
        .map_err(|error| DataError::Backend(error.to_string()))?;
    let group_expr = groups
        .iter()
        .map(|column| col(column_field(*column)))
        .collect::<Vec<_>>();
    let measures = names
        .iter()
        .map(|pivot_name| {
            let matches_name = name.clone().eq(lit(pivot_name.clone()));
            let value = col(column_field(values_from));
            let selected_value = when(matches_name.clone(), value)
                .otherwise(lit(datafusion::common::ScalarValue::Null))
                .map_err(|error| DataError::Backend(error.to_string()))?;
            let aggregate = match function {
                AggregateFunction::CountAll => {
                    let selected_row = when(matches_name, lit(1_i64))
                        .otherwise(lit(datafusion::common::ScalarValue::Null))
                        .map_err(|error| DataError::Backend(error.to_string()))?;
                    count(selected_row)
                }
                AggregateFunction::Count => count(selected_value),
                AggregateFunction::SumV1 => sum(selected_value),
                AggregateFunction::MeanV1 => avg(selected_value),
                AggregateFunction::MinimumV1 => min(selected_value),
                AggregateFunction::MaximumV1 => max(selected_value),
            };
            Ok(aggregate.alias(column_field(ColumnId::derived(
                operation,
                pivot_name.as_bytes(),
            ))))
        })
        .collect::<Result<Vec<_>>>()?;
    input
        .aggregate(group_expr, measures)
        .map_err(|error| DataError::Backend(error.to_string()))
}

pub(super) fn unpivot(
    input: DataFrame,
    schema: &TableSchema,
    ids: &[ColumnId],
    values: &[ColumnId],
    name_column: &ColumnSchema,
    value_column: &ColumnSchema,
) -> Result<DataFrame> {
    let mut frames = values.iter().enumerate().map(|(source_index, source)| {
        let source_name = schema
            .column(*source)
            .ok_or(DataError::MissingColumn(*source))?
            .name
            .clone();
        let expressions: Vec<_> = ids
            .iter()
            .map(|column| col(column_field(*column)))
            .chain(std::iter::once(
                lit(source_name).alias(column_field(name_column.id)),
            ))
            .chain(std::iter::once(
                col(column_field(*source)).alias(column_field(value_column.id)),
            ))
            .chain([
                col(ROW_ID_FIELD),
                col(ROW_POSITION_FIELD),
                lit(source.to_string()).alias(UNPIVOT_SOURCE_ID_FIELD),
                lit(source_index as i64).alias(UNPIVOT_SOURCE_POSITION_FIELD),
            ])
            .collect();
        input
            .clone()
            .select(expressions)
            .map_err(|error| DataError::Backend(error.to_string()))
    });
    let mut output = frames
        .next()
        .ok_or_else(|| DataError::InvalidPlan("unpivot has no value columns".into()))??;
    for frame in frames {
        output = output
            .union(frame?)
            .map_err(|error| DataError::Backend(error.to_string()))?;
    }
    output
        .sort(vec![
            col(ROW_POSITION_FIELD).sort(true, false),
            col(UNPIVOT_SOURCE_POSITION_FIELD).sort(true, false),
        ])
        .map_err(|error| DataError::Backend(error.to_string()))
}
