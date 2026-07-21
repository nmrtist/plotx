use crate::{
    CodecRegistry, ColumnChunk, ColumnValues, DataError, LogicalType, MaterializedTable, Result,
    ScalarValue, SnapshotBuilder, TableId, TableSnapshot, Validity,
};

/// Encode an execution result into PlotX's chunked snapshot format. The
/// caller chooses the output table identity so refreshes can keep one stable
/// derived-table identity while producing new revisions.
pub fn snapshot_from_materialized(
    table: &MaterializedTable,
    output_table: TableId,
    store: &dyn crate::BlockStore,
    codecs: &CodecRegistry,
    batch_rows: usize,
) -> Result<TableSnapshot> {
    table.validate()?;
    if batch_rows == 0 {
        return Err(DataError::InvalidArray(
            "snapshot batch size must be positive".into(),
        ));
    }
    let mut builder = SnapshotBuilder::new(output_table, table.schema.clone(), store, codecs)?;
    for start in (0..table.row_ids.len()).step_by(batch_rows) {
        let end = (start + batch_rows).min(table.row_ids.len());
        let chunks = table
            .columns
            .iter()
            .map(|column| {
                chunk_from_scalars(&column.schema.logical_type, &column.values[start..end])
            })
            .collect::<Result<Vec<_>>>()?;
        builder.push_batch(&table.row_ids[start..end], &chunks)?;
    }
    builder.finish()
}

fn chunk_from_scalars(logical_type: &LogicalType, values: &[ScalarValue]) -> Result<ColumnChunk> {
    let validity = Validity::from_valid(
        values
            .iter()
            .map(|value| !matches!(value, ScalarValue::Null)),
    );
    macro_rules! collect {
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
    let stored = match logical_type {
        LogicalType::Null => {
            if values
                .iter()
                .any(|value| !matches!(value, ScalarValue::Null))
            {
                return Err(DataError::TypeMismatch {
                    expected: LogicalType::Null,
                    actual: values[0].logical_type(),
                });
            }
            ColumnValues::Null(values.len())
        }
        LogicalType::Boolean => ColumnValues::Boolean(collect!(Boolean, false)),
        LogicalType::Int64 => ColumnValues::Int64(collect!(Int64, 0)),
        LogicalType::Float64 => ColumnValues::Float64(collect!(Float64, 0.0)),
        LogicalType::Utf8 => ColumnValues::Utf8(collect!(Utf8, String::new())),
        LogicalType::Categorical { .. } => ColumnValues::Categorical(collect!(Categorical, 0)),
        LogicalType::Date => ColumnValues::Date(collect!(Date, 0)),
        LogicalType::Time => ColumnValues::Time(collect!(Time, 0)),
        LogicalType::Timestamp { .. } => ColumnValues::Timestamp(collect!(Timestamp, 0)),
        LogicalType::Duration => ColumnValues::Duration(collect!(Duration, 0)),
        LogicalType::Extension(extension) => {
            let storage_values = values
                .iter()
                .map(|value| match value {
                    ScalarValue::Extension { type_id, storage } if *type_id == extension.id => {
                        Ok((**storage).clone())
                    }
                    ScalarValue::Null => Ok(ScalarValue::Null),
                    other => Err(DataError::TypeMismatch {
                        expected: logical_type.clone(),
                        actual: other.logical_type(),
                    }),
                })
                .collect::<Result<Vec<_>>>()?;
            let storage = chunk_from_scalars(&extension.storage, &storage_values)?;
            ColumnValues::Extension {
                type_id: extension.id.clone(),
                storage: Box::new(storage.values().clone()),
            }
        }
    };
    ColumnChunk::new(stored, validity)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ColumnSchema, MaterializedColumn, RowId, TableSchema};

    #[test]
    fn materialized_results_rechunk_without_losing_null_or_non_finite_values() {
        let mut column = ColumnSchema::new("value", LogicalType::Float64);
        column.nullable = true;
        let table = MaterializedTable {
            table_id: TableId::new(),
            schema: TableSchema::new(vec![column.clone()]).unwrap(),
            row_ids: vec![RowId::new(), RowId::new(), RowId::new()],
            columns: vec![MaterializedColumn {
                schema: column,
                values: vec![
                    ScalarValue::Null,
                    ScalarValue::Float64(f64::NAN),
                    ScalarValue::Float64(f64::INFINITY),
                ],
            }],
        };
        let store = crate::MemoryBlockStore::default();
        let codecs = CodecRegistry::with_arrow_ipc();
        let snapshot =
            snapshot_from_materialized(&table, TableId::new(), &store, &codecs, 2).unwrap();
        assert_eq!(snapshot.batch_count(), 2);
        let batch = crate::SnapshotReader::new(&snapshot, &store, &codecs)
            .unwrap()
            .read_batch(0, &[])
            .unwrap();
        assert_eq!(batch.columns[0].1.value(0), Some(ScalarValue::Null));
        assert!(
            matches!(batch.columns[0].1.value(1), Some(ScalarValue::Float64(value)) if value.is_nan())
        );
    }
}
