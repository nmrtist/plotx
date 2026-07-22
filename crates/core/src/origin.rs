//! Conversion from the bounded Origin transport model to PlotX tables.

use std::{collections::BTreeMap, mem::size_of};

use plotx_data::{
    BlockStore, CodecRegistry, ColumnChunk, ColumnSchema, ColumnValues, LogicalType, RowId,
    SnapshotBuilder, TableId, TableSchema, TableSnapshot, Validity,
};
use plotx_io::origin::{
    OriginCell, OriginColumn, OriginColumnType, OriginDiagnostic, OriginDiagnosticCode,
    OriginDiagnosticSeverity, OriginError, OriginLimits, OriginMetadataEntry, OriginNote,
    OriginProject, OriginResourceUsage, OriginUnsupportedObjectSummary, OriginWorksheet,
};
use serde_json::{Map, Value};

mod names;
mod preflight;

/// Stable operation identifier stored in revisions created from Origin imports.
pub const ORIGIN_IMPORT_OPERATION: &str = "plotx.import.origin.v1";

const CHUNK_ROWS: usize = 65_536;

const FORMAT_KEY: &str = "space.nmrtist.plotx.import.format";
const VERSION_KEY: &str = "space.nmrtist.plotx.import.origin.producer_version";
const WORKBOOK_KEY: &str = "space.nmrtist.plotx.import.origin.workbook";
const WORKSHEET_KEY: &str = "space.nmrtist.plotx.import.origin.worksheet";
const PARAMETERS_KEY: &str = "space.nmrtist.plotx.import.origin.parameters";
const NOTES_KEY: &str = "space.nmrtist.plotx.import.origin.notes";
const DIAGNOSTICS_KEY: &str = "space.nmrtist.plotx.import.origin.diagnostics";
const UNSUPPORTED_KEY: &str = "space.nmrtist.plotx.import.origin.unsupported_objects";
const USAGE_KEY: &str = "space.nmrtist.plotx.import.origin.resource_usage";
const COLUMNS_KEY: &str = "space.nmrtist.plotx.import.origin.columns";
const WORKSHEET_METADATA_KEY: &str = "space.nmrtist.plotx.import.origin.worksheet_metadata";
const ORIGINAL_NAME_KEY: &str = "space.nmrtist.plotx.import.origin.original_name";
const LONG_NAME_KEY: &str = "space.nmrtist.plotx.import.origin.long_name";
const ROLE_KEY: &str = "space.nmrtist.plotx.import.origin.role";
const UNITS_KEY: &str = "space.nmrtist.plotx.import.origin.units";
const COMMENTS_KEY: &str = "space.nmrtist.plotx.import.origin.comments";

/// One worksheet ready for the application's existing import-preview flow.
#[derive(Debug)]
pub struct ImportedOriginWorksheet {
    /// Human-readable candidate label preserving workbook and worksheet identity.
    pub name: String,
    /// Typed, validity-aware table snapshot.
    pub snapshot: TableSnapshot,
    /// Bounded metadata suitable for direct attachment to `TableImportSource`.
    pub source_metadata: BTreeMap<String, Value>,
    /// Recoverable parser diagnostics retained for the operation report.
    pub diagnostics: Vec<OriginDiagnostic>,
    /// Shared cumulative parser and conversion allocation estimate.
    pub resource_usage: OriginResourceUsage,
}

struct PreparedOriginWorksheet {
    name: String,
    worksheet: OriginWorksheet,
    schema: TableSchema,
    source_metadata: BTreeMap<String, Value>,
    snapshot_metadata: BTreeMap<String, Value>,
    diagnostics: Vec<OriginDiagnostic>,
    resource_usage: OriginResourceUsage,
}

/// Errors raised while validating or converting an engine-neutral Origin model.
#[derive(Debug, thiserror::Error)]
pub enum OriginImportError {
    #[error(transparent)]
    Origin(#[from] OriginError),

    #[error(transparent)]
    Data(#[from] plotx_data::DataError),

    #[error("the Origin project contains no supported worksheet data")]
    NoSupportedWorksheet,

    #[error("invalid Origin project model: {detail}")]
    InvalidModel { detail: String },

    #[error(
        "Origin column {column:?} row {row} contains {actual}, but its declared type is {expected:?}"
    )]
    InvalidCellType {
        column: String,
        row: usize,
        expected: OriginColumnType,
        actual: &'static str,
    },

    #[error("Origin table conversion size calculation overflowed for {resource}")]
    ArithmeticOverflow { resource: &'static str },

    #[error(
        "Origin table conversion {resource} is {actual}, exceeding the configured limit of {limit}"
    )]
    LimitExceeded {
        resource: &'static str,
        limit: usize,
        actual: usize,
    },

    #[error("Origin table conversion could not reserve {requested} bytes for {resource}")]
    AllocationFailed {
        resource: &'static str,
        requested: usize,
    },
}

/// Converts every nonempty Origin worksheet into an independent typed snapshot.
///
/// The complete neutral model is validated and conservatively charged against
/// `max_total_owned_bytes` before a snapshot builder or block-store write is
/// created. Source cell vectors are then drained batch by batch, so text cell
/// storage is moved rather than cloned.
pub fn import_origin_project(
    project: OriginProject,
    store: &dyn BlockStore,
    codecs: &CodecRegistry,
    limits: OriginLimits,
) -> Result<Vec<ImportedOriginWorksheet>, OriginImportError> {
    limits.validate()?;
    let preflight = preflight::validate(&project, &limits)?;
    if preflight.worksheets.is_empty() {
        return Err(OriginImportError::NoSupportedWorksheet);
    }

    let OriginProject {
        probe,
        parameters,
        notes,
        workbooks,
        diagnostics,
        unsupported_objects,
        mut resource_usage,
    } = project;
    resource_usage.total_owned_bytes = preflight.total_owned_bytes;
    let candidate_count = preflight.worksheets.len();
    let mut worksheet_preflights = preflight.worksheets.into_iter();
    let mut prepared = Vec::new();
    try_reserve(&mut prepared, candidate_count, "prepared Origin worksheets")?;

    for workbook in workbooks {
        let workbook_name = workbook.name;
        for worksheet in workbook.worksheets {
            if worksheet.row_count == 0 || worksheet.columns.is_empty() {
                continue;
            }
            let worksheet_preflight =
                worksheet_preflights
                    .next()
                    .ok_or_else(|| OriginImportError::InvalidModel {
                        detail: "worksheet preflight count does not match the retained model"
                            .to_owned(),
                    })?;
            let source_metadata = source_metadata(
                &probe.raw_version,
                &parameters,
                &notes,
                &diagnostics,
                &unsupported_objects,
                &resource_usage,
                &workbook_name,
                &worksheet,
                &worksheet_preflight.imported_names,
            )?;
            let snapshot_metadata = snapshot_metadata(&source_metadata)?;
            let name = candidate_name(&workbook_name, &worksheet.name)?;
            let schema = build_schema(&worksheet.columns, worksheet_preflight.imported_names)?;
            prepared.push(PreparedOriginWorksheet {
                name,
                worksheet,
                schema,
                source_metadata,
                snapshot_metadata,
                diagnostics: diagnostics.clone(),
                resource_usage: resource_usage.clone(),
            });
        }
    }
    if worksheet_preflights.next().is_some() {
        return Err(OriginImportError::InvalidModel {
            detail: "worksheet preflight count does not match the retained model".to_owned(),
        });
    }

    let mut imported = Vec::new();
    try_reserve(
        &mut imported,
        candidate_count,
        "Origin worksheet candidates",
    )?;
    for prepared in prepared {
        let snapshot = build_snapshot(
            prepared.worksheet,
            prepared.schema,
            prepared.snapshot_metadata,
            store,
            codecs,
        )?;
        imported.push(ImportedOriginWorksheet {
            name: prepared.name,
            snapshot,
            source_metadata: prepared.source_metadata,
            diagnostics: prepared.diagnostics,
            resource_usage: prepared.resource_usage,
        });
    }
    Ok(imported)
}

fn build_snapshot(
    mut worksheet: OriginWorksheet,
    schema: TableSchema,
    metadata: BTreeMap<String, Value>,
    store: &dyn BlockStore,
    codecs: &CodecRegistry,
) -> Result<TableSnapshot, OriginImportError> {
    let mut builder =
        SnapshotBuilder::new(TableId::new(), schema, store, codecs)?.with_trusted_row_identity();
    *builder.metadata_mut() = metadata;

    let mut row_start = 0_usize;
    while row_start < worksheet.row_count {
        let row_count = (worksheet.row_count - row_start).min(CHUNK_ROWS);
        let mut chunks = Vec::new();
        try_reserve(&mut chunks, worksheet.columns.len(), "Origin column chunks")?;
        for column in &mut worksheet.columns {
            chunks.push(drain_column_chunk(column, row_start, row_count)?);
        }
        let mut row_ids = Vec::new();
        try_reserve(&mut row_ids, row_count, "Origin row identities")?;
        row_ids.extend((0..row_count).map(|_| RowId::new()));
        builder.push_batch(&row_ids, &chunks)?;
        row_start = checked_add(row_start, row_count, "worksheet row offset")?;
    }
    Ok(builder.finish()?)
}

fn build_schema(
    columns: &[OriginColumn],
    imported_names: Vec<String>,
) -> Result<TableSchema, OriginImportError> {
    let mut schemas = Vec::new();
    try_reserve(&mut schemas, columns.len(), "Origin column schemas")?;
    for (column, name) in columns.iter().zip(imported_names) {
        let logical_type = match column.column_type {
            OriginColumnType::Float => LogicalType::Float64,
            OriginColumnType::Integer => LogicalType::Int64,
            OriginColumnType::Text | OriginColumnType::Mixed => LogicalType::Utf8,
        };
        let changed = name != column.name;
        let mut schema = ColumnSchema::new(name, logical_type);
        if changed {
            insert_text(&mut schema.metadata, ORIGINAL_NAME_KEY, &column.name)?;
        }
        for (key, value) in [
            (LONG_NAME_KEY, column.long_name.as_deref()),
            (ROLE_KEY, column.role.as_deref()),
            (UNITS_KEY, column.units.as_deref()),
            (COMMENTS_KEY, column.comments.as_deref()),
        ] {
            if let Some(value) = value {
                insert_text(&mut schema.metadata, key, value)?;
            }
        }
        schemas.push(schema);
    }
    Ok(TableSchema::new(schemas)?)
}

fn drain_column_chunk(
    column: &mut OriginColumn,
    row_start: usize,
    row_count: usize,
) -> Result<ColumnChunk, OriginImportError> {
    let take = row_count.min(column.cells.len());
    let column_name = column.name.as_str();
    let column_type = column.column_type;
    let mut validity = Vec::new();
    try_reserve(&mut validity, row_count, "Origin validity input")?;
    let values = match column.column_type {
        OriginColumnType::Float => {
            let mut values = Vec::new();
            try_reserve(&mut values, row_count, "Origin Float64 values")?;
            for (index, cell) in column.cells.drain(..take).enumerate() {
                match cell {
                    OriginCell::Float(value) => {
                        values.push(value);
                        validity.push(true);
                    }
                    OriginCell::Null => {
                        values.push(0.0);
                        validity.push(false);
                    }
                    other => {
                        return cell_type_error(
                            column_name,
                            column_type,
                            checked_add(row_start, index, "worksheet row index")?,
                            &other,
                        );
                    }
                }
            }
            values.resize(row_count, 0.0);
            ColumnValues::Float64(values)
        }
        OriginColumnType::Integer => {
            let mut values = Vec::new();
            try_reserve(&mut values, row_count, "Origin Int64 values")?;
            for (index, cell) in column.cells.drain(..take).enumerate() {
                match cell {
                    OriginCell::Integer(value) => {
                        values.push(value);
                        validity.push(true);
                    }
                    OriginCell::Null => {
                        values.push(0);
                        validity.push(false);
                    }
                    other => {
                        return cell_type_error(
                            column_name,
                            column_type,
                            checked_add(row_start, index, "worksheet row index")?,
                            &other,
                        );
                    }
                }
            }
            values.resize(row_count, 0);
            ColumnValues::Int64(values)
        }
        OriginColumnType::Text | OriginColumnType::Mixed => {
            let mut values = Vec::new();
            try_reserve(&mut values, row_count, "Origin UTF-8 values")?;
            for (index, cell) in column.cells.drain(..take).enumerate() {
                match cell {
                    OriginCell::Text(value) => {
                        values.push(value);
                        validity.push(true);
                    }
                    OriginCell::Float(value) if column.column_type == OriginColumnType::Mixed => {
                        values.push(value.to_string());
                        validity.push(true);
                    }
                    OriginCell::Integer(value) if column.column_type == OriginColumnType::Mixed => {
                        values.push(value.to_string());
                        validity.push(true);
                    }
                    OriginCell::Null => {
                        values.push(String::new());
                        validity.push(false);
                    }
                    other => {
                        return cell_type_error(
                            column_name,
                            column_type,
                            checked_add(row_start, index, "worksheet row index")?,
                            &other,
                        );
                    }
                }
            }
            values.resize_with(row_count, String::new);
            ColumnValues::Utf8(values)
        }
    };
    validity.resize(row_count, false);
    Ok(ColumnChunk::new(values, Validity::from_valid(validity))?)
}

fn cell_type_error<T>(
    column: &str,
    expected: OriginColumnType,
    row: usize,
    cell: &OriginCell,
) -> Result<T, OriginImportError> {
    Err(OriginImportError::InvalidCellType {
        column: copy_text(column, "Origin column name")?,
        row,
        expected,
        actual: cell_kind(cell),
    })
}

#[allow(clippy::too_many_arguments)]
fn source_metadata(
    version: &str,
    parameters: &[OriginMetadataEntry],
    notes: &[OriginNote],
    diagnostics: &[OriginDiagnostic],
    unsupported: &[OriginUnsupportedObjectSummary],
    usage: &OriginResourceUsage,
    workbook: &str,
    worksheet: &OriginWorksheet,
    imported_names: &[String],
) -> Result<BTreeMap<String, Value>, OriginImportError> {
    let mut metadata = BTreeMap::new();
    insert_text(&mut metadata, FORMAT_KEY, "opj")?;
    insert_text(&mut metadata, VERSION_KEY, version)?;
    insert_text(&mut metadata, WORKBOOK_KEY, workbook)?;
    insert_text(&mut metadata, WORKSHEET_KEY, &worksheet.name)?;
    metadata.insert(PARAMETERS_KEY.to_owned(), entries_json(parameters)?);
    metadata.insert(NOTES_KEY.to_owned(), notes_json(notes)?);
    metadata.insert(DIAGNOSTICS_KEY.to_owned(), diagnostics_json(diagnostics)?);
    metadata.insert(UNSUPPORTED_KEY.to_owned(), unsupported_json(unsupported)?);
    metadata.insert(USAGE_KEY.to_owned(), usage_json(usage));
    metadata.insert(
        COLUMNS_KEY.to_owned(),
        columns_json(&worksheet.columns, imported_names)?,
    );
    metadata.insert(
        WORKSHEET_METADATA_KEY.to_owned(),
        entries_json(&worksheet.metadata)?,
    );
    Ok(metadata)
}

fn snapshot_metadata(
    source: &BTreeMap<String, Value>,
) -> Result<BTreeMap<String, Value>, OriginImportError> {
    let mut metadata = BTreeMap::new();
    for key in [
        FORMAT_KEY,
        VERSION_KEY,
        WORKBOOK_KEY,
        WORKSHEET_KEY,
        DIAGNOSTICS_KEY,
    ] {
        let value = source
            .get(key)
            .ok_or_else(|| OriginImportError::InvalidModel {
                detail: format!("source metadata is missing {key}"),
            })?;
        metadata.insert(key.to_owned(), value.clone());
    }
    Ok(metadata)
}

fn entries_json(entries: &[OriginMetadataEntry]) -> Result<Value, OriginImportError> {
    let mut values = Vec::new();
    try_reserve(&mut values, entries.len(), "Origin metadata JSON")?;
    for entry in entries {
        values.push(Value::Object(Map::from_iter([
            (
                "key".to_owned(),
                Value::String(copy_text(&entry.key, "metadata key")?),
            ),
            (
                "value".to_owned(),
                Value::String(copy_text(&entry.value, "metadata value")?),
            ),
        ])));
    }
    Ok(Value::Array(values))
}

fn notes_json(notes: &[OriginNote]) -> Result<Value, OriginImportError> {
    let mut values = Vec::new();
    try_reserve(&mut values, notes.len(), "Origin notes JSON")?;
    for note in notes {
        values.push(Value::Object(Map::from_iter([
            (
                "name".to_owned(),
                Value::String(copy_text(&note.name, "note name")?),
            ),
            (
                "content".to_owned(),
                Value::String(copy_text(&note.content, "note content")?),
            ),
        ])));
    }
    Ok(Value::Array(values))
}

fn diagnostics_json(diagnostics: &[OriginDiagnostic]) -> Result<Value, OriginImportError> {
    let mut values = Vec::new();
    try_reserve(&mut values, diagnostics.len(), "Origin diagnostics JSON")?;
    for diagnostic in diagnostics {
        let location = diagnostic.location.as_ref().map(|location| {
            Value::Object(Map::from_iter([
                (
                    "workbook".to_owned(),
                    option_text(location.workbook.as_deref()),
                ),
                (
                    "worksheet".to_owned(),
                    option_text(location.worksheet.as_deref()),
                ),
                ("column".to_owned(), option_text(location.column.as_deref())),
                ("byte_offset".to_owned(), usize_value(location.byte_offset)),
            ]))
        });
        values.push(Value::Object(Map::from_iter([
            (
                "code".to_owned(),
                Value::String(diagnostic_code(diagnostic.code).to_owned()),
            ),
            (
                "severity".to_owned(),
                Value::String(diagnostic_severity(diagnostic.severity).to_owned()),
            ),
            ("location".to_owned(), location.unwrap_or(Value::Null)),
            (
                "message".to_owned(),
                Value::String(copy_text(&diagnostic.message, "diagnostic message")?),
            ),
        ])));
    }
    Ok(Value::Array(values))
}

fn unsupported_json(
    unsupported: &[OriginUnsupportedObjectSummary],
) -> Result<Value, OriginImportError> {
    let mut values = Vec::new();
    try_reserve(
        &mut values,
        unsupported.len(),
        "unsupported Origin object JSON",
    )?;
    for summary in unsupported {
        values.push(Value::Object(Map::from_iter([
            (
                "kind".to_owned(),
                Value::String(copy_text(&summary.kind, "object kind")?),
            ),
            ("count".to_owned(), Value::from(summary.count)),
        ])));
    }
    Ok(Value::Array(values))
}

fn columns_json(
    columns: &[OriginColumn],
    imported_names: &[String],
) -> Result<Value, OriginImportError> {
    let mut values = Vec::new();
    try_reserve(&mut values, columns.len(), "Origin column metadata JSON")?;
    for (index, (column, imported_name)) in columns.iter().zip(imported_names).enumerate() {
        let mut value = Map::new();
        value.insert("index".to_owned(), Value::from(index));
        value.insert(
            "source_name".to_owned(),
            Value::String(copy_text(&column.name, "column source name")?),
        );
        value.insert(
            "imported_name".to_owned(),
            Value::String(copy_text(imported_name, "column imported name")?),
        );
        value.insert(
            "long_name".to_owned(),
            option_text(column.long_name.as_deref()),
        );
        value.insert("role".to_owned(), option_text(column.role.as_deref()));
        value.insert("units".to_owned(), option_text(column.units.as_deref()));
        value.insert(
            "comments".to_owned(),
            option_text(column.comments.as_deref()),
        );
        values.push(Value::Object(value));
    }
    Ok(Value::Array(values))
}

fn usage_json(usage: &OriginResourceUsage) -> Value {
    Value::Object(Map::from_iter([
        ("input_bytes".to_owned(), Value::from(usage.input_bytes)),
        ("parser_bytes".to_owned(), Value::from(usage.parser_bytes)),
        (
            "decoded_text_bytes".to_owned(),
            Value::from(usage.decoded_text_bytes),
        ),
        (
            "total_owned_bytes".to_owned(),
            Value::from(usage.total_owned_bytes),
        ),
        ("workbooks".to_owned(), Value::from(usage.workbooks)),
        ("worksheets".to_owned(), Value::from(usage.worksheets)),
        ("columns".to_owned(), Value::from(usage.columns)),
        ("cells".to_owned(), Value::from(usage.cells)),
        (
            "metadata_records".to_owned(),
            Value::from(usage.metadata_records),
        ),
    ]))
}

fn candidate_name(workbook: &str, worksheet: &str) -> Result<String, OriginImportError> {
    let capacity = checked_add(
        checked_add(workbook.len(), worksheet.len(), "candidate name")?,
        3,
        "candidate name",
    )?;
    let mut name = String::new();
    name.try_reserve_exact(capacity)
        .map_err(|_| OriginImportError::AllocationFailed {
            resource: "Origin candidate name",
            requested: capacity,
        })?;
    name.push_str(workbook);
    name.push_str(" / ");
    name.push_str(worksheet);
    Ok(name)
}

fn insert_text(
    metadata: &mut BTreeMap<String, Value>,
    key: &str,
    value: &str,
) -> Result<(), OriginImportError> {
    metadata.insert(
        key.to_owned(),
        Value::String(copy_text(value, "Origin metadata text")?),
    );
    Ok(())
}

fn option_text(value: Option<&str>) -> Value {
    value.map_or(Value::Null, |value| Value::String(value.to_owned()))
}

fn usize_value(value: Option<usize>) -> Value {
    value.map_or(Value::Null, Value::from)
}

fn diagnostic_code(code: OriginDiagnosticCode) -> &'static str {
    match code {
        OriginDiagnosticCode::UnsupportedObjectSkipped => "unsupported_object_skipped",
        OriginDiagnosticCode::UnsupportedColumnSkipped => "unsupported_column_skipped",
        OriginDiagnosticCode::MetadataSkipped => "metadata_skipped",
        OriginDiagnosticCode::DecodingWarning => "decoding_warning",
    }
}

fn diagnostic_severity(severity: OriginDiagnosticSeverity) -> &'static str {
    match severity {
        OriginDiagnosticSeverity::Info => "info",
        OriginDiagnosticSeverity::Warning => "warning",
    }
}

fn cell_matches(column_type: OriginColumnType, cell: &OriginCell) -> bool {
    matches!(
        (column_type, cell),
        (_, OriginCell::Null)
            | (OriginColumnType::Float, OriginCell::Float(_))
            | (OriginColumnType::Integer, OriginCell::Integer(_))
            | (OriginColumnType::Text, OriginCell::Text(_))
            | (
                OriginColumnType::Mixed,
                OriginCell::Float(_) | OriginCell::Integer(_) | OriginCell::Text(_),
            )
    )
}

fn cell_kind(cell: &OriginCell) -> &'static str {
    match cell {
        OriginCell::Null => "null",
        OriginCell::Float(_) => "a floating-point value",
        OriginCell::Integer(_) => "an integer",
        OriginCell::Text(_) => "text",
    }
}

fn copy_text(text: &str, resource: &'static str) -> Result<String, OriginImportError> {
    let mut copy = String::new();
    copy.try_reserve_exact(text.len())
        .map_err(|_| OriginImportError::AllocationFailed {
            resource,
            requested: text.len(),
        })?;
    copy.push_str(text);
    Ok(copy)
}

fn try_reserve<T>(
    values: &mut Vec<T>,
    additional: usize,
    resource: &'static str,
) -> Result<(), OriginImportError> {
    let requested = checked_mul(additional, size_of::<T>(), resource)?;
    values
        .try_reserve_exact(additional)
        .map_err(|_| OriginImportError::AllocationFailed {
            resource,
            requested,
        })
}

fn enforce(resource: &'static str, actual: usize, limit: usize) -> Result<(), OriginImportError> {
    if actual > limit {
        return Err(OriginImportError::LimitExceeded {
            resource,
            limit,
            actual,
        });
    }
    Ok(())
}

fn checked_add(
    left: usize,
    right: usize,
    resource: &'static str,
) -> Result<usize, OriginImportError> {
    left.checked_add(right)
        .ok_or(OriginImportError::ArithmeticOverflow { resource })
}

fn checked_mul(
    left: usize,
    right: usize,
    resource: &'static str,
) -> Result<usize, OriginImportError> {
    left.checked_mul(right)
        .ok_or(OriginImportError::ArithmeticOverflow { resource })
}
