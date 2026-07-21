//! Typed table sheet: toolbar, spreadsheet grid, and status bar. Edit
//! recording still flows through `TableEditDelta`; the implementation model
//! (RowId, revisions, patches) never appears in the UI.

use egui::Ui;
use plotx_core::state::{TableDataset, TableEditDelta};

use super::grid::{self, SheetState};
use super::transform::TableSheetContext;

pub(super) fn typed_table_sheet(ui: &mut Ui, table: &TableDataset, context: TableSheetContext<'_>) {
    let TableSheetContext {
        dataset,
        commit,
        transform,
        refresh,
        catalog,
        transform_running,
    } = context;
    let typed = &table.typed_state;
    let snapshot = &typed.envelope.revision.snapshot;
    toolbar(
        ui,
        table,
        dataset,
        transform_running,
        transform,
        refresh,
        catalog,
    );
    ui.separator();
    let codecs = plotx_core::data::CodecRegistry::with_arrow_ipc();
    let reader =
        match plotx_core::data::SnapshotReader::new(snapshot, typed.store.as_ref(), &codecs) {
            Ok(reader) => reader,
            Err(error) => {
                ui.colored_label(ui.visuals().error_fg_color, error.to_string());
                return;
            }
        };
    let row_count = usize::try_from(snapshot.row_count).unwrap_or(usize::MAX);
    let state_id = ui
        .id()
        .with(("typed_table_state", typed.envelope.revision.table_id));
    let mut state = ui
        .ctx()
        .data_mut(|data| data.get_temp::<SheetState>(state_id))
        .unwrap_or_default();
    let outcome = grid::typed_table_grid(
        ui,
        &snapshot.schema,
        row_count,
        &snapshot.row_id_chunks,
        &reader,
        &mut state,
        &mut |ui, column| {
            super::column_menu::column_header_menu(
                ui,
                table,
                dataset,
                column,
                transform_running,
                transform,
            );
        },
    );
    status_bar(ui, table, row_count, &state, outcome.error.as_deref());
    ui.ctx().data_mut(|data| data.insert_temp(state_id, state));
    if let Some((row, column, before, after)) = outcome.edit {
        let mut delta = TableEditDelta::new_dataset(table);
        delta.record_typed_value(row, column, before, after);
        delta.finish_dataset(table);
        if !delta.is_empty() {
            *commit = Some(delta);
        }
    }
}

fn toolbar(
    ui: &mut Ui,
    table: &TableDataset,
    dataset: usize,
    running: bool,
    request: &mut Option<super::transform::TableTransformRequest>,
    refresh: &mut Option<(usize, Vec<usize>)>,
    catalog: &[super::transform::TableCatalogEntry],
) {
    ui.horizontal(|ui| {
        let refresh_sources = table
            .lineage
            .as_ref()
            .filter(|_| table.typed_state.envelope.revision.operation.plan.is_some())
            .map(|lineage| lineage.sources.clone());
        if ui
            .add_enabled(
                !running && refresh_sources.is_some(),
                egui::Button::new("Refresh"),
            )
            .on_hover_text("Re-run this table's source recipe and keep your cell edits")
            .clicked()
        {
            *refresh = Some((dataset, refresh_sources.unwrap()));
        }
        ui.menu_button("Combine", |ui| {
            super::relationship::combine_menu(ui, table, dataset, running, request, catalog);
        });
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.weak("Right-click a column header to sort, rename, or transform.");
        });
    });
}

fn status_bar(
    ui: &mut Ui,
    table: &TableDataset,
    row_count: usize,
    state: &SheetState,
    error: Option<&str>,
) {
    let schema = &table.typed_state.envelope.revision.snapshot.schema;
    let mut status = format!("{row_count} rows × {} columns", schema.columns.len());
    if let Some((row, column)) = state.cell {
        if let Some(column) = schema.column(column) {
            status.push_str(&format!(" · Row {}, {}", row + 1, column.name));
        }
    } else if let Some(row) = state.row {
        status.push_str(&format!(" · Row {}", row + 1));
    }
    ui.horizontal(|ui| {
        ui.weak(status);
        if let Some(error) = error {
            ui.colored_label(ui.visuals().error_fg_color, error);
        }
    });
}
