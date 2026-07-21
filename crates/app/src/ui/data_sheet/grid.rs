//! Spreadsheet-style grid for typed tables: painted cells, a single active
//! editor, keyboard navigation, and header context menus. Row identity
//! (RowId) stays internal; users see 1-based row numbers.

use egui::{Align, Event, Key, Layout, Modifiers, Sense, StrokeKind, Ui};
use egui_extras::{Column as TableColumn, TableBuilder};
use plotx_core::data::{
    ChunkDescriptor, ColumnId, LiteralValue, RowId, ScalarValue, SnapshotReader, TableBatch,
    TableSchema,
};

use super::cell;

#[derive(Clone, Default)]
pub(super) struct SheetState {
    /// Selected cell as (display row index, column).
    pub cell: Option<(usize, ColumnId)>,
    /// Whole-row selection from the row-number gutter.
    pub row: Option<usize>,
    pub edit: Option<EditState>,
    /// Row to scroll into view after keyboard navigation.
    scroll_to: Option<usize>,
}

#[derive(Clone)]
pub(super) struct EditState {
    pub row: usize,
    pub column: ColumnId,
    /// None until the first render seeds the buffer from the cell value.
    pub text: Option<String>,
    pub error: Option<String>,
    take_focus: bool,
}

#[derive(Default)]
pub(super) struct GridOutcome {
    pub edit: Option<(RowId, ColumnId, LiteralValue, LiteralValue)>,
    pub error: Option<String>,
}

/// What to do with the editor buffer once it is parsed.
#[derive(Clone, Copy, PartialEq)]
enum CommitMove {
    Stay,
    Down,
    Right,
}

pub(super) fn typed_table_grid(
    ui: &mut Ui,
    schema: &TableSchema,
    row_count: usize,
    chunks: &[ChunkDescriptor],
    reader: &SnapshotReader<'_>,
    state: &mut SheetState,
    column_menu: &mut dyn FnMut(&mut Ui, ColumnId),
) -> GridOutcome {
    let mut outcome = GridOutcome::default();
    sanitize_state(state, schema, row_count);
    if let Some((row, column, seed)) = keyboard_navigation(ui, schema, row_count, state) {
        // Type-to-edit seeds the buffer with the typed character(s).
        start_edit(state, row, column, seed);
        state.scroll_to = Some(row);
    }
    let copy_requested = ui.ctx().input(|input| {
        input
            .events
            .iter()
            .any(|event| matches!(event, Event::Copy))
    }) && state.edit.is_none()
        && no_widget_focused(ui);
    let row_height = ui.spacing().interact_size.y + 2.0;
    let header_height = row_height * 2.0 - 4.0;
    let gutter_width = 12.0 + 9.0 * (row_count.max(1).ilog10() as f32 + 1.0);
    let mut builder = TableBuilder::new(ui)
        .striped(true)
        .sense(Sense::click())
        .min_scrolled_height(120.0)
        .column(TableColumn::exact(gutter_width));
    for _ in &schema.columns {
        builder = builder.column(
            TableColumn::initial(120.0)
                .at_least(48.0)
                .resizable(true)
                .clip(true),
        );
    }
    if let Some(row) = state.scroll_to.take() {
        builder = builder.scroll_to_row(row, Some(Align::Center));
    }
    builder
        .header(header_height, |mut header| {
            header.col(|_ui| {});
            for column in &schema.columns {
                let (_, response) = header.col(|ui| {
                    ui.vertical(|ui| {
                        ui.strong(&column.name);
                        let mut detail = cell::type_label(&column.logical_type).to_owned();
                        if let Some(unit) = &column.unit {
                            detail = format!("{} · {detail}", unit.display_unit);
                        }
                        ui.weak(detail);
                    });
                });
                response.context_menu(|ui| column_menu(ui, column.id));
            }
        })
        .body(|body| {
            // Rows arrive in index order, so a one-batch cache serves the
            // whole visible window without repeated chunk decodes.
            let mut cache: Option<TableBatch> = None;
            body.rows(row_height, row_count, |mut row| {
                let index = row.index();
                row.set_selected(state.row == Some(index));
                let batch = batch_for_row(reader, chunks, index, &mut cache, &mut outcome.error);
                let local = batch
                    .map(|batch| index - usize::try_from(batch.row_start).unwrap_or(usize::MAX));
                gutter_cell(
                    &mut row,
                    schema,
                    state,
                    index,
                    copy_requested,
                    local.map(|local| (cache.as_ref().unwrap(), local)),
                );
                let Some(local) = local else {
                    for _ in &schema.columns {
                        row.col(|_ui| {});
                    }
                    return;
                };
                let batch = cache.as_ref().unwrap();
                let row_id = batch.row_ids[local];
                for ((column_id, values), column) in batch.columns.iter().zip(&schema.columns) {
                    let value = values.value(local).unwrap_or(ScalarValue::Null);
                    let editing = state
                        .edit
                        .as_ref()
                        .is_some_and(|edit| edit.row == index && edit.column == *column_id);
                    if editing {
                        editor_cell(
                            &mut row,
                            state,
                            schema,
                            row_count,
                            row_id,
                            &value,
                            column,
                            &mut outcome,
                        );
                        continue;
                    }
                    let selected = state.cell == Some((index, *column_id));
                    let (rect, response) = row.col(|ui| {
                        painted_value(ui, &value, column, copy_requested && selected);
                    });
                    if selected {
                        ui_selection_stroke(&mut row, rect);
                    }
                    if response.clicked() {
                        state.cell = Some((index, *column_id));
                        state.row = None;
                    }
                    if response.double_clicked() {
                        start_edit(state, index, *column_id, None);
                    }
                }
            });
        });
    outcome
}

/// Drop selection and edit state that no longer maps onto the current
/// schema or row count (e.g. after a transform replaced the table).
fn sanitize_state(state: &mut SheetState, schema: &TableSchema, row_count: usize) {
    let column_exists = |id: &ColumnId| schema.columns.iter().any(|column| column.id == *id);
    if state
        .cell
        .is_some_and(|(row, column)| row >= row_count || !column_exists(&column))
    {
        state.cell = None;
    }
    if state.row.is_some_and(|row| row >= row_count) {
        state.row = None;
    }
    if state
        .edit
        .as_ref()
        .is_some_and(|edit| edit.row >= row_count || !column_exists(&edit.column))
    {
        state.edit = None;
    }
}

fn no_widget_focused(ui: &Ui) -> bool {
    ui.ctx().memory(|memory| memory.focused().is_none())
}

/// Arrow-key navigation, Enter/F2 to edit, and type-to-edit. Returns
/// Some((row, column, seed_text)) when an edit should start.
fn keyboard_navigation(
    ui: &mut Ui,
    schema: &TableSchema,
    row_count: usize,
    state: &mut SheetState,
) -> Option<(usize, ColumnId, Option<String>)> {
    if state.edit.is_some() || row_count == 0 || schema.columns.is_empty() {
        return None;
    }
    if !no_widget_focused(ui) {
        return None;
    }
    let (mut row, column) = state.cell?;
    let mut column_index = schema
        .columns
        .iter()
        .position(|candidate| candidate.id == column)?;
    let mut moved = false;
    let mut start = None;
    ui.input_mut(|input| {
        let steps: [(Key, isize, isize); 4] = [
            (Key::ArrowUp, -1, 0),
            (Key::ArrowDown, 1, 0),
            (Key::ArrowLeft, 0, -1),
            (Key::ArrowRight, 0, 1),
        ];
        for (key, dr, dc) in steps {
            if input.consume_key(Modifiers::NONE, key) {
                row = row.saturating_add_signed(dr).min(row_count - 1);
                column_index = column_index
                    .saturating_add_signed(dc)
                    .min(schema.columns.len() - 1);
                moved = true;
            }
        }
        if input.consume_key(Modifiers::NONE, Key::Enter)
            || input.consume_key(Modifiers::NONE, Key::F2)
        {
            start = Some(None);
        }
        if start.is_none() {
            let typed: String = input
                .events
                .iter()
                .filter_map(|event| match event {
                    Event::Text(text) => Some(text.as_str()),
                    _ => None,
                })
                .collect();
            if !typed.is_empty() {
                start = Some(Some(typed));
                input
                    .events
                    .retain(|event| !matches!(event, Event::Text(_)));
            }
        }
    });
    let column = schema.columns[column_index].id;
    if moved {
        state.cell = Some((row, column));
        state.row = None;
        state.scroll_to = Some(row);
    }
    start.map(|seed| (row, column, seed))
}

fn start_edit(state: &mut SheetState, row: usize, column: ColumnId, seed: Option<String>) {
    state.cell = Some((row, column));
    state.row = None;
    state.edit = Some(EditState {
        row,
        column,
        text: seed,
        error: None,
        take_focus: true,
    });
}

/// Locate and cache the batch containing `index`; None when reading failed.
fn batch_for_row<'c>(
    reader: &SnapshotReader<'_>,
    chunks: &[ChunkDescriptor],
    index: usize,
    cache: &'c mut Option<TableBatch>,
    error: &mut Option<String>,
) -> Option<&'c TableBatch> {
    let in_batch = |batch: &TableBatch| {
        let start = usize::try_from(batch.row_start).unwrap_or(usize::MAX);
        (start..start + batch.row_ids.len()).contains(&index)
    };
    if !cache.as_ref().is_some_and(in_batch) {
        let batch_index = batch_containing_row(chunks, index)?;
        match reader.read_batch(batch_index, &[]) {
            Ok(batch) => *cache = Some(batch),
            Err(read_error) => {
                *error = Some(read_error.to_string());
                *cache = None;
            }
        }
    }
    cache.as_ref().filter(|batch| in_batch(batch))
}

pub(super) fn batch_containing_row(chunks: &[ChunkDescriptor], row: usize) -> Option<usize> {
    let index = chunks.partition_point(|chunk| {
        let end = chunk.row_start.saturating_add(chunk.row_count);
        usize::try_from(end).unwrap_or(usize::MAX) <= row
    });
    (index < chunks.len()).then_some(index)
}

fn gutter_cell(
    row: &mut egui_extras::TableRow<'_, '_>,
    schema: &TableSchema,
    state: &mut SheetState,
    index: usize,
    copy_requested: bool,
    batch: Option<(&TableBatch, usize)>,
) {
    let selected = state.row == Some(index);
    let (_, response) = row.col(|ui| {
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.weak((index + 1).to_string());
        });
    });
    if response.clicked() {
        state.row = (!selected).then_some(index);
        state.cell = None;
        state.edit = None;
    }
    if selected
        && copy_requested
        && let Some((batch, local)) = batch
    {
        let line = batch
            .columns
            .iter()
            .zip(&schema.columns)
            .map(|((_, values), column)| {
                cell::typed_cell_text(
                    &values.value(local).unwrap_or(ScalarValue::Null),
                    &column.logical_type,
                )
            })
            .collect::<Vec<_>>()
            .join("\t");
        response.ctx.copy_text(line);
    }
}

fn painted_value(
    ui: &mut Ui,
    value: &ScalarValue,
    column: &plotx_core::data::ColumnSchema,
    copy_requested: bool,
) {
    let text = cell::typed_cell_text(value, &column.logical_type);
    if copy_requested {
        ui.ctx().copy_text(text.clone());
    }
    let placeholder = cell::is_placeholder(value);
    let shown = if matches!(value, ScalarValue::Null) {
        "—".to_owned()
    } else {
        text
    };
    let layout = if cell::right_aligned(&column.logical_type) {
        Layout::right_to_left(Align::Center)
    } else {
        Layout::left_to_right(Align::Center)
    };
    ui.with_layout(layout, |ui| {
        if placeholder {
            ui.weak(shown);
        } else {
            ui.label(shown);
        }
    });
}

fn ui_selection_stroke(row: &mut egui_extras::TableRow<'_, '_>, rect: egui::Rect) {
    let painter = row.response().ctx.layer_painter(egui::LayerId::new(
        egui::Order::Middle,
        egui::Id::new("plotx_typed_grid_selection"),
    ));
    let stroke = row.response().ctx.global_style().visuals.selection.stroke;
    painter.rect_stroke(rect, 1.0, stroke, StrokeKind::Inside);
}

#[allow(clippy::too_many_arguments)]
fn editor_cell(
    row: &mut egui_extras::TableRow<'_, '_>,
    state: &mut SheetState,
    schema: &TableSchema,
    row_count: usize,
    row_id: RowId,
    value: &ScalarValue,
    column: &plotx_core::data::ColumnSchema,
    outcome: &mut GridOutcome,
) {
    let index = state.edit.as_ref().map(|edit| edit.row).unwrap_or_default();
    let edit = state.edit.as_mut().unwrap();
    let original = cell::typed_cell_text(value, &column.logical_type);
    let text = edit.text.get_or_insert_with(|| original.clone());
    let mut commit = None;
    let mut cancel = false;
    let error = edit.error.clone();
    let take_focus = std::mem::take(&mut edit.take_focus);
    let (rect, _) = row.col(|ui| {
        let response = ui.add(
            egui::TextEdit::singleline(text)
                .desired_width(f32::INFINITY)
                .margin(egui::vec2(2.0, 0.0)),
        );
        if take_focus {
            response.request_focus();
            // Put the caret at the end instead of selecting everything so
            // type-to-edit keeps appending naturally.
            if let Some(mut editor_state) = egui::TextEdit::load_state(ui.ctx(), response.id) {
                let end = egui::text::CCursor::new(text.chars().count());
                editor_state
                    .cursor
                    .set_char_range(Some(egui::text::CCursorRange::one(end)));
                editor_state.store(ui.ctx(), response.id);
            }
        }
        let (enter, tab, escape) = ui.input(|input| {
            (
                input.key_pressed(Key::Enter),
                input.key_pressed(Key::Tab),
                input.key_pressed(Key::Escape),
            )
        });
        if escape {
            cancel = true;
        } else if enter {
            commit = Some(CommitMove::Down);
        } else if tab {
            commit = Some(CommitMove::Right);
        } else if response.lost_focus() {
            commit = Some(CommitMove::Stay);
        }
        if let Some(error) = &error {
            response.on_hover_text(error);
        }
    });
    if error.is_some() {
        let painter = row.response().ctx.layer_painter(egui::LayerId::new(
            egui::Order::Middle,
            egui::Id::new("plotx_typed_grid_error"),
        ));
        let color = row.response().ctx.global_style().visuals.error_fg_color;
        painter.rect_stroke(
            rect,
            1.0,
            egui::Stroke::new(1.5_f32, color),
            StrokeKind::Inside,
        );
    }
    if cancel {
        state.edit = None;
        return;
    }
    let Some(step) = commit else { return };
    let edit = state.edit.as_mut().unwrap();
    let text = edit.text.clone().unwrap_or_default();
    if text != original {
        match cell::parse_typed_cell(&text, column) {
            Ok(after) => match cell::scalar_to_literal(value) {
                Ok(before) => outcome.edit = Some((row_id, edit.column, before, after)),
                Err(error) => {
                    edit.error = Some(error);
                    edit.take_focus = true;
                    return;
                }
            },
            Err(error) => {
                edit.error = Some(error);
                edit.take_focus = true;
                return;
            }
        }
    }
    state.edit = None;
    let next = match step {
        CommitMove::Stay => Some((index, column.id)),
        CommitMove::Down => (index + 1 < row_count).then_some((index + 1, column.id)),
        CommitMove::Right => {
            let position = schema
                .columns
                .iter()
                .position(|candidate| candidate.id == column.id)
                .unwrap_or_default();
            schema
                .columns
                .get(position + 1)
                .map(|next| (index, next.id))
        }
    };
    if let Some((row, _)) = next.filter(|_| step != CommitMove::Stay) {
        state.scroll_to = Some(row);
    }
    state.cell = next.or(state.cell);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ten_million_row_lookup_lands_on_the_intersecting_chunk() {
        let chunks = (0..153)
            .map(|index| ChunkDescriptor {
                row_start: index * 65_536,
                row_count: if index == 152 { 38_528 } else { 65_536 },
                codec: plotx_core::data::ARROW_IPC_CODEC_V1.into(),
                byte_hash: plotx_core::data::ContentHash::of(&index.to_le_bytes()),
                logical_fingerprint: plotx_core::data::ContentHash::of(&(index + 1).to_le_bytes()),
            })
            .collect::<Vec<_>>();
        assert_eq!(batch_containing_row(&chunks, 8_765_420), Some(133));
        assert_eq!(batch_containing_row(&chunks, 0), Some(0));
        assert_eq!(batch_containing_row(&chunks, 10_000_000), None);
    }
}
