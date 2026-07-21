use crate::{
    ARROW_IPC_CODEC_V1, BlockStore, CellPatch, CodecRegistry, ColumnChunk, ColumnId, ColumnValues,
    ContentHash, DataError, LogicalType, Result, RowId, TableId, TableSchema, UncertaintyRelation,
    logical_fingerprint,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::str::FromStr;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChunkDescriptor {
    pub row_start: u64,
    pub row_count: u64,
    pub codec: String,
    /// Hash of the exact encoded bytes used for corruption detection and block
    /// addressing. It may differ when a future codec re-encodes equal values.
    pub byte_hash: ContentHash,
    /// Backend-independent fingerprint of logical values plus null validity.
    pub logical_fingerprint: ContentHash,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ColumnManifest {
    pub column: ColumnId,
    pub chunks: Vec<ChunkDescriptor>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TableSnapshot {
    pub table_id: TableId,
    pub schema: TableSchema,
    pub row_count: u64,
    pub row_id_chunks: Vec<ChunkDescriptor>,
    pub columns: Vec<ColumnManifest>,
    #[serde(default)]
    pub uncertainty: Vec<UncertaintyRelation>,
    #[serde(default)]
    pub metadata: BTreeMap<String, serde_json::Value>,
    pub fingerprint: ContentHash,
}

impl TableSnapshot {
    pub fn column(&self, id: ColumnId) -> Option<&ColumnManifest> {
        self.columns.iter().find(|column| column.column == id)
    }

    pub fn batch_count(&self) -> usize {
        self.row_id_chunks.len()
    }

    pub fn validate(&self) -> Result<()> {
        self.schema.validate()?;
        validate_chunk_sequence(&self.row_id_chunks, self.row_count, "row identity")?;
        if self.columns.len() != self.schema.columns.len() {
            return Err(DataError::InvalidSchema(format!(
                "snapshot has {} column manifests for {} schema columns",
                self.columns.len(),
                self.schema.columns.len()
            )));
        }
        for (schema, manifest) in self.schema.columns.iter().zip(&self.columns) {
            if schema.id != manifest.column {
                return Err(DataError::InvalidSchema(format!(
                    "column manifest {} is out of schema order",
                    manifest.column
                )));
            }
            validate_chunk_sequence(&manifest.chunks, self.row_count, &schema.name)?;
            if manifest
                .chunks
                .iter()
                .zip(&self.row_id_chunks)
                .any(|(column, rows)| {
                    column.row_start != rows.row_start || column.row_count != rows.row_count
                })
            {
                return Err(DataError::InvalidSchema(format!(
                    "column {:?} chunks do not align with row identity chunks",
                    schema.name
                )));
            }
        }
        for relation in &self.uncertainty {
            relation.validate(&self.schema)?;
        }
        let expected = snapshot_fingerprint(
            self.table_id,
            &self.schema,
            self.row_count,
            &self.row_id_chunks,
            &self.columns,
            &self.uncertainty,
            &self.metadata,
        )?;
        if expected != self.fingerprint {
            return Err(DataError::CorruptBlock(
                "snapshot logical fingerprint does not match its manifest".into(),
            ));
        }
        Ok(())
    }
}

fn validate_chunk_sequence(chunks: &[ChunkDescriptor], total: u64, label: &str) -> Result<()> {
    let mut expected_start = 0;
    for chunk in chunks {
        if chunk.row_start != expected_start || chunk.row_count == 0 {
            return Err(DataError::InvalidSchema(format!(
                "{label} has a gap, overlap, or empty chunk at row {}",
                chunk.row_start
            )));
        }
        expected_start = expected_start
            .checked_add(chunk.row_count)
            .ok_or_else(|| DataError::InvalidSchema(format!("{label} row count overflowed")))?;
    }
    if expected_start != total {
        return Err(DataError::InvalidSchema(format!(
            "{label} chunks contain {expected_start} rows, expected {total}"
        )));
    }
    Ok(())
}

pub struct SnapshotBuilder<'a> {
    table_id: TableId,
    schema: TableSchema,
    store: &'a dyn BlockStore,
    codecs: &'a CodecRegistry,
    codec: String,
    row_count: u64,
    row_ids: Vec<ChunkDescriptor>,
    columns: Vec<ColumnManifest>,
    seen_rows: BTreeSet<RowId>,
    verify_row_uniqueness: bool,
    business_key_values: Vec<BTreeSet<Vec<Vec<u8>>>>,
    uncertainty: Vec<UncertaintyRelation>,
    metadata: BTreeMap<String, serde_json::Value>,
}

impl<'a> SnapshotBuilder<'a> {
    pub fn new(
        table_id: TableId,
        schema: TableSchema,
        store: &'a dyn BlockStore,
        codecs: &'a CodecRegistry,
    ) -> Result<Self> {
        schema.validate()?;
        let columns = schema
            .columns
            .iter()
            .map(|column| ColumnManifest {
                column: column.id,
                chunks: Vec::new(),
            })
            .collect();
        let business_key_count = schema.business_keys.len();
        Ok(Self {
            table_id,
            schema,
            store,
            codecs,
            codec: ARROW_IPC_CODEC_V1.into(),
            row_count: 0,
            row_ids: Vec::new(),
            columns,
            business_key_values: vec![BTreeSet::new(); business_key_count],
            seen_rows: BTreeSet::new(),
            verify_row_uniqueness: true,
            uncertainty: Vec::new(),
            metadata: BTreeMap::new(),
        })
    }

    /// Skip the cross-batch RowId set when the producer already guarantees
    /// identity uniqueness (for example, a verified row-preserving plan).
    /// Per-batch duplicates are still rejected. This keeps streamed snapshots
    /// bounded by the batch size instead of the total row count.
    pub fn with_trusted_row_identity(mut self) -> Self {
        self.verify_row_uniqueness = false;
        self
    }

    pub fn with_codec(mut self, codec: impl Into<String>) -> Result<Self> {
        let codec = codec.into();
        self.codecs.get(&codec)?;
        self.codec = codec;
        Ok(self)
    }

    pub fn set_uncertainty(&mut self, relations: Vec<UncertaintyRelation>) -> Result<()> {
        for relation in &relations {
            relation.validate(&self.schema)?;
        }
        self.uncertainty = relations;
        Ok(())
    }

    pub fn metadata_mut(&mut self) -> &mut BTreeMap<String, serde_json::Value> {
        &mut self.metadata
    }

    /// Append one aligned record batch. Chunks are encoded and stored
    /// immediately, so callers never need to materialize the complete table.
    pub fn push_batch(&mut self, row_ids: &[RowId], columns: &[ColumnChunk]) -> Result<()> {
        if row_ids.is_empty() {
            return Err(DataError::InvalidArray("empty snapshot batch".into()));
        }
        if columns.len() != self.schema.columns.len() {
            return Err(DataError::InvalidArray(format!(
                "batch contains {} columns, expected {}",
                columns.len(),
                self.schema.columns.len()
            )));
        }
        let mut batch_rows = BTreeSet::new();
        for row in row_ids {
            if (self.verify_row_uniqueness && self.seen_rows.contains(row))
                || !batch_rows.insert(*row)
            {
                return Err(DataError::InvalidArray(format!(
                    "duplicate stable row id {row}"
                )));
            }
        }
        let row_count = u64::try_from(row_ids.len())
            .map_err(|_| DataError::InvalidArray("batch is too large".into()))?;
        for (schema, chunk) in self.schema.columns.iter().zip(columns) {
            if chunk.len() != row_ids.len() {
                return Err(DataError::InvalidArray(format!(
                    "column {:?} has {} values for {} rows",
                    schema.name,
                    chunk.len(),
                    row_ids.len()
                )));
            }
            chunk.validate_type(&schema.logical_type)?;
            if !schema.nullable && chunk.validity().null_count() != 0 {
                return Err(DataError::InvalidArray(format!(
                    "non-null column {:?} contains null values",
                    schema.name
                )));
            }
        }
        let mut batch_business_keys = Vec::with_capacity(self.schema.business_keys.len());
        for (key_index, key) in self.schema.business_keys.iter().enumerate() {
            let indices = key
                .columns
                .iter()
                .map(|column| {
                    self.schema
                        .column_index(*column)
                        .ok_or(DataError::MissingColumn(*column))
                })
                .collect::<Result<Vec<_>>>()?;
            let mut encoded = BTreeSet::new();
            for row in 0..row_ids.len() {
                let value = indices
                    .iter()
                    .map(|column| {
                        columns[*column]
                            .value(row)
                            .map(|value| crate::execute::scalar_key(&value))
                            .ok_or_else(|| {
                                DataError::InvalidArray(format!(
                                    "business key {:?} contains null",
                                    key.name
                                ))
                            })
                    })
                    .collect::<Result<Vec<_>>>()?;
                if self.business_key_values[key_index].contains(&value) || !encoded.insert(value) {
                    return Err(DataError::InvalidArray(format!(
                        "business key {:?} is not unique",
                        key.name
                    )));
                }
            }
            batch_business_keys.push(encoded);
        }

        if self.verify_row_uniqueness {
            self.seen_rows.extend(batch_rows);
        }
        for (target, values) in self.business_key_values.iter_mut().zip(batch_business_keys) {
            target.extend(values);
        }
        let row_values = ColumnValues::Utf8(row_ids.iter().map(ToString::to_string).collect());
        let row_chunk = ColumnChunk::all_valid(row_values);
        let row_descriptor = self.store_chunk(&LogicalType::Utf8, &row_chunk, row_count)?;
        self.row_ids.push(row_descriptor);
        for (index, chunk) in columns.iter().enumerate() {
            let descriptor =
                self.store_chunk(&self.schema.columns[index].logical_type, chunk, row_count)?;
            self.columns[index].chunks.push(descriptor);
        }
        self.row_count = self
            .row_count
            .checked_add(row_count)
            .ok_or_else(|| DataError::InvalidArray("table row count overflowed".into()))?;
        Ok(())
    }

    fn store_chunk(
        &self,
        logical_type: &crate::LogicalType,
        chunk: &ColumnChunk,
        row_count: u64,
    ) -> Result<ChunkDescriptor> {
        let codec = self.codecs.get(&self.codec)?;
        let bytes = codec.encode(logical_type, chunk)?;
        let byte_hash = self.store.put(bytes)?;
        Ok(ChunkDescriptor {
            row_start: self.row_count,
            row_count,
            codec: self.codec.clone(),
            byte_hash,
            logical_fingerprint: logical_fingerprint(logical_type, chunk)?,
        })
    }

    pub fn finish(self) -> Result<TableSnapshot> {
        let fingerprint = snapshot_fingerprint(
            self.table_id,
            &self.schema,
            self.row_count,
            &self.row_ids,
            &self.columns,
            &self.uncertainty,
            &self.metadata,
        )?;
        let snapshot = TableSnapshot {
            table_id: self.table_id,
            schema: self.schema,
            row_count: self.row_count,
            row_id_chunks: self.row_ids,
            columns: self.columns,
            uncertainty: self.uncertainty,
            metadata: self.metadata,
            fingerprint,
        };
        snapshot.validate()?;
        Ok(snapshot)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct TableBatch {
    pub row_start: u64,
    pub row_ids: Vec<RowId>,
    pub columns: Vec<(ColumnId, ColumnChunk)>,
}

pub struct SnapshotReader<'a> {
    snapshot: &'a TableSnapshot,
    store: &'a dyn BlockStore,
    codecs: &'a CodecRegistry,
}

impl<'a> SnapshotReader<'a> {
    pub fn new(
        snapshot: &'a TableSnapshot,
        store: &'a dyn BlockStore,
        codecs: &'a CodecRegistry,
    ) -> Result<Self> {
        snapshot.validate()?;
        Ok(Self {
            snapshot,
            store,
            codecs,
        })
    }

    pub fn snapshot(&self) -> &TableSnapshot {
        self.snapshot
    }

    pub fn read_row_ids(&self, index: usize) -> Result<(u64, Vec<RowId>)> {
        let descriptor = self
            .snapshot
            .row_id_chunks
            .get(index)
            .ok_or_else(|| DataError::InvalidArray(format!("batch {index} does not exist")))?;
        let chunk = self.read_chunk(&LogicalType::Utf8, descriptor)?;
        let ColumnValues::Utf8(values) = chunk.values() else {
            return Err(DataError::CorruptBlock(
                "row identity block is not UTF-8".into(),
            ));
        };
        let rows = values
            .iter()
            .map(|value| {
                RowId::from_str(value).map_err(|_| {
                    DataError::CorruptBlock(format!("invalid stable row id {value:?}"))
                })
            })
            .collect::<Result<Vec<_>>>()?;
        Ok((descriptor.row_start, rows))
    }

    pub fn read_batch(&self, index: usize, projection: &[ColumnId]) -> Result<TableBatch> {
        let row_descriptor = self
            .snapshot
            .row_id_chunks
            .get(index)
            .ok_or_else(|| DataError::InvalidArray(format!("batch {index} does not exist")))?;
        let (_, row_ids) = self.read_row_ids(index)?;
        let selected: Vec<ColumnId> = if projection.is_empty() {
            self.snapshot
                .schema
                .columns
                .iter()
                .map(|column| column.id)
                .collect()
        } else {
            projection.to_vec()
        };
        let mut columns = Vec::with_capacity(selected.len());
        for id in selected {
            let schema = self
                .snapshot
                .schema
                .column(id)
                .ok_or(DataError::MissingColumn(id))?;
            let descriptor = self
                .snapshot
                .column(id)
                .and_then(|column| column.chunks.get(index))
                .ok_or_else(|| DataError::CorruptBlock(format!("missing chunk for column {id}")))?;
            columns.push((id, self.read_chunk(&schema.logical_type, descriptor)?));
        }
        Ok(TableBatch {
            row_start: row_descriptor.row_start,
            row_ids,
            columns,
        })
    }

    /// Stream declared business-key columns and verify uniqueness without
    /// materializing unrelated table values.
    pub fn validate_business_keys(&self) -> Result<()> {
        let mut seen = vec![BTreeSet::new(); self.snapshot.schema.business_keys.len()];
        for (key_index, key) in self.snapshot.schema.business_keys.iter().enumerate() {
            for batch_index in 0..self.snapshot.batch_count() {
                let batch = self.read_batch(batch_index, &key.columns)?;
                for row in 0..batch.row_ids.len() {
                    let encoded = batch
                        .columns
                        .iter()
                        .map(|(_, column)| {
                            column
                                .value(row)
                                .map(|value| crate::execute::scalar_key(&value))
                                .ok_or_else(|| {
                                    DataError::InvalidArray(format!(
                                        "business key {:?} contains null",
                                        key.name
                                    ))
                                })
                        })
                        .collect::<Result<Vec<_>>>()?;
                    if !seen[key_index].insert(encoded) {
                        return Err(DataError::InvalidArray(format!(
                            "business key {:?} is not unique",
                            key.name
                        )));
                    }
                }
            }
        }
        Ok(())
    }

    fn read_chunk(
        &self,
        logical_type: &crate::LogicalType,
        descriptor: &ChunkDescriptor,
    ) -> Result<ColumnChunk> {
        let bytes = self.store.get(descriptor.byte_hash)?;
        if ContentHash::of(&bytes) != descriptor.byte_hash {
            return Err(DataError::CorruptBlock(format!(
                "byte hash mismatch for {}",
                descriptor.byte_hash
            )));
        }
        let chunk = self
            .codecs
            .get(&descriptor.codec)?
            .decode(logical_type, &bytes)?;
        if chunk.len() as u64 != descriptor.row_count
            || logical_fingerprint(logical_type, &chunk)? != descriptor.logical_fingerprint
        {
            return Err(DataError::CorruptBlock(format!(
                "logical fingerprint mismatch for {}",
                descriptor.byte_hash
            )));
        }
        Ok(chunk)
    }
}

/// Apply stable cell patches by rewriting only affected column chunks. Row ID
/// chunks and every untouched value chunk retain their content hashes.
pub fn patch_snapshot(
    snapshot: &TableSnapshot,
    edits: &[CellPatch],
    store: &dyn BlockStore,
    codecs: &CodecRegistry,
) -> Result<TableSnapshot> {
    snapshot.validate()?;
    let mut pending = BTreeMap::new();
    let mut projection = BTreeSet::new();
    for edit in edits {
        let schema = snapshot
            .schema
            .column(edit.column)
            .ok_or(DataError::MissingColumn(edit.column))?;
        let value = crate::execute_expr::literal_scalar(&edit.value);
        crate::execute::validate_scalar(&value, schema)?;
        if pending.insert((edit.row, edit.column), value).is_some() {
            return Err(DataError::InvalidPlan(
                "a patch addresses the same cell more than once".into(),
            ));
        }
        projection.insert(edit.column);
    }
    if pending.is_empty() {
        return Ok(snapshot.clone());
    }

    let projection = projection.into_iter().collect::<Vec<_>>();
    let reader = SnapshotReader::new(snapshot, store, codecs)?;
    let mut result = snapshot.clone();
    for batch_index in 0..snapshot.batch_count() {
        let batch = reader.read_batch(batch_index, &projection)?;
        for (column_id, mut chunk) in batch.columns {
            let mut changed = false;
            for (row_index, row_id) in batch.row_ids.iter().enumerate() {
                if let Some(value) = pending.remove(&(*row_id, column_id)) {
                    chunk.set_value(row_index, value)?;
                    changed = true;
                }
            }
            if !changed {
                continue;
            }
            let schema = snapshot
                .schema
                .column(column_id)
                .ok_or(DataError::MissingColumn(column_id))?;
            let descriptor = result
                .columns
                .iter_mut()
                .find(|column| column.column == column_id)
                .and_then(|column| column.chunks.get_mut(batch_index))
                .ok_or_else(|| {
                    DataError::CorruptBlock(format!("missing chunk for column {column_id}"))
                })?;
            let bytes = codecs
                .get(&descriptor.codec)?
                .encode(&schema.logical_type, &chunk)?;
            descriptor.byte_hash = store.put(bytes)?;
            descriptor.logical_fingerprint = logical_fingerprint(&schema.logical_type, &chunk)?;
        }
    }
    if let Some(((row, _), _)) = pending.into_iter().next() {
        return Err(DataError::MissingRow(row));
    }
    result.fingerprint = snapshot_fingerprint(
        result.table_id,
        &result.schema,
        result.row_count,
        &result.row_id_chunks,
        &result.columns,
        &result.uncertainty,
        &result.metadata,
    )?;
    result.validate()?;
    Ok(result)
}

#[allow(clippy::too_many_arguments)]
fn snapshot_fingerprint(
    table_id: TableId,
    schema: &TableSchema,
    row_count: u64,
    row_ids: &[ChunkDescriptor],
    columns: &[ColumnManifest],
    uncertainty: &[UncertaintyRelation],
    metadata: &BTreeMap<String, serde_json::Value>,
) -> Result<ContentHash> {
    let canonical = serde_json::to_vec(&(
        "plotx.table-snapshot.v1",
        table_id,
        schema,
        row_count,
        row_ids,
        columns,
        uncertainty,
        metadata,
    ))
    .map_err(|error| DataError::Backend(error.to_string()))?;
    Ok(ContentHash::of(&canonical))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        BusinessKey, ColumnSchema, FiniteOrSpecial, LiteralValue, LogicalType, MemoryBlockStore,
        ScalarValue, Validity,
    };

    #[test]
    fn snapshots_stream_batches_and_deduplicate_equal_blocks() {
        let x = ColumnSchema::new("x", LogicalType::Float64);
        let schema = TableSchema::new(vec![x.clone()]).unwrap();
        let store = MemoryBlockStore::default();
        let codecs = CodecRegistry::with_arrow_ipc();
        let mut builder = SnapshotBuilder::new(TableId::new(), schema, &store, &codecs).unwrap();
        let rows = [RowId::new(), RowId::new()];
        builder
            .push_batch(
                &rows,
                &[ColumnChunk::new(
                    ColumnValues::Float64(vec![1.0, f64::NAN]),
                    Validity::from_valid([true, false]),
                )
                .unwrap()],
            )
            .unwrap();
        let snapshot = builder.finish().unwrap();
        let reader = SnapshotReader::new(&snapshot, &store, &codecs).unwrap();
        let batch = reader.read_batch(0, &[x.id]).unwrap();
        assert_eq!(batch.row_ids, rows);
        assert_eq!(batch.columns[0].1.value(1), Some(ScalarValue::Null));
        assert_eq!(snapshot.row_count, 2);
    }

    #[test]
    fn patch_rewrites_only_the_affected_column_chunk() {
        let x = ColumnSchema::new("x", LogicalType::Float64);
        let y = ColumnSchema::new("y", LogicalType::Float64);
        let schema = TableSchema::new(vec![x.clone(), y.clone()]).unwrap();
        let store = MemoryBlockStore::default();
        let codecs = CodecRegistry::with_arrow_ipc();
        let mut builder = SnapshotBuilder::new(TableId::new(), schema, &store, &codecs).unwrap();
        let rows = [RowId::new(), RowId::new(), RowId::new(), RowId::new()];
        for batch in 0..2 {
            let start = batch * 2;
            builder
                .push_batch(
                    &rows[start..start + 2],
                    &[
                        ColumnChunk::all_valid(ColumnValues::Float64(vec![
                            start as f64,
                            start as f64 + 1.0,
                        ])),
                        ColumnChunk::all_valid(ColumnValues::Float64(vec![10.0, 20.0])),
                    ],
                )
                .unwrap();
        }
        let before = builder.finish().unwrap();
        let after = patch_snapshot(
            &before,
            &[CellPatch {
                row: rows[3],
                column: y.id,
                value: LiteralValue::Float64(FiniteOrSpecial::Finite(99.0)),
            }],
            &store,
            &codecs,
        )
        .unwrap();

        assert_eq!(before.row_id_chunks, after.row_id_chunks);
        assert_eq!(before.columns[0].chunks, after.columns[0].chunks);
        assert_eq!(before.columns[1].chunks[0], after.columns[1].chunks[0]);
        assert_ne!(
            before.columns[1].chunks[1].byte_hash,
            after.columns[1].chunks[1].byte_hash
        );
        let batch = SnapshotReader::new(&after, &store, &codecs)
            .unwrap()
            .read_batch(1, &[y.id])
            .unwrap();
        assert_eq!(
            batch.columns[0].1.value(1),
            Some(ScalarValue::Float64(99.0))
        );
    }

    #[test]
    fn business_key_uniqueness_is_enforced_across_streamed_batches() {
        let mut sample = ColumnSchema::new("sample", LogicalType::Int64);
        sample.nullable = false;
        let mut schema = TableSchema::new(vec![sample.clone()]).unwrap();
        schema.business_keys.push(BusinessKey {
            name: "sample".into(),
            columns: vec![sample.id],
        });
        let store = MemoryBlockStore::default();
        let codecs = CodecRegistry::with_arrow_ipc();
        let mut builder = SnapshotBuilder::new(TableId::new(), schema, &store, &codecs).unwrap();
        builder
            .push_batch(
                &[RowId::new()],
                &[ColumnChunk::all_valid(ColumnValues::Int64(vec![1]))],
            )
            .unwrap();
        let error = builder
            .push_batch(
                &[RowId::new()],
                &[ColumnChunk::all_valid(ColumnValues::Int64(vec![1]))],
            )
            .unwrap_err();
        assert!(error.to_string().contains("not unique"));
        builder
            .push_batch(
                &[RowId::new()],
                &[ColumnChunk::all_valid(ColumnValues::Int64(vec![2]))],
            )
            .unwrap();
        assert_eq!(builder.finish().unwrap().row_count, 2);
    }
}
