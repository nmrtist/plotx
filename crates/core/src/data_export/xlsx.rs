use super::*;
use crate::xlsx::{PlotxDelimitedSchemaV1, PlotxXlsxSchemaV1, PlotxXlsxSheetSchemaV1};
use plotx_data::{LogicalType, ScalarValue};
use plotx_io::{
    delimited::{DelimitedWriter, Field},
    xlsx::{StreamingXlsxWriter, XlsxValue},
};
use std::path::Path;

impl DataExportSnapshot {
    pub fn clipboard_schema_json(&self) -> Result<Option<String>, DataExportError> {
        let SnapshotData::Table(typed) = &self.data else {
            return Ok(None);
        };
        let snapshot = &typed.envelope.revision.snapshot;
        let sidecar = PlotxDelimitedSchemaV1 {
            schema_version: 1,
            table_id: snapshot.table_id,
            schema: snapshot.schema.clone(),
            uncertainty: snapshot.uncertainty.clone(),
        };
        serde_json::to_string(&sidecar)
            .map(Some)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error).into())
    }

    pub fn write_xlsx(&self, path: impl AsRef<Path>) -> Result<(), DataExportError> {
        if let SnapshotData::Table(typed) = &self.data {
            return self.write_typed_xlsx(path, typed);
        }
        self.write_text_xlsx(path)
    }

    pub fn write_delimited_sidecar(
        &self,
        data_path: impl AsRef<Path>,
    ) -> Result<Option<std::path::PathBuf>, DataExportError> {
        let SnapshotData::Table(typed) = &self.data else {
            return Ok(None);
        };
        let snapshot = &typed.envelope.revision.snapshot;
        let sidecar = PlotxDelimitedSchemaV1 {
            schema_version: 1,
            table_id: snapshot.table_id,
            schema: snapshot.schema.clone(),
            uncertainty: snapshot.uncertainty.clone(),
        };
        let bytes = serde_json::to_vec_pretty(&sidecar)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        let path = delimited_sidecar_path(data_path.as_ref());
        std::fs::write(&path, bytes)?;
        Ok(Some(path))
    }

    fn write_typed_xlsx(
        &self,
        path: impl AsRef<Path>,
        typed: &crate::state::TypedTableState,
    ) -> Result<(), DataExportError> {
        let snapshot = &typed.envelope.revision.snapshot;
        let codecs = plotx_data::CodecRegistry::with_arrow_ipc();
        let reader = plotx_data::SnapshotReader::new(snapshot, typed.store.as_ref(), &codecs)?;
        let mut writer = StreamingXlsxWriter::new();
        let sheet_name = safe_sheet_name(&self.dataset_name);
        let sheet = writer.add_worksheet(&sheet_name, false)?;
        let headers = snapshot
            .schema
            .columns
            .iter()
            .map(|column| XlsxValue::Utf8(column.name.clone()))
            .collect::<Vec<_>>();
        writer.write_row(sheet, 0, headers.iter())?;
        let mut output_row = 1_u32;
        for batch_index in 0..snapshot.batch_count() {
            let batch = reader.read_batch(batch_index, &[])?;
            for row in 0..batch.row_ids.len() {
                let values = batch
                    .columns
                    .iter()
                    .zip(&snapshot.schema.columns)
                    .map(|((_, column), schema)| {
                        column.value(row).map_or(XlsxValue::Empty, |value| {
                            scalar_to_xlsx(value, &schema.logical_type)
                        })
                    })
                    .collect::<Vec<_>>();
                writer.write_row(sheet, output_row, values.iter())?;
                output_row = output_row
                    .checked_add(1)
                    .ok_or(plotx_io::xlsx::XlsxIoError::SheetTooLarge)?;
            }
        }
        let schema = PlotxXlsxSchemaV1 {
            schema_version: 1,
            sheets: vec![PlotxXlsxSheetSchemaV1 {
                name: sheet_name,
                table_id: snapshot.table_id,
                schema: snapshot.schema.clone(),
                uncertainty: snapshot.uncertainty.clone(),
            }],
        };
        let schema = serde_json::to_value(schema).expect("XLSX schema always serializes");
        writer.finish(path, Some(&schema))?;
        Ok(())
    }

    fn write_text_xlsx(&self, path: impl AsRef<Path>) -> Result<(), DataExportError> {
        let text = self.to_text(Delimiter::Tab)?;
        let parsed = crate::delimited::parse_delimited(
            &text,
            crate::delimited::ParseOptions {
                delimiter: Some(Delimiter::Tab),
                header: crate::delimited::HeaderMode::Present,
            },
        )
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        let mut writer = StreamingXlsxWriter::new();
        let sheet = writer.add_worksheet(&safe_sheet_name(&self.dataset_name), false)?;
        let headers = parsed
            .headers
            .unwrap_or_default()
            .into_iter()
            .map(XlsxValue::Utf8)
            .collect::<Vec<_>>();
        writer.write_row(sheet, 0, headers.iter())?;
        for (row, input) in parsed.rows.iter().enumerate() {
            let output = input
                .cells
                .iter()
                .map(|cell| match cell.value {
                    crate::delimited::CellValue::Missing => XlsxValue::Empty,
                    crate::delimited::CellValue::Number(value) => XlsxValue::Float64(value),
                    crate::delimited::CellValue::Text => XlsxValue::Utf8(cell.raw.clone()),
                })
                .collect::<Vec<_>>();
            let row =
                u32::try_from(row + 1).map_err(|_| plotx_io::xlsx::XlsxIoError::SheetTooLarge)?;
            writer.write_row(sheet, row, output.iter())?;
        }
        writer.finish(path, None)?;
        Ok(())
    }
}

pub fn delimited_sidecar_path(data_path: &Path) -> std::path::PathBuf {
    let name = data_path
        .file_name()
        .map_or_else(|| "table".into(), |name| name.to_string_lossy());
    data_path.with_file_name(format!("{name}.plotx-schema.json"))
}

pub(super) fn write_typed_delimited<W: Write>(
    writer: &mut DelimitedWriter<W>,
    typed: &crate::state::TypedTableState,
) -> Result<(), DataExportError> {
    let snapshot = &typed.envelope.revision.snapshot;
    let headers = snapshot
        .schema
        .columns
        .iter()
        .map(|column| Field::Text(column.name.as_str()))
        .collect::<Vec<_>>();
    writer.write_record(&headers)?;
    let codecs = plotx_data::CodecRegistry::with_arrow_ipc();
    let reader = plotx_data::SnapshotReader::new(snapshot, typed.store.as_ref(), &codecs)?;
    for batch_index in 0..snapshot.batch_count() {
        let batch = reader.read_batch(batch_index, &[])?;
        for row in 0..batch.row_ids.len() {
            let owned = batch
                .columns
                .iter()
                .zip(&snapshot.schema.columns)
                .map(|((_, column), schema)| {
                    column.value(row).map_or(OwnedField::Empty, |value| {
                        owned_field(value, &schema.logical_type)
                    })
                })
                .collect::<Vec<_>>();
            let fields = owned.iter().map(OwnedField::borrowed).collect::<Vec<_>>();
            writer.write_record(&fields)?;
        }
    }
    Ok(())
}

enum OwnedField {
    Empty,
    Number(f64),
    Text(String),
}

impl OwnedField {
    fn borrowed(&self) -> Field<'_> {
        match self {
            Self::Empty => Field::Empty,
            Self::Number(value) => Field::Number(*value),
            Self::Text(value) => Field::Text(value),
        }
    }
}

fn owned_field(value: ScalarValue, logical_type: &LogicalType) -> OwnedField {
    match scalar_to_xlsx(value, logical_type) {
        XlsxValue::Empty | XlsxValue::Error(_) => OwnedField::Empty,
        XlsxValue::Float64(value) => OwnedField::Number(value),
        XlsxValue::Int64(value) => OwnedField::Text(value.to_string()),
        XlsxValue::Boolean(value) => OwnedField::Text(value.to_string()),
        XlsxValue::Utf8(value) | XlsxValue::DateTimeIso(value) | XlsxValue::DurationIso(value) => {
            OwnedField::Text(value)
        }
        XlsxValue::ExcelDateTime { iso8601, .. } => OwnedField::Text(iso8601),
    }
}

fn scalar_to_xlsx(value: ScalarValue, logical_type: &LogicalType) -> XlsxValue {
    match value {
        ScalarValue::Null => XlsxValue::Empty,
        ScalarValue::Boolean(value) => XlsxValue::Boolean(value),
        ScalarValue::Int64(value) => XlsxValue::Int64(value),
        ScalarValue::Float64(value) if value.is_finite() => XlsxValue::Float64(value),
        ScalarValue::Float64(value) if value.is_nan() => XlsxValue::Utf8("NaN".into()),
        ScalarValue::Float64(value) if value.is_sign_positive() => XlsxValue::Utf8("+Inf".into()),
        ScalarValue::Float64(_) => XlsxValue::Utf8("-Inf".into()),
        ScalarValue::Utf8(value) => XlsxValue::Utf8(value),
        ScalarValue::Categorical(index) => match logical_type {
            LogicalType::Categorical { levels } => levels
                .get(index as usize)
                .map_or(XlsxValue::Empty, |level| {
                    XlsxValue::Utf8(level.value.clone())
                }),
            _ => XlsxValue::Empty,
        },
        ScalarValue::Date(days) => XlsxValue::DateTimeIso(format_date(days)),
        ScalarValue::Time(nanos) => XlsxValue::DateTimeIso(format_time(nanos)),
        ScalarValue::Timestamp(nanos) => {
            let days = nanos.div_euclid(86_400_000_000_000);
            let time = nanos.rem_euclid(86_400_000_000_000);
            XlsxValue::DateTimeIso(format!(
                "{}T{}Z",
                format_date(days as i32),
                format_time(time)
            ))
        }
        ScalarValue::Duration(nanos) => {
            XlsxValue::DurationIso(format!("PT{}S", nanos as f64 / 1_000_000_000.0))
        }
        ScalarValue::Extension { storage, .. } => scalar_to_xlsx(*storage, logical_type),
    }
}

fn safe_sheet_name(name: &str) -> String {
    let mut result = name
        .chars()
        .map(|character| {
            if matches!(character, '[' | ']' | ':' | '*' | '?' | '/' | '\\') {
                '_'
            } else {
                character
            }
        })
        .take(31)
        .collect::<String>();
    if result.trim().is_empty() {
        result = "Data".into();
    }
    result
}

fn format_date(days: i32) -> String {
    let (year, month, day) = civil_from_days(days);
    format!("{year:04}-{month:02}-{day:02}")
}

fn format_time(nanos: i64) -> String {
    let nanos = nanos.rem_euclid(86_400_000_000_000);
    let seconds = nanos / 1_000_000_000;
    let fraction = nanos % 1_000_000_000;
    format!(
        "{:02}:{:02}:{:02}.{:09}",
        seconds / 3_600,
        seconds % 3_600 / 60,
        seconds % 60,
        fraction
    )
}

fn civil_from_days(days: i32) -> (i32, u32, u32) {
    let days = days + 719_468;
    let era = if days >= 0 { days } else { days - 146_096 } / 146_097;
    let day_of_era = days - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let mut year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_prime + 2) / 5 + 1;
    let month = month_prime + if month_prime < 10 { 3 } else { -9 };
    year += i32::from(month <= 2);
    (year, month as u32, day as u32)
}

#[cfg(test)]
mod tests {
    use super::*;
    use plotx_data::{
        ColumnChunk, ColumnSchema, ColumnValues, RowId, SnapshotBuilder, TableId, TableSchema,
        Validity,
    };

    #[test]
    fn sheet_names_are_valid_and_date_epoch_is_stable() {
        assert_eq!(safe_sheet_name("a/b:c"), "a_b_c");
        assert_eq!(format_date(0), "1970-01-01");
        assert_eq!(format_date(10_957), "2000-01-01");
    }

    #[test]
    fn typed_xlsx_round_trip_preserves_ids_null_and_non_finite_values() {
        let value = ColumnSchema::new("value", LogicalType::Float64);
        let count = ColumnSchema::new("count", LogicalType::Int64);
        let date = ColumnSchema::new("date", LogicalType::Date);
        let schema = TableSchema::new(vec![value.clone(), count, date]).unwrap();
        let store = Arc::new(plotx_data::MemoryBlockStore::default());
        let codecs = plotx_data::CodecRegistry::with_arrow_ipc();
        let mut builder =
            SnapshotBuilder::new(TableId::new(), schema, store.as_ref(), &codecs).unwrap();
        let rows = (0..5).map(|_| RowId::new()).collect::<Vec<_>>();
        builder
            .push_batch(
                &rows,
                &[
                    ColumnChunk::new(
                        ColumnValues::Float64(vec![
                            1.0,
                            0.0,
                            f64::NAN,
                            f64::INFINITY,
                            f64::NEG_INFINITY,
                        ]),
                        Validity::from_valid([true, false, true, true, true]),
                    )
                    .unwrap(),
                    ColumnChunk::all_valid(ColumnValues::Int64(vec![1, 2, 3, 4, 5])),
                    ColumnChunk::all_valid(ColumnValues::Date(vec![0, 1, 2, 3, 10_957])),
                ],
            )
            .unwrap();
        let snapshot = builder.finish().unwrap();
        let typed = crate::state::TypedTableState::imported_with_operation(
            snapshot,
            Arc::clone(&store),
            "plotx.test.xlsx.v1",
        )
        .unwrap();
        let mut dataset = crate::state::TableDataset::from_typed(typed);
        dataset.name = Some("Roundtrip".into());
        let export = DataExportSnapshot::capture(
            &crate::state::Dataset::Table(Box::new(dataset)),
            DataExportRequest {
                content: DataExportContent::TypedTable,
                channel: IntensityChannel::Real,
                shape: TableShape::Matrix,
            },
        )
        .unwrap();
        let csv = export.to_text(Delimiter::Comma).unwrap();
        assert!(csv.starts_with("value,count,date\n"));
        assert!(csv.contains("NaN,3,1970-01-03"));
        assert!(csv.contains("+Inf,4,1970-01-04"));
        let csv_path =
            std::env::temp_dir().join(format!("plotx-typed-sidecar-{}.csv", std::process::id()));
        let sidecar_path = export.write_delimited_sidecar(&csv_path).unwrap().unwrap();
        let contract: PlotxDelimitedSchemaV1 =
            serde_json::from_slice(&std::fs::read(&sidecar_path).unwrap()).unwrap();
        std::fs::remove_file(sidecar_path).unwrap();
        let parsed = crate::delimited::parse_delimited(
            &csv,
            crate::delimited::ParseOptions {
                delimiter: Some(Delimiter::Comma),
                header: crate::delimited::HeaderMode::Present,
            },
        )
        .unwrap();
        let csv_store = plotx_data::MemoryBlockStore::default();
        let csv_import =
            crate::xlsx::import_delimited_with_schema(&parsed, &contract, &csv_store, &codecs)
                .unwrap();
        assert_eq!(csv_import.snapshot.schema.columns[0].id, value.id);
        let csv_batch = plotx_data::SnapshotReader::new(&csv_import.snapshot, &csv_store, &codecs)
            .unwrap()
            .read_batch(0, &[])
            .unwrap();
        assert!(matches!(
            csv_batch.columns[0].1.value(2),
            Some(ScalarValue::Float64(value)) if value.is_nan()
        ));
        let path = std::env::temp_dir().join(format!(
            "plotx-typed-roundtrip-{}-{}.xlsx",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        export.write_xlsx(&path).unwrap();
        let workbook = plotx_io::xlsx::read_xlsx(&path).unwrap();
        std::fs::remove_file(path).unwrap();
        let restored_store = plotx_data::MemoryBlockStore::default();
        let restored =
            crate::xlsx::import_xlsx_workbook(&workbook, &restored_store, &codecs).unwrap();
        assert_eq!(restored[0].snapshot.schema.columns[0].id, value.id);
        let reader =
            plotx_data::SnapshotReader::new(&restored[0].snapshot, &restored_store, &codecs)
                .unwrap();
        let batch = reader.read_batch(0, &[]).unwrap();
        let values = &batch.columns[0].1;
        assert_eq!(values.value(0), Some(ScalarValue::Float64(1.0)));
        assert_eq!(values.value(1), Some(ScalarValue::Null));
        assert!(matches!(values.value(2), Some(ScalarValue::Float64(value)) if value.is_nan()));
        assert_eq!(values.value(3), Some(ScalarValue::Float64(f64::INFINITY)));
        assert_eq!(
            values.value(4),
            Some(ScalarValue::Float64(f64::NEG_INFINITY))
        );
    }
}
