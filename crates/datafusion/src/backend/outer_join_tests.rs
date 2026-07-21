use super::tests::{completed, request};
use crate::{
    ColumnSchema, ContentHash, ExecutionInput, JoinCardinality, JoinKey, JoinKind, LogicalType,
    MaterializedColumn, MaterializedTable, RelPlanV1, Relation, RevisionId, RowId, ScalarValue,
    SnapshotRead, TableId, TableSchema,
};

#[test]
fn left_and_full_joins_keep_unmatched_rows_and_never_match_null_keys() {
    let mut request = request(|read, _| Relation::SnapshotRead(read));
    let left_read = match &request.plan.root {
        Relation::SnapshotRead(read) => read.clone(),
        _ => unreachable!(),
    };
    let left = request
        .inputs
        .values_mut()
        .next()
        .unwrap()
        .materialized_table_mut()
        .unwrap();
    let left_group = left.schema.columns[0].id;
    left.columns[0].values = vec![
        ScalarValue::Utf8("a".into()),
        ScalarValue::Null,
        ScalarValue::Utf8("a".into()),
    ];

    let mut right_group = ColumnSchema::new("right_group", LogicalType::Utf8);
    right_group.nullable = true;
    let right_value = ColumnSchema::new("right_value", LogicalType::Int64);
    let right_table = TableId::new();
    let right_revision = RevisionId::new();
    let right_fingerprint = ContentHash::of(b"datafusion-outer-join-right");
    request.inputs.insert(
        (right_table, right_revision),
        ExecutionInput::materialized(
            MaterializedTable {
                table_id: right_table,
                schema: TableSchema::new(vec![right_group.clone(), right_value.clone()]).unwrap(),
                row_ids: vec![RowId::new(), RowId::new(), RowId::new()],
                columns: vec![
                    MaterializedColumn {
                        schema: right_group.clone(),
                        values: vec![
                            ScalarValue::Utf8("a".into()),
                            ScalarValue::Utf8("c".into()),
                            ScalarValue::Null,
                        ],
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
    let join = |kind| Relation::Join {
        left: Box::new(Relation::SnapshotRead(left_read.clone())),
        right: Box::new(Relation::SnapshotRead(SnapshotRead {
            table: right_table,
            revision: right_revision,
            fingerprint: right_fingerprint,
        })),
        kind,
        keys: vec![JoinKey {
            left: left_group,
            right: right_group.id,
        }],
        cardinality: JoinCardinality::ManyToMany,
    };

    let mut left_request = request.clone();
    left_request.plan = RelPlanV1::new(join(JoinKind::Left));
    let left_output = completed(left_request);
    assert_eq!(left_output.table.row_ids.len(), 3);
    assert_eq!(
        left_output.table.columns[3].values,
        vec![
            ScalarValue::Int64(10),
            ScalarValue::Null,
            ScalarValue::Int64(10),
        ]
    );

    request.plan = RelPlanV1::new(join(JoinKind::Full));
    let full_output = completed(request);
    assert_eq!(full_output.table.row_ids.len(), 5);
    assert_eq!(
        full_output.table.columns[3]
            .values
            .iter()
            .filter(|value| matches!(value, ScalarValue::Int64(30)))
            .count(),
        1,
        "the right null key must remain unmatched, not join the left null key"
    );
}
