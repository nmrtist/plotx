use super::*;
use crate::state::{FloatSeries, materialized_float_column, materialized_float_series_table};
use plotx_data::{
    ColumnId, Expression, FiniteOrSpecial, LiteralValue, Relation, RowMapping, SnapshotRead,
};

fn state_with_ids(
    x: Vec<f64>,
    y: Vec<f64>,
    ids: Option<(ColumnId, ColumnId)>,
) -> (TypedTableState, ColumnId, ColumnId) {
    let (mut x_schema, x_values) = materialized_float_column("time", "", x.into_iter().map(Some));
    let (mut y_schema, y_values) = materialized_float_column("signal", "", y.into_iter().map(Some));
    if let Some((x, y)) = ids {
        x_schema.id = x;
        y_schema.id = y;
    }
    let ids = (x_schema.id, y_schema.id);
    let state = TypedTableState::materialized(
        vec![(x_schema, x_values), (y_schema, y_values)],
        Vec::new(),
        "plotx.test.execution-input.v1",
    )
    .unwrap();
    (state, ids.0, ids.1)
}

#[test]
fn one_plan_boundary_persists_snapshot_revision_diagnostics_and_row_mapping() {
    let (mut input, _, signal) = state_with_ids(vec![0.0, 1.0, 2.0], vec![1.0, 2.0, 3.0], None);
    let revision = &input.envelope.revision;
    let plan = plotx_data::RelPlanV1::new(Relation::Filter {
        input: Box::new(Relation::SnapshotRead(SnapshotRead {
            table: revision.table_id,
            revision: revision.id,
            fingerprint: revision.snapshot.fingerprint,
        })),
        predicate: Expression::call(
            "eq.v1",
            vec![
                Expression::column(signal),
                Expression::Literal {
                    value: LiteralValue::Float64(FiniteOrSpecial::new(2.0)),
                },
            ],
        ),
    });
    let derived = execute_typed_plan(
        plan.clone(),
        &[&input],
        TableId::new(),
        16 * 1024 * 1024,
        &BTreeSet::new(),
    )
    .unwrap();

    assert_eq!(derived.envelope.revision.snapshot.row_count, 1);
    assert_eq!(derived.envelope.revision.reason, RevisionReason::Transform);
    assert_eq!(derived.envelope.revision.operation.plan, Some(plan));
    assert_eq!(
        derived.envelope.revision.operation.column_lineage.len(),
        derived.envelope.revision.snapshot.schema.columns.len()
    );
    assert!(
        derived
            .envelope
            .revision
            .operation
            .column_lineage
            .iter()
            .any(|lineage| lineage.output == signal && lineage.inputs == [signal])
    );
    assert!(matches!(
        derived.envelope.revision.operation.row_mapping,
        Some(RowMapping::Selection { ref runs }) if runs == &[(1, 1)]
    ));
    assert_eq!(
        derived.envelope.revision.operation.parameters["backend"],
        if cfg!(feature = "datafusion") {
            "plotx.datafusion.v1"
        } else {
            "plotx.reference.v1"
        }
    );

    let old_derived_revision = derived.envelope.revision.id;
    let old_fingerprint = derived.envelope.revision.snapshot.fingerprint;
    let source_row = plotx_data::SnapshotReader::new(
        &input.envelope.revision.snapshot,
        input.store.as_ref(),
        &plotx_data::CodecRegistry::with_arrow_ipc(),
    )
    .unwrap()
    .read_row_ids(0)
    .unwrap()
    .1[0];
    let mut edit = plotx_data::TableTransaction::new(&input.envelope.revision);
    edit.set(
        source_row,
        signal,
        LiteralValue::Float64(FiniteOrSpecial::new(2.0)),
    );
    let revision = edit
        .execute_and_commit(
            &input.envelope.revision,
            input.store.as_ref(),
            &plotx_data::CodecRegistry::with_arrow_ipc(),
            env!("CARGO_PKG_VERSION"),
        )
        .unwrap();
    input.envelope.advance(revision).unwrap();
    let refreshed =
        refresh_typed_plan(&derived, &[&input], 16 * 1024 * 1024, &BTreeSet::new()).unwrap();
    assert_eq!(refreshed.envelope.revision.reason, RevisionReason::Refresh);
    assert_eq!(refreshed.envelope.revision.parents, [old_derived_revision]);
    assert_eq!(refreshed.envelope.revision.snapshot.row_count, 2);
    assert_ne!(
        refreshed.envelope.revision.snapshot.fingerprint,
        old_fingerprint
    );
    assert!(
        refreshed
            .envelope
            .history
            .iter()
            .any(|revision| revision.id == old_derived_revision)
    );
}

#[test]
fn union_and_unpivot_persist_compact_semantic_row_mappings() {
    let (left, x, signal) = state_with_ids(vec![0.0, 1.0], vec![1.0, 2.0], None);
    let (right, _, _) = state_with_ids(vec![2.0], vec![3.0], Some((x, signal)));
    let reads = [&left, &right]
        .map(|state| {
            let revision = &state.envelope.revision;
            Relation::SnapshotRead(SnapshotRead {
                table: revision.table_id,
                revision: revision.id,
                fingerprint: revision.snapshot.fingerprint,
            })
        })
        .to_vec();
    let union = execute_typed_plan(
        plotx_data::RelPlanV1::new(Relation::Union { inputs: reads }),
        &[&left, &right],
        TableId::new(),
        16 * 1024 * 1024,
        &BTreeSet::new(),
    )
    .unwrap();
    assert!(matches!(
        union.envelope.revision.operation.row_mapping,
        Some(RowMapping::UnionNamespaces { ref sources }) if sources == &[
            left.envelope.revision.table_id,
            right.envelope.revision.table_id,
        ]
    ));

    let revision = &left.envelope.revision;
    let name = plotx_data::ColumnSchema::new("quantity", plotx_data::LogicalType::Utf8);
    let value = plotx_data::ColumnSchema::new("reading", plotx_data::LogicalType::Float64);
    let unpivot = execute_typed_plan(
        plotx_data::RelPlanV1::new(Relation::Unpivot {
            input: Box::new(Relation::SnapshotRead(SnapshotRead {
                table: revision.table_id,
                revision: revision.id,
                fingerprint: revision.snapshot.fingerprint,
            })),
            ids: Vec::new(),
            values: vec![x, signal],
            name_column: Box::new(name),
            value_column: Box::new(value),
        }),
        &[&left],
        TableId::new(),
        16 * 1024 * 1024,
        &BTreeSet::new(),
    )
    .unwrap();
    assert!(matches!(
        unpivot.envelope.revision.operation.row_mapping,
        Some(RowMapping::Unpivot { source, ref value_columns })
            if source == revision.table_id && value_columns == &[x, signal]
    ));
}

#[test]
fn generic_relational_result_needs_no_fake_xy_binding_and_reopens() {
    let source = materialized_float_series_table(
        (
            "time".into(),
            "s".into(),
            vec![Some(0.0), Some(1.0), Some(2.0)],
        ),
        vec![FloatSeries {
            name: "signal".into(),
            unit: String::new(),
            values: vec![Some(1.0), Some(2.0), Some(3.0)],
            uncertainty: None,
            fit: None,
        }],
        "plotx.test.execution-source.v1",
    )
    .unwrap();
    let revision = &source.typed_state.envelope.revision;
    let signal = source.series_bindings[0].value_column;
    let plan = plotx_data::RelPlanV1::new(Relation::Project {
        input: Box::new(Relation::SnapshotRead(SnapshotRead {
            table: revision.table_id,
            revision: revision.id,
            fingerprint: revision.snapshot.fingerprint,
        })),
        columns: vec![signal],
    });
    let mut app = crate::state::PlotxApp::new();
    app.doc
        .datasets
        .push(crate::state::Dataset::Table(Box::new(source)));
    let derived = app
        .derive_table_from_plan(plan, &[0], "Projected signal".into(), 16 * 1024 * 1024)
        .unwrap();
    let table = app.doc.datasets[derived].as_table().unwrap();
    assert!(table.x_binding.is_none());
    assert!(table.series_bindings.is_empty());
    assert_eq!(
        table
            .typed_state
            .envelope
            .revision
            .snapshot
            .schema
            .columns
            .len(),
        1
    );
    assert_eq!(
        table.lineage.as_ref().unwrap().kind,
        crate::state::DerivationKind::RelationalTransform
    );

    let path = std::env::temp_dir().join(format!(
        "plotx-generic-derived-{}.plotx",
        uuid::Uuid::new_v4()
    ));
    crate::project::save_project(&app, &path, false).unwrap();
    let reopened = crate::project::load_project(&path).unwrap();
    std::fs::remove_file(path).unwrap();
    let table = reopened.doc.datasets[derived].as_table().unwrap();
    assert!(table.x_binding.is_none());
    assert_eq!(
        table
            .typed_state
            .envelope
            .revision
            .snapshot
            .schema
            .columns
            .len(),
        1
    );
    assert!(table.typed_state.envelope.revision.operation.plan.is_some());
}

#[test]
fn app_refresh_is_one_undoable_revision_switch() {
    let source = materialized_float_series_table(
        ("time".into(), "s".into(), vec![Some(0.0), Some(1.0)]),
        vec![FloatSeries {
            name: "signal".into(),
            unit: String::new(),
            values: vec![Some(1.0), Some(2.0)],
            uncertainty: None,
            fit: None,
        }],
        "plotx.test.refresh-source.v1",
    )
    .unwrap();
    let revision = &source.typed_state.envelope.revision;
    let plan = plotx_data::RelPlanV1::new(Relation::SnapshotRead(SnapshotRead {
        table: revision.table_id,
        revision: revision.id,
        fingerprint: revision.snapshot.fingerprint,
    }));
    let mut app = crate::state::PlotxApp::new();
    app.doc
        .datasets
        .push(crate::state::Dataset::Table(Box::new(source)));
    let derived = app
        .derive_table_from_plan(plan, &[0], "copy".into(), 16 * 1024 * 1024)
        .unwrap();
    let before = app.doc.datasets[derived]
        .as_table()
        .unwrap()
        .typed_state
        .envelope
        .revision
        .id;
    app.refresh_derived_table(derived, &[0], 16 * 1024 * 1024)
        .unwrap();
    let refreshed = &app.doc.datasets[derived]
        .as_table()
        .unwrap()
        .typed_state
        .envelope
        .revision;
    assert_ne!(refreshed.id, before);
    assert_eq!(refreshed.parents, [before]);
    app.undo();
    assert_eq!(
        app.doc.datasets[derived]
            .as_table()
            .unwrap()
            .typed_state
            .envelope
            .revision
            .id,
        before
    );
}

#[test]
fn refresh_rebases_manual_patch_by_stable_row_identity() {
    let (source, _, signal) = state_with_ids(vec![0.0, 1.0], vec![2.0, 3.0], None);
    let source_revision = &source.envelope.revision;
    let derived = execute_typed_plan(
        plotx_data::RelPlanV1::new(Relation::Project {
            input: Box::new(Relation::SnapshotRead(SnapshotRead {
                table: source_revision.table_id,
                revision: source_revision.id,
                fingerprint: source_revision.snapshot.fingerprint,
            })),
            columns: source_revision
                .snapshot
                .schema
                .columns
                .iter()
                .map(|column| column.id)
                .collect(),
        }),
        &[&source],
        TableId::new(),
        16 * 1024 * 1024,
        &BTreeSet::new(),
    )
    .unwrap();
    let codecs = CodecRegistry::with_arrow_ipc();
    let row = plotx_data::SnapshotReader::new(
        &derived.envelope.revision.snapshot,
        derived.store.as_ref(),
        &codecs,
    )
    .unwrap()
    .read_row_ids(0)
    .unwrap()
    .1[0];
    let mut transaction = plotx_data::TableTransaction::new(&derived.envelope.revision);
    transaction.set(
        row,
        signal,
        LiteralValue::Float64(FiniteOrSpecial::new(99.0)),
    );
    let revision = transaction
        .execute_and_commit(
            &derived.envelope.revision,
            derived.store.as_ref(),
            &codecs,
            env!("CARGO_PKG_VERSION"),
        )
        .unwrap();
    let mut edited = derived;
    edited.envelope.advance(revision).unwrap();

    let refreshed =
        refresh_typed_plan(&edited, &[&source], 16 * 1024 * 1024, &BTreeSet::new()).unwrap();
    assert_eq!(refreshed.envelope.revision.reason, RevisionReason::Rebase);
    let batch = plotx_data::SnapshotReader::new(
        &refreshed.envelope.revision.snapshot,
        refreshed.store.as_ref(),
        &codecs,
    )
    .unwrap()
    .read_batch(0, &[signal])
    .unwrap();
    assert_eq!(
        batch.columns[0].1.value(0),
        Some(plotx_data::ScalarValue::Float64(99.0))
    );
}
