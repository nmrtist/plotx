use crate::{
    BlockStore, ColumnId, DataError, DerivedRowMappingV1, DerivedRowSourcesV1, ExecutionInput,
    InputRowRunsV1, JoinKind, RelPlanV1, Relation, Result, RevisionId, RowId, RowMapping,
    ScalarValue, SnapshotRead, TableId,
};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Key {
    Null,
    Boolean(bool),
    Int64(i64),
    Float64(u64),
    Utf8(String),
    Categorical(u32),
    Date(i32),
    Time(i64),
}

type RowKey = Vec<Key>;
type InputCatalog = BTreeMap<(TableId, RevisionId), ExecutionInput>;

/// Build a verified, compressed source mapping for a completed frozen plan.
/// The output identities are checked against the same derivation rules used by
/// execution before any mapping is persisted.
pub fn derive_row_mapping(
    plan: &RelPlanV1,
    inputs: &InputCatalog,
    output: &[RowId],
    store: &dyn BlockStore,
) -> Result<RowMapping> {
    match &plan.root {
        Relation::Union { inputs: relations } => union_mapping(relations, inputs, output),
        Relation::Unpivot { input, values, .. } => {
            unpivot_mapping(plan, input, values, inputs, output)
        }
        Relation::Aggregate { input, groups, .. } => {
            aggregate_mapping(plan, input, groups, inputs, output, store)
        }
        Relation::Pivot {
            input,
            groups,
            names_from,
            ..
        } => pivot_mapping(plan, input, groups, *names_from, inputs, output, store),
        Relation::Join {
            left,
            right,
            kind,
            keys,
            ..
        } => join_mapping(plan, left, right, *kind, keys, inputs, output, store),
        relation => same_row_mapping(relation, inputs, output, store),
    }
}

fn same_row_mapping(
    relation: &Relation,
    inputs: &InputCatalog,
    output: &[RowId],
    store: &dyn BlockStore,
) -> Result<RowMapping> {
    let read = single_source_read(relation)?;
    let (input_index, input) = input_for_read(inputs, &read)?;
    let mut source_rows = Vec::with_capacity(input.row_count() as usize);
    input.visit_materialized_batches(|batch| {
        source_rows.extend(batch.row_ids.iter().copied());
        Ok(())
    })?;
    let positions = source_rows
        .iter()
        .enumerate()
        .map(|(position, row)| (*row, position as u64))
        .collect::<BTreeMap<_, _>>();
    let selected = output
        .iter()
        .map(|row| {
            positions.get(row).copied().ok_or_else(|| {
                DataError::CorruptBlock(format!("output row {row} is absent from its source"))
            })
        })
        .collect::<Result<Vec<_>>>()?;
    if output == source_rows {
        return Ok(RowMapping::Identity);
    }
    if selected.windows(2).all(|pair| pair[0] < pair[1]) {
        return Ok(RowMapping::Selection {
            runs: compress_runs(&selected),
        });
    }
    let outputs = output
        .iter()
        .copied()
        .zip(selected)
        .map(|(output, position)| DerivedRowSourcesV1 {
            output,
            sources: vec![InputRowRunsV1 {
                input: input_index,
                runs: vec![(position, 1)],
            }],
        })
        .collect();
    store_derived(outputs, store)
}

fn union_mapping(
    relations: &[Relation],
    inputs: &InputCatalog,
    output: &[RowId],
) -> Result<RowMapping> {
    let reads = relations
        .iter()
        .map(single_source_read)
        .collect::<Result<Vec<_>>>()?;
    let mut expected = Vec::new();
    for read in &reads {
        let (_, input) = input_for_read(inputs, read)?;
        input.visit_materialized_batches(|batch| {
            expected.extend(
                batch
                    .row_ids
                    .iter()
                    .map(|row| RowId::namespaced(read.table, *row)),
            );
            Ok(())
        })?;
    }
    require_output(output, &expected, "union")?;
    Ok(RowMapping::UnionNamespaces {
        sources: reads.into_iter().map(|read| read.table).collect(),
    })
}

fn unpivot_mapping(
    plan: &RelPlanV1,
    relation: &Relation,
    values: &[ColumnId],
    inputs: &InputCatalog,
    output: &[RowId],
) -> Result<RowMapping> {
    let read = single_source_read(relation)?;
    let (_, input) = input_for_read(inputs, &read)?;
    let mut expected = Vec::new();
    input.visit_materialized_batches(|batch| {
        for row in &batch.row_ids {
            expected.extend(
                values
                    .iter()
                    .map(|column| RowId::derived(plan.operation_id, &[*row], column.as_bytes())),
            );
        }
        Ok(())
    })?;
    require_output(output, &expected, "unpivot")?;
    Ok(RowMapping::Unpivot {
        source: read.table,
        value_columns: values.to_vec(),
    })
}

fn aggregate_mapping(
    plan: &RelPlanV1,
    relation: &Relation,
    groups: &[ColumnId],
    inputs: &InputCatalog,
    output: &[RowId],
    store: &dyn BlockStore,
) -> Result<RowMapping> {
    let read = exact_read(relation, "aggregate")?;
    let (input_index, input) = input_for_read(inputs, &read)?;
    let mut grouped: BTreeMap<RowKey, Vec<(RowId, u64)>> = BTreeMap::new();
    if groups.is_empty() {
        grouped.insert(Vec::new(), Vec::new());
    }
    let mut position = 0_u64;
    input.visit_materialized_batches(|batch| {
        for row in 0..batch.row_ids.len() {
            let key = row_key(batch, row, groups, true)?.expect("null-equal group key");
            grouped
                .entry(key)
                .or_default()
                .push((batch.row_ids[row], position));
            position += 1;
        }
        Ok(())
    })?;
    let entries = grouped
        .into_values()
        .enumerate()
        .map(|(index, sources)| {
            let ids = sources.iter().map(|(row, _)| *row).collect::<Vec<_>>();
            let output = RowId::derived(plan.operation_id, &ids, &(index as u64).to_le_bytes());
            source_entry(output, input_index, sources.into_iter().map(|(_, pos)| pos))
        })
        .collect::<Vec<_>>();
    require_output(
        output,
        &entries.iter().map(|entry| entry.output).collect::<Vec<_>>(),
        "aggregate",
    )?;
    store_derived(entries, store)
}

#[allow(clippy::too_many_arguments)]
fn pivot_mapping(
    plan: &RelPlanV1,
    relation: &Relation,
    groups: &[ColumnId],
    names_from: ColumnId,
    inputs: &InputCatalog,
    output: &[RowId],
    store: &dyn BlockStore,
) -> Result<RowMapping> {
    let read = exact_read(relation, "pivot")?;
    let (input_index, input) = input_for_read(inputs, &read)?;
    let name_type = &input
        .schema()
        .column(names_from)
        .ok_or(DataError::MissingColumn(names_from))?
        .logical_type;
    let mut grouped: BTreeMap<RowKey, BTreeMap<String, Vec<(RowId, u64)>>> = BTreeMap::new();
    let mut position = 0_u64;
    input.visit_materialized_batches(|batch| {
        let names = batch.column(names_from)?;
        for row in 0..batch.row_ids.len() {
            if let Some(name) = pivot_name(&names.values[row], name_type)? {
                let key = row_key(batch, row, groups, true)?.expect("null-equal pivot key");
                grouped
                    .entry(key)
                    .or_default()
                    .entry(name)
                    .or_default()
                    .push((batch.row_ids[row], position));
            }
            position += 1;
        }
        Ok(())
    })?;
    let entries = grouped
        .into_values()
        .enumerate()
        .map(|(index, by_name)| {
            let sources = by_name.into_values().flatten().collect::<Vec<_>>();
            let ids = sources.iter().map(|(row, _)| *row).collect::<Vec<_>>();
            let output = RowId::derived(plan.operation_id, &ids, &(index as u64).to_le_bytes());
            source_entry(output, input_index, sources.into_iter().map(|(_, pos)| pos))
        })
        .collect::<Vec<_>>();
    require_output(
        output,
        &entries.iter().map(|entry| entry.output).collect::<Vec<_>>(),
        "pivot",
    )?;
    store_derived(entries, store)
}

#[allow(clippy::too_many_arguments)]
fn join_mapping(
    plan: &RelPlanV1,
    left_relation: &Relation,
    right_relation: &Relation,
    kind: JoinKind,
    keys: &[crate::JoinKey],
    inputs: &InputCatalog,
    output: &[RowId],
    store: &dyn BlockStore,
) -> Result<RowMapping> {
    let left_read = exact_read(left_relation, "join")?;
    let right_read = exact_read(right_relation, "join")?;
    let (left_input_index, left) = input_for_read(inputs, &left_read)?;
    let (right_input_index, right) = input_for_read(inputs, &right_read)?;
    let left_columns = keys.iter().map(|key| key.left).collect::<Vec<_>>();
    let right_columns = keys.iter().map(|key| key.right).collect::<Vec<_>>();
    let left_rows = indexed_rows(left, &left_columns)?;
    let right_rows = indexed_rows(right, &right_columns)?;
    let mut right_by_key: BTreeMap<RowKey, Vec<usize>> = BTreeMap::new();
    for (index, row) in right_rows.iter().enumerate() {
        if let Some(key) = &row.key {
            right_by_key.entry(key.clone()).or_default().push(index);
        }
    }
    let mut pairs = Vec::new();
    let mut matched_right = BTreeSet::new();
    for (left_index, left_row) in left_rows.iter().enumerate() {
        if let Some(matches) = left_row.key.as_ref().and_then(|key| right_by_key.get(key)) {
            for right_index in matches {
                pairs.push((Some(left_index), Some(*right_index)));
                matched_right.insert(*right_index);
            }
        } else if kind != JoinKind::Inner {
            pairs.push((Some(left_index), None));
        }
    }
    if kind == JoinKind::Full {
        pairs.extend(
            (0..right_rows.len())
                .filter(|row| !matched_right.contains(row))
                .map(|row| (None, Some(row))),
        );
    }
    let entries = pairs
        .into_iter()
        .enumerate()
        .map(|(index, (left_row, right_row))| {
            let ids = left_row
                .map(|row| left_rows[row].id)
                .into_iter()
                .chain(right_row.map(|row| right_rows[row].id))
                .collect::<Vec<_>>();
            let output = RowId::derived(plan.operation_id, &ids, &(index as u64).to_le_bytes());
            let mut sources = Vec::new();
            if let Some(row) = left_row {
                sources.push(InputRowRunsV1 {
                    input: left_input_index,
                    runs: vec![(left_rows[row].position, 1)],
                });
            }
            if let Some(row) = right_row {
                sources.push(InputRowRunsV1 {
                    input: right_input_index,
                    runs: vec![(right_rows[row].position, 1)],
                });
            }
            DerivedRowSourcesV1 { output, sources }
        })
        .collect::<Vec<_>>();
    require_output(
        output,
        &entries.iter().map(|entry| entry.output).collect::<Vec<_>>(),
        "join",
    )?;
    store_derived(entries, store)
}

struct IndexedRow {
    id: RowId,
    position: u64,
    key: Option<RowKey>,
}

fn indexed_rows(input: &ExecutionInput, columns: &[ColumnId]) -> Result<Vec<IndexedRow>> {
    let mut rows = Vec::with_capacity(input.row_count() as usize);
    let mut position = 0_u64;
    input.visit_materialized_batches(|batch| {
        for row in 0..batch.row_ids.len() {
            rows.push(IndexedRow {
                id: batch.row_ids[row],
                position,
                key: row_key(batch, row, columns, false)?,
            });
            position += 1;
        }
        Ok(())
    })?;
    Ok(rows)
}

fn row_key(
    table: &crate::MaterializedTable,
    row: usize,
    columns: &[ColumnId],
    null_equal: bool,
) -> Result<Option<RowKey>> {
    columns
        .iter()
        .map(|column| match &table.column(*column)?.values[row] {
            ScalarValue::Null => Ok(null_equal.then_some(Key::Null)),
            ScalarValue::Boolean(value) => Ok(Some(Key::Boolean(*value))),
            ScalarValue::Int64(value) => Ok(Some(Key::Int64(*value))),
            ScalarValue::Float64(value) => Ok(Some(Key::Float64(float_key(*value)))),
            ScalarValue::Utf8(value) => Ok(Some(Key::Utf8(value.clone()))),
            ScalarValue::Categorical(value) => Ok(Some(Key::Categorical(*value))),
            ScalarValue::Date(value) => Ok(Some(Key::Date(*value))),
            ScalarValue::Time(value)
            | ScalarValue::Timestamp(value)
            | ScalarValue::Duration(value) => Ok(Some(Key::Time(*value))),
            ScalarValue::Extension { .. } => Err(DataError::Unsupported(
                "extension row keys require a registered comparison".into(),
            )),
        })
        .collect::<Result<Option<Vec<_>>>>()
}

fn pivot_name(value: &ScalarValue, logical_type: &crate::LogicalType) -> Result<Option<String>> {
    match value {
        ScalarValue::Null => Ok(None),
        ScalarValue::Utf8(value) => Ok(Some(value.clone())),
        ScalarValue::Categorical(index) => {
            let crate::LogicalType::Categorical { levels } = logical_type else {
                return Err(DataError::InvalidArray(
                    "categorical pivot name has non-categorical schema".into(),
                ));
            };
            levels
                .get(*index as usize)
                .map(|level| Some(level.value.clone()))
                .ok_or_else(|| DataError::InvalidArray("categorical level is out of range".into()))
        }
        _ => Err(DataError::InvalidPlan(
            "pivot name value is not text/categorical".into(),
        )),
    }
}

fn input_for_read<'a>(
    inputs: &'a InputCatalog,
    read: &SnapshotRead,
) -> Result<(u32, &'a ExecutionInput)> {
    let index = inputs
        .keys()
        .position(|identity| *identity == (read.table, read.revision))
        .and_then(|index| u32::try_from(index).ok())
        .ok_or_else(|| DataError::InvalidPlan("pinned provenance input is unavailable".into()))?;
    let input = inputs
        .get(&(read.table, read.revision))
        .expect("catalog index resolved an input");
    if input.snapshot_fingerprint() != read.fingerprint {
        return Err(DataError::InvalidPlan(
            "pinned provenance fingerprint differs from the plan".into(),
        ));
    }
    Ok((index, input))
}

fn exact_read(relation: &Relation, operation: &str) -> Result<SnapshotRead> {
    match relation {
        Relation::SnapshotRead(read) => Ok(read.clone()),
        _ => Err(DataError::Unsupported(format!(
            "{operation} row mapping currently requires a pinned snapshot input"
        ))),
    }
}

fn single_source_read(relation: &Relation) -> Result<SnapshotRead> {
    match relation {
        Relation::SnapshotRead(read) => Ok(read.clone()),
        Relation::Project { input, .. }
        | Relation::Rename { input, .. }
        | Relation::ComputedColumn { input, .. }
        | Relation::Filter { input, .. }
        | Relation::StableSort { input, .. }
        | Relation::Patch { input, .. }
        | Relation::UnitConvert { input, .. }
        | Relation::MarkMissing { input, .. } => single_source_read(input),
        _ => Err(DataError::Unsupported(
            "row mapping requires one row-preserving source relation".into(),
        )),
    }
}

fn source_entry(
    output: RowId,
    input: u32,
    positions: impl IntoIterator<Item = u64>,
) -> DerivedRowSourcesV1 {
    DerivedRowSourcesV1 {
        output,
        sources: vec![InputRowRunsV1 {
            input,
            runs: compress_runs(&positions.into_iter().collect::<Vec<_>>()),
        }],
    }
}

fn store_derived(outputs: Vec<DerivedRowSourcesV1>, store: &dyn BlockStore) -> Result<RowMapping> {
    let mapping = DerivedRowMappingV1 {
        version: 1,
        outputs,
    };
    Ok(RowMapping::Derived {
        mapping_block: store.put(mapping.encode()?)?,
        codec: DerivedRowMappingV1::CODEC.into(),
    })
}

fn require_output(actual: &[RowId], expected: &[RowId], operation: &str) -> Result<()> {
    if actual == expected {
        Ok(())
    } else {
        Err(DataError::CorruptBlock(format!(
            "{operation} output identities differ from its source mapping"
        )))
    }
}

fn compress_runs(positions: &[u64]) -> Vec<(u64, u64)> {
    let mut runs = Vec::new();
    for &position in positions {
        match runs.last_mut() {
            Some((start, length)) if *start + *length == position => *length += 1,
            _ => runs.push((position, 1)),
        }
    }
    runs
}

fn float_key(value: f64) -> u64 {
    if value.is_nan() {
        f64::NAN.to_bits()
    } else if value == 0.0 {
        0
    } else {
        value.to_bits()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        AggregateFunction, AggregateMeasure, ColumnSchema, ContentHash, ExecutionRequest,
        JoinCardinality, JoinKey, LogicalType, MaterializedColumn, MaterializedTable, TableSchema,
    };
    use std::sync::atomic::AtomicBool;

    fn input(
        table_id: TableId,
        revision: RevisionId,
        columns: Vec<(ColumnSchema, Vec<ScalarValue>)>,
    ) -> ((TableId, RevisionId), ExecutionInput, SnapshotRead) {
        let fingerprint = ContentHash::of(&table_id.as_bytes()[..]);
        let row_count = columns.first().map_or(0, |(_, values)| values.len());
        let schema =
            TableSchema::new(columns.iter().map(|(schema, _)| schema.clone()).collect()).unwrap();
        let table = MaterializedTable {
            table_id,
            schema,
            row_ids: (0..row_count).map(|_| RowId::new()).collect(),
            columns: columns
                .into_iter()
                .map(|(schema, values)| MaterializedColumn { schema, values })
                .collect(),
        };
        let read = SnapshotRead {
            table: table_id,
            revision,
            fingerprint,
        };
        (
            (table_id, revision),
            ExecutionInput::materialized(table, fingerprint),
            read,
        )
    }

    fn execute(plan: RelPlanV1, inputs: InputCatalog) -> crate::ExecutionOutput {
        crate::execute_reference(
            &ExecutionRequest {
                plan,
                inputs,
                memory_limit_bytes: 16 * 1024 * 1024,
            },
            &AtomicBool::new(false),
        )
        .unwrap()
    }

    fn decoded(mapping: RowMapping, store: &crate::MemoryBlockStore) -> DerivedRowMappingV1 {
        let RowMapping::Derived {
            mapping_block,
            codec,
        } = mapping
        else {
            panic!("expected derived mapping")
        };
        assert_eq!(codec, DerivedRowMappingV1::CODEC);
        DerivedRowMappingV1::decode(&store.get(mapping_block).unwrap()).unwrap()
    }

    #[test]
    fn aggregate_pivot_and_join_record_exact_input_runs() {
        let group = ColumnSchema::new("group", LogicalType::Utf8);
        let name = ColumnSchema::new("name", LogicalType::Utf8);
        let value = ColumnSchema::new("value", LogicalType::Float64);
        let (identity, source, read) = input(
            TableId::new(),
            RevisionId::new(),
            vec![
                (
                    group.clone(),
                    ["a", "a", "b", "b"]
                        .map(|value| ScalarValue::Utf8(value.into()))
                        .into(),
                ),
                (
                    name.clone(),
                    ["height", "area", "height", "area"]
                        .map(|value| ScalarValue::Utf8(value.into()))
                        .into(),
                ),
                (
                    value.clone(),
                    [1.0, 2.0, 3.0, 4.0].map(ScalarValue::Float64).into(),
                ),
            ],
        );
        let inputs = BTreeMap::from([(identity, source)]);
        let aggregate = RelPlanV1::new(Relation::Aggregate {
            input: Box::new(Relation::SnapshotRead(read.clone())),
            groups: vec![group.id],
            measures: vec![AggregateMeasure {
                output: ColumnSchema::new("mean", LogicalType::Float64),
                function: AggregateFunction::MeanV1,
                input: Some(crate::Expression::column(value.id)),
            }],
        });
        let aggregate_output = execute(aggregate.clone(), inputs.clone());
        let store = crate::MemoryBlockStore::default();
        let mapping = decoded(
            derive_row_mapping(&aggregate, &inputs, &aggregate_output.table.row_ids, &store)
                .unwrap(),
            &store,
        );
        assert_eq!(mapping.outputs.len(), 2);
        assert_eq!(mapping.outputs[0].sources[0].runs, [(0, 2)]);
        assert_eq!(mapping.outputs[1].sources[0].runs, [(2, 2)]);

        let pivot = RelPlanV1::new(Relation::Pivot {
            input: Box::new(Relation::SnapshotRead(read)),
            groups: vec![group.id],
            names_from: name.id,
            values_from: value.id,
            aggregate: AggregateFunction::MeanV1,
        });
        let pivot_output = execute(pivot.clone(), inputs.clone());
        let mapping = decoded(
            derive_row_mapping(&pivot, &inputs, &pivot_output.table.row_ids, &store).unwrap(),
            &store,
        );
        assert_eq!(mapping.outputs.len(), 2);
        assert_eq!(
            mapping.outputs[0].sources[0]
                .runs
                .iter()
                .map(|(_, length)| length)
                .sum::<u64>(),
            2
        );

        let left_key = ColumnSchema::new("left key", LogicalType::Utf8);
        let left_value = ColumnSchema::new("left value", LogicalType::Float64);
        let right_key = ColumnSchema::new("right key", LogicalType::Utf8);
        let right_value = ColumnSchema::new("right value", LogicalType::Float64);
        let (left_identity, left, left_read) = input(
            TableId::new(),
            RevisionId::new(),
            vec![
                (
                    left_key.clone(),
                    ["a", "b"]
                        .map(|value| ScalarValue::Utf8(value.into()))
                        .into(),
                ),
                (left_value, [1.0, 2.0].map(ScalarValue::Float64).into()),
            ],
        );
        let (right_identity, right, right_read) = input(
            TableId::new(),
            RevisionId::new(),
            vec![
                (
                    right_key.clone(),
                    ["a", "c"]
                        .map(|value| ScalarValue::Utf8(value.into()))
                        .into(),
                ),
                (right_value, [10.0, 30.0].map(ScalarValue::Float64).into()),
            ],
        );
        let join_inputs = BTreeMap::from([(left_identity, left), (right_identity, right)]);
        let join = RelPlanV1::new(Relation::Join {
            left: Box::new(Relation::SnapshotRead(left_read)),
            right: Box::new(Relation::SnapshotRead(right_read)),
            kind: JoinKind::Full,
            keys: vec![JoinKey {
                left: left_key.id,
                right: right_key.id,
            }],
            cardinality: JoinCardinality::OneToOne,
        });
        let join_output = execute(join.clone(), join_inputs.clone());
        let mapping = decoded(
            derive_row_mapping(&join, &join_inputs, &join_output.table.row_ids, &store).unwrap(),
            &store,
        );
        assert_eq!(mapping.outputs.len(), 3);
        assert_eq!(mapping.outputs[0].sources.len(), 2);
        assert_eq!(mapping.outputs[1].sources.len(), 1);
        assert_eq!(mapping.outputs[2].sources.len(), 1);
    }
}
