use std::mem::size_of;

use plotx_data::{
    ChunkDescriptor, ColumnChunk, ColumnManifest, ColumnSchema, RowId, TableSnapshot,
};
use plotx_io::origin::{
    OriginCell, OriginColumn, OriginColumnType, OriginDiagnostic, OriginFormat, OriginLimits,
    OriginMetadataEntry, OriginProfile, OriginProject, OriginResourceUsage, OriginSupport,
    OriginWorksheet,
};

use super::{
    CHUNK_ROWS, ImportedOriginWorksheet, OriginImportError, cell_kind, cell_matches, checked_add,
    checked_mul, copy_text, enforce,
};

const ARROW_BLOCK_OVERHEAD: usize = 4_096;
const JSON_ENTRY_OVERHEAD: usize = 256;
const MIXED_FLOAT_TEXT_MAX: usize = 32;
const MIXED_INTEGER_TEXT_MAX: usize = 20;

pub(super) struct Preflight {
    pub(super) candidate_count: usize,
    pub(super) total_owned_bytes: usize,
}

pub(super) fn validate(
    project: &OriginProject,
    limits: &OriginLimits,
) -> Result<Preflight, OriginImportError> {
    if project.probe.format != OriginFormat::Opj
        || project.probe.support != OriginSupport::Supported
        || project.probe.profile != Some(OriginProfile::Origin7V552)
    {
        return Err(OriginImportError::InvalidModel {
            detail: "only the verified Origin7V552 OPJ profile can be converted".to_owned(),
        });
    }
    validate_reported_usage(&project.resource_usage, limits)?;
    enforce("workbooks", project.workbooks.len(), limits.max_workbooks)?;

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
    let mut candidate_count = 0_usize;
    let mut estimated_total = project.resource_usage.total_owned_bytes;
    for workbook in &project.workbooks {
        charge_text(&mut text_bytes, &workbook.name, limits)?;
        enforce(
            "worksheets per workbook",
            workbook.worksheets.len(),
            limits.max_worksheets_per_workbook,
        )?;
        for worksheet in &workbook.worksheets {
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
                candidate_count = checked_add(candidate_count, 1, "worksheet candidates")?;
                estimate_worksheet(
                    &mut estimated_total,
                    project,
                    &workbook.name,
                    worksheet,
                    limits,
                )?;
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
    enforce(
        "total owned bytes",
        estimated_total,
        limits.max_total_owned_bytes,
    )?;
    Ok(Preflight {
        candidate_count,
        total_owned_bytes: estimated_total,
    })
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
    limits: &OriginLimits,
) -> Result<(), OriginImportError> {
    let rows = worksheet.row_count;
    let columns = worksheet.columns.len();
    let batches = checked_add((rows - 1) / CHUNK_ROWS, 1, "snapshot batches")?;
    estimate_add(total, size_of::<ImportedOriginWorksheet>(), limits)?;
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
    // SnapshotBuilder formats RowIds as UUID strings and checks each batch in
    // a BTreeSet. The trusted-identity mode avoids retaining a second global
    // set, while this charge still covers per-batch work cumulatively.
    estimate_mul(total, rows, size_of::<String>() + 36 + 64, limits)?;
    estimate_utf8_array(
        total,
        rows,
        checked_mul(rows, 36, "row identity text")?,
        limits,
    )?;
    estimate_mul(
        total,
        checked_mul(columns, batches, "column chunks")?,
        size_of::<ColumnChunk>(),
        limits,
    )?;

    for column in &worksheet.columns {
        match column.column_type {
            OriginColumnType::Float | OriginColumnType::Integer => {
                estimate_numeric_array(total, rows, limits)?;
            }
            OriginColumnType::Text | OriginColumnType::Mixed => {
                estimate_utf8_array(total, rows, estimated_column_text(column)?, limits)?;
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

fn estimate_numeric_array(
    total: &mut usize,
    rows: usize,
    limits: &OriginLimits,
) -> Result<(), OriginImportError> {
    let validity = bitmap_bytes(rows);
    estimate_mul(total, rows, 16, limits)?;
    estimate_add(total, rows, limits)?;
    estimate_mul(total, validity, 2, limits)
}

fn estimate_utf8_array(
    total: &mut usize,
    rows: usize,
    text_bytes: usize,
    limits: &OriginLimits,
) -> Result<(), OriginImportError> {
    let validity = bitmap_bytes(rows);
    estimate_mul(total, rows, size_of::<String>(), limits)?;
    estimate_add(total, rows, limits)?;
    estimate_mul(total, validity, 2, limits)?;
    estimate_mul(total, text_bytes, 2, limits)?;
    estimate_mul(total, checked_add(rows, 1, "UTF-8 offsets")?, 4, limits)
}

fn estimated_column_text(column: &OriginColumn) -> Result<usize, OriginImportError> {
    let mut bytes = 0_usize;
    for cell in &column.cells {
        let additional = match cell {
            OriginCell::Text(value) => value.len(),
            OriginCell::Float(_) => MIXED_FLOAT_TEXT_MAX,
            OriginCell::Integer(_) => MIXED_INTEGER_TEXT_MAX,
            OriginCell::Null => 0,
        };
        bytes = checked_add(bytes, additional, "UTF-8 cell data")?;
    }
    Ok(bytes)
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
