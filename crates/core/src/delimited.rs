//! UI-independent parsing and normalization for delimited scientific tables.
//!
//! Parsing and typed snapshot materialization are deliberately separate stages
//! so an importer can inspect [`DelimitedTable`] without duplicating CSV rules
//! or committing data to the document.

use thiserror::Error;

mod typed;

pub use plotx_io::delimited::Delimiter;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum HeaderMode {
    #[default]
    Auto,
    Present,
    Absent,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ParseOptions {
    /// `None` performs strict auto-detection across comma, tab, and semicolon.
    pub delimiter: Option<Delimiter>,
    pub header: HeaderMode,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CellValue {
    Missing,
    Number(f64),
    Text,
}

/// A cell retains its source text as well as its normalized interpretation.
/// Surrounding whitespace is ignored only when deciding missing/numeric status.
#[derive(Debug, Clone, PartialEq)]
pub struct DelimitedCell {
    pub raw: String,
    pub value: CellValue,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DelimitedRow {
    /// One-based physical source line on which this record starts.
    pub source_line: usize,
    pub cells: Vec<DelimitedCell>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiagnosticLevel {
    Info,
    Warning,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DelimitedDiagnostic {
    pub level: DiagnosticLevel,
    pub message: String,
}

/// Parsed rectangular data with header intent resolved, but without assuming
/// which columns should be plotted.
#[derive(Debug, Clone, PartialEq)]
pub struct DelimitedTable {
    pub delimiter: Delimiter,
    pub headers: Option<Vec<String>>,
    pub rows: Vec<DelimitedRow>,
    pub diagnostics: Vec<DelimitedDiagnostic>,
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum DelimitedError {
    #[error("the table is empty")]
    EmptyInput,
    #[error("could not detect comma, tab, or semicolon as a rectangular table delimiter")]
    DelimiterNotDetected,
    #[error("delimiter is ambiguous between {candidates}; choose it explicitly")]
    AmbiguousDelimiter { candidates: String },
    #[error("row {row} has {actual} columns; expected {expected}")]
    RaggedRow {
        row: usize,
        expected: usize,
        actual: usize,
    },
    #[error("row {row}, column {column}: unterminated quoted field")]
    UnterminatedQuote { row: usize, column: usize },
    #[error("row {row}, column {column}: quote appears inside an unquoted field")]
    UnexpectedQuote { row: usize, column: usize },
    #[error("row {row}, column {column}: unexpected text after a closing quote")]
    TextAfterClosingQuote { row: usize, column: usize },
    #[error("the table must contain at least two columns")]
    TooFewColumns,
    #[error("the header consumes the only row; no data rows remain")]
    NoDataRows,
    #[error("row 1 mixes numeric and text cells, so header presence is ambiguous")]
    AmbiguousHeader,
}

pub fn parse_delimited(
    input: &str,
    options: ParseOptions,
) -> Result<DelimitedTable, DelimitedError> {
    let input = input.strip_prefix('\u{feff}').unwrap_or(input);
    if input.trim().is_empty() {
        return Err(DelimitedError::EmptyInput);
    }

    let (delimiter, raw_rows) = match options.delimiter {
        Some(delimiter) => (delimiter, rectangular_rows(input, delimiter)?),
        None => detect_delimiter(input)?,
    };
    if raw_rows.first().map_or(0, |(_, row)| row.len()) < 2 {
        return Err(DelimitedError::TooFewColumns);
    }

    let first_kinds = raw_rows[0]
        .1
        .iter()
        .enumerate()
        .map(|(column, raw)| classify(raw, raw_rows[0].0, column + 1))
        .collect::<Result<Vec<_>, _>>()?;
    let header_present = match options.header {
        HeaderMode::Present => true,
        HeaderMode::Absent => false,
        HeaderMode::Auto => {
            let numeric = first_kinds
                .iter()
                .filter(|kind| matches!(kind, CellValue::Number(_)))
                .count();
            let text = first_kinds
                .iter()
                .filter(|kind| matches!(kind, CellValue::Text))
                .count();
            match (numeric > 0, text > 0) {
                (true, true) => return Err(DelimitedError::AmbiguousHeader),
                (false, true) => true,
                (true, false) => false,
                (false, false) => return Err(DelimitedError::EmptyInput),
            }
        }
    };

    let (headers, data_rows) = if header_present {
        if raw_rows.len() == 1 {
            return Err(DelimitedError::NoDataRows);
        }
        (Some(raw_rows[0].1.clone()), &raw_rows[1..])
    } else {
        (None, raw_rows.as_slice())
    };

    let rows = data_rows
        .iter()
        .map(|(source_line, fields)| {
            let cells = fields
                .iter()
                .enumerate()
                .map(|(column, raw)| {
                    Ok(DelimitedCell {
                        raw: raw.clone(),
                        value: classify(raw, *source_line, column + 1)?,
                    })
                })
                .collect::<Result<Vec<_>, DelimitedError>>()?;
            Ok(DelimitedRow {
                source_line: *source_line,
                cells,
            })
        })
        .collect::<Result<Vec<_>, DelimitedError>>()?;

    let diagnostics = if options.header == HeaderMode::Auto {
        vec![DelimitedDiagnostic {
            level: DiagnosticLevel::Info,
            message: if header_present {
                "Detected a header row.".to_owned()
            } else {
                "Detected a headerless table; generated column names.".to_owned()
            },
        }]
    } else {
        Vec::new()
    };

    Ok(DelimitedTable {
        delimiter,
        headers,
        rows,
        diagnostics,
    })
}

fn classify(raw: &str, _row: usize, _column: usize) -> Result<CellValue, DelimitedError> {
    let value = raw.trim();
    if value.is_empty() {
        return Ok(CellValue::Missing);
    }
    match value.parse::<f64>() {
        Ok(number) => Ok(CellValue::Number(number)),
        Err(_) => Ok(CellValue::Text),
    }
}

/// Parsed rows as `(source_line, cells)` pairs.
type SourceRows = Vec<(usize, Vec<String>)>;

fn detect_delimiter(input: &str) -> Result<(Delimiter, SourceRows), DelimitedError> {
    let mut candidates = Delimiter::ALL
        .into_iter()
        .filter_map(|delimiter| {
            rectangular_rows(input, delimiter)
                .ok()
                .filter(|rows| rows.first().is_some_and(|(_, row)| row.len() >= 2))
                .map(|rows| (delimiter, rows))
        })
        .collect::<Vec<_>>();
    let Some(max_width) = candidates
        .iter()
        .filter_map(|(_, rows)| rows.first().map(|(_, row)| row.len()))
        .max()
    else {
        return Err(DelimitedError::DelimiterNotDetected);
    };
    candidates.retain(|(_, rows)| rows[0].1.len() == max_width);
    if candidates.len() > 1 {
        return Err(DelimitedError::AmbiguousDelimiter {
            candidates: candidates
                .iter()
                .map(|(delimiter, _)| delimiter.to_string())
                .collect::<Vec<_>>()
                .join(", "),
        });
    }
    Ok(candidates.remove(0))
}

fn rectangular_rows(input: &str, delimiter: Delimiter) -> Result<SourceRows, DelimitedError> {
    let rows = parse_rows(input, delimiter)?;
    let expected = rows.first().map_or(0, |(_, row)| row.len());
    for (source_line, row) in &rows {
        if row.len() != expected {
            return Err(DelimitedError::RaggedRow {
                row: *source_line,
                expected,
                actual: row.len(),
            });
        }
    }
    Ok(rows)
}

fn parse_rows(input: &str, delimiter: Delimiter) -> Result<SourceRows, DelimitedError> {
    let delimiter = delimiter.as_char();
    let mut chars = input.chars().peekable();
    let mut rows = Vec::new();
    let mut row = Vec::new();
    let mut field = String::new();
    let mut source_line = 1usize;
    let mut record_line = 1usize;
    let mut in_quotes = false;
    let mut closed_quote = false;

    while let Some(ch) = chars.next() {
        if in_quotes {
            if ch == '"' {
                if chars.peek() == Some(&'"') {
                    chars.next();
                    field.push('"');
                } else {
                    in_quotes = false;
                    closed_quote = true;
                }
            } else {
                if ch == '\n' {
                    source_line += 1;
                }
                field.push(ch);
            }
            continue;
        }

        if closed_quote {
            if ch == delimiter {
                row.push(std::mem::take(&mut field));
                closed_quote = false;
            } else if ch == '\n' || ch == '\r' {
                if ch == '\r' && chars.peek() == Some(&'\n') {
                    chars.next();
                }
                row.push(std::mem::take(&mut field));
                rows.push((record_line, std::mem::take(&mut row)));
                source_line += 1;
                record_line = source_line;
                closed_quote = false;
            } else {
                return Err(DelimitedError::TextAfterClosingQuote {
                    row: source_line,
                    column: row.len() + 1,
                });
            }
            continue;
        }

        if ch == delimiter {
            row.push(std::mem::take(&mut field));
        } else if ch == '\n' || ch == '\r' {
            if ch == '\r' && chars.peek() == Some(&'\n') {
                chars.next();
            }
            row.push(std::mem::take(&mut field));
            rows.push((record_line, std::mem::take(&mut row)));
            source_line += 1;
            record_line = source_line;
        } else if ch == '"' {
            if field.is_empty() {
                in_quotes = true;
            } else {
                return Err(DelimitedError::UnexpectedQuote {
                    row: source_line,
                    column: row.len() + 1,
                });
            }
        } else {
            field.push(ch);
        }
    }

    if in_quotes {
        return Err(DelimitedError::UnterminatedQuote {
            row: source_line,
            column: row.len() + 1,
        });
    }
    if closed_quote || !field.is_empty() || !row.is_empty() {
        row.push(field);
        rows.push((record_line, row));
    }
    if rows.is_empty() {
        return Err(DelimitedError::EmptyInput);
    }
    Ok(rows)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn imports_quoted_csv_header_and_preserves_missing_cells() {
        let parsed = parse_delimited(
            "\u{feff}\"time, seconds\",signal,note\n0,10,first\n1,,second\n2,4,third\n",
            ParseOptions::default(),
        )
        .unwrap();
        assert_eq!(parsed.delimiter, Delimiter::Comma);
        assert_eq!(parsed.headers.as_ref().unwrap()[0], "time, seconds");
        assert!(matches!(parsed.rows[1].cells[1].value, CellValue::Missing));
        assert_eq!(parsed.rows[2].cells[2].raw, "third");
    }

    #[test]
    fn imports_headerless_tsv_with_generated_names() {
        let parsed = parse_delimited("0\t1\n1\t2\n", ParseOptions::default()).unwrap();
        assert_eq!(parsed.delimiter, Delimiter::Tab);
        assert!(parsed.headers.is_none());
        assert_eq!(parsed.rows.len(), 2);
    }

    #[test]
    fn detects_semicolon_tables() {
        let parsed = parse_delimited("x;y\n0;1\n1;2", ParseOptions::default()).unwrap();
        assert_eq!(parsed.delimiter, Delimiter::Semicolon);
    }

    #[test]
    fn refuses_ambiguous_delimiters() {
        let error = parse_delimited("a,b;c\n1,2;3", ParseOptions::default()).unwrap_err();
        assert!(matches!(error, DelimitedError::AmbiguousDelimiter { .. }));
    }

    #[test]
    fn preserves_mixed_numeric_and_text_columns_for_typed_inference() {
        let parsed = parse_delimited("x,y\n0,1\n1,oops", ParseOptions::default()).unwrap();
        assert!(matches!(
            parsed.rows[0].cells[1].value,
            CellValue::Number(1.0)
        ));
        assert!(matches!(parsed.rows[1].cells[1].value, CellValue::Text));
    }

    #[test]
    fn typed_import_preserves_text_columns_and_explicit_nulls() {
        let parsed = parse_delimited(
            "sample,value,note\na,1,ok\nb,,review\n",
            ParseOptions::default(),
        )
        .unwrap();
        let store = plotx_data::MemoryBlockStore::default();
        let codecs = plotx_data::CodecRegistry::with_arrow_ipc();
        let snapshot = parsed
            .into_typed_snapshot(plotx_data::TableId::new(), &store, &codecs)
            .unwrap();
        assert_eq!(snapshot.schema.columns.len(), 3);
        assert_eq!(
            snapshot.schema.columns[0].logical_type,
            plotx_data::LogicalType::Utf8
        );
        let batch = plotx_data::SnapshotReader::new(&snapshot, &store, &codecs)
            .unwrap()
            .read_batch(0, &[])
            .unwrap();
        assert_eq!(
            batch.columns[1].1.value(1),
            Some(plotx_data::ScalarValue::Null)
        );
        assert_eq!(
            batch.columns[2].1.value(0),
            Some(plotx_data::ScalarValue::Utf8("ok".into()))
        );
    }

    #[test]
    fn typed_import_records_symmetric_uncertainty_relation() {
        let parsed = parse_delimited(
            "x,signal,signal_sigma\n0,10,1\n1,20,2\n",
            ParseOptions::default(),
        )
        .unwrap();
        let store = plotx_data::MemoryBlockStore::default();
        let codecs = plotx_data::CodecRegistry::with_arrow_ipc();
        let snapshot = parsed
            .into_typed_snapshot(plotx_data::TableId::new(), &store, &codecs)
            .unwrap();
        assert_eq!(snapshot.uncertainty.len(), 1);
        let relation = &snapshot.uncertainty[0];
        assert_eq!(relation.value, snapshot.schema.columns[1].id);
        let plotx_data::UncertaintyKind::Symmetric { column, .. } = &relation.kind else {
            panic!("expected symmetric uncertainty");
        };
        assert_eq!(*column, snapshot.schema.columns[2].id);
    }
}
