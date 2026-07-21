use super::{
    LEFT_ROW_ID_FIELD, RIGHT_ROW_ID_FIELD, ROW_ID_FIELD, ROW_POSITION_FIELD,
    UNPIVOT_SOURCE_ID_FIELD, column_field, result,
};
use crate::{
    ColumnChunk, ColumnValues, DataError, LogicalType, MaterializedColumn, MaterializedTable,
    Result, ScalarValue, Validity,
};
use arrow::{
    array::{Array, ArrayRef, Int64Array, StringArray},
    datatypes::{DataType, Field, Schema, SchemaRef, TimeUnit},
    record_batch::RecordBatch,
};
use std::{str::FromStr, sync::Arc};

pub(super) fn table_to_batch(
    table: &MaterializedTable,
    namespace: Option<crate::TableId>,
) -> Result<RecordBatch> {
    table.validate()?;
    let mut fields = vec![
        Field::new(ROW_ID_FIELD, arrow::datatypes::DataType::Utf8, false),
        Field::new(ROW_POSITION_FIELD, arrow::datatypes::DataType::Int64, false),
    ];
    let mut arrays: Vec<ArrayRef> = vec![
        Arc::new(StringArray::from(
            table
                .row_ids
                .iter()
                .map(|row| namespace.map_or(*row, |source| crate::RowId::namespaced(source, *row)))
                .map(|row| row.to_string())
                .collect::<Vec<_>>(),
        )),
        Arc::new(Int64Array::from_iter_values(
            (0..table.row_ids.len()).map(|row| row as i64),
        )),
    ];
    for column in &table.columns {
        let chunk = materialized_chunk(column)?;
        let array = crate::storage::to_arrow(chunk.values(), chunk.validity())?;
        fields.push(Field::new(
            column_field(column.schema.id),
            array.data_type().clone(),
            column.schema.nullable,
        ));
        arrays.push(array);
    }
    RecordBatch::try_new(Arc::new(Schema::new(fields)), arrays)
        .map_err(|error| DataError::Backend(error.to_string()))
}

pub(super) fn snapshot_schema(schema: &crate::TableSchema) -> Result<SchemaRef> {
    let mut fields = vec![
        Field::new(ROW_ID_FIELD, DataType::Utf8, false),
        Field::new(ROW_POSITION_FIELD, DataType::Int64, false),
    ];
    for column in &schema.columns {
        fields.push(Field::new(
            column_field(column.id),
            arrow_type(&column.logical_type)?,
            column.nullable,
        ));
    }
    Ok(Arc::new(Schema::new(fields)))
}

pub(super) fn snapshot_batch_to_record(
    batch: crate::TableBatch,
    schema: &crate::TableSchema,
    namespace: Option<crate::TableId>,
) -> Result<RecordBatch> {
    let arrow_schema = snapshot_schema(schema)?;
    let row_count = batch.row_ids.len();
    let mut arrays: Vec<ArrayRef> = vec![
        Arc::new(StringArray::from(
            batch
                .row_ids
                .into_iter()
                .map(|row| namespace.map_or(row, |source| crate::RowId::namespaced(source, row)))
                .map(|row| row.to_string())
                .collect::<Vec<_>>(),
        )),
        Arc::new(Int64Array::from_iter_values(
            (0..row_count).map(|row| batch.row_start as i64 + row as i64),
        )),
    ];
    for (_, chunk) in batch.columns {
        arrays.push(crate::storage::to_arrow(chunk.values(), chunk.validity())?);
    }
    RecordBatch::try_new(arrow_schema, arrays)
        .map_err(|error| DataError::Backend(error.to_string()))
}

fn arrow_type(logical_type: &LogicalType) -> Result<DataType> {
    Ok(match logical_type {
        LogicalType::Null => DataType::Null,
        LogicalType::Boolean => DataType::Boolean,
        LogicalType::Int64 => DataType::Int64,
        LogicalType::Float64 => DataType::Float64,
        LogicalType::Utf8 => DataType::Utf8,
        LogicalType::Categorical { .. } => DataType::UInt32,
        LogicalType::Date => DataType::Date32,
        LogicalType::Time => DataType::Time64(TimeUnit::Nanosecond),
        LogicalType::Timestamp { .. } => {
            DataType::Timestamp(TimeUnit::Nanosecond, Some("UTC".into()))
        }
        LogicalType::Duration => DataType::Duration(TimeUnit::Nanosecond),
        LogicalType::Extension(extension) => arrow_type(&extension.storage)?,
    })
}

fn materialized_chunk(column: &MaterializedColumn) -> Result<ColumnChunk> {
    let validity = Validity::from_valid(
        column
            .values
            .iter()
            .map(|value| !matches!(value, ScalarValue::Null)),
    );
    let values = scalars_to_values(&column.schema.logical_type, &column.values)?;
    ColumnChunk::new(values, validity)
}

fn scalars_to_values(logical_type: &LogicalType, values: &[ScalarValue]) -> Result<ColumnValues> {
    macro_rules! collect_values {
        ($variant:ident, $default:expr) => {
            values
                .iter()
                .map(|value| match value {
                    ScalarValue::$variant(value) => Ok(value.clone()),
                    ScalarValue::Null => Ok($default),
                    other => Err(DataError::TypeMismatch {
                        expected: logical_type.clone(),
                        actual: other.logical_type(),
                    }),
                })
                .collect::<Result<Vec<_>>>()?
        };
    }
    Ok(match logical_type {
        LogicalType::Null => ColumnValues::Null(values.len()),
        LogicalType::Boolean => ColumnValues::Boolean(collect_values!(Boolean, false)),
        LogicalType::Int64 => ColumnValues::Int64(collect_values!(Int64, 0)),
        LogicalType::Float64 => ColumnValues::Float64(collect_values!(Float64, 0.0)),
        LogicalType::Utf8 => ColumnValues::Utf8(collect_values!(Utf8, String::new())),
        LogicalType::Categorical { .. } => {
            ColumnValues::Categorical(collect_values!(Categorical, 0))
        }
        LogicalType::Date => ColumnValues::Date(collect_values!(Date, 0)),
        LogicalType::Time => ColumnValues::Time(collect_values!(Time, 0)),
        LogicalType::Timestamp { .. } => ColumnValues::Timestamp(collect_values!(Timestamp, 0)),
        LogicalType::Duration => ColumnValues::Duration(collect_values!(Duration, 0)),
        LogicalType::Extension(_) => {
            return Err(DataError::Unsupported(
                "DataFusion logical extension arrays".into(),
            ));
        }
    })
}

pub(super) fn batches_to_table(
    batches: &[RecordBatch],
    expected: &MaterializedTable,
    operation: crate::OperationId,
) -> Result<MaterializedTable> {
    let mut row_ids = Vec::new();
    let mut backend_row_count = 0;
    let mut columns = expected
        .schema
        .columns
        .iter()
        .cloned()
        .map(|schema| MaterializedColumn {
            schema,
            values: Vec::new(),
        })
        .collect::<Vec<_>>();
    for record_batch in batches {
        let batch_start = backend_row_count;
        backend_row_count += record_batch.num_rows();
        append_row_ids(record_batch, operation, batch_start, &mut row_ids)?;
        for column in &mut columns {
            let index = record_batch
                .schema()
                .index_of(&column_field(column.schema.id))
                .map_err(|error| DataError::Backend(error.to_string()))?;
            let chunk = crate::storage::from_arrow(
                &column.schema.logical_type,
                record_batch.column(index).as_ref(),
            )?;
            column
                .values
                .extend((0..chunk.len()).filter_map(|row| chunk.value(row)));
        }
    }
    if row_ids.is_empty() && backend_row_count > 0 {
        return result::align_row_changing_output(columns, expected, backend_row_count);
    }
    let table = MaterializedTable {
        table_id: expected.table_id,
        schema: expected.schema.clone(),
        row_ids,
        columns,
    };
    table.validate()?;
    Ok(table)
}

pub(super) fn record_batch_to_chunks(
    batch: &RecordBatch,
    schema: &crate::TableSchema,
    operation: crate::OperationId,
    batch_start: usize,
    derived_rows: &mut std::vec::IntoIter<crate::RowId>,
) -> Result<(Vec<crate::RowId>, Vec<ColumnChunk>)> {
    let mut row_ids = Vec::with_capacity(batch.num_rows());
    append_row_ids(batch, operation, batch_start, &mut row_ids)?;
    if row_ids.is_empty() {
        row_ids.extend(derived_rows.take(batch.num_rows()));
    }
    if row_ids.len() != batch.num_rows() {
        return Err(DataError::Backend(
            "DataFusion output row count differs from its deterministic identity plan".into(),
        ));
    }
    let chunks = schema
        .columns
        .iter()
        .map(|column| {
            let index = batch
                .schema()
                .index_of(&column_field(column.id))
                .map_err(|error| DataError::Backend(error.to_string()))?;
            crate::storage::from_arrow(&column.logical_type, batch.column(index).as_ref())
        })
        .collect::<Result<Vec<_>>>()?;
    Ok((row_ids, chunks))
}

fn append_row_ids(
    batch: &RecordBatch,
    operation: crate::OperationId,
    batch_start: usize,
    output: &mut Vec<crate::RowId>,
) -> Result<()> {
    if let (Ok(row_index), Ok(source_index)) = (
        batch.schema().index_of(ROW_ID_FIELD),
        batch.schema().index_of(UNPIVOT_SOURCE_ID_FIELD),
    ) {
        let rows = string_array(batch, row_index, "unpivot row identity")?;
        let sources = string_array(batch, source_index, "unpivot source identity")?;
        for row in 0..batch.num_rows() {
            if rows.is_null(row) || sources.is_null(row) {
                return Err(DataError::Backend("null unpivot identity".into()));
            }
            let input = crate::RowId::from_str(rows.value(row)).map_err(|error| {
                DataError::Backend(format!("invalid unpivot row identity: {error}"))
            })?;
            let source = crate::ColumnId::from_str(sources.value(row)).map_err(|error| {
                DataError::Backend(format!("invalid unpivot source identity: {error}"))
            })?;
            output.push(crate::RowId::derived(
                operation,
                &[input],
                source.as_bytes(),
            ));
        }
        return Ok(());
    }
    if let Ok(row_index) = batch.schema().index_of(ROW_ID_FIELD) {
        let rows = string_array(batch, row_index, "DataFusion row identity")?;
        for value in rows.iter() {
            let value = value.ok_or_else(|| DataError::Backend("null row identity".into()))?;
            output.push(crate::RowId::from_str(value).map_err(|error| {
                DataError::Backend(format!("invalid DataFusion row identity: {error}"))
            })?);
        }
        return Ok(());
    }
    let (Ok(left_index), Ok(right_index)) = (
        batch.schema().index_of(LEFT_ROW_ID_FIELD),
        batch.schema().index_of(RIGHT_ROW_ID_FIELD),
    ) else {
        return Ok(());
    };
    let left = string_array(batch, left_index, "left join row identity")?;
    let right = string_array(batch, right_index, "right join row identity")?;
    for row in 0..batch.num_rows() {
        let ids = [
            (!left.is_null(row)).then(|| left.value(row)),
            (!right.is_null(row)).then(|| right.value(row)),
        ]
        .into_iter()
        .flatten()
        .map(|value| {
            crate::RowId::from_str(value).map_err(|error| {
                DataError::Backend(format!("invalid joined row identity: {error}"))
            })
        })
        .collect::<Result<Vec<_>>>()?;
        output.push(crate::RowId::derived(
            operation,
            &ids,
            &((batch_start + row) as u64).to_le_bytes(),
        ));
    }
    Ok(())
}

fn string_array<'a>(
    batch: &'a RecordBatch,
    index: usize,
    description: &str,
) -> Result<&'a StringArray> {
    batch
        .column(index)
        .as_any()
        .downcast_ref::<StringArray>()
        .ok_or_else(|| DataError::Backend(format!("{description} is not UTF-8")))
}

pub(super) fn tables_equal(left: &MaterializedTable, right: &MaterializedTable) -> bool {
    left.table_id == right.table_id
        && left.schema == right.schema
        && left.row_ids == right.row_ids
        && left.columns.len() == right.columns.len()
        && left
            .columns
            .iter()
            .zip(&right.columns)
            .all(|(left, right)| {
                left.schema == right.schema
                    && left.values.len() == right.values.len()
                    && left
                        .values
                        .iter()
                        .zip(&right.values)
                        .all(|(left, right)| result::scalar_equal(left, right))
            })
}
