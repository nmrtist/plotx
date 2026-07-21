use std::mem::size_of;

use plotx_io::origin::{
    OriginCell, OriginColumn, OriginDiagnostic, OriginMetadataEntry, OriginNote, OriginProject,
    OriginUnsupportedObjectSummary, OriginWorkbook, OriginWorksheet,
};

use super::super::{OriginImportError, checked_add, checked_mul};

pub(super) fn owned_lower_bound(project: &OriginProject) -> Result<usize, OriginImportError> {
    let mut bytes = size_of::<OriginProject>();
    add_text_storage(&mut bytes, &project.probe.raw_version)?;
    add_vec_storage::<OriginMetadataEntry>(&mut bytes, project.parameters.len())?;
    for entry in &project.parameters {
        add_entry_storage(&mut bytes, entry)?;
    }
    add_vec_storage::<OriginNote>(&mut bytes, project.notes.len())?;
    for note in &project.notes {
        add_text_storage(&mut bytes, &note.name)?;
        add_text_storage(&mut bytes, &note.content)?;
    }
    add_vec_storage::<OriginWorkbook>(&mut bytes, project.workbooks.len())?;
    for workbook in &project.workbooks {
        add_text_storage(&mut bytes, &workbook.name)?;
        add_vec_storage::<OriginWorksheet>(&mut bytes, workbook.worksheets.len())?;
        for worksheet in &workbook.worksheets {
            add_worksheet_storage(&mut bytes, worksheet)?;
        }
    }
    add_vec_storage::<OriginDiagnostic>(&mut bytes, project.diagnostics.len())?;
    for diagnostic in &project.diagnostics {
        add_text_storage(&mut bytes, &diagnostic.message)?;
        if let Some(location) = &diagnostic.location {
            for text in [
                location.workbook.as_ref(),
                location.worksheet.as_ref(),
                location.column.as_ref(),
            ]
            .into_iter()
            .flatten()
            {
                add_text_storage(&mut bytes, text)?;
            }
        }
    }
    add_vec_storage::<OriginUnsupportedObjectSummary>(
        &mut bytes,
        project.unsupported_objects.len(),
    )?;
    for summary in &project.unsupported_objects {
        add_text_storage(&mut bytes, &summary.kind)?;
    }
    Ok(bytes)
}

fn add_worksheet_storage(
    bytes: &mut usize,
    worksheet: &OriginWorksheet,
) -> Result<(), OriginImportError> {
    add_text_storage(bytes, &worksheet.name)?;
    add_vec_storage::<OriginMetadataEntry>(bytes, worksheet.metadata.len())?;
    for entry in &worksheet.metadata {
        add_entry_storage(bytes, entry)?;
    }
    add_vec_storage::<OriginColumn>(bytes, worksheet.columns.len())?;
    for column in &worksheet.columns {
        for text in [
            Some(&column.name),
            column.long_name.as_ref(),
            column.role.as_ref(),
            column.units.as_ref(),
            column.comments.as_ref(),
        ]
        .into_iter()
        .flatten()
        {
            add_text_storage(bytes, text)?;
        }
        add_vec_storage::<OriginCell>(bytes, column.cells.len())?;
        for cell in &column.cells {
            if let OriginCell::Text(text) = cell {
                add_text_storage(bytes, text)?;
            }
        }
    }
    Ok(())
}

fn add_entry_storage(
    bytes: &mut usize,
    entry: &OriginMetadataEntry,
) -> Result<(), OriginImportError> {
    add_text_storage(bytes, &entry.key)?;
    add_text_storage(bytes, &entry.value)
}

fn add_text_storage(bytes: &mut usize, text: &str) -> Result<(), OriginImportError> {
    *bytes = checked_add(*bytes, text.len(), "retained Origin model")?;
    Ok(())
}

fn add_vec_storage<T>(bytes: &mut usize, len: usize) -> Result<(), OriginImportError> {
    *bytes = checked_add(
        *bytes,
        checked_mul(len, size_of::<T>(), "retained Origin model")?,
        "retained Origin model",
    )?;
    Ok(())
}
