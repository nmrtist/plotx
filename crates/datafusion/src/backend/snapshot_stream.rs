use super::batch;
use crate::{BlockStore, CodecRegistry, DataError, ExecutionInput, Result, TableSnapshot};
use arrow::datatypes::SchemaRef;
use datafusion::{
    catalog::streaming::StreamingTable,
    error::DataFusionError,
    physical_plan::{
        SendableRecordBatchStream, stream::RecordBatchStreamAdapter, streaming::PartitionStream,
    },
};
use futures::stream;
use std::{fmt, sync::Arc};

pub(super) fn provider(
    input: &ExecutionInput,
    namespace: Option<crate::TableId>,
) -> Result<Arc<dyn datafusion::catalog::TableProvider>> {
    let (snapshot, store) = input
        .snapshot_parts()
        .ok_or_else(|| DataError::InvalidPlan("execution input is not snapshot-backed".into()))?;
    let schema = batch::snapshot_schema(&snapshot.schema)?;
    let partition = Arc::new(SnapshotPartition {
        snapshot: snapshot.clone(),
        store: Arc::clone(store),
        schema: Arc::clone(&schema),
        namespace,
    });
    let provider = StreamingTable::try_new(schema, vec![partition])
        .map_err(|error| DataError::Backend(error.to_string()))?;
    Ok(Arc::new(provider))
}

struct SnapshotPartition {
    snapshot: TableSnapshot,
    store: Arc<dyn BlockStore>,
    schema: SchemaRef,
    namespace: Option<crate::TableId>,
}

impl fmt::Debug for SnapshotPartition {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PlotxSnapshotPartition")
            .field("table_id", &self.snapshot.table_id)
            .field("row_count", &self.snapshot.row_count)
            .field("batch_count", &self.snapshot.batch_count())
            .finish()
    }
}

impl PartitionStream for SnapshotPartition {
    fn schema(&self) -> &SchemaRef {
        &self.schema
    }

    fn execute(
        &self,
        _context: Arc<datafusion::execution::TaskContext>,
    ) -> SendableRecordBatchStream {
        let state = SnapshotStreamState {
            snapshot: self.snapshot.clone(),
            store: Arc::clone(&self.store),
            namespace: self.namespace,
            batch_index: 0,
        };
        let batches = stream::unfold(state, |mut state| async move {
            if state.batch_index >= state.snapshot.batch_count() {
                return None;
            }
            let result = read_batch(&state);
            state.batch_index += 1;
            Some((result, state))
        });
        Box::pin(RecordBatchStreamAdapter::new(
            Arc::clone(&self.schema),
            batches,
        ))
    }
}

struct SnapshotStreamState {
    snapshot: TableSnapshot,
    store: Arc<dyn BlockStore>,
    namespace: Option<crate::TableId>,
    batch_index: usize,
}

fn read_batch(
    state: &SnapshotStreamState,
) -> std::result::Result<arrow::record_batch::RecordBatch, DataFusionError> {
    let codecs = CodecRegistry::with_arrow_ipc();
    let reader = crate::SnapshotReader::new(&state.snapshot, state.store.as_ref(), &codecs)
        .map_err(external_error)?;
    let batch = reader
        .read_batch(state.batch_index, &[])
        .map_err(external_error)?;
    batch::snapshot_batch_to_record(batch, &state.snapshot.schema, state.namespace)
        .map_err(external_error)
}

fn external_error(error: DataError) -> DataFusionError {
    DataFusionError::External(Box::new(error))
}
