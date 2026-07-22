use plotx_core::data::{LogicalType, ScalarValue};
use plotx_core::state::{PlotxApp, TableImportPreviewState};

use super::commit_table_import_preview;

pub(super) fn candidate_selector_label() -> &'static str {
    "Table"
}

pub(super) fn all_candidate_import_summary(count: usize) -> String {
    if count == 1 {
        "The candidate table will be imported.".to_owned()
    } else {
        format!("All {count} candidate tables will be imported.")
    }
}

pub(crate) fn table_import_preview_window(app: &mut PlotxApp, ctx: &egui::Context) {
    let Some(mut preview) = app.session.ui.table_import_preview.take() else {
        return;
    };
    if preview.candidates.is_empty() {
        app.session.ui.table_import_preview = Some(preview);
        let committed = commit_table_import_preview(app);
        debug_assert!(!committed);
        return;
    }
    if preview.selected >= preview.candidates.len() {
        preview.selected = 0;
    }
    let mut import = false;
    let mut cancel = false;
    let modal = super::super::modal(
        ctx,
        "table_import_preview_modal",
        super::super::ModalKind::Dialog,
    )
    .show(ctx, |ui| {
        ui.set_width(720.0);
        ui.heading("Review table import");
        ui.label("Confirm the inferred schema before the table is added to the project.");
        ui.separator();
        if preview.candidates.len() > 1 {
            egui::ComboBox::from_label(candidate_selector_label())
                .selected_text(&preview.candidates[preview.selected].name)
                .show_ui(ui, |ui| {
                    for (index, candidate) in preview.candidates.iter().enumerate() {
                        ui.selectable_value(&mut preview.selected, index, &candidate.name);
                    }
                });
        }
        import_summary(ui, &preview);
        ui.separator();
        schema_table(ui, &preview);
        ui.separator();
        value_preview(ui, &preview);
        if !preview.report.diagnostics.is_empty() {
            ui.separator();
            ui.collapsing("Import diagnostics", |ui| {
                for diagnostic in &preview.report.diagnostics {
                    ui.label(format!("{:?}: {}", diagnostic.severity, diagnostic.message));
                }
            });
        }
        ui.separator();
        ui.horizontal(|ui| {
            if ui.button("Import table").clicked() {
                import = true;
            }
            if ui.button("Cancel").clicked() {
                cancel = true;
            }
        });
    });
    if import {
        app.session.ui.table_import_preview = Some(preview);
        let committed = commit_table_import_preview(app);
        debug_assert!(committed);
    } else if !cancel && !modal.should_close() {
        app.session.ui.table_import_preview = Some(preview);
    }
}

fn import_summary(ui: &mut egui::Ui, preview: &TableImportPreviewState) {
    let candidate = &preview.candidates[preview.selected];
    let snapshot = &candidate.typed_state.envelope.revision.snapshot;
    ui.label(format!("Name: {}", candidate.name));
    ui.label(all_candidate_import_summary(preview.candidates.len()));
    ui.label(format!(
        "{} row(s), {} column(s)",
        snapshot.row_count,
        snapshot.schema.columns.len()
    ));
    if let Some(path) = &preview.recent_path {
        ui.weak(format!("Source: {}", path.display()));
    } else {
        ui.weak("Source: clipboard");
    }
}

fn schema_table(ui: &mut egui::Ui, preview: &TableImportPreviewState) {
    ui.strong("Inferred schema");
    egui::ScrollArea::vertical()
        .max_height(170.0)
        .show(ui, |ui| {
            egui::Grid::new("table_import_schema_grid")
                .striped(true)
                .show(ui, |ui| {
                    ui.strong("Column");
                    ui.strong("Type");
                    ui.strong("Unit");
                    ui.strong("Nullable");
                    ui.end_row();
                    for column in &preview.candidates[preview.selected]
                        .typed_state
                        .envelope
                        .revision
                        .snapshot
                        .schema
                        .columns
                    {
                        ui.label(&column.name);
                        ui.label(logical_type_label(&column.logical_type));
                        ui.label(
                            column
                                .unit
                                .as_ref()
                                .map_or("—", |unit| unit.display_unit.as_str()),
                        );
                        ui.label(if column.nullable { "yes" } else { "no" });
                        ui.end_row();
                    }
                });
        });
}

fn value_preview(ui: &mut egui::Ui, preview: &TableImportPreviewState) {
    ui.strong("First rows");
    let typed = &preview.candidates[preview.selected].typed_state;
    let snapshot = &typed.envelope.revision.snapshot;
    let codecs = plotx_core::data::CodecRegistry::with_arrow_ipc();
    let reader = plotx_core::data::SnapshotReader::new(snapshot, typed.store.as_ref(), &codecs);
    let batch = reader.and_then(|reader| reader.read_batch(0, &[]));
    let Ok(batch) = batch else {
        ui.colored_label(
            ui.visuals().error_fg_color,
            "The preview batch could not be read.",
        );
        return;
    };
    egui::ScrollArea::horizontal()
        .max_height(210.0)
        .show(ui, |ui| {
            egui::Grid::new("table_import_value_grid")
                .striped(true)
                .show(ui, |ui| {
                    for column in &snapshot.schema.columns {
                        ui.strong(&column.name);
                    }
                    ui.end_row();
                    let rows = batch.row_ids.len().min(8);
                    for row in 0..rows {
                        for (index, (_, chunk)) in batch.columns.iter().enumerate() {
                            let value = chunk.value(row).unwrap_or(ScalarValue::Null);
                            ui.label(scalar_text(
                                &value,
                                &snapshot.schema.columns[index].logical_type,
                            ));
                        }
                        ui.end_row();
                    }
                });
        });
}

fn logical_type_label(value: &LogicalType) -> &'static str {
    match value {
        LogicalType::Null => "Null",
        LogicalType::Boolean => "Boolean",
        LogicalType::Int64 => "Int64",
        LogicalType::Float64 => "Float64",
        LogicalType::Utf8 => "Text",
        LogicalType::Categorical { .. } => "Categorical",
        LogicalType::Date => "Date",
        LogicalType::Time => "Time",
        LogicalType::Timestamp { .. } => "Timestamp",
        LogicalType::Duration => "Duration",
        LogicalType::Extension(_) => "Extension",
    }
}

fn scalar_text(value: &ScalarValue, logical_type: &LogicalType) -> String {
    match value {
        ScalarValue::Null => "NULL".into(),
        ScalarValue::Boolean(value) => value.to_string(),
        ScalarValue::Int64(value) => value.to_string(),
        ScalarValue::Float64(value) if value.is_nan() => "NaN".into(),
        ScalarValue::Float64(value) if *value == f64::INFINITY => "+Inf".into(),
        ScalarValue::Float64(value) if *value == f64::NEG_INFINITY => "-Inf".into(),
        ScalarValue::Float64(value) => value.to_string(),
        ScalarValue::Utf8(value) => value.clone(),
        ScalarValue::Categorical(index) => match logical_type {
            LogicalType::Categorical { levels } => levels
                .get(*index as usize)
                .map_or_else(|| format!("#{index}"), |level| level.value.clone()),
            _ => format!("#{index}"),
        },
        ScalarValue::Date(value) => value.to_string(),
        ScalarValue::Time(value) | ScalarValue::Timestamp(value) | ScalarValue::Duration(value) => {
            value.to_string()
        }
        ScalarValue::Extension { storage, .. } => scalar_text(storage, logical_type),
    }
}
