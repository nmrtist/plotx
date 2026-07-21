use super::TypedTableState;
use plotx_data::{
    BlockStore, CodecRegistry, DataError, ExecutionRequest, RevisionInput, RevisionOperation,
    RevisionReason, TableEnvelopeV1, TableId, TableRevision,
};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

struct SnapshotExecutionOutput {
    snapshot: plotx_data::TableSnapshot,
    row_ids: Option<Vec<plotx_data::RowId>>,
    diagnostics: Vec<plotx_data::Diagnostic>,
    backend: String,
}

// The reference path is deliberately complete rather than a mock: it preserves
// cancellation, diagnostics, and snapshot encoding for lightweight builds.
#[cfg(not(feature = "datafusion"))]
fn execute_backend(
    request: &plotx_data::ExecutionRequest,
    output_table: TableId,
    store: &dyn BlockStore,
    codecs: &CodecRegistry,
    cancel: &AtomicBool,
) -> plotx_data::Result<SnapshotExecutionOutput> {
    let output = plotx_data::execute_reference(request, cancel)?;
    let snapshot =
        plotx_data::snapshot_from_materialized(&output.table, output_table, store, codecs, 65_536)?;
    Ok(SnapshotExecutionOutput {
        snapshot,
        row_ids: Some(output.table.row_ids),
        diagnostics: output.diagnostics,
        backend: output.backend,
    })
}

#[cfg(feature = "datafusion")]
fn execute_backend(
    request: &plotx_data::ExecutionRequest,
    output_table: TableId,
    store: &dyn BlockStore,
    codecs: &CodecRegistry,
    cancel: &AtomicBool,
) -> plotx_data::Result<SnapshotExecutionOutput> {
    let output = plotx_datafusion::execute_datafusion_to_snapshot_cancellable(
        request,
        output_table,
        store,
        codecs,
        cancel,
    )?;
    Ok(SnapshotExecutionOutput {
        snapshot: output.snapshot,
        row_ids: output.row_ids,
        diagnostics: output.diagnostics,
        backend: output.backend,
    })
}

/// Execute one frozen PlotX IR plan against pinned typed revisions and persist
/// the result as a new derived table. This is the shared headless boundary for
/// future GUI, CLI, and automation transform commands.
pub fn execute_typed_plan(
    plan: plotx_data::RelPlanV1,
    inputs: &[&TypedTableState],
    output_table: TableId,
    memory_limit_bytes: u64,
    understood_extensions: &BTreeSet<String>,
) -> plotx_data::Result<TypedTableState> {
    execute_typed_plan_cancellable(
        plan,
        inputs,
        output_table,
        memory_limit_bytes,
        understood_extensions,
        &AtomicBool::new(false),
    )
}

pub fn execute_typed_plan_cancellable(
    plan: plotx_data::RelPlanV1,
    inputs: &[&TypedTableState],
    output_table: TableId,
    memory_limit_bytes: u64,
    understood_extensions: &BTreeSet<String>,
    cancel: &AtomicBool,
) -> plotx_data::Result<TypedTableState> {
    let mut catalog = BTreeMap::new();
    let mut revision_inputs = Vec::new();
    for input in inputs {
        input.envelope.validate(understood_extensions)?;
        let revision = &input.envelope.revision;
        catalog.insert((revision.table_id, revision.id), input.execution_input()?);
        revision_inputs.push(RevisionInput {
            table: revision.table_id,
            revision: revision.id,
            snapshot_fingerprint: revision.snapshot.fingerprint,
        });
    }
    revision_inputs.sort_by_key(|input| (input.table, input.revision));
    let request = ExecutionRequest {
        plan: plan.clone(),
        inputs: catalog,
        memory_limit_bytes,
    };
    let store = Arc::new(plotx_data::MemoryBlockStore::default());
    let codecs = CodecRegistry::with_arrow_ipc();
    let output = execute_backend(&request, output_table, store.as_ref(), &codecs, cancel)?;
    if cancel.load(Ordering::Relaxed) {
        return Err(DataError::Cancelled);
    }
    let snapshot = output.snapshot;
    let row_mapping = match &output.row_ids {
        Some(row_ids) => {
            plotx_data::derive_row_mapping(&plan, &request.inputs, row_ids, store.as_ref())?
        }
        None => plotx_data::RowMapping::Identity,
    };
    let column_lineage =
        plotx_data::derive_column_lineage(&plan, &request.inputs, &snapshot.schema)?;
    let operation = RevisionOperation {
        id: plan.operation_id,
        name: "rel-plan.v1".into(),
        inputs: revision_inputs,
        plan_fingerprint: plan.fingerprint()?,
        result_fingerprint: snapshot.fingerprint,
        plan: Some(plan),
        software_version: env!("CARGO_PKG_VERSION").into(),
        function_versions: BTreeMap::new(),
        parameters: BTreeMap::from([("backend".into(), serde_json::Value::String(output.backend))]),
        diagnostics: output.diagnostics,
        column_lineage,
        row_mapping: Some(row_mapping),
    };
    let revision = TableRevision {
        id: plotx_data::RevisionId::new(),
        table_id: output_table,
        snapshot,
        parents: Vec::new(),
        operation,
        reason: RevisionReason::Transform,
        created_at_utc: None,
    };
    revision.validate()?;
    Ok(TypedTableState {
        envelope: TableEnvelopeV1::new(revision),
        store,
    })
}

/// Re-run a persisted derived-table plan against explicitly supplied current
/// inputs. The old result remains in history; callers decide whether to adopt
/// the returned state after presenting their own diff/rebase UI.
pub fn refresh_typed_plan(
    derived: &TypedTableState,
    inputs: &[&TypedTableState],
    memory_limit_bytes: u64,
    understood_extensions: &BTreeSet<String>,
) -> plotx_data::Result<TypedTableState> {
    refresh_typed_plan_cancellable(
        derived,
        inputs,
        memory_limit_bytes,
        understood_extensions,
        &AtomicBool::new(false),
    )
}

pub fn refresh_typed_plan_cancellable(
    derived: &TypedTableState,
    inputs: &[&TypedTableState],
    memory_limit_bytes: u64,
    understood_extensions: &BTreeSet<String>,
    cancel: &AtomicBool,
) -> plotx_data::Result<TypedTableState> {
    if derived.envelope.revision.reason == RevisionReason::ManualEdit {
        return refresh_patched_plan(
            derived,
            inputs,
            memory_limit_bytes,
            understood_extensions,
            cancel,
        );
    }
    refresh_unpatched_plan(
        derived,
        inputs,
        memory_limit_bytes,
        understood_extensions,
        cancel,
    )
}

fn refresh_unpatched_plan(
    derived: &TypedTableState,
    inputs: &[&TypedTableState],
    memory_limit_bytes: u64,
    understood_extensions: &BTreeSet<String>,
    cancel: &AtomicBool,
) -> plotx_data::Result<TypedTableState> {
    let mut plan = derived
        .envelope
        .revision
        .operation
        .plan
        .clone()
        .ok_or_else(|| DataError::InvalidPlan("derived table has no refresh plan".into()))?;
    let bindings = inputs
        .iter()
        .map(|input| {
            let revision = &input.envelope.revision;
            (
                revision.table_id,
                (revision.id, revision.snapshot.fingerprint),
            )
        })
        .collect::<BTreeMap<_, _>>();
    rebind_snapshots(&mut plan.root, &bindings)?;
    plan.operation_id = plotx_data::OperationId::new();
    let mut refreshed = execute_typed_plan_cancellable(
        plan,
        inputs,
        derived.envelope.revision.table_id,
        memory_limit_bytes,
        understood_extensions,
        cancel,
    )?;
    for hash in derived.envelope.referenced_blocks() {
        let bytes = derived.store.get(hash)?;
        let copied = refreshed.store.put(bytes)?;
        if copied != hash {
            return Err(DataError::CorruptBlock(
                "copied history block changed its content hash".into(),
            ));
        }
    }
    let mut revision = refreshed.envelope.revision;
    revision.parents = vec![derived.envelope.revision.id];
    revision.reason = RevisionReason::Refresh;
    revision.validate()?;
    let mut envelope = derived.envelope.clone();
    envelope.advance(revision)?;
    refreshed.envelope = envelope;
    Ok(refreshed)
}

fn refresh_patched_plan(
    derived: &TypedTableState,
    inputs: &[&TypedTableState],
    memory_limit_bytes: u64,
    understood_extensions: &BTreeSet<String>,
    cancel: &AtomicBool,
) -> plotx_data::Result<TypedTableState> {
    let (base, patches) = patch_chain(derived)?;
    let refreshed_base = refresh_unpatched_plan(
        &TypedTableState {
            envelope: TableEnvelopeV1::new(base.clone()),
            store: derived.store.clone(),
        },
        inputs,
        memory_limit_bytes,
        understood_extensions,
        cancel,
    )?;
    let codecs = CodecRegistry::with_arrow_ipc();
    let wanted = patches
        .iter()
        .map(|patch| patch.row)
        .collect::<BTreeSet<_>>();
    let mut provenance_rows = BTreeMap::new();
    let reader = plotx_data::SnapshotReader::new(
        &refreshed_base.envelope.revision.snapshot,
        refreshed_base.store.as_ref(),
        &codecs,
    )?;
    for batch in 0..refreshed_base.envelope.revision.snapshot.batch_count() {
        if cancel.load(Ordering::Relaxed) {
            return Err(DataError::Cancelled);
        }
        for row in reader.read_row_ids(batch)?.1 {
            if wanted.contains(&row) {
                provenance_rows.insert(row, row);
            }
        }
    }
    let rebased = plotx_data::rebase_patches_to_snapshot(plotx_data::PatchRebaseRequest {
        patches: &patches,
        provenance_rows: &provenance_rows,
        old_snapshot: &base.snapshot,
        old_store: derived.store.as_ref(),
        new_snapshot: &refreshed_base.envelope.revision.snapshot,
        new_store: refreshed_base.store.as_ref(),
        codecs: &codecs,
        business_key: None,
    })?;
    let refreshed_revision = &refreshed_base.envelope.revision;
    let patch_plan = plotx_data::RelPlanV1::new(plotx_data::Relation::Patch {
        input: Box::new(plotx_data::Relation::SnapshotRead(
            plotx_data::SnapshotRead {
                table: refreshed_revision.table_id,
                revision: refreshed_revision.id,
                fingerprint: refreshed_revision.snapshot.fingerprint,
            },
        )),
        edits: rebased,
    });
    let mut patched = execute_typed_plan_cancellable(
        patch_plan,
        &[&refreshed_base],
        refreshed_revision.table_id,
        memory_limit_bytes,
        understood_extensions,
        cancel,
    )?;
    let mut patch_revision = patched.envelope.revision;
    patch_revision.parents = vec![refreshed_revision.id];
    patch_revision.reason = RevisionReason::Rebase;
    patch_revision.validate()?;
    for hash in refreshed_base.envelope.referenced_blocks() {
        let copied = patched.store.put(refreshed_base.store.get(hash)?)?;
        if copied != hash {
            return Err(DataError::CorruptBlock(
                "copied refreshed block changed its content hash".into(),
            ));
        }
    }
    for hash in derived.envelope.referenced_blocks() {
        let copied = patched.store.put(derived.store.get(hash)?)?;
        if copied != hash {
            return Err(DataError::CorruptBlock(
                "copied patch history block changed its content hash".into(),
            ));
        }
    }
    let mut envelope = derived.envelope.clone();
    envelope.history.push(envelope.revision.clone());
    let known = envelope
        .history
        .iter()
        .map(|revision| revision.id)
        .collect::<BTreeSet<_>>();
    envelope.history.extend(
        refreshed_base
            .envelope
            .history
            .into_iter()
            .filter(|revision| !known.contains(&revision.id)),
    );
    envelope.revision = refreshed_base.envelope.revision;
    envelope.validate_structure()?;
    envelope.advance(patch_revision)?;
    patched.envelope = envelope;
    Ok(patched)
}

fn patch_chain(
    derived: &TypedTableState,
) -> plotx_data::Result<(TableRevision, Vec<plotx_data::CellPatch>)> {
    let revisions = derived
        .envelope
        .history
        .iter()
        .chain(std::iter::once(&derived.envelope.revision))
        .map(|revision| (revision.id, revision))
        .collect::<BTreeMap<_, _>>();
    let mut current = &derived.envelope.revision;
    let mut edits = BTreeMap::new();
    while current.reason == RevisionReason::ManualEdit {
        let Some(plotx_data::RelPlanV1 {
            root: plotx_data::Relation::Patch { edits: patches, .. },
            ..
        }) = &current.operation.plan
        else {
            return Err(DataError::PatchConflict(
                "manual revision does not contain a Patch plan".into(),
            ));
        };
        for patch in patches.iter().rev() {
            edits
                .entry((patch.row, patch.column))
                .or_insert_with(|| patch.value.clone());
        }
        let parent = current.parents.first().ok_or_else(|| {
            DataError::PatchConflict("manual revision has no base revision".into())
        })?;
        current = revisions.get(parent).copied().ok_or_else(|| {
            DataError::PatchConflict("manual revision base is absent from history".into())
        })?;
    }
    if !matches!(
        current.reason,
        RevisionReason::Transform | RevisionReason::Refresh
    ) {
        return Err(DataError::InvalidPlan(
            "patched table has no refreshable derived base".into(),
        ));
    }
    Ok((
        current.clone(),
        edits
            .into_iter()
            .map(|((row, column), value)| plotx_data::CellPatch { row, column, value })
            .collect(),
    ))
}

fn rebind_snapshots(
    relation: &mut plotx_data::Relation,
    bindings: &BTreeMap<TableId, (plotx_data::RevisionId, plotx_data::ContentHash)>,
) -> plotx_data::Result<()> {
    use plotx_data::Relation;
    match relation {
        Relation::SnapshotRead(read) => {
            let (revision, fingerprint) = bindings.get(&read.table).ok_or_else(|| {
                DataError::InvalidPlan(format!("refresh input {} is unavailable", read.table))
            })?;
            read.revision = *revision;
            read.fingerprint = *fingerprint;
        }
        Relation::Project { input, .. }
        | Relation::Rename { input, .. }
        | Relation::ComputedColumn { input, .. }
        | Relation::Filter { input, .. }
        | Relation::StableSort { input, .. }
        | Relation::Aggregate { input, .. }
        | Relation::Pivot { input, .. }
        | Relation::Unpivot { input, .. }
        | Relation::Patch { input, .. }
        | Relation::UnitConvert { input, .. }
        | Relation::MarkMissing { input, .. } => rebind_snapshots(input, bindings)?,
        Relation::Union { inputs } => {
            for input in inputs {
                rebind_snapshots(input, bindings)?;
            }
        }
        Relation::Join { left, right, .. } => {
            rebind_snapshots(left, bindings)?;
            rebind_snapshots(right, bindings)?;
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "table_execution_tests.rs"]
mod tests;
