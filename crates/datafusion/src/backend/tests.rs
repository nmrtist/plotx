use super::*;
use crate::{
    AggregateFunction, AggregateMeasure, CellPatch, ColumnSchema, ContentHash, JoinCardinality,
    JoinKey, JoinKind, LogicalType, MaterializedColumn, RelPlanV1, RevisionId, RowId, ScalarValue,
    SnapshotRead, SortKey, TableId, TableSchema, UnitSpec,
};
use crate::{LiteralValue, MaterializedTable};

pub(super) fn request(
    root: impl FnOnce(SnapshotRead, &TableSchema) -> Relation,
) -> ExecutionRequest {
    let group = ColumnSchema::new("group", LogicalType::Utf8);
    let mut value = ColumnSchema::new("value", LogicalType::Float64);
    value.nullable = true;
    let schema = TableSchema::new(vec![group.clone(), value.clone()]).unwrap();
    let table_id = TableId::new();
    let revision = RevisionId::new();
    let fingerprint = ContentHash::of(b"datafusion-test");
    let table = MaterializedTable {
        table_id,
        schema: schema.clone(),
        row_ids: vec![RowId::new(), RowId::new(), RowId::new()],
        columns: vec![
            MaterializedColumn {
                schema: group,
                values: ["a", "b", "a"]
                    .map(|value| ScalarValue::Utf8(value.into()))
                    .into(),
            },
            MaterializedColumn {
                schema: value,
                values: vec![
                    ScalarValue::Float64(1.0),
                    ScalarValue::Null,
                    ScalarValue::Float64(f64::NAN),
                ],
            },
        ],
    };
    let read = SnapshotRead {
        table: table_id,
        revision,
        fingerprint,
    };
    ExecutionRequest {
        plan: RelPlanV1::new(root(read, &schema)),
        inputs: BTreeMap::from([(
            (table_id, revision),
            ExecutionInput::materialized(table, fingerprint),
        )]),
        memory_limit_bytes: 100_000_000,
    }
}

pub(super) fn completed(request: ExecutionRequest) -> ExecutionOutput {
    let handle = DataFusionExecutionService.execute(request);
    loop {
        match handle.recv().unwrap() {
            ExecutionEvent::Progress(_) => {}
            ExecutionEvent::Completed(output) => return output,
            ExecutionEvent::Failed(error) => panic!("execution failed: {error}"),
        }
    }
}

#[test]
fn project_and_filter_match_reference_null_and_nan_semantics() {
    let request = request(|read, schema| {
        let group = schema.columns[0].id;
        let value = schema.columns[1].id;
        Relation::Project {
            input: Box::new(Relation::Filter {
                input: Box::new(Relation::SnapshotRead(read)),
                predicate: Expression::call(
                    "eq.v1",
                    vec![
                        Expression::column(group),
                        Expression::Literal {
                            value: LiteralValue::Utf8("a".into()),
                        },
                    ],
                ),
            }),
            columns: vec![value],
        }
    });
    assert_eq!(
        datafusion_capability(&request.plan),
        DataFusionCapability::Equivalent
    );
    let output = completed(request);
    assert_eq!(output.backend, "plotx.datafusion.v1");
    assert_eq!(output.table.row_ids.len(), 2);
    assert!(matches!(
        output.table.columns[0].values[1],
        ScalarValue::Float64(value) if value.is_nan()
    ));
}

#[test]
fn aggregate_matches_reference_and_uses_datafusion() {
    let request = request(|read, schema| Relation::Aggregate {
        input: Box::new(Relation::SnapshotRead(read)),
        groups: vec![schema.columns[0].id],
        measures: vec![AggregateMeasure {
            output: ColumnSchema::new("count", LogicalType::Int64),
            function: AggregateFunction::CountAll,
            input: None,
        }],
    });
    assert_eq!(
        datafusion_capability(&request.plan),
        DataFusionCapability::Equivalent
    );
    let output = completed(request);
    assert_eq!(output.backend, "plotx.datafusion.v1");
    assert_eq!(output.table.row_ids.len(), 2);
    assert_eq!(
        output.table.columns[1].values,
        vec![ScalarValue::Int64(2), ScalarValue::Int64(1)]
    );
}

#[test]
fn count_expression_ignores_null_but_count_all_keeps_the_row_and_diagnoses_it() {
    let request = request(|read, schema| Relation::Aggregate {
        input: Box::new(Relation::SnapshotRead(read)),
        groups: vec![schema.columns[0].id],
        measures: vec![
            AggregateMeasure {
                output: ColumnSchema::new("rows", LogicalType::Int64),
                function: AggregateFunction::CountAll,
                input: None,
            },
            AggregateMeasure {
                output: ColumnSchema::new("present", LogicalType::Int64),
                function: AggregateFunction::Count,
                input: Some(Expression::column(schema.columns[1].id)),
            },
        ],
    });
    let output = completed(request);
    assert_eq!(
        output.table.columns[1].values,
        vec![ScalarValue::Int64(2), ScalarValue::Int64(1)]
    );
    assert_eq!(
        output.table.columns[2].values,
        vec![ScalarValue::Int64(2), ScalarValue::Int64(0)]
    );
    assert!(output.diagnostics.iter().any(|diagnostic| {
        diagnostic.code == "aggregate.null_excluded"
            && diagnostic.counts.values().copied().sum::<u64>() == 1
    }));
}

#[test]
fn descending_sort_keeps_explicit_nulls_last_and_nan_above_finite() {
    let request = request(|read, schema| Relation::StableSort {
        input: Box::new(Relation::SnapshotRead(read)),
        keys: vec![SortKey {
            column: schema.columns[1].id,
            direction: SortDirection::Descending,
            nulls: NullPlacement::Last,
        }],
    });
    let output = completed(request);
    assert_eq!(output.backend, "plotx.datafusion.v1");
    assert!(matches!(
        output.table.columns[1].values[0],
        ScalarValue::Float64(value) if value.is_nan()
    ));
    assert_eq!(output.table.columns[1].values[1], ScalarValue::Float64(1.0));
    assert_eq!(output.table.columns[1].values[2], ScalarValue::Null);
}

#[test]
fn affine_unit_conversion_matches_reference_operation_order() {
    let mut source = UnitSpec::dimensionless("source");
    source.scale = 2.0;
    source.offset = 3.0;
    let mut target = UnitSpec::dimensionless("target");
    target.scale = 4.0;
    target.offset = 1.0;
    let source_for_plan = source.clone();
    let target_for_plan = target.clone();
    let mut request = request(move |read, schema| Relation::UnitConvert {
        input: Box::new(Relation::SnapshotRead(read)),
        column: schema.columns[1].id,
        source: source_for_plan,
        target: target_for_plan,
    });
    let input = request
        .inputs
        .values_mut()
        .next()
        .unwrap()
        .materialized_table_mut()
        .unwrap();
    input.schema.columns[1].unit = Some(source.clone());
    input.columns[1].schema.unit = Some(source);
    let output = completed(request);
    assert_eq!(output.backend, "plotx.datafusion.v1");
    assert_eq!(output.table.columns[1].values[0], ScalarValue::Float64(1.0));
    assert_eq!(output.table.columns[1].values[1], ScalarValue::Null);
    assert!(matches!(
        output.table.columns[1].values[2],
        ScalarValue::Float64(value) if value.is_nan()
    ));
    assert_eq!(output.table.columns[1].schema.unit, Some(target));
}

#[test]
fn patch_matches_rows_by_identity_and_preserves_nulls() {
    let mut request = request(|read, _| Relation::SnapshotRead(read));
    let input = request
        .inputs
        .values()
        .next()
        .unwrap()
        .materialized_table()
        .unwrap();
    let row = input.row_ids[2];
    let column = input.schema.columns[1].id;
    let read = match &request.plan.root {
        Relation::SnapshotRead(read) => read.clone(),
        _ => unreachable!(),
    };
    request.plan = RelPlanV1::new(Relation::Patch {
        input: Box::new(Relation::SnapshotRead(read)),
        edits: vec![CellPatch {
            row,
            column,
            value: LiteralValue::Float64(crate::FiniteOrSpecial::Finite(9.0)),
        }],
    });
    let output = completed(request);
    assert_eq!(output.backend, "plotx.datafusion.v1");
    assert_eq!(output.table.columns[1].values[0], ScalarValue::Float64(1.0));
    assert_eq!(output.table.columns[1].values[1], ScalarValue::Null);
    assert_eq!(output.table.columns[1].values[2], ScalarValue::Float64(9.0));
}

#[test]
fn mark_missing_treats_null_predicate_as_false() {
    let request = request(|read, schema| Relation::MarkMissing {
        input: Box::new(Relation::SnapshotRead(read)),
        columns: vec![schema.columns[0].id],
        predicate: Expression::call("is_null.v1", vec![Expression::column(schema.columns[1].id)]),
    });
    let output = completed(request);
    assert_eq!(output.backend, "plotx.datafusion.v1");
    assert_eq!(
        output.table.columns[0].values,
        vec![
            ScalarValue::Utf8("a".into()),
            ScalarValue::Null,
            ScalarValue::Utf8("a".into()),
        ]
    );
}

#[test]
fn finite_predicate_excludes_null_and_nan() {
    let request = request(|read, schema| Relation::Filter {
        input: Box::new(Relation::SnapshotRead(read)),
        predicate: Expression::call(
            "is_finite.v1",
            vec![Expression::column(schema.columns[1].id)],
        ),
    });
    let output = completed(request);
    assert_eq!(output.backend, "plotx.datafusion.v1");
    assert_eq!(output.table.row_ids.len(), 1);
    assert_eq!(output.table.columns[1].values[0], ScalarValue::Float64(1.0));
}

#[test]
fn union_namespaces_each_source_row_identity() {
    let mut request = request(|read, _| Relation::SnapshotRead(read));
    let first_read = match &request.plan.root {
        Relation::SnapshotRead(read) => read.clone(),
        _ => unreachable!(),
    };
    let first_rows = request
        .inputs
        .get(&(first_read.table, first_read.revision))
        .unwrap()
        .materialized_table()
        .unwrap()
        .row_ids
        .clone();
    let second_table = TableId::new();
    let second_revision = RevisionId::new();
    let second_fingerprint = ContentHash::of(b"datafusion-union-second");
    let mut second = request
        .inputs
        .values()
        .next()
        .unwrap()
        .materialized_table()
        .unwrap()
        .clone();
    second.table_id = second_table;
    second.row_ids = vec![RowId::new(), RowId::new(), RowId::new()];
    let second_rows = second.row_ids.clone();
    request.inputs.insert(
        (second_table, second_revision),
        ExecutionInput::materialized(second, second_fingerprint),
    );
    request.plan = RelPlanV1::new(Relation::Union {
        inputs: vec![
            Relation::SnapshotRead(first_read.clone()),
            Relation::SnapshotRead(SnapshotRead {
                table: second_table,
                revision: second_revision,
                fingerprint: second_fingerprint,
            }),
        ],
    });
    let output = completed(request);
    let expected = first_rows
        .into_iter()
        .map(|row| RowId::namespaced(first_read.table, row))
        .chain(
            second_rows
                .into_iter()
                .map(|row| RowId::namespaced(second_table, row)),
        )
        .collect::<Vec<_>>();
    assert_eq!(output.backend, "plotx.datafusion.v1");
    assert_eq!(output.table.row_ids, expected);
}

#[test]
fn inner_join_matches_reference_identity_and_values() {
    let mut request = request(|read, _| Relation::SnapshotRead(read));
    let left_read = match &request.plan.root {
        Relation::SnapshotRead(read) => read.clone(),
        _ => unreachable!(),
    };
    let left_group = request.inputs.values().next().unwrap().schema().columns[0].id;
    let right_group = ColumnSchema::new("right_group", LogicalType::Utf8);
    let right_value = ColumnSchema::new("right_value", LogicalType::Int64);
    let right_schema = TableSchema::new(vec![right_group.clone(), right_value.clone()]).unwrap();
    let right_table = TableId::new();
    let right_revision = RevisionId::new();
    let right_fingerprint = ContentHash::of(b"datafusion-join-right");
    request.inputs.insert(
        (right_table, right_revision),
        ExecutionInput::materialized(
            MaterializedTable {
                table_id: right_table,
                schema: right_schema,
                row_ids: vec![RowId::new(), RowId::new(), RowId::new()],
                columns: vec![
                    MaterializedColumn {
                        schema: right_group.clone(),
                        values: ["a", "c", "a"]
                            .map(|value| ScalarValue::Utf8(value.into()))
                            .into(),
                    },
                    MaterializedColumn {
                        schema: right_value,
                        values: [10, 20, 30].map(ScalarValue::Int64).into(),
                    },
                ],
            },
            right_fingerprint,
        ),
    );
    request.plan = RelPlanV1::new(Relation::Join {
        left: Box::new(Relation::SnapshotRead(left_read)),
        right: Box::new(Relation::SnapshotRead(SnapshotRead {
            table: right_table,
            revision: right_revision,
            fingerprint: right_fingerprint,
        })),
        kind: JoinKind::Inner,
        keys: vec![JoinKey {
            left: left_group,
            right: right_group.id,
        }],
        cardinality: JoinCardinality::ManyToMany,
    });

    assert_eq!(
        datafusion_capability(&request.plan),
        DataFusionCapability::Equivalent
    );
    let output = completed(request);
    assert_eq!(output.backend, "plotx.datafusion.v1");
    assert_eq!(output.table.row_ids.len(), 4);
    assert_eq!(
        output.table.columns[3].values,
        vec![
            ScalarValue::Int64(10),
            ScalarValue::Int64(30),
            ScalarValue::Int64(10),
            ScalarValue::Int64(30),
        ]
    );
}

#[test]
fn large_snapshot_join_uses_stable_positions_and_compact_cardinality_check() {
    let mut request = request(|read, _| Relation::SnapshotRead(read));
    let left_read = match &request.plan.root {
        Relation::SnapshotRead(read) => read.clone(),
        _ => unreachable!(),
    };
    let rows = DIFFERENTIAL_INPUT_ROW_LIMIT + 1;
    let left = request
        .inputs
        .values_mut()
        .next()
        .unwrap()
        .materialized_table_mut()
        .unwrap();
    left.row_ids = (0..rows).map(|_| RowId::new()).collect();
    left.columns[0].values = (0..rows)
        .map(|row| ScalarValue::Utf8(if row % 2 == 0 { "a" } else { "b" }.into()))
        .collect();
    left.columns[1].values = (0..rows)
        .map(|row| ScalarValue::Float64(row as f64))
        .collect();
    let left_rows = left.row_ids.clone();
    let left_group = left.schema.columns[0].id;

    let right_group = ColumnSchema::new("right_group", LogicalType::Utf8);
    let right_value = ColumnSchema::new("right_value", LogicalType::Int64);
    let right_schema = TableSchema::new(vec![right_group.clone(), right_value.clone()]).unwrap();
    let right_table = TableId::new();
    let right_revision = RevisionId::new();
    let right_fingerprint = ContentHash::of(b"datafusion-large-join-right");
    let right_rows = vec![RowId::new(), RowId::new()];
    request.inputs.insert(
        (right_table, right_revision),
        ExecutionInput::materialized(
            MaterializedTable {
                table_id: right_table,
                schema: right_schema,
                row_ids: right_rows.clone(),
                columns: vec![
                    MaterializedColumn {
                        schema: right_group.clone(),
                        values: ["b", "a"]
                            .map(|value| ScalarValue::Utf8(value.into()))
                            .into(),
                    },
                    MaterializedColumn {
                        schema: right_value,
                        values: [20, 10].map(ScalarValue::Int64).into(),
                    },
                ],
            },
            right_fingerprint,
        ),
    );
    request.plan = RelPlanV1::new(Relation::Join {
        left: Box::new(Relation::SnapshotRead(left_read)),
        right: Box::new(Relation::SnapshotRead(SnapshotRead {
            table: right_table,
            revision: right_revision,
            fingerprint: right_fingerprint,
        })),
        kind: JoinKind::Inner,
        keys: vec![JoinKey {
            left: left_group,
            right: right_group.id,
        }],
        cardinality: JoinCardinality::ManyToOne,
    });
    let operation = request.plan.operation_id;

    let output = completed(request);
    assert_eq!(output.table.row_ids.len(), rows);
    assert_eq!(output.table.columns[3].values[0], ScalarValue::Int64(10));
    assert_eq!(output.table.columns[3].values[1], ScalarValue::Int64(20));
    for index in [0, 1, rows - 1] {
        let right = if index % 2 == 0 {
            right_rows[1]
        } else {
            right_rows[0]
        };
        assert_eq!(
            output.table.row_ids[index],
            RowId::derived(
                operation,
                &[left_rows[index], right],
                &(index as u64).to_le_bytes()
            )
        );
    }
    assert!(
        output
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "backend.differential.deferred")
    );
}

#[test]
fn unpivot_uses_logical_source_names_and_reference_row_ids() {
    let mut request = request(|read, _| Relation::SnapshotRead(read));
    let input = request
        .inputs
        .values_mut()
        .next()
        .unwrap()
        .materialized_table_mut()
        .unwrap();
    let group = input.schema.columns[0].id;
    let first_value = input.schema.columns[1].id;
    let second_value = ColumnSchema::new("second value", LogicalType::Float64);
    input.schema.columns.push(second_value.clone());
    input.columns.push(MaterializedColumn {
        schema: second_value.clone(),
        values: [2.0, 3.0, 4.0].map(ScalarValue::Float64).into(),
    });
    let read = match &request.plan.root {
        Relation::SnapshotRead(read) => read.clone(),
        _ => unreachable!(),
    };
    let name_column = ColumnSchema::new("variable", LogicalType::Utf8);
    let value_column = ColumnSchema::new("reading", LogicalType::Float64);
    request.plan = RelPlanV1::new(Relation::Unpivot {
        input: Box::new(Relation::SnapshotRead(read)),
        ids: vec![group],
        values: vec![first_value, second_value.id],
        name_column: Box::new(name_column),
        value_column: Box::new(value_column),
    });

    let output = completed(request);
    assert_eq!(output.backend, "plotx.datafusion.v1");
    assert_eq!(output.table.row_ids.len(), 6);
    assert_eq!(
        output.table.columns[1].values,
        vec![
            ScalarValue::Utf8("value".into()),
            ScalarValue::Utf8("second value".into()),
            ScalarValue::Utf8("value".into()),
            ScalarValue::Utf8("second value".into()),
            ScalarValue::Utf8("value".into()),
            ScalarValue::Utf8("second value".into()),
        ]
    );
}

#[test]
fn large_snapshot_unpivot_derives_identity_without_reference_materialization() {
    let mut request = request(|read, _| Relation::SnapshotRead(read));
    let rows = DIFFERENTIAL_INPUT_ROW_LIMIT + 1;
    let input = request
        .inputs
        .values_mut()
        .next()
        .unwrap()
        .materialized_table_mut()
        .unwrap();
    let group = input.schema.columns[0].id;
    let first_value = input.schema.columns[1].id;
    input.row_ids = (0..rows).map(|_| RowId::new()).collect();
    let source_rows = input.row_ids.clone();
    input.columns[0].values = (0..rows)
        .map(|_| ScalarValue::Utf8("group".into()))
        .collect();
    input.columns[1].values = (0..rows)
        .map(|row| ScalarValue::Float64(row as f64))
        .collect();
    let second_value = ColumnSchema::new("second value", LogicalType::Float64);
    input.schema.columns.push(second_value.clone());
    input.columns.push(MaterializedColumn {
        schema: second_value.clone(),
        values: (0..rows)
            .map(|row| ScalarValue::Float64((row + 1) as f64))
            .collect(),
    });
    let read = match &request.plan.root {
        Relation::SnapshotRead(read) => read.clone(),
        _ => unreachable!(),
    };
    request.plan = RelPlanV1::new(Relation::Unpivot {
        input: Box::new(Relation::SnapshotRead(read)),
        ids: vec![group],
        values: vec![first_value, second_value.id],
        name_column: Box::new(ColumnSchema::new("variable", LogicalType::Utf8)),
        value_column: Box::new(ColumnSchema::new("reading", LogicalType::Float64)),
    });
    let operation = request.plan.operation_id;

    let output = completed(request);
    assert_eq!(output.table.row_ids.len(), rows * 2);
    assert_eq!(
        output.table.row_ids[0],
        RowId::derived(operation, &[source_rows[0]], first_value.as_bytes())
    );
    assert_eq!(
        output.table.row_ids[1],
        RowId::derived(operation, &[source_rows[0]], second_value.id.as_bytes())
    );
    assert_eq!(
        output.table.columns[1].values[rows * 2 - 1],
        ScalarValue::Utf8("second value".into())
    );
    assert!(
        output
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "backend.differential.deferred")
    );
}

#[test]
fn pivot_compiles_dynamic_columns_and_matches_reference() {
    let mut request = request(|read, _| Relation::SnapshotRead(read));
    let input = request
        .inputs
        .values_mut()
        .next()
        .unwrap()
        .materialized_table_mut()
        .unwrap();
    let group = input.schema.columns[0].id;
    let value = input.schema.columns[1].id;
    let name = ColumnSchema::new("metric", LogicalType::Utf8);
    input.schema.columns.push(name.clone());
    input.columns.push(MaterializedColumn {
        schema: name.clone(),
        values: ["height", "area", "height"]
            .map(|value| ScalarValue::Utf8(value.into()))
            .into(),
    });
    let read = match &request.plan.root {
        Relation::SnapshotRead(read) => read.clone(),
        _ => unreachable!(),
    };
    request.plan = RelPlanV1::new(Relation::Pivot {
        input: Box::new(Relation::SnapshotRead(read)),
        groups: vec![group],
        names_from: name.id,
        values_from: value,
        aggregate: AggregateFunction::SumV1,
    });

    assert_eq!(
        datafusion_capability(&request.plan),
        DataFusionCapability::Equivalent
    );
    let output = completed(request);
    assert_eq!(output.backend, "plotx.datafusion.v1");
    assert_eq!(
        output
            .table
            .schema
            .columns
            .iter()
            .map(|column| column.name.as_str())
            .collect::<Vec<_>>(),
        vec!["group", "area", "height"]
    );
    assert_eq!(output.table.row_ids.len(), 2);
    assert_eq!(output.table.columns[1].values[0], ScalarValue::Null);
    assert!(matches!(
        output.table.columns[2].values[0],
        ScalarValue::Float64(value) if value.is_nan()
    ));
    assert_eq!(output.table.columns[1].values[1], ScalarValue::Null);
    assert_eq!(output.table.columns[2].values[1], ScalarValue::Null);
}

#[test]
fn large_snapshot_pivot_builds_dynamic_schema_and_identity_without_full_reference() {
    let mut request = request(|read, _| Relation::SnapshotRead(read));
    let rows = DIFFERENTIAL_INPUT_ROW_LIMIT + 1;
    let input = request
        .inputs
        .values_mut()
        .next()
        .unwrap()
        .materialized_table_mut()
        .unwrap();
    let group = input.schema.columns[0].id;
    let value = input.schema.columns[1].id;
    input.row_ids = (0..rows).map(|_| RowId::new()).collect();
    let source_rows = input.row_ids.clone();
    input.columns[0].values = (0..rows)
        .map(|row| ScalarValue::Utf8(if row % 4 < 2 { "a" } else { "b" }.into()))
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
    let name = ColumnSchema::new("metric", LogicalType::Utf8);
    input.schema.columns.push(name.clone());
    input.columns.push(MaterializedColumn {
        schema: name.clone(),
        values: (0..rows)
            .map(|row| ScalarValue::Utf8(if row % 2 == 0 { "height" } else { "area" }.into()))
            .collect(),
    });
    let read = match &request.plan.root {
        Relation::SnapshotRead(read) => read.clone(),
        _ => unreachable!(),
    };
    request.plan = RelPlanV1::new(Relation::Pivot {
        input: Box::new(Relation::SnapshotRead(read)),
        groups: vec![group],
        names_from: name.id,
        values_from: value,
        aggregate: AggregateFunction::MeanV1,
    });
    let operation = request.plan.operation_id;

    let output = completed(request);
    assert_eq!(output.table.row_ids.len(), 2);
    assert_eq!(
        output
            .table
            .schema
            .columns
            .iter()
            .map(|column| column.name.as_str())
            .collect::<Vec<_>>(),
        vec!["group", "area", "height"]
    );
    let expected_a_sources = (0..rows)
        .filter(|row| row % 4 < 2 && row % 2 == 1)
        .chain((0..rows).filter(|row| row % 4 < 2 && row % 2 == 0))
        .map(|row| source_rows[row])
        .collect::<Vec<_>>();
    assert_eq!(
        output.table.row_ids[0],
        RowId::derived(operation, &expected_a_sources, &0_u64.to_le_bytes())
    );
    assert_eq!(
        output
            .diagnostics
            .iter()
            .find(|diagnostic| diagnostic.code == "pivot.null_excluded")
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
