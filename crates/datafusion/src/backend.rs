use crate::{
    DataError, Diagnostic, DiagnosticSeverity, ExecutionEvent, ExecutionHandle, ExecutionInput,
    ExecutionOutput, ExecutionProgress, ExecutionRequest, ExecutionService, Expression,
    NullPlacement, Relation, Result, SortDirection,
};
use datafusion::{
    common::ScalarValue as DataFusionScalar,
    dataframe::DataFrame,
    datasource::MemTable,
    execution::{memory_pool::FairSpillPool, runtime_env::RuntimeEnvBuilder},
    logical_expr::expr_fn::when,
    prelude::{SessionConfig, SessionContext, col, lit},
};
use futures::TryStreamExt;
use std::{
    collections::BTreeMap,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread,
};

mod batch;
mod compiler_helpers;
mod identity;
mod relations;
mod result;
mod runtime;
mod snapshot_stream;

use compiler_helpers::{
    checked_schema, column_field, compile_expression, compile_literal, reference_relation,
};

const ROW_ID_FIELD: &str = "__plotx_row_id";
const ROW_POSITION_FIELD: &str = "__plotx_row_position";
const LEFT_ROW_ID_FIELD: &str = "__plotx_left_row_id";
const RIGHT_ROW_ID_FIELD: &str = "__plotx_right_row_id";
const LEFT_ROW_POSITION_FIELD: &str = "__plotx_left_row_position";
const RIGHT_ROW_POSITION_FIELD: &str = "__plotx_right_row_position";
const UNPIVOT_SOURCE_ID_FIELD: &str = "__plotx_unpivot_source_id";
const UNPIVOT_SOURCE_POSITION_FIELD: &str = "__plotx_unpivot_source_position";
const DIFFERENTIAL_INPUT_ROW_LIMIT: usize = 50_000;

/// Whether the current adapter can compile a plan without changing its frozen
/// PlotX semantics. Unsupported nodes use the reference interpreter and are
/// reported explicitly in the execution diagnostics.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DataFusionCapability {
    Equivalent,
    ReferenceFallback { reason: String },
}

pub fn datafusion_capability(plan: &crate::RelPlanV1) -> DataFusionCapability {
    match supported_relation(&plan.root) {
        Ok(()) => DataFusionCapability::Equivalent,
        Err(reason) => DataFusionCapability::ReferenceFallback { reason },
    }
}

/// Production-facing adapter. Small supported plans are checked differentially
/// against the reference interpreter. Large row-preserving plans execute only
/// in DataFusion so validation cannot force a second full materialization.
#[derive(Default)]
pub struct DataFusionExecutionService;

/// A production result written directly to PlotX's chunked storage boundary.
/// Row IDs are retained only when provenance needs an explicit output order.
/// Exact identity plans use `None`, because their mapping is known without
/// retaining one identifier per row.
pub struct SnapshotExecutionOutput {
    pub snapshot: crate::TableSnapshot,
    pub row_ids: Option<Vec<crate::RowId>>,
    pub diagnostics: Vec<Diagnostic>,
    pub backend: String,
}

/// Execute into a content-addressed snapshot. Large plans are consumed as an
/// Arrow batch stream regardless of whether they preserve or derive rows;
/// small plans retain differential checking against the reference interpreter.
pub fn execute_datafusion_to_snapshot(
    request: &ExecutionRequest,
    output_table: crate::TableId,
    store: &dyn crate::BlockStore,
    codecs: &crate::CodecRegistry,
) -> Result<SnapshotExecutionOutput> {
    execute_datafusion_to_snapshot_cancellable(
        request,
        output_table,
        store,
        codecs,
        &AtomicBool::new(false),
    )
}

pub fn execute_datafusion_to_snapshot_cancellable(
    request: &ExecutionRequest,
    output_table: crate::TableId,
    store: &dyn crate::BlockStore,
    codecs: &crate::CodecRegistry,
    cancel: &AtomicBool,
) -> Result<SnapshotExecutionOutput> {
    let row_count = request
        .inputs
        .values()
        .map(ExecutionInput::row_count)
        .sum::<u64>();
    if row_count <= DIFFERENTIAL_INPUT_ROW_LIMIT as u64 {
        let output = run_runtime(request, cancel)?;
        let snapshot =
            crate::snapshot_from_materialized(&output.table, output_table, store, codecs, 65_536)?;
        return Ok(SnapshotExecutionOutput {
            snapshot,
            row_ids: Some(output.table.row_ids),
            diagnostics: output.diagnostics,
            backend: output.backend,
        });
    }
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .map_err(|error| DataError::Backend(error.to_string()))?;
    runtime.block_on(stream_datafusion_snapshot(
        request,
        output_table,
        store,
        codecs,
        cancel,
    ))
}

async fn stream_datafusion_snapshot(
    request: &ExecutionRequest,
    output_table: crate::TableId,
    store: &dyn crate::BlockStore,
    codecs: &crate::CodecRegistry,
    cancel: &AtomicBool,
) -> Result<SnapshotExecutionOutput> {
    request.plan.validate()?;
    let catalog = request
        .inputs
        .iter()
        .map(|(identity, input)| (*identity, input.schema().clone()))
        .collect::<BTreeMap<_, _>>();
    let checked = crate::typecheck_plan(&request.plan, &catalog)?;
    let memory_limit = usize::try_from(request.memory_limit_bytes).unwrap_or(usize::MAX);
    let runtime = RuntimeEnvBuilder::new()
        .with_memory_pool(Arc::new(FairSpillPool::new(memory_limit.max(1))))
        .build()
        .map_err(|error| DataError::Backend(error.to_string()))?;
    let config = production_session_config(&request.plan.root);
    let context = SessionContext::new_with_config_rt(config, Arc::new(runtime));
    let frame = compile_relation(
        &context,
        &request.plan.root,
        &request.inputs,
        request.plan.operation_id,
    )?;
    let mut stream = frame
        .execute_stream()
        .await
        .map_err(|error| DataError::Backend(error.to_string()))?;
    let exact_identity = runtime::preserves_exact_row_sequence(&request.plan.root);
    let mut derived_rows = if exact_identity {
        Vec::new().into_iter()
    } else {
        identity::expected_output(
            &request.plan.root,
            &request.inputs,
            checked.schema.clone(),
            request.plan.operation_id,
            cancel,
        )?
        .row_ids
        .into_iter()
    };
    // DataFusion row identities are either preserved unique input IDs or are
    // deterministically derived from unique source combinations. Avoid a
    // second all-row uniqueness set at this verified adapter boundary.
    let mut builder =
        crate::SnapshotBuilder::new(output_table, checked.schema.clone(), store, codecs)?
            .with_trusted_row_identity();
    let mut row_ids = (!exact_identity).then(Vec::new);
    let mut row_start = 0_usize;
    while let Some(batch) = stream
        .try_next()
        .await
        .map_err(|error| DataError::Backend(error.to_string()))?
    {
        if cancel.load(Ordering::Relaxed) {
            return Err(DataError::Cancelled);
        }
        let (rows, chunks) = batch::record_batch_to_chunks(
            &batch,
            &checked.schema,
            request.plan.operation_id,
            row_start,
            &mut derived_rows,
        )?;
        row_start += rows.len();
        builder.push_batch(&rows, &chunks)?;
        if let Some(output_rows) = &mut row_ids {
            output_rows.extend(rows);
        }
    }
    if derived_rows.next().is_some() {
        return Err(DataError::Backend(
            "DataFusion produced fewer rows than the deterministic identity plan".into(),
        ));
    }
    let snapshot = builder.finish()?;
    let mut diagnostics = runtime::large_filter_diagnostics(&context, request, cancel).await?;
    diagnostics.push(Diagnostic {
        severity: DiagnosticSeverity::Info,
        code: "backend.output.streamed".into(),
        message: "DataFusion output was persisted batch-by-batch without materializing all columns"
            .into(),
        counts: BTreeMap::from([("rows".into(), snapshot.row_count)]),
    });
    Ok(SnapshotExecutionOutput {
        snapshot,
        row_ids,
        diagnostics,
        backend: "plotx.datafusion.v1".into(),
    })
}

impl ExecutionService for DataFusionExecutionService {
    fn execute(&self, request: ExecutionRequest) -> ExecutionHandle {
        let cancel = Arc::new(AtomicBool::new(false));
        let worker_cancel = cancel.clone();
        let (sender, events) = mpsc::channel();
        thread::spawn(move || {
            let _ = sender.send(ExecutionEvent::Progress(ExecutionProgress {
                completed_units: 0,
                total_units: None,
                message: "compiling PlotX IR for DataFusion".into(),
            }));
            let result = match datafusion_capability(&request.plan) {
                DataFusionCapability::Equivalent => run_runtime(&request, &worker_cancel),
                DataFusionCapability::ReferenceFallback { reason } => {
                    crate::execute_reference(&request, &worker_cancel).map(|mut output| {
                        output.backend = "plotx.reference.v1 (DataFusion fallback)".into();
                        output.diagnostics.push(Diagnostic {
                            severity: DiagnosticSeverity::Info,
                            code: "backend.datafusion.unsupported".into(),
                            message: reason,
                            counts: BTreeMap::new(),
                        });
                        output
                    })
                }
            };
            let event = match result {
                Ok(output) => ExecutionEvent::Completed(output),
                Err(error) => ExecutionEvent::Failed(error),
            };
            let _ = sender.send(event);
        });
        ExecutionHandle::new(cancel, events)
    }
}

fn run_runtime(request: &ExecutionRequest, cancel: &AtomicBool) -> Result<ExecutionOutput> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .map_err(|error| DataError::Backend(error.to_string()))?;
    runtime.block_on(execute_datafusion(request, cancel))
}

async fn execute_datafusion(
    request: &ExecutionRequest,
    cancel: &AtomicBool,
) -> Result<ExecutionOutput> {
    request.plan.validate()?;
    if cancel.load(Ordering::Relaxed) {
        return Err(DataError::Cancelled);
    }
    let differential = request
        .inputs
        .values()
        .map(ExecutionInput::row_count)
        .sum::<u64>()
        <= DIFFERENTIAL_INPUT_ROW_LIMIT as u64
        || !identity::supports_large(&request.plan.root);
    let reference = differential
        .then(|| crate::execute_reference(request, cancel))
        .transpose()?;
    let checked = if reference.is_none() {
        let catalog = request
            .inputs
            .iter()
            .map(|(identity, input)| (*identity, input.schema().clone()))
            .collect::<BTreeMap<_, _>>();
        Some(crate::typecheck_plan(&request.plan, &catalog)?)
    } else {
        None
    };
    let memory_limit = usize::try_from(request.memory_limit_bytes).unwrap_or(usize::MAX);
    let runtime = RuntimeEnvBuilder::new()
        .with_memory_pool(Arc::new(FairSpillPool::new(memory_limit.max(1))))
        .build()
        .map_err(|error| DataError::Backend(error.to_string()))?;
    // A single physical partition is the first deterministic merge strategy.
    // Later parallel lowering must retain the frozen fixed-merge-tree result.
    let config = production_session_config(&request.plan.root);
    let context = SessionContext::new_with_config_rt(config, Arc::new(runtime));
    if !differential {
        identity::preflight_large(&request.plan.root, &request.inputs, cancel)?;
    }
    let frame = compile_relation(
        &context,
        &request.plan.root,
        &request.inputs,
        request.plan.operation_id,
    )?;
    let batches = runtime::collect_frame(frame, cancel).await?;
    if cancel.load(Ordering::Relaxed) {
        return Err(DataError::Cancelled);
    }
    let expected = reference.as_ref().map_or_else(
        || {
            identity::expected_output(
                &request.plan.root,
                &request.inputs,
                checked
                    .as_ref()
                    .expect("large execution has a checked schema")
                    .schema
                    .clone(),
                request.plan.operation_id,
                cancel,
            )
        },
        |reference| Ok(reference.table.clone()),
    )?;
    let table = batch::batches_to_table(&batches, &expected, request.plan.operation_id)?;
    if let Some(reference) = &reference
        && !batch::tables_equal(&table, &reference.table)
    {
        return Err(DataError::Backend(
            "DataFusion result differs from the PlotX reference semantics".into(),
        ));
    }
    let mut diagnostics = reference.map_or_else(Vec::new, |output| output.diagnostics);
    if !differential {
        diagnostics.extend(runtime::large_filter_diagnostics(&context, request, cancel).await?);
        diagnostics.extend(identity::aggregate_diagnostics(
            &request.plan.root,
            &request.inputs,
            cancel,
        )?);
        diagnostics.push(Diagnostic {
            severity: DiagnosticSeverity::Info,
            code: "backend.differential.deferred".into(),
            message: "Large row-preserving execution used the golden-tested DataFusion adapter without a duplicate reference materialization".into(),
            counts: BTreeMap::new(),
        });
    }
    Ok(ExecutionOutput {
        table,
        diagnostics,
        backend: "plotx.datafusion.v1".into(),
    })
}

fn production_session_config(relation: &Relation) -> SessionConfig {
    // DataFusion selects SortMergeJoin only for a repartitioned plan. PlotX
    // fixes this at two partitions rather than hardware concurrency so the
    // merge topology and fingerprints remain machine-independent.
    let partitions = if contains_join(relation) { 2 } else { 1 };
    let mut config = SessionConfig::new().with_target_partitions(partitions);
    // HashJoin retains an entire build side and cannot honor PlotX's bounded
    // memory contract. SortMergeJoin uses spill-capable SortExec inputs.
    config.options_mut().optimizer.prefer_hash_join = false;
    if partitions > 1 {
        // Let the fair pool trigger both input sorts to spill without locking a
        // per-sort merge reserve, and cap join output batches explicitly.
        config.options_mut().execution.sort_spill_reservation_bytes = 0;
        config.options_mut().execution.enforce_batch_size_in_joins = true;
    }
    config
}

fn contains_join(relation: &Relation) -> bool {
    match relation {
        Relation::Join { .. } => true,
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
        | Relation::MarkMissing { input, .. } => contains_join(input),
        Relation::Union { inputs } => inputs.iter().any(contains_join),
        Relation::SnapshotRead(_) => false,
    }
}

fn supported_relation(relation: &Relation) -> std::result::Result<(), String> {
    match relation {
        Relation::SnapshotRead(_) => Ok(()),
        Relation::Project { input, .. } | Relation::Rename { input, .. } => {
            supported_relation(input)
        }
        Relation::ComputedColumn {
            input, expression, ..
        } => {
            supported_relation(input)?;
            supported_expression(expression)
        }
        Relation::StableSort { input, .. } => supported_relation(input),
        Relation::UnitConvert { input, .. } => supported_relation(input),
        Relation::Filter { input, predicate } => {
            supported_relation(input)?;
            supported_expression(predicate)
        }
        Relation::Patch { input, .. } => supported_relation(input),
        Relation::MarkMissing {
            input, predicate, ..
        } => {
            supported_relation(input)?;
            supported_expression(predicate)
        }
        Relation::Union { inputs } => {
            for input in inputs {
                supported_relation(input)?;
                source_table(input)?;
            }
            Ok(())
        }
        Relation::Aggregate {
            input, measures, ..
        } => {
            supported_relation(input)?;
            for measure in measures {
                if let Some(expression) = &measure.input {
                    supported_expression(expression)?;
                }
            }
            Ok(())
        }
        Relation::Pivot { input, .. } => supported_relation(input),
        Relation::Join { left, right, .. } => {
            supported_relation(left)?;
            supported_relation(right)
        }
        Relation::Unpivot { input, .. } => supported_relation(input),
    }
}

fn source_table(relation: &Relation) -> std::result::Result<crate::TableId, String> {
    match relation {
        Relation::SnapshotRead(read) => Ok(read.table),
        Relation::Project { input, .. }
        | Relation::Rename { input, .. }
        | Relation::ComputedColumn { input, .. }
        | Relation::Filter { input, .. }
        | Relation::StableSort { input, .. }
        | Relation::Patch { input, .. }
        | Relation::UnitConvert { input, .. }
        | Relation::MarkMissing { input, .. } => source_table(input),
        _ => Err("union input does not have one row-preserving source table".into()),
    }
}

fn supported_expression(expression: &Expression) -> std::result::Result<(), String> {
    match expression {
        Expression::Column { .. } | Expression::Literal { .. } => Ok(()),
        Expression::Call { function, args }
            if matches!(
                function.as_str(),
                "is_null.v1"
                    | "is_finite.v1"
                    | "not.v1"
                    | "and.v1"
                    | "or.v1"
                    | "eq.v1"
                    | "add.v1"
                    | "subtract.v1"
                    | "multiply.v1"
                    | "divide.v1"
            ) =>
        {
            args.iter().try_for_each(supported_expression)
        }
        Expression::Call { function, .. } => {
            Err(format!("function '{function}' has no equivalent adapter"))
        }
        Expression::Cast { .. } => Err("casts require PlotX failure-policy lowering".into()),
    }
}

fn compile_relation(
    context: &SessionContext,
    relation: &Relation,
    inputs: &BTreeMap<(crate::TableId, crate::RevisionId), ExecutionInput>,
    operation: crate::OperationId,
) -> Result<DataFrame> {
    compile_relation_with_namespace(context, relation, inputs, None, operation)
}

#[doc(hidden)]
pub fn compile_for_interop(
    request: &ExecutionRequest,
) -> Result<(
    datafusion::logical_expr::LogicalPlan,
    datafusion::execution::SessionState,
)> {
    request.plan.validate()?;
    let context = SessionContext::new_with_config(SessionConfig::new().with_target_partitions(1));
    let frame = compile_relation(
        &context,
        &request.plan.root,
        &request.inputs,
        request.plan.operation_id,
    )?;
    Ok((frame.logical_plan().clone(), context.state()))
}

fn compile_relation_with_namespace(
    context: &SessionContext,
    relation: &Relation,
    inputs: &BTreeMap<(crate::TableId, crate::RevisionId), ExecutionInput>,
    namespace: Option<crate::TableId>,
    operation: crate::OperationId,
) -> Result<DataFrame> {
    match relation {
        Relation::SnapshotRead(read) => {
            let input = inputs
                .get(&(read.table, read.revision))
                .ok_or_else(|| DataError::InvalidPlan("snapshot input is unavailable".into()))?;
            if input.snapshot_fingerprint() != read.fingerprint {
                return Err(DataError::InvalidPlan(
                    "snapshot fingerprint differs from the pinned plan".into(),
                ));
            }
            let provider: Arc<dyn datafusion::catalog::TableProvider> =
                if input.snapshot_parts().is_some() {
                    snapshot_stream::provider(input, namespace)?
                } else {
                    let table = input.materialize()?;
                    let batch = batch::table_to_batch(&table, namespace)?;
                    Arc::new(
                        MemTable::try_new(batch.schema(), vec![vec![batch]])
                            .map_err(|error| DataError::Backend(error.to_string()))?,
                    )
                };
            context
                .read_table(provider)
                .map_err(|error| DataError::Backend(error.to_string()))
        }
        Relation::StableSort { input, keys } => {
            let frame =
                compile_relation_with_namespace(context, input, inputs, namespace, operation)?;
            let sorts = keys
                .iter()
                .map(|key| {
                    col(column_field(key.column)).sort(
                        key.direction == SortDirection::Ascending,
                        key.nulls == NullPlacement::First,
                    )
                })
                .chain(std::iter::once(col(ROW_ID_FIELD).sort(true, false)))
                .collect::<Vec<_>>();
            frame
                .sort(sorts)
                .map_err(|error| DataError::Backend(error.to_string()))
        }
        Relation::UnitConvert {
            input,
            column,
            source,
            target,
        } => {
            source.convert_value(0.0, target)?;
            let frame =
                compile_relation_with_namespace(context, input, inputs, namespace, operation)?;
            let value = col(column_field(*column));
            let converted = (value * lit(source.scale) + lit(source.offset) - lit(target.offset))
                / lit(target.scale);
            frame
                .with_column(&column_field(*column), converted)
                .map_err(|error| DataError::Backend(error.to_string()))
        }
        Relation::Project { input, columns } => {
            let frame =
                compile_relation_with_namespace(context, input, inputs, namespace, operation)?;
            let expressions = std::iter::once(col(ROW_ID_FIELD))
                .chain(columns.iter().map(|column| col(column_field(*column))))
                .collect::<Vec<_>>();
            frame
                .select(expressions)
                .map_err(|error| DataError::Backend(error.to_string()))
        }
        Relation::Rename { input, .. } => {
            compile_relation_with_namespace(context, input, inputs, namespace, operation)
        }
        Relation::ComputedColumn {
            input,
            column,
            expression,
        } => {
            let frame =
                compile_relation_with_namespace(context, input, inputs, namespace, operation)?;
            frame
                .with_column(&column_field(column.id), compile_expression(expression)?)
                .map_err(|error| DataError::Backend(error.to_string()))
        }
        Relation::Filter { input, predicate } => {
            let frame =
                compile_relation_with_namespace(context, input, inputs, namespace, operation)?;
            frame
                .filter(compile_expression(predicate)?)
                .map_err(|error| DataError::Backend(error.to_string()))
        }
        Relation::Patch { input, edits } => {
            let mut frame =
                compile_relation_with_namespace(context, input, inputs, namespace, operation)?;
            let mut by_column = BTreeMap::new();
            for edit in edits {
                by_column
                    .entry(edit.column)
                    .or_insert_with(Vec::new)
                    .push(edit);
            }
            for (column, edits) in by_column {
                let mut value = col(column_field(column));
                for edit in edits {
                    value = when(
                        col(ROW_ID_FIELD).eq(lit(edit.row.to_string())),
                        compile_literal(&edit.value),
                    )
                    .otherwise(value)
                    .map_err(|error| DataError::Backend(error.to_string()))?;
                }
                frame = frame
                    .with_column(&column_field(column), value)
                    .map_err(|error| DataError::Backend(error.to_string()))?;
            }
            Ok(frame)
        }
        Relation::MarkMissing {
            input,
            columns,
            predicate,
        } => {
            let mut frame =
                compile_relation_with_namespace(context, input, inputs, namespace, operation)?;
            let predicate = compile_expression(predicate)?;
            for column in columns {
                let value = when(predicate.clone(), lit(DataFusionScalar::Null))
                    .otherwise(col(column_field(*column)))
                    .map_err(|error| DataError::Backend(error.to_string()))?;
                frame = frame
                    .with_column(&column_field(*column), value)
                    .map_err(|error| DataError::Backend(error.to_string()))?;
            }
            Ok(frame)
        }
        Relation::Union { inputs: relations } => {
            let mut relations = relations.iter();
            let first = relations
                .next()
                .ok_or_else(|| DataError::InvalidPlan("union has no input".into()))?;
            let mut frame = compile_relation_with_namespace(
                context,
                first,
                inputs,
                Some(source_table(first).map_err(DataError::Unsupported)?),
                operation,
            )?;
            for relation in relations {
                let next = compile_relation_with_namespace(
                    context,
                    relation,
                    inputs,
                    Some(source_table(relation).map_err(DataError::Unsupported)?),
                    operation,
                )?;
                frame = frame
                    .union(next)
                    .map_err(|error| DataError::Backend(error.to_string()))?;
            }
            Ok(frame)
        }
        Relation::Aggregate {
            input,
            groups,
            measures,
        } => {
            let frame =
                compile_relation_with_namespace(context, input, inputs, namespace, operation)?;
            relations::aggregate(frame, groups, measures)
        }
        Relation::Join {
            left,
            right,
            kind,
            keys,
            ..
        } => {
            let left =
                compile_relation_with_namespace(context, left, inputs, namespace, operation)?;
            let right =
                compile_relation_with_namespace(context, right, inputs, namespace, operation)?;
            relations::join(left, right, *kind, keys)
        }
        Relation::Pivot {
            input,
            groups,
            names_from,
            values_from,
            aggregate,
        } => {
            let names = if let Relation::SnapshotRead(read) = input.as_ref() {
                let input = inputs.get(&(read.table, read.revision)).ok_or_else(|| {
                    DataError::InvalidPlan("snapshot input is unavailable".into())
                })?;
                identity::pivot_names(input, *names_from)?
            } else {
                let materialized = reference_relation(input, inputs, operation)?;
                relations::pivot_names(&materialized, *names_from)?
            };
            let input =
                compile_relation_with_namespace(context, input, inputs, namespace, operation)?;
            relations::pivot(
                input,
                groups,
                *names_from,
                *values_from,
                *aggregate,
                operation,
                &names,
            )
        }
        Relation::Unpivot {
            input,
            ids,
            values,
            name_column,
            value_column,
        } => {
            let schema = checked_schema(input, inputs)?;
            let input =
                compile_relation_with_namespace(context, input, inputs, namespace, operation)?;
            relations::unpivot(input, &schema, ids, values, name_column, value_column)
        }
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod large_tests;

#[cfg(test)]
mod outer_join_tests;
