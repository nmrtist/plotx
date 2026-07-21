//! Context menu on a column header: rename plus every column-scoped
//! transform. The menu replaces the old always-visible transform toolbar.

use egui::Ui;
use plotx_core::data::{
    AggregateFunction, AggregateMeasure, ColumnId, ColumnRename, ColumnSchema, Expression,
    LogicalType, NullPlacement, RelPlanV1, Relation, SnapshotRead, SortDirection, SortKey,
    UnitRegistry,
};
use plotx_core::state::TableDataset;

use super::transform::TableTransformRequest;

pub(super) fn column_header_menu(
    ui: &mut Ui,
    table: &TableDataset,
    dataset: usize,
    column: ColumnId,
    running: bool,
    request: &mut Option<TableTransformRequest>,
) {
    ui.set_min_width(180.0);
    rename_entry(ui, table, dataset, column, running, request);
    ui.separator();
    ui.add_enabled_ui(!running, |ui| {
        sort_entries(ui, table, dataset, column, request);
        shape_entries(ui, table, dataset, column, request);
        unit_entries(ui, table, dataset, column, request);
    });
}

fn rename_entry(
    ui: &mut Ui,
    table: &TableDataset,
    dataset: usize,
    column: ColumnId,
    running: bool,
    request: &mut Option<TableTransformRequest>,
) {
    let current = schema(table).column(column).map(|c| c.name.clone());
    let Some(current) = current else { return };
    let state_id = ui.id().with(("column_rename", column.to_string()));
    let mut name = ui
        .ctx()
        .data_mut(|data| data.get_temp::<String>(state_id))
        .unwrap_or_else(|| current.clone());
    ui.horizontal(|ui| {
        ui.add(egui::TextEdit::singleline(&mut name).desired_width(120.0));
        let changed = !running && !name.trim().is_empty() && name.trim() != current;
        if ui
            .add_enabled(changed, egui::Button::new("Rename"))
            .clicked()
        {
            let name = name.trim().to_owned();
            *request = Some(single_input_request(
                dataset,
                table,
                format!("{} — renamed", table_name(table)),
                |input| Relation::Rename {
                    input: Box::new(input),
                    renames: vec![ColumnRename { column, name }],
                },
            ));
            ui.close();
        }
    });
    ui.ctx().data_mut(|data| data.insert_temp(state_id, name));
}

fn sort_entries(
    ui: &mut Ui,
    table: &TableDataset,
    dataset: usize,
    column: ColumnId,
    request: &mut Option<TableTransformRequest>,
) {
    for (label, direction) in [
        ("Sort ascending", SortDirection::Ascending),
        ("Sort descending", SortDirection::Descending),
    ] {
        if ui.button(label).clicked() {
            *request = Some(single_input_request(
                dataset,
                table,
                format!("{} — sorted", table_name(table)),
                |input| Relation::StableSort {
                    input: Box::new(input),
                    keys: vec![SortKey {
                        column,
                        direction,
                        nulls: NullPlacement::Last,
                    }],
                },
            ));
            ui.close();
        }
    }
}

fn shape_entries(
    ui: &mut Ui,
    table: &TableDataset,
    dataset: usize,
    column: ColumnId,
    request: &mut Option<TableTransformRequest>,
) {
    let source = schema(table).column(column).cloned();
    let Some(source) = source else { return };
    if ui.button("Keep only this column").clicked() {
        *request = Some(single_input_request(
            dataset,
            table,
            format!("{} — projected", table_name(table)),
            |input| Relation::Project {
                input: Box::new(input),
                columns: vec![column],
            },
        ));
        ui.close();
    }
    if ui.button("Filter out empty rows").clicked() {
        *request = Some(single_input_request(
            dataset,
            table,
            format!("{} — filtered", table_name(table)),
            |input| Relation::Filter {
                input: Box::new(input),
                predicate: Expression::call(
                    "not.v1",
                    vec![Expression::call(
                        "is_null.v1",
                        vec![Expression::column(column)],
                    )],
                ),
            },
        ));
        ui.close();
    }
    if source.logical_type == LogicalType::Float64
        && ui.button("Mark non-finite as missing").clicked()
    {
        *request = Some(single_input_request(
            dataset,
            table,
            format!("{} — missing marked", table_name(table)),
            |input| Relation::MarkMissing {
                input: Box::new(input),
                columns: vec![column],
                predicate: Expression::call(
                    "not.v1",
                    vec![Expression::call(
                        "is_finite.v1",
                        vec![Expression::column(column)],
                    )],
                ),
            },
        ));
        ui.close();
    }
    if ui.button("Duplicate as computed column").clicked() {
        let mut output =
            ColumnSchema::new(format!("{}_copy", source.name), source.logical_type.clone());
        output.nullable = source.nullable;
        output.unit = source.unit.clone();
        *request = Some(single_input_request(
            dataset,
            table,
            format!("{} — computed", table_name(table)),
            |input| Relation::ComputedColumn {
                input: Box::new(input),
                column: output,
                expression: Expression::column(column),
            },
        ));
        ui.close();
    }
    if ui.button("Count rows per value").clicked() {
        *request = Some(single_input_request(
            dataset,
            table,
            format!("{} — grouped", table_name(table)),
            |input| Relation::Aggregate {
                input: Box::new(input),
                groups: vec![column],
                measures: vec![AggregateMeasure {
                    output: ColumnSchema::new("row_count", LogicalType::Int64),
                    function: AggregateFunction::CountAll,
                    input: None,
                }],
            },
        ));
        ui.close();
    }
    unpivot_entry(ui, table, dataset, &source, request);
    pivot_entry(ui, table, dataset, column, &source, request);
}

fn unpivot_entry(
    ui: &mut Ui,
    table: &TableDataset,
    dataset: usize,
    source: &ColumnSchema,
    request: &mut Option<TableTransformRequest>,
) {
    let peers = schema(table)
        .columns
        .iter()
        .filter(|candidate| candidate.logical_type == source.logical_type)
        .map(|candidate| candidate.id)
        .collect::<Vec<_>>();
    if peers.len() < 2 {
        return;
    }
    if ui
        .button("Unpivot matching columns")
        .on_hover_text("Stack all columns of this type into name/value rows")
        .clicked()
    {
        let ids = schema(table)
            .columns
            .iter()
            .map(|candidate| candidate.id)
            .filter(|id| !peers.contains(id))
            .collect();
        let value_type = source.logical_type.clone();
        *request = Some(single_input_request(
            dataset,
            table,
            format!("{} — unpivoted", table_name(table)),
            |input| Relation::Unpivot {
                input: Box::new(input),
                ids,
                values: peers,
                name_column: Box::new(ColumnSchema::new("source_column", LogicalType::Utf8)),
                value_column: Box::new(ColumnSchema::new("value", value_type)),
            },
        ));
        ui.close();
    }
}

fn pivot_entry(
    ui: &mut Ui,
    table: &TableDataset,
    dataset: usize,
    names_from: ColumnId,
    source: &ColumnSchema,
    request: &mut Option<TableTransformRequest>,
) {
    if !matches!(
        source.logical_type,
        LogicalType::Utf8 | LogicalType::Categorical { .. }
    ) {
        return;
    }
    let Some(values) = schema(table)
        .columns
        .iter()
        .find(|candidate| {
            candidate.id != names_from && candidate.logical_type == LogicalType::Float64
        })
        .cloned()
    else {
        return;
    };
    if ui
        .button("Pivot using these names")
        .on_hover_text(format!(
            "Use {} for column names and {} for values; other columns form groups",
            source.name, values.name
        ))
        .clicked()
    {
        let groups = schema(table)
            .columns
            .iter()
            .map(|candidate| candidate.id)
            .filter(|id| *id != names_from && *id != values.id)
            .collect();
        *request = Some(single_input_request(
            dataset,
            table,
            format!("{} — pivoted", table_name(table)),
            |input| Relation::Pivot {
                input: Box::new(input),
                groups,
                names_from,
                values_from: values.id,
                aggregate: AggregateFunction::MeanV1,
            },
        ));
        ui.close();
    }
}

fn unit_entries(
    ui: &mut Ui,
    table: &TableDataset,
    dataset: usize,
    column: ColumnId,
    request: &mut Option<TableTransformRequest>,
) {
    let Some(schema) = schema(table).column(column).cloned() else {
        return;
    };
    let Some(source) = schema.unit.clone() else {
        return;
    };
    if schema.logical_type != LogicalType::Float64 {
        return;
    }
    let registry = UnitRegistry::plotx_v1();
    let choices = registry.compatible_codes(&source);
    if choices.len() < 2 {
        return;
    }
    ui.menu_button("Convert unit", |ui| {
        for code in choices {
            if code == source.display_unit {
                continue;
            }
            if ui.button(code).clicked() {
                let target = registry.resolve(code).unwrap();
                let source = source.clone();
                *request = Some(single_input_request(
                    dataset,
                    table,
                    format!("{} — unit converted", table_name(table)),
                    |input| Relation::UnitConvert {
                        input: Box::new(input),
                        column,
                        source,
                        target,
                    },
                ));
                ui.close();
            }
        }
    });
}

fn schema(table: &TableDataset) -> &plotx_core::data::TableSchema {
    &table.typed_state.envelope.revision.snapshot.schema
}

pub(super) fn table_name(table: &TableDataset) -> &str {
    table.name.as_deref().unwrap_or("Data table")
}

pub(super) fn single_input_request(
    dataset: usize,
    table: &TableDataset,
    name: String,
    relation: impl FnOnce(Relation) -> Relation,
) -> TableTransformRequest {
    let revision = &table.typed_state.envelope.revision;
    let read = Relation::SnapshotRead(SnapshotRead {
        table: revision.table_id,
        revision: revision.id,
        fingerprint: revision.snapshot.fingerprint,
    });
    TableTransformRequest {
        input_datasets: vec![dataset],
        name,
        plan: RelPlanV1::new(relation(read)),
    }
}
