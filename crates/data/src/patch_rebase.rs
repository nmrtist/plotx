use crate::{
    BlockStore, BusinessKey, CellPatch, CodecRegistry, DataError, RowId, SnapshotReader,
    TableSnapshot,
};
use std::collections::{BTreeMap, BTreeSet};

/// Inputs for strict patch migration between immutable refresh results.
///
/// `provenance_rows` is authoritative. Rows it does not resolve may use one
/// explicitly selected, declared unique business key. No positional or fuzzy
/// fallback exists.
pub struct PatchRebaseRequest<'a> {
    pub patches: &'a [CellPatch],
    pub provenance_rows: &'a BTreeMap<RowId, RowId>,
    pub old_snapshot: &'a TableSnapshot,
    pub old_store: &'a dyn BlockStore,
    pub new_snapshot: &'a TableSnapshot,
    pub new_store: &'a dyn BlockStore,
    pub codecs: &'a CodecRegistry,
    pub business_key: Option<&'a str>,
}

/// Rebase cell patches by exact source identity and then a declared unique
/// business key. Every unresolved or ambiguous condition blocks the rebase.
pub fn rebase_patches_to_snapshot(
    request: PatchRebaseRequest<'_>,
) -> crate::Result<Vec<CellPatch>> {
    let patched_rows = request
        .patches
        .iter()
        .map(|patch| patch.row)
        .collect::<BTreeSet<_>>();
    let new_rows = read_row_set(request.new_snapshot, request.new_store, request.codecs)?;
    let mut resolved = BTreeMap::new();
    for row in &patched_rows {
        if let Some(target) = request.provenance_rows.get(row) {
            if !new_rows.contains(target) {
                return Err(DataError::PatchConflict(format!(
                    "source mapping for row {row} targets missing row {target}"
                )));
            }
            resolved.insert(*row, *target);
        }
    }

    let unresolved = patched_rows
        .difference(&resolved.keys().copied().collect())
        .copied()
        .collect::<BTreeSet<_>>();
    if !unresolved.is_empty() {
        let (old_key, new_key) = select_business_key(
            request.old_snapshot,
            request.new_snapshot,
            request.business_key,
        )?;
        validate_key_contract(old_key, new_key, request.old_snapshot, request.new_snapshot)?;
        let old_index = index_business_key(
            request.old_snapshot,
            request.old_store,
            request.codecs,
            old_key,
        )?;
        let new_index = index_business_key(
            request.new_snapshot,
            request.new_store,
            request.codecs,
            new_key,
        )?;
        for row in unresolved {
            let key = old_index.by_row.get(&row).ok_or_else(|| {
                DataError::PatchConflict(format!("row {row} is absent from the old snapshot"))
            })?;
            let target = new_index.by_key.get(key).copied().ok_or_else(|| {
                DataError::PatchConflict(format!(
                    "row {row} has no match for business key {:?}",
                    old_key.name
                ))
            })?;
            resolved.insert(row, target);
        }
    }

    crate::rebase_patches(
        request.patches,
        &resolved,
        &request.old_snapshot.schema,
        &request.new_snapshot.schema,
    )
}

fn read_row_set(
    snapshot: &TableSnapshot,
    store: &dyn BlockStore,
    codecs: &CodecRegistry,
) -> crate::Result<BTreeSet<RowId>> {
    let reader = SnapshotReader::new(snapshot, store, codecs)?;
    let mut rows = BTreeSet::new();
    for batch in 0..snapshot.batch_count() {
        rows.extend(reader.read_row_ids(batch)?.1);
    }
    Ok(rows)
}

fn select_business_key<'a>(
    old: &'a TableSnapshot,
    new: &'a TableSnapshot,
    requested: Option<&str>,
) -> crate::Result<(&'a BusinessKey, &'a BusinessKey)> {
    let shared = old
        .schema
        .business_keys
        .iter()
        .filter_map(|old_key| {
            let new_key = new
                .schema
                .business_keys
                .iter()
                .find(|candidate| candidate.name == old_key.name)?;
            requested
                .is_none_or(|name| name == old_key.name)
                .then_some((old_key, new_key))
        })
        .collect::<Vec<_>>();
    match shared.as_slice() {
        [pair] => Ok(*pair),
        [] => Err(DataError::PatchConflict(match requested {
            Some(name) => format!("business key {name:?} is not shared by both snapshots"),
            None => "no shared business key can resolve unmatched patch rows".into(),
        })),
        _ => Err(DataError::PatchConflict(
            "multiple shared business keys require explicit selection".into(),
        )),
    }
}

fn validate_key_contract(
    old_key: &BusinessKey,
    new_key: &BusinessKey,
    old: &TableSnapshot,
    new: &TableSnapshot,
) -> crate::Result<()> {
    if old_key.columns.len() != new_key.columns.len() {
        return Err(DataError::PatchConflict(format!(
            "business key {:?} changed arity",
            old_key.name
        )));
    }
    for (old_id, new_id) in old_key.columns.iter().zip(&new_key.columns) {
        let old_column = old
            .schema
            .column(*old_id)
            .ok_or(DataError::MissingColumn(*old_id))?;
        let new_column = new
            .schema
            .column(*new_id)
            .ok_or(DataError::MissingColumn(*new_id))?;
        if old_column.logical_type != new_column.logical_type {
            return Err(DataError::PatchConflict(format!(
                "business key {:?} changed type",
                old_key.name
            )));
        }
        if old_column.unit != new_column.unit {
            return Err(DataError::PatchConflict(format!(
                "business key {:?} changed unit",
                old_key.name
            )));
        }
    }
    Ok(())
}

struct KeyIndex {
    by_row: BTreeMap<RowId, Vec<Vec<u8>>>,
    by_key: BTreeMap<Vec<Vec<u8>>, RowId>,
}

fn index_business_key(
    snapshot: &TableSnapshot,
    store: &dyn BlockStore,
    codecs: &CodecRegistry,
    key: &BusinessKey,
) -> crate::Result<KeyIndex> {
    let reader = SnapshotReader::new(snapshot, store, codecs)?;
    let mut by_row = BTreeMap::new();
    let mut by_key = BTreeMap::new();
    for batch_index in 0..snapshot.batch_count() {
        let batch = reader.read_batch(batch_index, &key.columns)?;
        for (index, row) in batch.row_ids.iter().copied().enumerate() {
            let value = batch
                .columns
                .iter()
                .map(|(_, chunk)| {
                    let scalar = chunk.value(index).ok_or_else(|| {
                        DataError::PatchConflict(format!(
                            "business key {:?} contains null",
                            key.name
                        ))
                    })?;
                    if matches!(scalar, crate::ScalarValue::Null) {
                        return Err(DataError::PatchConflict(format!(
                            "business key {:?} contains null",
                            key.name
                        )));
                    }
                    Ok(crate::execute::scalar_key(&scalar))
                })
                .collect::<crate::Result<Vec<_>>>()?;
            if let Some(previous) = by_key.insert(value.clone(), row) {
                return Err(DataError::PatchConflict(format!(
                    "business key {:?} is duplicated by rows {previous} and {row}",
                    key.name
                )));
            }
            by_row.insert(row, value);
        }
    }
    Ok(KeyIndex { by_row, by_key })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        ColumnChunk, ColumnSchema, ColumnValues, FiniteOrSpecial, LiteralValue, LogicalType,
        MemoryBlockStore, SnapshotBuilder, TableId, TableSchema, Validity,
    };

    fn keyed_snapshot(
        key_column: crate::ColumnId,
        value_column: crate::ColumnId,
        keys: &[&str],
        rows: &[RowId],
    ) -> (TableSnapshot, MemoryBlockStore, CodecRegistry) {
        let mut key = ColumnSchema::new("sample", LogicalType::Utf8);
        key.id = key_column;
        key.nullable = false;
        let mut value = ColumnSchema::new("value", LogicalType::Float64);
        value.id = value_column;
        let mut schema = TableSchema::new(vec![key, value]).unwrap();
        schema.business_keys.push(BusinessKey {
            name: "sample-id".into(),
            columns: vec![key_column],
        });
        schema.validate().unwrap();
        let store = MemoryBlockStore::default();
        let codecs = CodecRegistry::with_arrow_ipc();
        let mut builder = SnapshotBuilder::new(TableId::new(), schema, &store, &codecs).unwrap();
        builder
            .push_batch(
                rows,
                &[
                    ColumnChunk::new(
                        ColumnValues::Utf8(keys.iter().map(|key| (*key).to_owned()).collect()),
                        Validity::all_valid(rows.len()),
                    )
                    .unwrap(),
                    ColumnChunk::new(
                        ColumnValues::Float64((0..rows.len()).map(|n| n as f64).collect()),
                        Validity::all_valid(rows.len()),
                    )
                    .unwrap(),
                ],
            )
            .unwrap();
        (builder.finish().unwrap(), store, codecs)
    }

    #[test]
    fn provenance_wins_then_business_key_resolves_remaining_rows() {
        let key = crate::ColumnId::new();
        let value = crate::ColumnId::new();
        let old_rows = [RowId::new(), RowId::new()];
        let new_rows = [RowId::new(), RowId::new()];
        let (old, old_store, codecs) = keyed_snapshot(key, value, &["one", "two"], &old_rows);
        let (new, new_store, _) = keyed_snapshot(key, value, &["two", "one"], &new_rows);
        let patches = old_rows
            .iter()
            .map(|row| CellPatch {
                row: *row,
                column: value,
                value: LiteralValue::Float64(FiniteOrSpecial::new(7.0)),
            })
            .collect::<Vec<_>>();
        let rebased = rebase_patches_to_snapshot(PatchRebaseRequest {
            patches: &patches,
            provenance_rows: &BTreeMap::from([(old_rows[0], new_rows[1])]),
            old_snapshot: &old,
            old_store: &old_store,
            new_snapshot: &new,
            new_store: &new_store,
            codecs: &codecs,
            business_key: Some("sample-id"),
        })
        .unwrap();
        assert_eq!(rebased[0].row, new_rows[1]);
        assert_eq!(rebased[1].row, new_rows[0]);
    }

    #[test]
    fn missing_business_key_value_blocks_rebase() {
        let key = crate::ColumnId::new();
        let value = crate::ColumnId::new();
        let old_row = RowId::new();
        let new_row = RowId::new();
        let (old, old_store, codecs) = keyed_snapshot(key, value, &["gone"], &[old_row]);
        let (new, new_store, _) = keyed_snapshot(key, value, &["new"], &[new_row]);
        let patch = CellPatch {
            row: old_row,
            column: value,
            value: LiteralValue::Null,
        };
        let error = rebase_patches_to_snapshot(PatchRebaseRequest {
            patches: &[patch],
            provenance_rows: &BTreeMap::new(),
            old_snapshot: &old,
            old_store: &old_store,
            new_snapshot: &new,
            new_store: &new_store,
            codecs: &codecs,
            business_key: None,
        })
        .unwrap_err();
        assert!(error.to_string().contains("no match for business key"));
    }
}
