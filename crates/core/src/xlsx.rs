//! Conversion between neutral XLSX cells and native typed table snapshots.

use plotx_data::{
    BlockStore, CodecRegistry, ColumnChunk, ColumnSchema, ColumnValues, LogicalType, RowId,
    SnapshotBuilder, TableId, TableSchema, TableSnapshot, UncertaintyRelation, Validity,
};
use plotx_io::xlsx::{XlsxCell, XlsxDiagnostic, XlsxSheet, XlsxValue, XlsxWorkbook};
use serde::{Deserialize, Serialize};

const NANOS_PER_SECOND: i64 = 1_000_000_000;
const NANOS_PER_DAY: i64 = 86_400 * NANOS_PER_SECOND;

#[derive(Debug, thiserror::Error)]
pub enum XlsxImportError {
    #[error(transparent)]
    Data(#[from] plotx_data::DataError),
    #[error("worksheet '{0}' has no tabular cells")]
    EmptySheet(String),
}

pub struct ImportedXlsxSheet {
    pub name: String,
    pub snapshot: TableSnapshot,
    pub diagnostics: Vec<String>,
}

/// PlotX's hidden XLSX metadata contract. It is versioned independently while
/// the project and table envelope remain schema v1.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PlotxXlsxSchemaV1 {
    pub schema_version: u32,
    pub sheets: Vec<PlotxXlsxSheetSchemaV1>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PlotxXlsxSheetSchemaV1 {
    pub name: String,
    pub table_id: TableId,
    pub schema: TableSchema,
    #[serde(default)]
    pub uncertainty: Vec<UncertaintyRelation>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PlotxDelimitedSchemaV1 {
    pub schema_version: u32,
    pub table_id: TableId,
    pub schema: TableSchema,
    #[serde(default)]
    pub uncertainty: Vec<UncertaintyRelation>,
}

pub fn import_delimited_with_schema(
    table: &crate::delimited::DelimitedTable,
    sidecar: &PlotxDelimitedSchemaV1,
    store: &dyn BlockStore,
    codecs: &CodecRegistry,
) -> Result<ImportedXlsxSheet, XlsxImportError> {
    let width = table.rows.first().map_or(0, |row| row.cells.len());
    let headers = table.headers.clone().unwrap_or_else(|| {
        (0..width)
            .map(|column| format!("Column {}", column + 1))
            .collect()
    });
    let rows = std::iter::once(
        headers
            .iter()
            .cloned()
            .map(|value| XlsxCell::value(XlsxValue::Utf8(value)))
            .collect(),
    )
    .chain(table.rows.iter().map(|row| {
        row.cells
            .iter()
            .enumerate()
            .map(|(column, cell)| {
                let logical_type = sidecar
                    .schema
                    .columns
                    .get(column)
                    .map(|column| &column.logical_type);
                XlsxCell::value(delimited_xlsx_value(cell, logical_type))
            })
            .collect()
    }))
    .collect();
    let schema = PlotxXlsxSchemaV1 {
        schema_version: sidecar.schema_version,
        sheets: vec![PlotxXlsxSheetSchemaV1 {
            name: "Data".into(),
            table_id: sidecar.table_id,
            schema: sidecar.schema.clone(),
            uncertainty: sidecar.uncertainty.clone(),
        }],
    };
    let workbook = XlsxWorkbook {
        sheets: vec![XlsxSheet {
            name: "Data".into(),
            hidden: false,
            rows,
        }],
        uses_1904_date_system: false,
        plotx_schema: Some(serde_json::to_value(schema).expect("sidecar schema serializes")),
        diagnostics: Vec::new(),
    };
    import_xlsx_workbook(&workbook, store, codecs)?
        .into_iter()
        .next()
        .ok_or_else(|| XlsxImportError::EmptySheet("Data".into()))
}

fn delimited_xlsx_value(
    cell: &crate::delimited::DelimitedCell,
    logical_type: Option<&LogicalType>,
) -> XlsxValue {
    if matches!(cell.value, crate::delimited::CellValue::Missing) {
        return XlsxValue::Empty;
    }
    let raw = cell.raw.trim();
    match logical_type {
        Some(LogicalType::Boolean) => raw
            .parse()
            .map_or_else(|_| XlsxValue::Utf8(cell.raw.clone()), XlsxValue::Boolean),
        Some(LogicalType::Int64) => raw
            .parse()
            .map_or_else(|_| XlsxValue::Utf8(cell.raw.clone()), XlsxValue::Int64),
        Some(LogicalType::Float64) => match raw {
            "NaN" | "+Inf" | "-Inf" => XlsxValue::Utf8(raw.into()),
            _ => raw
                .parse()
                .map_or_else(|_| XlsxValue::Utf8(cell.raw.clone()), XlsxValue::Float64),
        },
        _ => XlsxValue::Utf8(cell.raw.clone()),
    }
}

pub fn import_xlsx_workbook(
    workbook: &XlsxWorkbook,
    store: &dyn BlockStore,
    codecs: &CodecRegistry,
) -> Result<Vec<ImportedXlsxSheet>, XlsxImportError> {
    let sidecar = workbook
        .plotx_schema
        .as_ref()
        .and_then(|value| serde_json::from_value::<PlotxXlsxSchemaV1>(value.clone()).ok())
        .filter(|schema| schema.schema_version == 1);
    let workbook_diagnostics = workbook
        .diagnostics
        .iter()
        .map(format_xlsx_diagnostic)
        .collect::<Vec<_>>();
    let mut imported = Vec::new();
    for sheet in workbook.sheets.iter().filter(|sheet| !sheet.hidden) {
        let mut diagnostics = workbook_diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.starts_with(&format!("{}!", sheet.name)))
            .cloned()
            .collect::<Vec<_>>();
        let (headers, data_rows) = split_header(sheet, &mut diagnostics)?;
        let sidecar_sheet = sidecar
            .as_ref()
            .and_then(|schema| schema.sheets.iter().find(|entry| entry.name == sheet.name));
        let (table_id, schema, uncertainty) =
            resolve_schema(&headers, data_rows, sidecar_sheet, &mut diagnostics)?;
        let snapshot = build_snapshot(
            table_id,
            schema,
            uncertainty,
            sheet,
            data_rows,
            &diagnostics,
            store,
            codecs,
        )?;
        imported.push(ImportedXlsxSheet {
            name: sheet.name.clone(),
            snapshot,
            diagnostics,
        });
    }
    Ok(imported)
}

fn split_header<'a>(
    sheet: &'a XlsxSheet,
    diagnostics: &mut Vec<String>,
) -> Result<(Vec<String>, &'a [Vec<XlsxCell>]), XlsxImportError> {
    let width = sheet.rows.iter().map(Vec::len).max().unwrap_or(0);
    if width == 0 {
        return Err(XlsxImportError::EmptySheet(sheet.name.clone()));
    }
    let header = sheet.rows.first().is_some_and(|row| {
        row.len() == width
            && row.iter().all(
                |cell| matches!(&cell.value, XlsxValue::Utf8(value) if !value.trim().is_empty()),
            )
    });
    let (headers, rows) = if header {
        let headers = sheet.rows[0]
            .iter()
            .map(|cell| match &cell.value {
                XlsxValue::Utf8(value) => value.clone(),
                _ => unreachable!(),
            })
            .collect::<Vec<_>>();
        (headers, &sheet.rows[1..])
    } else {
        diagnostics
            .push("No unambiguous text header row was found; generated column names.".to_owned());
        (
            (0..width)
                .map(|column| format!("Column {}", column + 1))
                .collect(),
            sheet.rows.as_slice(),
        )
    };
    if rows.is_empty() {
        return Err(XlsxImportError::EmptySheet(sheet.name.clone()));
    }
    Ok((headers, rows))
}

fn resolve_schema(
    headers: &[String],
    rows: &[Vec<XlsxCell>],
    sidecar: Option<&PlotxXlsxSheetSchemaV1>,
    diagnostics: &mut Vec<String>,
) -> plotx_data::Result<(TableId, TableSchema, Vec<UncertaintyRelation>)> {
    if let Some(sidecar) = sidecar {
        let names_match = sidecar.schema.columns.len() == headers.len()
            && sidecar
                .schema
                .columns
                .iter()
                .zip(headers)
                .all(|(column, header)| column.name == *header);
        if names_match {
            sidecar.schema.validate()?;
            return Ok((
                sidecar.table_id,
                sidecar.schema.clone(),
                sidecar.uncertainty.clone(),
            ));
        }
        diagnostics.push(
            "The hidden PlotX schema does not match the visible headers; inferred types instead."
                .to_owned(),
        );
    }
    let columns = headers
        .iter()
        .enumerate()
        .map(|(column, header)| {
            ColumnSchema::new(header, infer_type(rows.iter().map(|row| cell(row, column))))
        })
        .collect();
    Ok((TableId::new(), TableSchema::new(columns)?, Vec::new()))
}

#[allow(clippy::too_many_arguments)]
fn build_snapshot(
    table_id: TableId,
    schema: TableSchema,
    uncertainty: Vec<UncertaintyRelation>,
    sheet: &XlsxSheet,
    rows: &[Vec<XlsxCell>],
    diagnostics: &[String],
    store: &dyn BlockStore,
    codecs: &CodecRegistry,
) -> plotx_data::Result<TableSnapshot> {
    let mut builder = SnapshotBuilder::new(table_id, schema.clone(), store, codecs)?;
    builder.set_uncertainty(uncertainty)?;
    builder.metadata_mut().insert(
        "space.nmrtist.plotx.import.format".into(),
        serde_json::Value::String("xlsx".into()),
    );
    builder.metadata_mut().insert(
        "space.nmrtist.plotx.import.worksheet".into(),
        serde_json::Value::String(sheet.name.clone()),
    );
    builder.metadata_mut().insert(
        "space.nmrtist.plotx.import.diagnostics".into(),
        serde_json::Value::Array(
            diagnostics
                .iter()
                .cloned()
                .map(serde_json::Value::String)
                .collect(),
        ),
    );
    let formulas = rows
        .iter()
        .enumerate()
        .flat_map(|(row, cells)| {
            cells.iter().enumerate().filter_map(move |(column, cell)| {
                cell.formula.as_ref().map(
                    |formula| serde_json::json!({"row": row, "column": column, "formula": formula}),
                )
            })
        })
        .collect::<Vec<_>>();
    builder.metadata_mut().insert(
        "space.nmrtist.plotx.import.xlsx.formulas".into(),
        serde_json::Value::Array(formulas),
    );

    const CHUNK_ROWS: usize = 65_536;
    for batch in rows.chunks(CHUNK_ROWS) {
        let columns = schema
            .columns
            .iter()
            .enumerate()
            .map(|(column, schema)| column_chunk(batch, column, &schema.logical_type))
            .collect::<plotx_data::Result<Vec<_>>>()?;
        let row_ids = (0..batch.len()).map(|_| RowId::new()).collect::<Vec<_>>();
        builder.push_batch(&row_ids, &columns)?;
    }
    builder.finish()
}

fn infer_type<'a>(cells: impl Iterator<Item = &'a XlsxCell>) -> LogicalType {
    let types = cells
        .filter_map(|cell| cell_type(&cell.value))
        .collect::<Vec<_>>();
    if types.is_empty() {
        return LogicalType::Null;
    }
    if types.iter().all(|kind| *kind == types[0]) {
        return types[0].clone();
    }
    if types
        .iter()
        .all(|kind| matches!(kind, LogicalType::Int64 | LogicalType::Float64))
    {
        LogicalType::Float64
    } else {
        LogicalType::Utf8
    }
}

fn cell_type(value: &XlsxValue) -> Option<LogicalType> {
    match value {
        XlsxValue::Empty | XlsxValue::Error(_) => None,
        XlsxValue::Boolean(_) => Some(LogicalType::Boolean),
        XlsxValue::Int64(_) => Some(LogicalType::Int64),
        XlsxValue::Float64(_) => Some(LogicalType::Float64),
        XlsxValue::Utf8(value) | XlsxValue::DateTimeIso(value) => {
            Some(parse_iso(value).map_or(LogicalType::Utf8, |parsed| parsed.logical_type()))
        }
        XlsxValue::DurationIso(value) => {
            Some(parse_iso_duration(value).map_or(LogicalType::Utf8, |_| LogicalType::Duration))
        }
        XlsxValue::ExcelDateTime {
            serial, duration, ..
        } => Some(if *duration {
            LogicalType::Duration
        } else if serial.fract() == 0.0 {
            LogicalType::Date
        } else if (0.0..1.0).contains(serial) {
            LogicalType::Time
        } else {
            LogicalType::Timestamp {
                display_timezone: "UTC".into(),
            }
        }),
    }
}

fn column_chunk(
    rows: &[Vec<XlsxCell>],
    column: usize,
    logical_type: &LogicalType,
) -> plotx_data::Result<ColumnChunk> {
    let values = rows
        .iter()
        .map(|row| scalar_for(cell(row, column), logical_type))
        .collect::<Vec<_>>();
    let validity = Validity::from_valid(values.iter().map(Option::is_some));
    let column_values = match logical_type {
        LogicalType::Null => ColumnValues::Null(rows.len()),
        LogicalType::Boolean => ColumnValues::Boolean(map_values(&values, |v| match v {
            ParsedValue::Boolean(value) => Some(*value),
            _ => None,
        })),
        LogicalType::Int64 => ColumnValues::Int64(map_values(&values, |v| match v {
            ParsedValue::Int64(value) => Some(*value),
            _ => None,
        })),
        LogicalType::Float64 => ColumnValues::Float64(map_values(&values, |v| match v {
            ParsedValue::Int64(value) => Some(*value as f64),
            ParsedValue::Float64(value) => Some(*value),
            _ => None,
        })),
        LogicalType::Utf8 => ColumnValues::Utf8(map_values(&values, |v| match v {
            ParsedValue::Utf8(value) => Some(value.clone()),
            _ => Some(format_parsed(v)),
        })),
        LogicalType::Categorical { levels } => {
            ColumnValues::Categorical(map_values(&values, |v| {
                let text = format_parsed(v);
                levels
                    .iter()
                    .position(|level| level.value == text)
                    .and_then(|index| u32::try_from(index).ok())
            }))
        }
        LogicalType::Date => ColumnValues::Date(map_values(&values, |v| match v {
            ParsedValue::Date(value) => Some(*value),
            _ => None,
        })),
        LogicalType::Time => ColumnValues::Time(map_values(&values, |v| match v {
            ParsedValue::Time(value) => Some(*value),
            _ => None,
        })),
        LogicalType::Timestamp { .. } => {
            ColumnValues::Timestamp(map_values(&values, |v| match v {
                ParsedValue::Timestamp(value) => Some(*value),
                _ => None,
            }))
        }
        LogicalType::Duration => ColumnValues::Duration(map_values(&values, |v| match v {
            ParsedValue::Duration(value) => Some(*value),
            _ => None,
        })),
        LogicalType::Extension(extension) => {
            let storage = column_chunk(rows, column, &extension.storage)?;
            return ColumnChunk::new(
                ColumnValues::Extension {
                    type_id: extension.id.clone(),
                    storage: Box::new(storage.values().clone()),
                },
                validity,
            );
        }
    };
    ColumnChunk::new(column_values, validity)
}

fn map_values<T: Default>(
    values: &[Option<ParsedValue>],
    map: impl Fn(&ParsedValue) -> Option<T>,
) -> Vec<T> {
    values
        .iter()
        .map(|value| value.as_ref().and_then(&map).unwrap_or_default())
        .collect()
}

fn scalar_for(cell: &XlsxCell, logical_type: &LogicalType) -> Option<ParsedValue> {
    if matches!(cell.value, XlsxValue::Empty | XlsxValue::Error(_)) {
        return None;
    }
    let native = native_value(&cell.value)?;
    match logical_type {
        LogicalType::Null => None,
        LogicalType::Utf8 | LogicalType::Categorical { .. } => {
            Some(ParsedValue::Utf8(format_parsed(&native)))
        }
        LogicalType::Boolean if matches!(native, ParsedValue::Boolean(_)) => Some(native),
        LogicalType::Int64 => match native {
            ParsedValue::Int64(_) => Some(native),
            ParsedValue::Float64(value)
                if value.is_finite()
                    && value.fract() == 0.0
                    && value >= i64::MIN as f64
                    && value <= i64::MAX as f64 =>
            {
                Some(ParsedValue::Int64(value as i64))
            }
            _ => None,
        },
        LogicalType::Float64 => match native {
            ParsedValue::Float64(_) => Some(native),
            ParsedValue::Int64(value) => Some(ParsedValue::Float64(value as f64)),
            ParsedValue::Utf8(value) => match value.as_str() {
                "NaN" => Some(ParsedValue::Float64(f64::NAN)),
                "+Inf" => Some(ParsedValue::Float64(f64::INFINITY)),
                "-Inf" => Some(ParsedValue::Float64(f64::NEG_INFINITY)),
                _ => None,
            },
            _ => None,
        },
        LogicalType::Date if matches!(native, ParsedValue::Date(_)) => Some(native),
        LogicalType::Time if matches!(native, ParsedValue::Time(_)) => Some(native),
        LogicalType::Timestamp { .. } if matches!(native, ParsedValue::Timestamp(_)) => {
            Some(native)
        }
        LogicalType::Duration if matches!(native, ParsedValue::Duration(_)) => Some(native),
        LogicalType::Extension(extension) => scalar_for(cell, &extension.storage),
        _ => None,
    }
}

fn native_value(value: &XlsxValue) -> Option<ParsedValue> {
    match value {
        XlsxValue::Empty | XlsxValue::Error(_) => None,
        XlsxValue::Boolean(value) => Some(ParsedValue::Boolean(*value)),
        XlsxValue::Int64(value) => Some(ParsedValue::Int64(*value)),
        XlsxValue::Float64(value) => Some(ParsedValue::Float64(*value)),
        XlsxValue::Utf8(value) | XlsxValue::DateTimeIso(value) => {
            Some(parse_iso(value).unwrap_or_else(|| ParsedValue::Utf8(value.clone())))
        }
        XlsxValue::DurationIso(value) => parse_iso_duration(value)
            .map(ParsedValue::Duration)
            .or_else(|| Some(ParsedValue::Utf8(value.clone()))),
        XlsxValue::ExcelDateTime {
            serial,
            duration,
            iso8601,
        } => Some(excel_serial_value(*serial, *duration, iso8601)),
    }
}

fn cell(row: &[XlsxCell], column: usize) -> &XlsxCell {
    static EMPTY: XlsxCell = XlsxCell {
        value: XlsxValue::Empty,
        formula: None,
    };
    row.get(column).unwrap_or(&EMPTY)
}

#[derive(Clone, Debug)]
enum ParsedValue {
    Boolean(bool),
    Int64(i64),
    Float64(f64),
    Utf8(String),
    Date(i32),
    Time(i64),
    Timestamp(i64),
    Duration(i64),
}

impl ParsedValue {
    fn logical_type(&self) -> LogicalType {
        match self {
            Self::Boolean(_) => LogicalType::Boolean,
            Self::Int64(_) => LogicalType::Int64,
            Self::Float64(_) => LogicalType::Float64,
            Self::Utf8(_) => LogicalType::Utf8,
            Self::Date(_) => LogicalType::Date,
            Self::Time(_) => LogicalType::Time,
            Self::Timestamp(_) => LogicalType::Timestamp {
                display_timezone: "UTC".into(),
            },
            Self::Duration(_) => LogicalType::Duration,
        }
    }
}

fn format_parsed(value: &ParsedValue) -> String {
    match value {
        ParsedValue::Boolean(value) => value.to_string(),
        ParsedValue::Int64(value) => value.to_string(),
        ParsedValue::Float64(value) => value.to_string(),
        ParsedValue::Utf8(value) => value.clone(),
        ParsedValue::Date(value) => value.to_string(),
        ParsedValue::Time(value) | ParsedValue::Timestamp(value) | ParsedValue::Duration(value) => {
            value.to_string()
        }
    }
}

fn excel_serial_value(serial: f64, duration: bool, iso8601: &str) -> ParsedValue {
    if duration {
        return ParsedValue::Duration((serial * NANOS_PER_DAY as f64).round() as i64);
    }
    if serial.fract() == 0.0 {
        parse_iso_date(iso8601.get(..10).unwrap_or(iso8601))
            .map(ParsedValue::Date)
            .unwrap_or_else(|| ParsedValue::Utf8(iso8601.to_owned()))
    } else if (0.0..1.0).contains(&serial) {
        ParsedValue::Time((serial * NANOS_PER_DAY as f64).round() as i64)
    } else {
        parse_iso(iso8601).unwrap_or_else(|| ParsedValue::Utf8(iso8601.to_owned()))
    }
}

fn parse_iso(value: &str) -> Option<ParsedValue> {
    let (date, time) = match value.split_once('T').or_else(|| value.split_once(' ')) {
        Some((date, time)) => (Some(date), Some(time.trim_end_matches('Z'))),
        None if value.contains('-') => (Some(value), None),
        None if value.contains(':') => (None, Some(value)),
        None => return None,
    };
    let days = match date {
        Some(date) => Some(parse_iso_date(date)?),
        None => None,
    };
    let nanos = match time {
        Some(time) => parse_iso_time(time)?,
        None => 0,
    };
    match (days, time) {
        (Some(days), Some(_)) => Some(ParsedValue::Timestamp(
            i64::from(days) * NANOS_PER_DAY + nanos,
        )),
        (Some(days), None) => Some(ParsedValue::Date(days)),
        (None, Some(_)) => Some(ParsedValue::Time(nanos)),
        _ => None,
    }
}

pub(crate) fn parse_iso_date(value: &str) -> Option<i32> {
    let mut parts = value.split('-');
    let year = parts.next()?.parse::<i32>().ok()?;
    let month = parts.next()?.parse::<u32>().ok()?;
    let day = parts.next()?.parse::<u32>().ok()?;
    if parts.next().is_some()
        || !(1..=12).contains(&month)
        || !(1..=days_in_month(year, month)).contains(&day)
    {
        return None;
    }
    Some(days_from_civil(year, month, day))
}

fn days_in_month(year: i32, month: u32) -> u32 {
    match month {
        4 | 6 | 9 | 11 => 30,
        2 if year % 4 == 0 && (year % 100 != 0 || year % 400 == 0) => 29,
        2 => 28,
        _ => 31,
    }
}

pub(crate) fn parse_iso_timestamp_utc(value: &str) -> Option<i64> {
    let value = value.strip_suffix('Z')?;
    let (date, time) = value.split_once('T')?;
    Some(i64::from(parse_iso_date(date)?) * NANOS_PER_DAY + parse_iso_time(time)?)
}

fn parse_iso_time(value: &str) -> Option<i64> {
    let mut parts = value.split(':');
    let hour = parts.next()?.parse::<u32>().ok()?;
    let minute = parts.next()?.parse::<u32>().ok()?;
    let seconds = parts.next()?;
    if parts.next().is_some() || hour > 23 || minute > 59 {
        return None;
    }
    let (second, fraction) = seconds.split_once('.').unwrap_or((seconds, ""));
    let second = second.parse::<u32>().ok()?;
    if second > 59 || fraction.len() > 9 || !fraction.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let fraction = if fraction.is_empty() {
        0
    } else {
        fraction.parse::<i64>().ok()? * 10_i64.pow(9 - fraction.len() as u32)
    };
    Some(
        (i64::from(hour) * 3_600 + i64::from(minute) * 60 + i64::from(second)) * NANOS_PER_SECOND
            + fraction,
    )
}

fn parse_iso_duration(value: &str) -> Option<i64> {
    let value = value.strip_prefix("PT")?;
    let seconds = value.strip_suffix('S')?.parse::<f64>().ok()?;
    seconds
        .is_finite()
        .then_some((seconds * NANOS_PER_SECOND as f64).round() as i64)
}

// Howard Hinnant's proleptic-Gregorian civil-date conversion.
fn days_from_civil(year: i32, month: u32, day: u32) -> i32 {
    let year = year - i32::from(month <= 2);
    let era = if year >= 0 { year } else { year - 399 } / 400;
    let year_of_era = year - era * 400;
    let month = month as i32;
    let day_of_year = (153 * (month + if month > 2 { -3 } else { 9 }) + 2) / 5 + day as i32 - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

fn format_xlsx_diagnostic(diagnostic: &XlsxDiagnostic) -> String {
    let location = diagnostic.sheet.as_deref().unwrap_or("workbook");
    match (diagnostic.row, diagnostic.column) {
        (Some(row), Some(column)) => format!(
            "{location}!R{}C{}: {}",
            row + 1,
            column + 1,
            diagnostic.message
        ),
        _ => format!("{location}: {}", diagnostic.message),
    }
}

#[cfg(test)]
#[path = "xlsx/tests.rs"]
mod tests;
