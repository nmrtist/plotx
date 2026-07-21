use std::mem::size_of;

use plotx_data::{
    ChunkDescriptor, ColumnChunk, ColumnManifest, ColumnSchema, RowId, TableSnapshot,
};
use plotx_io::origin::{
    OriginByteOrder, OriginCell, OriginColumn, OriginColumnType, OriginDiagnostic, OriginFormat,
    OriginHeaderVersion, OriginLimits, OriginMetadataEntry, OriginProfile, OriginProject,
    OriginResourceUsage, OriginSupport, OriginWorksheet,
};

use super::{
    CHUNK_ROWS, ImportedOriginWorksheet, OriginImportError, PreparedOriginWorksheet, cell_kind,
    cell_matches, checked_add, checked_mul, copy_text, enforce, names, try_reserve,
};

const ARROW_BLOCK_OVERHEAD: usize = 4_096;
const BTREE_ENTRY_OVERHEAD: usize = 64;
const IPC_OFFSET_BYTES: usize = 4;
const FINGERPRINT_LENGTH_BYTES: usize = 8;
const UUID_TEXT_BYTES: usize = 36;
const JSON_ENTRY_OVERHEAD: usize = 256;
const MIXED_FLOAT_TEXT_MAX: usize = 32;
const MIXED_INTEGER_TEXT_MAX: usize = 20;

mod model;

pub(super) struct Preflight {
    pub(super) worksheets: Vec<WorksheetPreflight>,
    pub(super) total_owned_bytes: usize,
}

pub(super) struct WorksheetPreflight {
    pub(super) imported_names: Vec<String>,
}

pub(super) fn validate(
    project: &OriginProject,
    limits: &OriginLimits,
) -> Result<Preflight, OriginImportError> {
    validate_probe(project)?;
    validate_reported_usage(&project.resource_usage, limits)?;
    enforce("workbooks", project.workbooks.len(), limits.max_workbooks)?;
    let retained_model = checked_add(
        project.resource_usage.input_bytes,
        model::owned_lower_bound(project)?,
        "retained Origin model",
    )?;
    let mut estimated_total = project.resource_usage.total_owned_bytes.max(retained_model);
    enforce(
        "total owned bytes",
        estimated_total,
        limits.max_total_owned_bytes,
    )?;

    let mut text_bytes = 0_usize;
    charge_text(&mut text_bytes, &project.probe.raw_version, limits)?;
    let mut metadata_records = 0_usize;
    validate_entries(
        &project.parameters,
        &mut text_bytes,
        &mut metadata_records,
        limits,
    )?;
    for note in &project.notes {
        add_record(&mut metadata_records, limits)?;
        charge_text(&mut text_bytes, &note.name, limits)?;
        charge_text(&mut text_bytes, &note.content, limits)?;
    }
    validate_diagnostics(
        &project.diagnostics,
        &mut text_bytes,
        &mut metadata_records,
        limits,
    )?;
    for summary in &project.unsupported_objects {
        add_record(&mut metadata_records, limits)?;
        charge_text(&mut text_bytes, &summary.kind, limits)?;
    }

    let mut total_columns = 0_usize;
    let mut total_cells = 0_usize;
    let mut total_worksheets = 0_usize;
    let mut retained_metadata_records = checked_add(
        project.parameters.len(),
        project.notes.len(),
        "retained metadata records",
    )?;
    let mut worksheets = Vec::new();
    for workbook in &project.workbooks {
        charge_text(&mut text_bytes, &workbook.name, limits)?;
        enforce(
            "worksheets per workbook",
            workbook.worksheets.len(),
            limits.max_worksheets_per_workbook,
        )?;
        for worksheet in &workbook.worksheets {
            total_worksheets = checked_add(total_worksheets, 1, "worksheets")?;
            retained_metadata_records = checked_add(
                retained_metadata_records,
                worksheet.metadata.len(),
                "retained metadata records",
            )?;
            charge_text(&mut text_bytes, &worksheet.name, limits)?;
            validate_entries(
                &worksheet.metadata,
                &mut text_bytes,
                &mut metadata_records,
                limits,
            )?;
            total_columns = checked_add(total_columns, worksheet.columns.len(), "columns")?;
            enforce("columns", total_columns, limits.max_columns)?;
            enforce(
                "rows per column",
                worksheet.row_count,
                limits.max_rows_per_column,
            )?;
            for column in &worksheet.columns {
                validate_column(
                    column,
                    worksheet.row_count,
                    &mut text_bytes,
                    &mut total_cells,
                    limits,
                )?;
            }
            if worksheet.row_count > 0 && !worksheet.columns.is_empty() {
                let imported_names = names::normalize(&worksheet.columns, limits)?;
                estimate_worksheet(
                    &mut estimated_total,
                    project,
                    &workbook.name,
                    worksheet,
                    &imported_names,
                    limits,
                )?;
                try_reserve(&mut worksheets, 1, "Origin worksheet preflight")?;
                worksheets.push(WorksheetPreflight { imported_names });
            }
        }
    }
    enforce(
        "decoded text bytes",
        text_bytes,
        limits.max_decoded_text_bytes,
    )?;
    enforce(
        "metadata records",
        metadata_records,
        limits.max_metadata_records,
    )?;
    enforce("cells", total_cells, limits.max_cells)?;
    validate_retained_counts(
        &project.resource_usage,
        project.workbooks.len(),
        total_worksheets,
        total_columns,
        total_cells,
        retained_metadata_records,
    )?;
    enforce(
        "total owned bytes",
        estimated_total,
        limits.max_total_owned_bytes,
    )?;
    Ok(Preflight {
        worksheets,
        total_owned_bytes: estimated_total,
    })
}

fn validate_probe(project: &OriginProject) -> Result<(), OriginImportError> {
    const EXPECTED_VERSION: OriginHeaderVersion = OriginHeaderVersion {
        major: 4,
        minor: 2673,
        build: 552,
    };
    if project.probe.format != OriginFormat::Opj
        || project.probe.support != OriginSupport::Supported
        || project.probe.profile != Some(OriginProfile::Origin7V552)
        || project.probe.byte_order != OriginByteOrder::LittleEndian
        || project.probe.raw_version != "4.2673 552"
        || project.probe.version != EXPECTED_VERSION
    {
        return Err(OriginImportError::InvalidModel {
            detail:
                "only the exact verified little-endian Origin7V552 OPJ profile can be converted"
                    .to_owned(),
        });
    }
    Ok(())
}

fn validate_reported_usage(
    usage: &OriginResourceUsage,
    limits: &OriginLimits,
) -> Result<(), OriginImportError> {
    enforce("input bytes", usage.input_bytes, limits.max_input_bytes)?;
    enforce("parser bytes", usage.parser_bytes, limits.max_parser_bytes)?;
    enforce(
        "decoded text bytes",
        usage.decoded_text_bytes,
        limits.max_decoded_text_bytes,
    )?;
    enforce(
        "total owned bytes",
        usage.total_owned_bytes,
        limits.max_total_owned_bytes,
    )?;
    enforce("workbooks", usage.workbooks, limits.max_workbooks)?;
    enforce("columns", usage.columns, limits.max_columns)?;
    enforce("cells", usage.cells, limits.max_cells)?;
    enforce(
        "metadata records",
        usage.metadata_records,
        limits.max_metadata_records,
    )?;
    let parser_minimum = checked_add(usage.input_bytes, usage.parser_bytes, "parser ownership")?;
    if usage.total_owned_bytes < parser_minimum {
        return Err(OriginImportError::InvalidModel {
            detail: "resource usage total is smaller than input plus parser ownership".to_owned(),
        });
    }
    if usage.decoded_text_bytes > usage.parser_bytes {
        return Err(OriginImportError::InvalidModel {
            detail: "decoded text accounting exceeds parser ownership".to_owned(),
        });
    }
    Ok(())
}

fn validate_retained_counts(
    usage: &OriginResourceUsage,
    workbooks: usize,
    worksheets: usize,
    columns: usize,
    cells: usize,
    metadata_records: usize,
) -> Result<(), OriginImportError> {
    for (resource, reported, retained, exact) in [
        ("workbooks", usage.workbooks, workbooks, true),
        ("worksheets", usage.worksheets, worksheets, true),
        ("columns", usage.columns, columns, false),
        ("cells", usage.cells, cells, false),
        (
            "metadata records",
            usage.metadata_records,
            metadata_records,
            false,
        ),
    ] {
        if (exact && reported != retained) || (!exact && reported < retained) {
            let relation = if exact { "equal" } else { "cover" };
            return Err(OriginImportError::InvalidModel {
                detail: format!(
                    "reported {resource} count {reported} does not {relation} the retained count {retained}"
                ),
            });
        }
    }
    Ok(())
}

fn validate_entries(
    entries: &[OriginMetadataEntry],
    text_bytes: &mut usize,
    records: &mut usize,
    limits: &OriginLimits,
) -> Result<(), OriginImportError> {
    for entry in entries {
        add_record(records, limits)?;
        charge_text(text_bytes, &entry.key, limits)?;
        charge_text(text_bytes, &entry.value, limits)?;
    }
    Ok(())
}

fn validate_diagnostics(
    diagnostics: &[OriginDiagnostic],
    text_bytes: &mut usize,
    records: &mut usize,
    limits: &OriginLimits,
) -> Result<(), OriginImportError> {
    for diagnostic in diagnostics {
        add_record(records, limits)?;
        charge_text(text_bytes, &diagnostic.message, limits)?;
        if let Some(location) = &diagnostic.location {
            for value in [
                location.workbook.as_deref(),
                location.worksheet.as_deref(),
                location.column.as_deref(),
            ]
            .into_iter()
            .flatten()
            {
                charge_text(text_bytes, value, limits)?;
            }
        }
    }
    Ok(())
}

fn validate_column(
    column: &OriginColumn,
    row_count: usize,
    text_bytes: &mut usize,
    total_cells: &mut usize,
    limits: &OriginLimits,
) -> Result<(), OriginImportError> {
    for text in [
        Some(column.name.as_str()),
        column.long_name.as_deref(),
        column.role.as_deref(),
        column.units.as_deref(),
        column.comments.as_deref(),
    ]
    .into_iter()
    .flatten()
    {
        charge_text(text_bytes, text, limits)?;
    }
    enforce(
        "rows per column",
        column.cells.len(),
        limits.max_rows_per_column,
    )?;
    if column.cells.len() > row_count {
        return Err(OriginImportError::InvalidModel {
            detail: format!(
                "column {:?} has {} cells but worksheet row_count is {}",
                column.name,
                column.cells.len(),
                row_count
            ),
        });
    }
    *total_cells = checked_add(*total_cells, column.cells.len(), "cells")?;
    enforce("cells", *total_cells, limits.max_cells)?;
    for (row, cell) in column.cells.iter().enumerate() {
        if let OriginCell::Text(text) = cell {
            charge_text(text_bytes, text, limits)?;
        }
        if !cell_matches(column.column_type, cell) {
            return Err(OriginImportError::InvalidCellType {
                column: copy_text(&column.name, "Origin column name")?,
                row,
                expected: column.column_type,
                actual: cell_kind(cell),
            });
        }
    }
    Ok(())
}

fn estimate_worksheet(
    total: &mut usize,
    project: &OriginProject,
    workbook_name: &str,
    worksheet: &OriginWorksheet,
    imported_names: &[String],
    limits: &OriginLimits,
) -> Result<(), OriginImportError> {
    let rows = worksheet.row_count;
    let columns = worksheet.columns.len();
    let batches = checked_add((rows - 1) / CHUNK_ROWS, 1, "snapshot batches")?;
    estimate_add(total, size_of::<ImportedOriginWorksheet>(), limits)?;
    estimate_add(total, size_of::<PreparedOriginWorksheet>(), limits)?;
    estimate_add(total, size_of::<WorksheetPreflight>(), limits)?;
    estimate_add(total, size_of::<TableSnapshot>(), limits)?;
    estimate_mul(total, columns, size_of::<ColumnSchema>(), limits)?;
    estimate_mul(total, columns, size_of::<ColumnManifest>(), limits)?;
    estimate_mul(total, batches, size_of::<ChunkDescriptor>(), limits)?;
    estimate_mul(
        total,
        checked_mul(columns, batches, "column descriptors")?,
        size_of::<ChunkDescriptor>(),
        limits,
    )?;
    estimate_mul(total, rows, size_of::<RowId>(), limits)?;
    // SnapshotBuilder validates every batch in a BTreeSet before formatting
    // UUID row ids as strings and sending them through the UTF-8 codec path.
    estimate_mul(
        total,
        rows,
        checked_add(size_of::<RowId>(), BTREE_ENTRY_OVERHEAD, "row identity set")?,
        limits,
    )?;
    let row_identity_text = checked_mul(rows, UUID_TEXT_BYTES, "row identity text")?;
    estimate_utf8_conversion(total, rows, row_identity_text, row_identity_text, limits)?;
    estimate_mul(
        total,
        checked_mul(columns, batches, "column chunks")?,
        size_of::<ColumnChunk>(),
        limits,
    )?;
    estimate_mul(total, columns, size_of::<String>(), limits)?;
    let imported_name_bytes = imported_names.iter().try_fold(0_usize, |total, name| {
        checked_add(total, name.len(), "imported column names")
    })?;
    // Normalization retains the final names and temporarily owns reserved and
    // used-name set copies while protecting genuine source names.
    estimate_mul(total, imported_name_bytes, 3, limits)?;
    estimate_mul(total, columns, BTREE_ENTRY_OVERHEAD * 2, limits)?;

    for column in &worksheet.columns {
        match column.column_type {
            OriginColumnType::Float | OriginColumnType::Integer => {
                estimate_numeric_conversion(total, rows, limits)?;
            }
            OriginColumnType::Text | OriginColumnType::Mixed => {
                let text = estimated_column_text(column)?;
                estimate_utf8_conversion(
                    total,
                    rows,
                    text.encoded_bytes,
                    text.new_target_bytes,
                    limits,
                )?;
            }
        }
        estimate_add(
            total,
            checked_add(
                column.name.len(),
                checked_mul(5, JSON_ENTRY_OVERHEAD, "column metadata")?,
                "column metadata",
            )?,
            limits,
        )?;
    }
    estimate_mul(
        total,
        source_metadata_text_bytes(project, workbook_name, worksheet)?,
        4,
        limits,
    )?;
    let records = checked_add(
        checked_add(
            project.parameters.len(),
            project.notes.len(),
            "metadata estimate",
        )?,
        checked_add(
            project.diagnostics.len(),
            project.unsupported_objects.len(),
            "metadata estimate",
        )?,
        "metadata estimate",
    )?;
    let records = checked_add(records, worksheet.metadata.len(), "metadata estimate")?;
    let records = checked_add(records, worksheet.columns.len(), "metadata estimate")?;
    estimate_mul(
        total,
        checked_add(records, 12, "metadata estimate")?,
        JSON_ENTRY_OVERHEAD,
        limits,
    )?;
    estimate_mul(
        total,
        checked_mul(
            batches,
            checked_add(columns, 1, "encoded blocks")?,
            "encoded blocks",
        )?,
        ARROW_BLOCK_OVERHEAD,
        limits,
    )
}

fn estimate_numeric_conversion(
    total: &mut usize,
    rows: usize,
    limits: &OriginLimits,
) -> Result<(), OriginImportError> {
    let validity = bitmap_bytes(rows);
    // Conversion target plus the byte-per-row validity input.
    estimate_mul(total, rows, size_of::<f64>(), limits)?;
    estimate_add(total, rows, limits)?;
    estimate_add(total, validity, limits)?;
    // The Arrow codec first materializes Vec<Option<T>>, then Arrow value and
    // validity buffers. IPC retains another value representation in the block
    // store, and logical_fingerprint builds a separate canonical byte vector.
    estimate_mul(total, rows, size_of::<Option<f64>>(), limits)?;
    estimate_numeric_buffers(total, rows, validity, limits)?;
    estimate_numeric_buffers(total, rows, validity, limits)?;
    estimate_numeric_buffers(total, rows, validity, limits)
}

fn estimate_numeric_buffers(
    total: &mut usize,
    rows: usize,
    validity: usize,
    limits: &OriginLimits,
) -> Result<(), OriginImportError> {
    estimate_mul(total, rows, size_of::<f64>(), limits)?;
    estimate_add(total, validity, limits)
}

fn estimate_utf8_conversion(
    total: &mut usize,
    rows: usize,
    text_bytes: usize,
    new_target_text_bytes: usize,
    limits: &OriginLimits,
) -> Result<(), OriginImportError> {
    let validity = bitmap_bytes(rows);
    // Conversion target plus the byte-per-row validity input and the retained
    // ColumnChunk bitmap.
    estimate_mul(total, rows, size_of::<String>(), limits)?;
    estimate_add(total, new_target_text_bytes, limits)?;
    estimate_add(total, rows, limits)?;
    estimate_add(total, validity, limits)?;
    // StringArray::from first collects borrowed values into Vec<Option<&str>>.
    estimate_mul(total, rows, size_of::<Option<&str>>(), limits)?;
    // Arrow, retained IPC, and the canonical fingerprint each own a separate
    // row-scaled representation. The fingerprint uses u64 lengths rather than
    // Arrow's i32 offsets.
    estimate_utf8_buffers(total, rows, text_bytes, validity, IPC_OFFSET_BYTES, limits)?;
    estimate_utf8_buffers(total, rows, text_bytes, validity, IPC_OFFSET_BYTES, limits)?;
    estimate_utf8_buffers(
        total,
        rows,
        text_bytes,
        validity,
        FINGERPRINT_LENGTH_BYTES,
        limits,
    )
}

fn estimate_utf8_buffers(
    total: &mut usize,
    rows: usize,
    text_bytes: usize,
    validity: usize,
    offset_width: usize,
    limits: &OriginLimits,
) -> Result<(), OriginImportError> {
    estimate_mul(
        total,
        checked_add(rows, 1, "UTF-8 offsets")?,
        offset_width,
        limits,
    )?;
    estimate_add(total, text_bytes, limits)?;
    estimate_add(total, validity, limits)
}

struct EstimatedColumnText {
    encoded_bytes: usize,
    new_target_bytes: usize,
}

fn estimated_column_text(column: &OriginColumn) -> Result<EstimatedColumnText, OriginImportError> {
    let mut encoded_bytes = 0_usize;
    let mut new_target_bytes = 0_usize;
    for cell in &column.cells {
        let (encoded, newly_allocated) = match cell {
            OriginCell::Text(value) => (value.len(), 0),
            OriginCell::Float(_) => (MIXED_FLOAT_TEXT_MAX, MIXED_FLOAT_TEXT_MAX),
            OriginCell::Integer(_) => (MIXED_INTEGER_TEXT_MAX, MIXED_INTEGER_TEXT_MAX),
            OriginCell::Null => (0, 0),
        };
        encoded_bytes = checked_add(encoded_bytes, encoded, "UTF-8 cell data")?;
        new_target_bytes = checked_add(
            new_target_bytes,
            newly_allocated,
            "converted UTF-8 cell data",
        )?;
    }
    Ok(EstimatedColumnText {
        encoded_bytes,
        new_target_bytes,
    })
}

fn source_metadata_text_bytes(
    project: &OriginProject,
    workbook_name: &str,
    worksheet: &OriginWorksheet,
) -> Result<usize, OriginImportError> {
    let mut bytes = checked_add(
        project.probe.raw_version.len(),
        workbook_name.len(),
        "metadata",
    )?;
    bytes = checked_add(bytes, worksheet.name.len(), "metadata")?;
    for entry in project.parameters.iter().chain(&worksheet.metadata) {
        bytes = checked_add(bytes, entry.key.len(), "metadata")?;
        bytes = checked_add(bytes, entry.value.len(), "metadata")?;
    }
    for note in &project.notes {
        bytes = checked_add(bytes, note.name.len(), "metadata")?;
        bytes = checked_add(bytes, note.content.len(), "metadata")?;
    }
    for diagnostic in &project.diagnostics {
        bytes = checked_add(bytes, diagnostic.message.len(), "metadata")?;
        if let Some(location) = &diagnostic.location {
            for text in [
                location.workbook.as_deref(),
                location.worksheet.as_deref(),
                location.column.as_deref(),
            ]
            .into_iter()
            .flatten()
            {
                bytes = checked_add(bytes, text.len(), "metadata")?;
            }
        }
    }
    for summary in &project.unsupported_objects {
        bytes = checked_add(bytes, summary.kind.len(), "metadata")?;
    }
    for column in &worksheet.columns {
        for text in [
            Some(column.name.as_str()),
            column.long_name.as_deref(),
            column.role.as_deref(),
            column.units.as_deref(),
            column.comments.as_deref(),
        ]
        .into_iter()
        .flatten()
        {
            bytes = checked_add(bytes, text.len(), "metadata")?;
        }
    }
    Ok(bytes)
}

fn add_record(records: &mut usize, limits: &OriginLimits) -> Result<(), OriginImportError> {
    *records = checked_add(*records, 1, "metadata records")?;
    enforce("metadata records", *records, limits.max_metadata_records)
}

fn charge_text(
    total: &mut usize,
    text: &str,
    limits: &OriginLimits,
) -> Result<(), OriginImportError> {
    enforce("string bytes", text.len(), limits.max_string_bytes)?;
    *total = checked_add(*total, text.len(), "decoded text bytes")?;
    enforce("decoded text bytes", *total, limits.max_decoded_text_bytes)
}

fn estimate_add(
    total: &mut usize,
    bytes: usize,
    limits: &OriginLimits,
) -> Result<(), OriginImportError> {
    *total = checked_add(*total, bytes, "total owned bytes")?;
    enforce("total owned bytes", *total, limits.max_total_owned_bytes)
}

fn estimate_mul(
    total: &mut usize,
    count: usize,
    width: usize,
    limits: &OriginLimits,
) -> Result<(), OriginImportError> {
    estimate_add(
        total,
        checked_mul(count, width, "total owned bytes")?,
        limits,
    )
}

fn bitmap_bytes(rows: usize) -> usize {
    rows / 8 + usize::from(!rows.is_multiple_of(8))
}
