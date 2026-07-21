use super::{CellValue, DelimitedTable};
use plotx_data::{
    ColumnChunk, ColumnId, ColumnSchema, ColumnValues, LogicalType, RowId, SnapshotBuilder,
    TableSchema, UncertaintyKind, UncertaintyMeaning, UncertaintyRelation, Validity,
};

impl DelimitedTable {
    /// Materialize the confirmed import preview as a typed, chunked snapshot.
    /// Mixed columns remain UTF-8, missing cells remain null, and no column is
    /// discarded merely because it is not numeric.
    pub fn into_typed_snapshot(
        self,
        table_id: plotx_data::TableId,
        store: &dyn plotx_data::BlockStore,
        codecs: &plotx_data::CodecRegistry,
    ) -> plotx_data::Result<plotx_data::TableSnapshot> {
        let width = self.rows.first().map_or(0, |row| row.cells.len());
        let names = self.headers.unwrap_or_else(|| {
            (0..width)
                .map(|index| format!("Column {}", index + 1))
                .collect()
        });
        let logical_types: Vec<LogicalType> = (0..width)
            .map(|column| infer_logical_type(&self.rows, column))
            .collect();
        let mut schemas: Vec<ColumnSchema> = names
            .iter()
            .zip(&logical_types)
            .enumerate()
            .map(|(column, (name, logical_type))| {
                let mut schema = ColumnSchema::new(name, logical_type.clone());
                schema.nullable = self
                    .rows
                    .iter()
                    .any(|row| matches!(row.cells[column].value, CellValue::Missing));
                schema
            })
            .collect();
        let mut uncertainty = Vec::new();
        for value_index in 0..width.saturating_sub(1) {
            let sigma_index = value_index + 1;
            let is_non_negative = self
                .rows
                .iter()
                .all(|row| match row.cells[sigma_index].value {
                    CellValue::Missing => true,
                    CellValue::Number(value) => value >= 0.0,
                    CellValue::Text => false,
                });
            if is_numeric(&logical_types[value_index])
                && is_numeric(&logical_types[sigma_index])
                && names[sigma_index] == format!("{}_sigma", names[value_index])
                && is_non_negative
            {
                let value = schemas[value_index].id;
                let sigma = ColumnId::derived_from(value, b"symmetric-sigma");
                schemas[sigma_index].id = sigma;
                uncertainty.push(UncertaintyRelation {
                    value,
                    kind: UncertaintyKind::Symmetric {
                        column: sigma,
                        meaning: UncertaintyMeaning::MeasurementStandardDeviation,
                    },
                });
            }
        }
        let schema = TableSchema::new(schemas)?;
        let mut builder = SnapshotBuilder::new(table_id, schema, store, codecs)?;
        builder.set_uncertainty(uncertainty)?;
        builder.metadata_mut().insert(
            "space.nmrtist.plotx.import.delimiter".into(),
            serde_json::Value::String(self.delimiter.to_string()),
        );
        builder.metadata_mut().insert(
            "space.nmrtist.plotx.import.diagnostics".into(),
            serde_json::Value::Array(
                self.diagnostics
                    .iter()
                    .map(|diagnostic| serde_json::Value::String(diagnostic.message.clone()))
                    .collect(),
            ),
        );
        builder.metadata_mut().insert(
            "space.nmrtist.plotx.import.inference".into(),
            serde_json::Value::Array(
                names
                    .iter()
                    .zip(&logical_types)
                    .map(|(name, logical_type)| {
                        serde_json::Value::String(format!(
                            "Inferred column {name:?} as {logical_type:?} without a PlotX schema contract."
                        ))
                    })
                    .collect(),
            ),
        );
        const IMPORT_CHUNK_ROWS: usize = 65_536;
        for source_rows in self.rows.chunks(IMPORT_CHUNK_ROWS) {
            let mut chunks = Vec::with_capacity(width);
            for (column, logical_type) in logical_types.iter().enumerate() {
                let validity = Validity::from_valid(
                    source_rows
                        .iter()
                        .map(|row| !matches!(row.cells[column].value, CellValue::Missing)),
                );
                let values = inferred_values(source_rows, column, logical_type);
                chunks.push(ColumnChunk::new(values, validity)?);
            }
            let rows = (0..source_rows.len())
                .map(|_| RowId::new())
                .collect::<Vec<_>>();
            builder.push_batch(&rows, &chunks)?;
        }
        builder.finish()
    }
}

fn infer_logical_type(rows: &[super::DelimitedRow], column: usize) -> LogicalType {
    let present = rows
        .iter()
        .map(|row| &row.cells[column])
        .filter(|cell| !matches!(cell.value, CellValue::Missing))
        .collect::<Vec<_>>();
    if present.is_empty() {
        return LogicalType::Null;
    }
    if present
        .iter()
        .all(|cell| matches!(cell.value, CellValue::Number(_)))
    {
        return if present
            .iter()
            .all(|cell| cell.raw.trim().parse::<i64>().is_ok())
        {
            LogicalType::Int64
        } else {
            LogicalType::Float64
        };
    }
    if !present
        .iter()
        .all(|cell| matches!(cell.value, CellValue::Text))
    {
        return LogicalType::Utf8;
    }
    if present.iter().all(|cell| parse_bool(&cell.raw).is_some()) {
        LogicalType::Boolean
    } else if present
        .iter()
        .all(|cell| crate::xlsx::parse_iso_date(cell.raw.trim()).is_some())
    {
        LogicalType::Date
    } else if present
        .iter()
        .all(|cell| crate::xlsx::parse_iso_timestamp_utc(cell.raw.trim()).is_some())
    {
        LogicalType::Timestamp {
            display_timezone: "UTC".into(),
        }
    } else {
        LogicalType::Utf8
    }
}

fn inferred_values(
    rows: &[super::DelimitedRow],
    column: usize,
    logical_type: &LogicalType,
) -> ColumnValues {
    match logical_type {
        LogicalType::Null => ColumnValues::Null(rows.len()),
        LogicalType::Boolean => ColumnValues::Boolean(
            rows.iter()
                .map(|row| parse_bool(raw_cell(row, column)).unwrap_or(false))
                .collect(),
        ),
        LogicalType::Int64 => ColumnValues::Int64(
            rows.iter()
                .map(|row| raw_cell(row, column).parse().unwrap_or_default())
                .collect(),
        ),
        LogicalType::Float64 => ColumnValues::Float64(
            rows.iter()
                .map(|row| match row.cells[column].value {
                    CellValue::Number(value) => value,
                    CellValue::Missing | CellValue::Text => 0.0,
                })
                .collect(),
        ),
        LogicalType::Date => ColumnValues::Date(
            rows.iter()
                .map(|row| crate::xlsx::parse_iso_date(raw_cell(row, column)).unwrap_or_default())
                .collect(),
        ),
        LogicalType::Timestamp { .. } => ColumnValues::Timestamp(
            rows.iter()
                .map(|row| {
                    crate::xlsx::parse_iso_timestamp_utc(raw_cell(row, column)).unwrap_or_default()
                })
                .collect(),
        ),
        LogicalType::Utf8 => ColumnValues::Utf8(
            rows.iter()
                .map(|row| row.cells[column].raw.clone())
                .collect(),
        ),
        LogicalType::Categorical { .. }
        | LogicalType::Time
        | LogicalType::Duration
        | LogicalType::Extension(_) => unreachable!("these types are never inferred implicitly"),
    }
}

fn raw_cell(row: &super::DelimitedRow, column: usize) -> &str {
    row.cells[column].raw.trim()
}

fn parse_bool(value: &str) -> Option<bool> {
    if value.trim().eq_ignore_ascii_case("true") {
        Some(true)
    } else if value.trim().eq_ignore_ascii_case("false") {
        Some(false)
    } else {
        None
    }
}

fn is_numeric(logical_type: &LogicalType) -> bool {
    matches!(logical_type, LogicalType::Int64 | LogicalType::Float64)
}

#[cfg(test)]
mod tests {
    use crate::delimited::{ParseOptions, parse_delimited};
    use plotx_data::{LogicalType, ScalarValue};

    #[test]
    fn large_imports_are_split_into_bounded_snapshot_chunks() {
        let mut input = String::from("x,y\n");
        for row in 0..65_537 {
            input.push_str(&format!("{row},{row}\n"));
        }
        let parsed = parse_delimited(&input, ParseOptions::default()).unwrap();
        let store = plotx_data::MemoryBlockStore::default();
        let codecs = plotx_data::CodecRegistry::with_arrow_ipc();
        let snapshot = parsed
            .into_typed_snapshot(plotx_data::TableId::new(), &store, &codecs)
            .unwrap();
        assert_eq!(snapshot.batch_count(), 2);
        assert_eq!(snapshot.row_id_chunks[0].row_count, 65_536);
        assert_eq!(snapshot.row_id_chunks[1].row_count, 1);
    }

    #[test]
    fn unambiguous_builtin_types_are_inferred_without_coercing_mixed_columns() {
        let parsed = parse_delimited(
            "id,flag,date,instant,mixed\n1,true,2026-07-20,2026-07-20T01:02:03Z,1\n2,false,2026-07-21,2026-07-21T01:02:03Z,text\n",
            ParseOptions::default(),
        )
        .unwrap();
        let store = plotx_data::MemoryBlockStore::default();
        let codecs = plotx_data::CodecRegistry::with_arrow_ipc();
        let snapshot = parsed
            .into_typed_snapshot(plotx_data::TableId::new(), &store, &codecs)
            .unwrap();

        assert_eq!(snapshot.schema.columns[0].logical_type, LogicalType::Int64);
        assert_eq!(
            snapshot.schema.columns[1].logical_type,
            LogicalType::Boolean
        );
        assert_eq!(snapshot.schema.columns[2].logical_type, LogicalType::Date);
        assert_eq!(
            snapshot.schema.columns[3].logical_type,
            LogicalType::Timestamp {
                display_timezone: "UTC".into()
            }
        );
        assert_eq!(snapshot.schema.columns[4].logical_type, LogicalType::Utf8);
        let reader = plotx_data::SnapshotReader::new(&snapshot, &store, &codecs).unwrap();
        let batch = reader.read_batch(0, &[]).unwrap();
        assert_eq!(batch.columns[0].1.value(1), Some(ScalarValue::Int64(2)));
        assert_eq!(
            batch.columns[1].1.value(1),
            Some(ScalarValue::Boolean(false))
        );
        assert_eq!(
            batch.columns[4].1.value(0),
            Some(ScalarValue::Utf8("1".into()))
        );
    }
}
