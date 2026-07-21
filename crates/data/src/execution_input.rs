use crate::{
    BlockStore, CodecRegistry, ContentHash, MaterializedColumn, MaterializedTable, Result,
    SnapshotReader, TableSchema, TableSnapshot,
};
use std::{fmt, sync::Arc};

/// A pinned execution input, either already materialized or backed by an
/// immutable chunked snapshot. Snapshot-backed inputs let production adapters
/// consume one batch at a time without exposing Arrow through PlotX's API.
#[derive(Clone)]
pub enum ExecutionInput {
    Materialized {
        table: MaterializedTable,
        snapshot_fingerprint: ContentHash,
    },
    Snapshot {
        snapshot: TableSnapshot,
        store: Arc<dyn BlockStore>,
    },
}

impl ExecutionInput {
    pub fn materialized(table: MaterializedTable, snapshot_fingerprint: ContentHash) -> Self {
        Self::Materialized {
            table,
            snapshot_fingerprint,
        }
    }

    pub fn snapshot(snapshot: TableSnapshot, store: Arc<dyn BlockStore>) -> Result<Self> {
        snapshot.validate()?;
        Ok(Self::Snapshot { snapshot, store })
    }

    pub fn schema(&self) -> &TableSchema {
        match self {
            Self::Materialized { table, .. } => &table.schema,
            Self::Snapshot { snapshot, .. } => &snapshot.schema,
        }
    }

    pub fn row_count(&self) -> u64 {
        match self {
            Self::Materialized { table, .. } => table.row_ids.len() as u64,
            Self::Snapshot { snapshot, .. } => snapshot.row_count,
        }
    }

    pub fn table_id(&self) -> crate::TableId {
        match self {
            Self::Materialized { table, .. } => table.table_id,
            Self::Snapshot { snapshot, .. } => snapshot.table_id,
        }
    }

    pub fn snapshot_fingerprint(&self) -> ContentHash {
        match self {
            Self::Materialized {
                snapshot_fingerprint,
                ..
            } => *snapshot_fingerprint,
            Self::Snapshot { snapshot, .. } => snapshot.fingerprint,
        }
    }

    pub fn materialize(&self) -> Result<MaterializedTable> {
        match self {
            Self::Materialized { table, .. } => Ok(table.clone()),
            Self::Snapshot { snapshot, store } => {
                let codecs = CodecRegistry::with_arrow_ipc();
                let reader = SnapshotReader::new(snapshot, store.as_ref(), &codecs)?;
                let mut row_ids = Vec::with_capacity(snapshot.row_count as usize);
                let mut columns = snapshot
                    .schema
                    .columns
                    .iter()
                    .cloned()
                    .map(|schema| MaterializedColumn {
                        schema,
                        values: Vec::new(),
                    })
                    .collect::<Vec<_>>();
                for batch_index in 0..snapshot.batch_count() {
                    let batch = reader.read_batch(batch_index, &[])?;
                    row_ids.extend(batch.row_ids);
                    for (target, (_, chunk)) in columns.iter_mut().zip(batch.columns) {
                        target
                            .values
                            .extend((0..chunk.len()).filter_map(|row| chunk.value(row)));
                    }
                }
                let table = MaterializedTable {
                    table_id: snapshot.table_id,
                    schema: snapshot.schema.clone(),
                    row_ids,
                    columns,
                };
                table.validate()?;
                Ok(table)
            }
        }
    }

    /// Visit bounded materialized batches without requiring a snapshot-backed
    /// input to coexist in memory as one complete table.
    #[doc(hidden)]
    pub fn visit_materialized_batches(
        &self,
        mut visitor: impl FnMut(&MaterializedTable) -> Result<()>,
    ) -> Result<()> {
        match self {
            Self::Materialized { table, .. } => visitor(table),
            Self::Snapshot { snapshot, store } => {
                let codecs = CodecRegistry::with_arrow_ipc();
                let reader = SnapshotReader::new(snapshot, store.as_ref(), &codecs)?;
                for batch_index in 0..snapshot.batch_count() {
                    let batch = reader.read_batch(batch_index, &[])?;
                    let columns = snapshot
                        .schema
                        .columns
                        .iter()
                        .cloned()
                        .zip(batch.columns)
                        .map(|(schema, (_, chunk))| MaterializedColumn {
                            schema,
                            values: (0..chunk.len())
                                .filter_map(|row| chunk.value(row))
                                .collect(),
                        })
                        .collect();
                    visitor(&MaterializedTable {
                        table_id: snapshot.table_id,
                        schema: snapshot.schema.clone(),
                        row_ids: batch.row_ids,
                        columns,
                    })?;
                }
                Ok(())
            }
        }
    }

    #[doc(hidden)]
    pub fn materialized_table(&self) -> Option<&MaterializedTable> {
        match self {
            Self::Materialized { table, .. } => Some(table),
            Self::Snapshot { .. } => None,
        }
    }

    #[doc(hidden)]
    pub fn materialized_table_mut(&mut self) -> Option<&mut MaterializedTable> {
        match self {
            Self::Materialized { table, .. } => Some(table),
            Self::Snapshot { .. } => None,
        }
    }

    #[doc(hidden)]
    pub fn snapshot_parts(&self) -> Option<(&TableSnapshot, &Arc<dyn BlockStore>)> {
        match self {
            Self::Snapshot { snapshot, store } => Some((snapshot, store)),
            Self::Materialized { .. } => None,
        }
    }
}

impl fmt::Debug for ExecutionInput {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Materialized {
                table,
                snapshot_fingerprint,
            } => formatter
                .debug_struct("MaterializedExecutionInput")
                .field("table", table)
                .field("snapshot_fingerprint", snapshot_fingerprint)
                .finish(),
            Self::Snapshot { snapshot, store: _ } => formatter
                .debug_struct("SnapshotExecutionInput")
                .field("snapshot", snapshot)
                .field("store", &"content-addressed BlockStore")
                .finish(),
        }
    }
}
