use plotx_data::{
    AggregateFunction, AggregateMeasure, CodecRegistry, ColumnChunk, ColumnRename, ColumnSchema,
    ColumnValues, DirectoryBlockStore, ExecutionInput, ExecutionRequest, Expression,
    JoinCardinality, JoinKey, JoinKind, LogicalType, NullPlacement, RelPlanV1, Relation,
    RevisionId, RowId, SnapshotBuilder, SnapshotRead, SortDirection, SortKey, TableId, TableSchema,
};
use plotx_datafusion::execute_datafusion_to_snapshot;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

const BATCH_ROWS: usize = 65_536;

struct Fixture {
    input: ExecutionInput,
    read: SnapshotRead,
    key: plotx_data::ColumnId,
    value: plotx_data::ColumnId,
    root: PathBuf,
}

fn fixture(rows: usize, namespace: u8) -> Fixture {
    let key = ColumnSchema::new("group", LogicalType::Int64);
    let value = ColumnSchema::new("value", LogicalType::Float64);
    let schema = TableSchema::new(vec![key.clone(), value.clone()]).unwrap();
    let table = TableId::from_bytes([namespace; 16]);
    let revision = RevisionId::from_bytes([namespace.wrapping_add(1); 16]);
    let root = temporary_root(&format!("input-{namespace}"));
    let store = Arc::new(DirectoryBlockStore::open(&root).unwrap());
    let codecs = CodecRegistry::with_arrow_ipc();
    let mut builder = SnapshotBuilder::new(table, schema, store.as_ref(), &codecs)
        .unwrap()
        .with_trusted_row_identity();
    for start in (0..rows).step_by(BATCH_ROWS) {
        let end = (start + BATCH_ROWS).min(rows);
        let ids = (start..end)
            .map(|row| {
                let mut bytes = [namespace; 16];
                bytes[8..].copy_from_slice(&(row as u64).to_le_bytes());
                RowId::from_bytes(bytes)
            })
            .collect::<Vec<_>>();
        builder
            .push_batch(
                &ids,
                &[
                    ColumnChunk::all_valid(ColumnValues::Int64(
                        (start..end).map(|row| (row % 1_000) as i64).collect(),
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
        table,
        revision,
        fingerprint: snapshot.fingerprint,
    };
    Fixture {
        input: ExecutionInput::snapshot(snapshot, store).unwrap(),
        read,
        key: key.id,
        value: value.id,
        root,
    }
}

fn run(
    name: &str,
    plan: RelPlanV1,
    inputs: Vec<Fixture>,
    expected_rows: u64,
    memory_limit_bytes: u64,
) {
    let roots = inputs
        .iter()
        .map(|fixture| fixture.root.clone())
        .collect::<Vec<_>>();
    let catalog = inputs
        .into_iter()
        .map(|fixture| ((fixture.read.table, fixture.read.revision), fixture.input))
        .collect::<BTreeMap<_, _>>();
    let request = ExecutionRequest {
        plan,
        inputs: catalog,
        memory_limit_bytes,
    };
    let output_root = temporary_root("output");
    let store = DirectoryBlockStore::open(&output_root).unwrap();
    let started = Instant::now();
    let output = execute_datafusion_to_snapshot(
        &request,
        TableId::new(),
        &store,
        &CodecRegistry::with_arrow_ipc(),
    )
    .unwrap();
    assert_eq!(output.snapshot.row_count, expected_rows);
    println!(
        "{name}: {:.3}s, process peak resident={} MiB",
        started.elapsed().as_secs_f64(),
        peak_resident_mib()
            .map(|value| format!("{value:.1}"))
            .unwrap_or_else(|| "unavailable".into())
    );
    drop(output);
    drop(request);
    for root in roots.into_iter().chain(std::iter::once(output_root)) {
        std::fs::remove_dir_all(root).unwrap();
    }
}

fn temporary_root(label: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "plotx-table-scale-{label}-{}-{}",
        std::process::id(),
        TableId::new()
    ))
}

#[cfg(windows)]
fn peak_resident_mib() -> Option<f64> {
    use windows_sys::Win32::System::{
        ProcessStatus::{GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS},
        Threading::GetCurrentProcess,
    };

    let mut counters = PROCESS_MEMORY_COUNTERS {
        cb: std::mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32,
        PageFaultCount: 0,
        PeakWorkingSetSize: 0,
        WorkingSetSize: 0,
        QuotaPeakPagedPoolUsage: 0,
        QuotaPagedPoolUsage: 0,
        QuotaPeakNonPagedPoolUsage: 0,
        QuotaNonPagedPoolUsage: 0,
        PagefileUsage: 0,
        PeakPagefileUsage: 0,
    };
    // SAFETY: `counters` is initialized with the exact ABI size and remains
    // valid for the duration of this read-only call on the current process.
    let succeeded = unsafe {
        GetProcessMemoryInfo(GetCurrentProcess(), (&raw mut counters).cast(), counters.cb)
    };
    (succeeded != 0).then_some(counters.PeakWorkingSetSize as f64 / (1024.0 * 1024.0))
}

#[cfg(not(windows))]
fn peak_resident_mib() -> Option<f64> {
    None
}

fn main() {
    let rows = std::env::var("PLOTX_BENCH_ROWS")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(1_000_000_usize);
    let memory_mib = std::env::var("PLOTX_BENCH_MEMORY_MIB")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(256_u64);
    let memory_limit_bytes = memory_mib * 1024 * 1024;
    println!(
        "PlotX typed-table scale benchmark: rows={rows}, memory={memory_mib} MiB, threads={}, os={}",
        std::thread::available_parallelism().map_or(1, usize::from),
        std::env::consts::OS
    );

    let filtered = fixture(rows, 1);
    let filter_read = filtered.read.clone();
    let filter_value = filtered.value;
    run(
        "filter",
        RelPlanV1::new(Relation::Filter {
            input: Box::new(Relation::SnapshotRead(filter_read)),
            predicate: Expression::call("is_finite.v1", vec![Expression::column(filter_value)]),
        }),
        vec![filtered],
        rows as u64,
        memory_limit_bytes,
    );

    let grouped = fixture(rows, 2);
    let group_read = grouped.read.clone();
    let group_key = grouped.key;
    let group_value = grouped.value;
    run(
        "aggregate",
        RelPlanV1::new(Relation::Aggregate {
            input: Box::new(Relation::SnapshotRead(group_read)),
            groups: vec![group_key],
            measures: vec![AggregateMeasure {
                output: ColumnSchema::new("mean", LogicalType::Float64),
                function: AggregateFunction::MeanV1,
                input: Some(Expression::column(group_value)),
            }],
        }),
        vec![grouped],
        1_000.min(rows) as u64,
        memory_limit_bytes,
    );

    let sorted = fixture(rows, 3);
    let sort_read = sorted.read.clone();
    let sort_value = sorted.value;
    run(
        "sort",
        RelPlanV1::new(Relation::StableSort {
            input: Box::new(Relation::SnapshotRead(sort_read)),
            keys: vec![SortKey {
                column: sort_value,
                direction: SortDirection::Descending,
                nulls: NullPlacement::Last,
            }],
        }),
        vec![sorted],
        rows as u64,
        memory_limit_bytes,
    );

    let left = fixture(rows, 4);
    let right = fixture(rows, 5);
    let left_read = left.read.clone();
    let right_read = right.read.clone();
    let right_group = right.key;
    let left_key = left.value;
    let right_key = right.value;
    run(
        "one-to-one join",
        RelPlanV1::new(Relation::Join {
            left: Box::new(Relation::SnapshotRead(left_read)),
            right: Box::new(Relation::Rename {
                input: Box::new(Relation::SnapshotRead(right_read)),
                renames: vec![
                    ColumnRename {
                        column: right_group,
                        name: "right_group".into(),
                    },
                    ColumnRename {
                        column: right_key,
                        name: "right_value".into(),
                    },
                ],
            }),
            kind: JoinKind::Inner,
            keys: vec![JoinKey {
                left: left_key,
                right: right_key,
            }],
            cardinality: JoinCardinality::OneToOne,
        }),
        vec![left, right],
        rows as u64,
        memory_limit_bytes,
    );
}
