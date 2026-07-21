use super::tests::{completed, request};
use super::*;
use crate::LiteralValue;
use crate::{
    AggregateFunction, AggregateMeasure, CodecRegistry, ColumnChunk, ColumnId, ColumnSchema,
    ColumnValues, ExecutionInput, LogicalType, MemoryBlockStore, RelPlanV1, RevisionId, RowId,
    ScalarValue, SnapshotBuilder, SnapshotRead, TableId, TableSchema,
};
use std::{collections::BTreeMap, sync::Arc};

fn chunked_snapshot() -> (ExecutionInput, SnapshotRead, ColumnId, ColumnId) {
    let group = ColumnSchema::new("group", LogicalType::Utf8);
    let value = ColumnSchema::new("value", LogicalType::Float64);
    let schema = TableSchema::new(vec![group.clone(), value.clone()]).unwrap();
    let table_id = TableId::new();
    let revision = RevisionId::new();
    let store = Arc::new(MemoryBlockStore::default());
    let codecs = CodecRegistry::with_arrow_ipc();
    let mut builder = SnapshotBuilder::new(table_id, schema, store.as_ref(), &codecs).unwrap();
    let rows = DIFFERENTIAL_INPUT_ROW_LIMIT + 1;
    for start in (0..rows).step_by(10_000) {
        let end = (start + 10_000).min(rows);
        let row_ids = (start..end).map(|_| RowId::new()).collect::<Vec<_>>();
        builder
            .push_batch(
                &row_ids,
                &[
                    ColumnChunk::all_valid(ColumnValues::Utf8(
                        (start..end).map(|_| "g".to_owned()).collect(),
                    )),
                    ColumnChunk::all_valid(ColumnValues::Float64(
                        (start..end).map(|row| row as f64).collect(),
                    )),
                ],
            )
            .unwrap();
    }
    let snapshot = builder.finish().unwrap();
    let read = SnapshotRead {
        table: table_id,
        revision,
        fingerprint: snapshot.fingerprint,
    };
    (
        ExecutionInput::snapshot(snapshot, store).unwrap(),
        read,
        group.id,
        value.id,
    )
}

#[test]
fn large_row_preserving_plan_avoids_duplicate_reference_materialization() {
    let mut request = request(|read, schema| Relation::Filter {
        input: Box::new(Relation::SnapshotRead(read)),
        predicate: Expression::call(
            "eq.v1",
            vec![
                Expression::column(schema.columns[1].id),
                Expression::Literal {
                    value: LiteralValue::Float64(crate::FiniteOrSpecial::new(1.0)),
                },
            ],
        ),
    });
    let input = request
        .inputs
        .values_mut()
        .next()
        .unwrap()
        .materialized_table_mut()
        .unwrap();
    let rows = DIFFERENTIAL_INPUT_ROW_LIMIT + 1;
    input.row_ids = (0..rows).map(|_| RowId::new()).collect();
    input.columns[0].values = (0..rows)
        .map(|row| ScalarValue::Utf8(format!("group-{row}")))
        .collect();
    input.columns[1].values = (0..rows)
        .map(|row| {
            if row.is_multiple_of(10) {
                ScalarValue::Null
            } else {
                ScalarValue::Float64(row as f64)
            }
        })
        .collect();

    let output = completed(request);
    assert_eq!(output.table.row_ids.len(), 1);
    assert!(
        output
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "backend.differential.deferred")
    );
    assert_eq!(
        output
            .diagnostics
            .iter()
            .find(|diagnostic| diagnostic.code == "filter.null_predicate")
            .unwrap()
            .counts["rows"],
        5_001
    );
}

#[test]
fn cancellation_stops_large_execution_before_result_publication() {
    let mut request = request(|read, _| Relation::SnapshotRead(read));
    let rows = DIFFERENTIAL_INPUT_ROW_LIMIT + 1;
    let input = request
        .inputs
        .values_mut()
        .next()
        .unwrap()
        .materialized_table_mut()
        .unwrap();
    input.row_ids = (0..rows).map(|_| RowId::new()).collect();
    input.columns[0].values = (0..rows)
        .map(|_| ScalarValue::Utf8("group".into()))
        .collect();
    input.columns[1].values = (0..rows)
        .map(|row| ScalarValue::Float64(row as f64))
        .collect();

    let handle = DataFusionExecutionService.execute(request);
    handle.cancel();
    loop {
        match handle.recv().unwrap() {
            ExecutionEvent::Progress(_) => {}
            ExecutionEvent::Failed(DataError::Cancelled) => break,
            ExecutionEvent::Failed(error) => panic!("unexpected execution failure: {error}"),
            ExecutionEvent::Completed(_) => panic!("cancelled execution published a result"),
        }
    }
}

#[test]
fn chunked_snapshot_input_executes_without_a_materialized_input_table() {
    let (input, read, _, value) = chunked_snapshot();
    let request = ExecutionRequest {
        plan: RelPlanV1::new(Relation::Filter {
            input: Box::new(Relation::SnapshotRead(read.clone())),
            predicate: Expression::call(
                "eq.v1",
                vec![
                    Expression::column(value),
                    Expression::Literal {
                        value: LiteralValue::Float64(crate::FiniteOrSpecial::new(42.0)),
                    },
                ],
            ),
        }),
        inputs: BTreeMap::from([((read.table, read.revision), input)]),
        memory_limit_bytes: 32 * 1024 * 1024,
    };

    let output = completed(request);
    assert_eq!(output.table.row_ids.len(), 1);
    assert_eq!(
        output.table.columns[1].values,
        vec![ScalarValue::Float64(42.0)]
    );
}

#[test]
fn directory_backed_snapshot_streams_through_the_execution_boundary() {
    let root = std::env::temp_dir().join(format!("plotx-data-{}", TableId::new()));
    let store = Arc::new(crate::DirectoryBlockStore::open(&root).unwrap());
    let value = ColumnSchema::new("value", LogicalType::Float64);
    let schema = TableSchema::new(vec![value.clone()]).unwrap();
    let table = TableId::new();
    let revision = RevisionId::new();
    let codecs = CodecRegistry::with_arrow_ipc();
    let mut builder = SnapshotBuilder::new(table, schema, store.as_ref(), &codecs).unwrap();
    builder
        .push_batch(
            &[RowId::new(), RowId::new()],
            &[ColumnChunk::all_valid(ColumnValues::Float64(vec![
                1.0, 2.0,
            ]))],
        )
        .unwrap();
    let snapshot = builder.finish().unwrap();
    let read = SnapshotRead {
        table,
        revision,
        fingerprint: snapshot.fingerprint,
    };
    let request = ExecutionRequest {
        plan: RelPlanV1::new(Relation::SnapshotRead(read.clone())),
        inputs: BTreeMap::from([(
            (table, revision),
            ExecutionInput::snapshot(snapshot, store).unwrap(),
        )]),
        memory_limit_bytes: 1024 * 1024,
    };
    let output = completed(request);
    assert_eq!(output.table.row_ids.len(), 2);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn large_row_preserving_output_is_written_directly_as_snapshot_batches() {
    let (input, read, _, _) = chunked_snapshot();
    let request = ExecutionRequest {
        plan: RelPlanV1::new(Relation::SnapshotRead(read.clone())),
        inputs: BTreeMap::from([((read.table, read.revision), input)]),
        memory_limit_bytes: 16 * 1024 * 1024,
    };
    let output_store = MemoryBlockStore::default();
    let codecs = CodecRegistry::with_arrow_ipc();
    let output =
        execute_datafusion_to_snapshot(&request, TableId::new(), &output_store, &codecs).unwrap();

    assert_eq!(
        output.snapshot.row_count,
        (DIFFERENTIAL_INPUT_ROW_LIMIT + 1) as u64
    );
    assert!(output.snapshot.batch_count() > 1);
    assert!(output.row_ids.is_none());
    assert!(
        output
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "backend.output.streamed")
    );
    let last = crate::SnapshotReader::new(&output.snapshot, &output_store, &codecs)
        .unwrap()
        .read_batch(output.snapshot.batch_count() - 1, &[])
        .unwrap();
    assert_eq!(
        last.columns[1].1.value(last.row_ids.len() - 1),
        Some(ScalarValue::Float64(50_000.0))
    );
}

#[test]
fn chunked_snapshot_aggregate_keeps_only_compact_identity_state() {
    let (input, read, group, value) = chunked_snapshot();
    let request = ExecutionRequest {
        plan: RelPlanV1::new(Relation::Aggregate {
            input: Box::new(Relation::SnapshotRead(read.clone())),
            groups: vec![group],
            measures: vec![AggregateMeasure {
                output: ColumnSchema::new("mean", LogicalType::Float64),
                function: AggregateFunction::MeanV1,
                input: Some(Expression::column(value)),
            }],
        }),
        inputs: BTreeMap::from([((read.table, read.revision), input)]),
        memory_limit_bytes: 32 * 1024 * 1024,
    };

    let output = completed(request);
    assert_eq!(output.table.row_ids.len(), 1);
    assert_eq!(
        output.table.columns[0].values,
        vec![ScalarValue::Utf8("g".into())]
    );
    assert_eq!(
        output.table.columns[1].values,
        vec![ScalarValue::Float64(25_000.0)]
    );
}

#[test]
fn memory_budget_limits_operators_without_rejecting_large_inputs() {
    let mut request = request(|read, _| Relation::SnapshotRead(read));
    let input = request
        .inputs
        .values_mut()
        .next()
        .unwrap()
        .materialized_table_mut()
        .unwrap();
    input.row_ids = (0..10_000).map(|_| RowId::new()).collect();
    input.columns[0].values = (0..10_000)
        .map(|_| ScalarValue::Utf8("one group".into()))
        .collect();
    input.columns[1].values = (0..10_000)
        .map(|value| ScalarValue::Float64(value as f64))
        .collect();
    let read = match &request.plan.root {
        Relation::SnapshotRead(read) => read.clone(),
        _ => unreachable!(),
    };
    let count = ColumnSchema::new("count", LogicalType::Int64);
    request.plan = RelPlanV1::new(Relation::Aggregate {
        input: Box::new(Relation::SnapshotRead(read)),
        groups: Vec::new(),
        measures: vec![AggregateMeasure {
            output: count,
            function: AggregateFunction::CountAll,
            input: None,
        }],
    });
    request.memory_limit_bytes = 64 * 1024;

    let output = completed(request);
    assert_eq!(output.backend, "plotx.datafusion.v1");
    assert_eq!(
        output.table.columns[0].values,
        vec![ScalarValue::Int64(10_000)]
    );
}

#[test]
fn large_snapshot_aggregate_uses_compact_identity_oracle_and_keeps_diagnostics() {
    let mut request = request(|read, schema| Relation::Aggregate {
        input: Box::new(Relation::SnapshotRead(read)),
        groups: vec![schema.columns[0].id],
        measures: vec![AggregateMeasure {
            output: ColumnSchema::new("mean", LogicalType::Float64),
            function: AggregateFunction::MeanV1,
            input: Some(Expression::column(schema.columns[1].id)),
        }],
    });
    let rows = DIFFERENTIAL_INPUT_ROW_LIMIT + 1;
    let input = request
        .inputs
        .values_mut()
        .next()
        .unwrap()
        .materialized_table_mut()
        .unwrap();
    input.row_ids = (0..rows).map(|_| RowId::new()).collect();
    input.columns[0].values = (0..rows)
        .map(|row| ScalarValue::Utf8(if row % 2 == 0 { "a" } else { "b" }.into()))
        .collect();
    input.columns[1].values = (0..rows)
        .map(|row| {
            if row.is_multiple_of(10) {
                ScalarValue::Null
            } else {
                ScalarValue::Float64(row as f64)
            }
        })
        .collect();

    let output = completed(request);
    assert_eq!(output.table.row_ids.len(), 2);
    assert_eq!(
        output.table.columns[0].values,
        vec![ScalarValue::Utf8("a".into()), ScalarValue::Utf8("b".into())]
    );
    assert_eq!(
        output
            .diagnostics
            .iter()
            .find(|diagnostic| diagnostic.code == "aggregate.null_excluded")
            .unwrap()
            .counts["rows"],
        5_001
    );
    assert!(
        output
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "backend.differential.deferred")
    );
}
