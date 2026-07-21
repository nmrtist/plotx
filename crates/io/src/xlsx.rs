//! XLSX value/cache transport with an optional hidden PlotX schema channel.
//!
//! PlotX deliberately does not evaluate Excel formulas. Imports retain the
//! formula text and use only the cached value stored in the workbook.

use calamine::{Data, HeaderRow, Reader, SheetVisible, Xlsx, open_workbook};
use rust_xlsxwriter::{Workbook, XlsxError as WriteError};
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use std::path::Path;

pub const PLOTX_SCHEMA_SHEET: &str = "_PlotX_schema_v1";
const PLOTX_SCHEMA_MARKER: &str = "plotx-schema-v1";
const SCHEMA_CHUNK_BYTES: usize = 30_000;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct XlsxWorkbook {
    pub sheets: Vec<XlsxSheet>,
    pub uses_1904_date_system: bool,
    pub plotx_schema: Option<JsonValue>,
    pub diagnostics: Vec<XlsxDiagnostic>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct XlsxSheet {
    pub name: String,
    pub hidden: bool,
    pub rows: Vec<Vec<XlsxCell>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct XlsxCell {
    pub value: XlsxValue,
    pub formula: Option<String>,
}

impl XlsxCell {
    pub fn value(value: XlsxValue) -> Self {
        Self {
            value,
            formula: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum XlsxValue {
    Empty,
    Boolean(bool),
    Int64(i64),
    Float64(f64),
    Utf8(String),
    ExcelDateTime {
        serial: f64,
        duration: bool,
        iso8601: String,
    },
    DateTimeIso(String),
    DurationIso(String),
    Error(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct XlsxDiagnostic {
    pub code: XlsxDiagnosticCode,
    pub sheet: Option<String>,
    pub row: Option<u32>,
    pub column: Option<u32>,
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum XlsxDiagnosticCode {
    FormulaWithoutCachedValue,
    ErrorCell,
    InvalidPlotxSchema,
}

#[derive(Debug, thiserror::Error)]
pub enum XlsxIoError {
    #[error("cannot read XLSX workbook: {0}")]
    Read(#[from] calamine::XlsxError),
    #[error("cannot write XLSX workbook: {0}")]
    Write(#[from] WriteError),
    #[error("XLSX export requires at least one visible worksheet")]
    NoVisibleWorksheet,
    #[error("XLSX export with the 1904 date system is not supported")]
    Unsupported1904DateSystem,
    #[error("worksheet row or column exceeds the XLSX format limit")]
    SheetTooLarge,
}

pub fn read_xlsx(path: impl AsRef<Path>) -> Result<XlsxWorkbook, XlsxIoError> {
    let mut reader: Xlsx<_> = open_workbook(path)?;
    reader.with_header_row(HeaderRow::Row(0));
    let uses_1904_date_system = reader.has_1904_epoch();
    let metadata = reader.sheets_metadata().to_vec();
    let mut sheets = Vec::new();
    let mut plotx_schema = None;
    let mut diagnostics = Vec::new();

    for metadata in metadata {
        let values = reader.worksheet_range(&metadata.name)?;
        let formulas = reader.worksheet_formula(&metadata.name)?;
        if metadata.name == PLOTX_SCHEMA_SHEET {
            plotx_schema = read_schema(&values, &mut diagnostics);
            continue;
        }
        let start = union_start(values.start(), formulas.start());
        let end = union_end(values.end(), formulas.end());
        let rows = match (start, end) {
            (Some(start), Some(end)) => read_rows(
                &metadata.name,
                start,
                end,
                &values,
                &formulas,
                &mut diagnostics,
            ),
            _ => Vec::new(),
        };
        sheets.push(XlsxSheet {
            name: metadata.name,
            hidden: metadata.visible != SheetVisible::Visible,
            rows,
        });
    }

    Ok(XlsxWorkbook {
        sheets,
        uses_1904_date_system,
        plotx_schema,
        diagnostics,
    })
}

pub fn write_xlsx(path: impl AsRef<Path>, workbook: &XlsxWorkbook) -> Result<(), XlsxIoError> {
    if workbook.uses_1904_date_system {
        return Err(XlsxIoError::Unsupported1904DateSystem);
    }
    if !workbook.sheets.iter().any(|sheet| !sheet.hidden) {
        return Err(XlsxIoError::NoVisibleWorksheet);
    }

    let mut output = StreamingXlsxWriter::new();
    for sheet in &workbook.sheets {
        let sheet_index = output.add_worksheet(&sheet.name, sheet.hidden)?;
        for (row_index, row) in sheet.rows.iter().enumerate() {
            let row_index = u32::try_from(row_index).map_err(|_| XlsxIoError::SheetTooLarge)?;
            output.write_row(sheet_index, row_index, row.iter().map(|cell| &cell.value))?;
        }
    }
    output.finish(path, workbook.plotx_schema.as_ref())
}

/// Backend-neutral, row-at-a-time XLSX writer. Callers never receive a
/// `rust_xlsxwriter` type and can therefore stream large typed snapshots.
pub struct StreamingXlsxWriter {
    output: Workbook,
    visible_sheets: usize,
}

impl StreamingXlsxWriter {
    pub fn new() -> Self {
        Self {
            output: Workbook::new(),
            visible_sheets: 0,
        }
    }

    pub fn add_worksheet(&mut self, name: &str, hidden: bool) -> Result<usize, XlsxIoError> {
        let index = self.output.worksheets().len();
        let worksheet = self.output.add_worksheet_with_constant_memory();
        worksheet.set_name(name)?;
        worksheet.set_hidden(hidden);
        if !hidden {
            self.visible_sheets += 1;
        }
        Ok(index)
    }

    pub fn write_row<'a>(
        &mut self,
        sheet: usize,
        row: u32,
        values: impl IntoIterator<Item = &'a XlsxValue>,
    ) -> Result<(), XlsxIoError> {
        let worksheet = self.output.worksheet_from_index(sheet)?;
        for (column, value) in values.into_iter().enumerate() {
            let column = u16::try_from(column).map_err(|_| XlsxIoError::SheetTooLarge)?;
            write_cell(worksheet, row, column, value)?;
        }
        Ok(())
    }

    pub fn finish(
        mut self,
        path: impl AsRef<Path>,
        plotx_schema: Option<&JsonValue>,
    ) -> Result<(), XlsxIoError> {
        if self.visible_sheets == 0 {
            return Err(XlsxIoError::NoVisibleWorksheet);
        }
        if let Some(schema) = plotx_schema {
            let worksheet = self.output.add_worksheet();
            worksheet.set_name(PLOTX_SCHEMA_SHEET)?;
            worksheet.set_hidden(true);
            worksheet.write_string(0, 0, PLOTX_SCHEMA_MARKER)?;
            let encoded = serde_json::to_string(schema).expect("JSON values always serialize");
            for (index, chunk) in utf8_chunks(&encoded, SCHEMA_CHUNK_BYTES).enumerate() {
                let row = u32::try_from(index + 1).map_err(|_| XlsxIoError::SheetTooLarge)?;
                worksheet.write_string(row, 0, chunk)?;
            }
        }
        self.output.save(path)?;
        Ok(())
    }
}

impl Default for StreamingXlsxWriter {
    fn default() -> Self {
        Self::new()
    }
}

fn read_rows(
    sheet: &str,
    start: (u32, u32),
    end: (u32, u32),
    values: &calamine::Range<Data>,
    formulas: &calamine::Range<String>,
    diagnostics: &mut Vec<XlsxDiagnostic>,
) -> Vec<Vec<XlsxCell>> {
    (start.0..=end.0)
        .map(|row| {
            (start.1..=end.1)
                .map(|column| {
                    let value = values.get_value((row, column)).unwrap_or(&Data::Empty);
                    let formula = formulas
                        .get_value((row, column))
                        .filter(|value| !value.is_empty())
                        .cloned();
                    if formula.is_some() && matches!(value, Data::Empty) {
                        diagnostics.push(cell_diagnostic(
                            XlsxDiagnosticCode::FormulaWithoutCachedValue,
                            sheet,
                            row,
                            column,
                            "formula has no cached value; imported as null",
                        ));
                    }
                    if let Data::Error(error) = value {
                        diagnostics.push(cell_diagnostic(
                            XlsxDiagnosticCode::ErrorCell,
                            sheet,
                            row,
                            column,
                            format!("Excel error cell {error}"),
                        ));
                    }
                    XlsxCell {
                        value: convert_value(value),
                        formula,
                    }
                })
                .collect()
        })
        .collect()
}

fn convert_value(value: &Data) -> XlsxValue {
    match value {
        Data::Empty => XlsxValue::Empty,
        Data::Bool(value) => XlsxValue::Boolean(*value),
        Data::Int(value) => XlsxValue::Int64(*value),
        Data::Float(value) => XlsxValue::Float64(*value),
        Data::String(value) => XlsxValue::Utf8(value.clone()),
        Data::DateTime(value) => {
            let (year, month, day, hour, minute, second, millis) = value.to_ymd_hms_milli();
            XlsxValue::ExcelDateTime {
                serial: value.as_f64(),
                duration: value.is_duration(),
                iso8601: format!(
                    "{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}.{millis:03}"
                ),
            }
        }
        Data::DateTimeIso(value) => XlsxValue::DateTimeIso(value.clone()),
        Data::DurationIso(value) => XlsxValue::DurationIso(value.clone()),
        Data::Error(value) => XlsxValue::Error(value.to_string()),
    }
}

fn write_cell(
    worksheet: &mut rust_xlsxwriter::Worksheet,
    row: u32,
    column: u16,
    value: &XlsxValue,
) -> Result<(), WriteError> {
    match value {
        XlsxValue::Empty | XlsxValue::Error(_) => {}
        XlsxValue::Boolean(value) => {
            worksheet.write_boolean(row, column, *value)?;
        }
        XlsxValue::Int64(value) => {
            worksheet.write_number(row, column, *value as f64)?;
        }
        XlsxValue::Float64(value) | XlsxValue::ExcelDateTime { serial: value, .. } => {
            worksheet.write_number(row, column, *value)?;
        }
        XlsxValue::Utf8(value) | XlsxValue::DateTimeIso(value) | XlsxValue::DurationIso(value) => {
            worksheet.write_string(row, column, value)?;
        }
    }
    Ok(())
}

fn read_schema(
    values: &calamine::Range<Data>,
    diagnostics: &mut Vec<XlsxDiagnostic>,
) -> Option<JsonValue> {
    let marker = values.get_value((0, 0));
    if marker != Some(&Data::String(PLOTX_SCHEMA_MARKER.to_owned())) {
        diagnostics.push(XlsxDiagnostic {
            code: XlsxDiagnosticCode::InvalidPlotxSchema,
            sheet: Some(PLOTX_SCHEMA_SHEET.to_owned()),
            row: Some(0),
            column: Some(0),
            message: "hidden PlotX schema sheet has an invalid marker".to_owned(),
        });
        return None;
    }
    let mut encoded = String::new();
    let end_row = values.end().map_or(0, |end| end.0);
    for row in 1..=end_row {
        if let Some(Data::String(chunk)) = values.get_value((row, 0)) {
            encoded.push_str(chunk);
        }
    }
    match serde_json::from_str(&encoded) {
        Ok(schema) => Some(schema),
        Err(error) => {
            diagnostics.push(XlsxDiagnostic {
                code: XlsxDiagnosticCode::InvalidPlotxSchema,
                sheet: Some(PLOTX_SCHEMA_SHEET.to_owned()),
                row: None,
                column: None,
                message: format!("hidden PlotX schema JSON is invalid: {error}"),
            });
            None
        }
    }
}

fn cell_diagnostic(
    code: XlsxDiagnosticCode,
    sheet: &str,
    row: u32,
    column: u32,
    message: impl Into<String>,
) -> XlsxDiagnostic {
    XlsxDiagnostic {
        code,
        sheet: Some(sheet.to_owned()),
        row: Some(row),
        column: Some(column),
        message: message.into(),
    }
}

fn union_start(left: Option<(u32, u32)>, right: Option<(u32, u32)>) -> Option<(u32, u32)> {
    match (left, right) {
        (Some(left), Some(right)) => Some((left.0.min(right.0), left.1.min(right.1))),
        (left, right) => left.or(right),
    }
}

fn union_end(left: Option<(u32, u32)>, right: Option<(u32, u32)>) -> Option<(u32, u32)> {
    match (left, right) {
        (Some(left), Some(right)) => Some((left.0.max(right.0), left.1.max(right.1))),
        (left, right) => left.or(right),
    }
}

fn utf8_chunks(value: &str, max_bytes: usize) -> impl Iterator<Item = &str> {
    let mut start = 0;
    std::iter::from_fn(move || {
        if start >= value.len() {
            return None;
        }
        let mut end = (start + max_bytes).min(value.len());
        while !value.is_char_boundary(end) {
            end -= 1;
        }
        let chunk = &value[start..end];
        start = end;
        Some(chunk)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_xlsxwriter::Formula;
    use serde_json::json;
    use std::io::{Read, Write};

    #[test]
    fn round_trips_values_and_hidden_schema() {
        let path = std::env::temp_dir().join(format!(
            "plotx-xlsx-{}-{}.xlsx",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let workbook = XlsxWorkbook {
            sheets: vec![XlsxSheet {
                name: "Data".to_owned(),
                hidden: false,
                rows: vec![
                    vec![
                        XlsxCell::value(XlsxValue::Utf8("sample".to_owned())),
                        XlsxCell::value(XlsxValue::Utf8("value".to_owned())),
                    ],
                    vec![
                        XlsxCell::value(XlsxValue::Utf8("α".to_owned())),
                        XlsxCell::value(XlsxValue::Float64(2.5)),
                    ],
                ],
            }],
            uses_1904_date_system: false,
            plotx_schema: Some(json!({"schema_version": 1, "note": "光谱"})),
            diagnostics: Vec::new(),
        };
        write_xlsx(&path, &workbook).unwrap();
        let restored = read_xlsx(&path).unwrap();
        std::fs::remove_file(&path).unwrap();

        assert_eq!(restored.sheets, workbook.sheets);
        assert_eq!(restored.plotx_schema, workbook.plotx_schema);
        assert!(restored.diagnostics.is_empty());
    }

    #[test]
    fn round_trips_multiple_visible_worksheets() {
        let path = temporary_xlsx_path("multiple-sheets");
        let workbook = XlsxWorkbook {
            sheets: vec![
                XlsxSheet {
                    name: "Samples".to_owned(),
                    hidden: false,
                    rows: vec![vec![XlsxCell::value(XlsxValue::Utf8("sample".into()))]],
                },
                XlsxSheet {
                    name: "Measurements".to_owned(),
                    hidden: false,
                    rows: vec![vec![XlsxCell::value(XlsxValue::Float64(2.5))]],
                },
            ],
            uses_1904_date_system: false,
            plotx_schema: None,
            diagnostics: Vec::new(),
        };
        write_xlsx(&path, &workbook).unwrap();
        let restored = read_xlsx(&path).unwrap();
        std::fs::remove_file(path).unwrap();

        assert_eq!(restored.sheets, workbook.sheets);
    }

    #[test]
    fn error_cells_are_explicit_and_diagnosed() {
        let mut values = calamine::Range::new((0, 0), (0, 0));
        values.set_value((0, 0), Data::Error(calamine::CellErrorType::Div0));
        let formulas = calamine::Range::empty();
        let mut diagnostics = Vec::new();

        let rows = read_rows(
            "Errors",
            (0, 0),
            (0, 0),
            &values,
            &formulas,
            &mut diagnostics,
        );

        assert!(matches!(rows[0][0].value, XlsxValue::Error(_)));
        assert_eq!(diagnostics[0].code, XlsxDiagnosticCode::ErrorCell);
        assert_eq!(diagnostics[0].sheet.as_deref(), Some("Errors"));
    }

    #[test]
    fn export_rejects_the_unsupported_1904_date_system() {
        let path = temporary_xlsx_path("1904-date-system");
        let workbook = XlsxWorkbook {
            sheets: vec![XlsxSheet {
                name: "Data".to_owned(),
                hidden: false,
                rows: vec![vec![XlsxCell::value(XlsxValue::Int64(1))]],
            }],
            uses_1904_date_system: true,
            plotx_schema: None,
            diagnostics: Vec::new(),
        };

        assert!(matches!(
            write_xlsx(&path, &workbook),
            Err(XlsxIoError::Unsupported1904DateSystem)
        ));
        assert!(!path.exists());
    }

    #[test]
    fn utf8_schema_chunks_do_not_split_code_points() {
        let input = "光".repeat(20_001);
        let chunks = utf8_chunks(&input, SCHEMA_CHUNK_BYTES).collect::<Vec<_>>();
        assert!(chunks.iter().all(|chunk| chunk.len() <= SCHEMA_CHUNK_BYTES));
        assert_eq!(chunks.concat(), input);
    }

    #[test]
    fn formulas_keep_text_and_missing_cache_becomes_a_diagnostic() {
        let path = temporary_xlsx_path("formula-cache");
        let mut workbook = Workbook::new();
        let worksheet = workbook.add_worksheet();
        worksheet.set_name("Calculations").unwrap();
        worksheet
            .write_formula(0, 0, Formula::new("=1+1").set_result(""))
            .unwrap();
        worksheet
            .write_formula(1, 0, Formula::new("=2+2").set_result("4"))
            .unwrap();
        workbook.save(&path).unwrap();
        let path_without_cache = remove_first_formula_cache(&path);

        let restored = read_xlsx(&path_without_cache).unwrap();
        std::fs::remove_file(path).unwrap();
        std::fs::remove_file(path_without_cache).unwrap();
        assert!(restored.sheets[0].rows[0][0].formula.is_some());
        assert_eq!(restored.sheets[0].rows[0][0].value, XlsxValue::Empty);
        assert_eq!(restored.sheets[0].rows[1][0].value, XlsxValue::Float64(4.0));
        assert!(restored.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == XlsxDiagnosticCode::FormulaWithoutCachedValue
        }));
    }

    fn temporary_xlsx_path(label: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "plotx-{label}-{}-{}.xlsx",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ))
    }

    fn remove_first_formula_cache(path: &Path) -> std::path::PathBuf {
        let output = path.with_file_name(format!(
            "{}-no-cache.xlsx",
            path.file_stem().unwrap().to_string_lossy()
        ));
        let file = std::fs::File::open(path).unwrap();
        let mut archive = zip::ZipArchive::new(file).unwrap();
        let mut entries = Vec::new();
        for index in 0..archive.len() {
            let mut entry = archive.by_index(index).unwrap();
            let mut bytes = Vec::new();
            entry.read_to_end(&mut bytes).unwrap();
            entries.push((entry.name().to_owned(), entry.is_dir(), bytes));
        }
        let file = std::fs::File::create(&output).unwrap();
        let mut writer = zip::ZipWriter::new(file);
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);
        for (name, directory, mut bytes) in entries {
            if name == "xl/worksheets/sheet1.xml" {
                let xml = String::from_utf8(bytes).unwrap();
                bytes = xml.replacen("<v>0</v>", "", 1).into_bytes();
            }
            if directory {
                writer.add_directory(name, options).unwrap();
            } else {
                writer.start_file(name, options).unwrap();
                writer.write_all(&bytes).unwrap();
            }
        }
        writer.finish().unwrap();
        output
    }
}
