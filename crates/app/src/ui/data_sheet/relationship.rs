//! "Combine" menu: union and join against another table in the project.

use egui::Ui;
use plotx_core::data::{
    ColumnId, ColumnRename, JoinCardinality, JoinKey, JoinKind, RelPlanV1, Relation, SnapshotRead,
};
use plotx_core::state::TableDataset;

use super::column_menu::table_name;
use super::transform::{TableCatalogEntry, TableTransformRequest};

pub(super) fn combine_menu(
    ui: &mut Ui,
    table: &TableDataset,
    dataset: usize,
    running: bool,
    request: &mut Option<TableTransformRequest>,
    catalog: &[TableCatalogEntry],
) {
    ui.set_min_width(220.0);
    let candidates = catalog
        .iter()
        .filter(|entry| entry.dataset != dataset)
        .collect::<Vec<_>>();
    let Some(first) = candidates.first() else {
        ui.weak("Import another table to union or join with this one.");
        return;
    };
    let choice_id = ui.id().with("table_relationship_input");
    let mut other = ui
        .ctx()
        .data_mut(|data| data.get_temp::<usize>(choice_id))
        .filter(|selected| candidates.iter().any(|entry| entry.dataset == *selected))
        .unwrap_or(first.dataset);
    let right = candidates
        .iter()
        .find(|entry| entry.dataset == other)
        .copied()
        .unwrap_or(*first);
    ui.horizontal(|ui| {
        ui.label("Other table");
        egui::ComboBox::from_id_salt(choice_id.with("combo"))
            .selected_text(&right.name)
            .show_ui(ui, |ui| {
                for entry in &candidates {
                    ui.selectable_value(&mut other, entry.dataset, &entry.name);
                }
            });
    });
    union_button(ui, table, dataset, running, request, right);
    ui.separator();
    join_section(ui, table, dataset, running, request, right, choice_id);
    ui.ctx().data_mut(|data| data.insert_temp(choice_id, other));
}

fn union_button(
    ui: &mut Ui,
    table: &TableDataset,
    dataset: usize,
    running: bool,
    request: &mut Option<TableTransformRequest>,
    right: &TableCatalogEntry,
) {
    let union_ok = right.schema == table.typed_state.envelope.revision.snapshot.schema;
    if ui
        .add_enabled(!running && union_ok, egui::Button::new("Union rows"))
        .on_hover_text("Requires identical stable column schemas")
        .clicked()
    {
        *request = Some(TableTransformRequest {
            input_datasets: vec![dataset, right.dataset],
            name: format!("{} — union", table_name(table)),
            plan: RelPlanV1::new(Relation::Union {
                inputs: vec![
                    Relation::SnapshotRead(snapshot_read(table)),
                    Relation::SnapshotRead(right.read.clone()),
                ],
            }),
        });
        ui.close();
    }
}

fn join_section(
    ui: &mut Ui,
    table: &TableDataset,
    dataset: usize,
    running: bool,
    request: &mut Option<TableTransformRequest>,
    right: &TableCatalogEntry,
    choice_id: egui::Id,
) {
    let schema = &table.typed_state.envelope.revision.snapshot.schema;
    let Some(first_column) = schema.columns.first() else {
        return;
    };
    let key_id = choice_id.with("left_key");
    let mut left_column = ui
        .ctx()
        .data_mut(|data| data.get_temp::<ColumnId>(key_id))
        .filter(|id| schema.column(*id).is_some())
        .unwrap_or(first_column.id);
    ui.horizontal(|ui| {
        ui.label("Key column");
        egui::ComboBox::from_id_salt(key_id.with("combo"))
            .selected_text(
                schema
                    .column(left_column)
                    .map_or("Missing", |column| column.name.as_str()),
            )
            .show_ui(ui, |ui| {
                for column in &schema.columns {
                    ui.selectable_value(&mut left_column, column.id, &column.name);
                }
            });
    });
    let kind_id = choice_id.with("kind");
    let mut kind = ui
        .ctx()
        .data_mut(|data| data.get_temp::<JoinKind>(kind_id))
        .unwrap_or(JoinKind::Inner);
    let cardinality_id = choice_id.with("cardinality");
    let mut cardinality = ui
        .ctx()
        .data_mut(|data| data.get_temp::<JoinCardinality>(cardinality_id))
        .unwrap_or(JoinCardinality::OneToOne);
    ui.horizontal(|ui| {
        egui::ComboBox::from_id_salt(kind_id)
            .selected_text(join_kind_label(kind))
            .show_ui(ui, |ui| {
                for value in [JoinKind::Inner, JoinKind::Left, JoinKind::Full] {
                    ui.selectable_value(&mut kind, value, join_kind_label(value));
                }
            });
        egui::ComboBox::from_id_salt(cardinality_id)
            .selected_text(cardinality_label(cardinality))
            .show_ui(ui, |ui| {
                for value in [
                    JoinCardinality::OneToOne,
                    JoinCardinality::OneToMany,
                    JoinCardinality::ManyToOne,
                    JoinCardinality::ManyToMany,
                ] {
                    ui.selectable_value(&mut cardinality, value, cardinality_label(value));
                }
            });
    });
    join_button(
        ui,
        table,
        dataset,
        left_column,
        running,
        request,
        right,
        kind,
        cardinality,
    );
    ui.ctx().data_mut(|data| {
        data.insert_temp(key_id, left_column);
        data.insert_temp(kind_id, kind);
        data.insert_temp(cardinality_id, cardinality);
    });
}

#[allow(clippy::too_many_arguments)]
fn join_button(
    ui: &mut Ui,
    table: &TableDataset,
    dataset: usize,
    left_column: ColumnId,
    running: bool,
    request: &mut Option<TableTransformRequest>,
    right: &TableCatalogEntry,
    kind: JoinKind,
    cardinality: JoinCardinality,
) {
    let left_schema = table
        .typed_state
        .envelope
        .revision
        .snapshot
        .schema
        .column(left_column)
        .unwrap();
    let right_key = right.schema.columns.iter().find(|column| {
        column.logical_type == left_schema.logical_type && column.unit == left_schema.unit
    });
    if ui
        .add_enabled(
            !running && right_key.is_some(),
            egui::Button::new("Join tables"),
        )
        .on_hover_text("Null never matches null; selected cardinality is validated")
        .clicked()
    {
        let right_relation = renamed_right(table, right);
        *request = Some(TableTransformRequest {
            input_datasets: vec![dataset, right.dataset],
            name: format!("{} — joined", table_name(table)),
            plan: RelPlanV1::new(Relation::Join {
                left: Box::new(Relation::SnapshotRead(snapshot_read(table))),
                right: Box::new(right_relation),
                kind,
                keys: vec![JoinKey {
                    left: left_column,
                    right: right_key.unwrap().id,
                }],
                cardinality,
            }),
        });
        ui.close();
    }
}

fn renamed_right(table: &TableDataset, right: &TableCatalogEntry) -> Relation {
    let left = &table.typed_state.envelope.revision.snapshot.schema;
    let renames = right
        .schema
        .columns
        .iter()
        .filter(|column| left.columns.iter().any(|left| left.name == column.name))
        .map(|column| ColumnRename {
            column: column.id,
            name: format!("right_{}", column.name),
        })
        .collect::<Vec<_>>();
    let read = Relation::SnapshotRead(right.read.clone());
    if renames.is_empty() {
        read
    } else {
        Relation::Rename {
            input: Box::new(read),
            renames,
        }
    }
}

fn snapshot_read(table: &TableDataset) -> SnapshotRead {
    let revision = &table.typed_state.envelope.revision;
    SnapshotRead {
        table: revision.table_id,
        revision: revision.id,
        fingerprint: revision.snapshot.fingerprint,
    }
}

fn join_kind_label(kind: JoinKind) -> &'static str {
    match kind {
        JoinKind::Inner => "Inner",
        JoinKind::Left => "Left",
        JoinKind::Full => "Full",
    }
}

fn cardinality_label(cardinality: JoinCardinality) -> &'static str {
    match cardinality {
        JoinCardinality::OneToOne => "1:1",
        JoinCardinality::OneToMany => "1:N",
        JoinCardinality::ManyToOne => "N:1",
        JoinCardinality::ManyToMany => "N:N",
    }
}
