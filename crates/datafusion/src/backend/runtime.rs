use super::{compile_expression, compile_relation};
use crate::{DataError, Diagnostic, ExecutionRequest, Expression, Relation, Result};
use arrow::{
    array::{Int64Array, UInt64Array},
    record_batch::RecordBatch,
};
use datafusion::{
    dataframe::DataFrame,
    logical_expr::Expr,
    prelude::{SessionContext, lit},
};
use std::{
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};

pub(super) async fn collect_frame(
    frame: DataFrame,
    cancel: &AtomicBool,
) -> Result<Vec<RecordBatch>> {
    let mut collection = tokio::spawn(frame.collect());
    loop {
        tokio::select! {
            result = &mut collection => {
                return result
                    .map_err(|error| DataError::Backend(error.to_string()))?
                    .map_err(|error| DataError::Backend(error.to_string()));
            }
            _ = tokio::time::sleep(Duration::from_millis(10)) => {
                if cancel.load(Ordering::Relaxed) {
                    collection.abort();
                    return Err(DataError::Cancelled);
                }
            }
        }
    }
}

pub(super) fn preserves_row_identity(relation: &Relation) -> bool {
    match relation {
        Relation::SnapshotRead(_) => true,
        Relation::Project { input, .. }
        | Relation::Rename { input, .. }
        | Relation::ComputedColumn { input, .. }
        | Relation::Filter { input, .. }
        | Relation::StableSort { input, .. }
        | Relation::Patch { input, .. }
        | Relation::UnitConvert { input, .. }
        | Relation::MarkMissing { input, .. } => preserves_row_identity(input),
        Relation::Union { inputs } => inputs.iter().all(preserves_row_identity),
        Relation::Aggregate { .. }
        | Relation::Pivot { .. }
        | Relation::Join { .. }
        | Relation::Unpivot { .. } => false,
    }
}

pub(super) fn preserves_exact_row_sequence(relation: &Relation) -> bool {
    match relation {
        Relation::SnapshotRead(_) => true,
        Relation::Project { input, .. }
        | Relation::Rename { input, .. }
        | Relation::ComputedColumn { input, .. }
        | Relation::Patch { input, .. }
        | Relation::UnitConvert { input, .. }
        | Relation::MarkMissing { input, .. } => preserves_exact_row_sequence(input),
        Relation::Filter { .. }
        | Relation::StableSort { .. }
        | Relation::Aggregate { .. }
        | Relation::Pivot { .. }
        | Relation::Unpivot { .. }
        | Relation::Union { .. }
        | Relation::Join { .. } => false,
    }
}

pub(super) fn first_source_table(relation: &Relation) -> Result<crate::TableId> {
    match relation {
        Relation::SnapshotRead(read) => Ok(read.table),
        Relation::Project { input, .. }
        | Relation::Rename { input, .. }
        | Relation::ComputedColumn { input, .. }
        | Relation::Filter { input, .. }
        | Relation::StableSort { input, .. }
        | Relation::Patch { input, .. }
        | Relation::UnitConvert { input, .. }
        | Relation::MarkMissing { input, .. }
        | Relation::Aggregate { input, .. }
        | Relation::Pivot { input, .. }
        | Relation::Unpivot { input, .. } => first_source_table(input),
        Relation::Union { inputs } => inputs
            .first()
            .ok_or_else(|| DataError::InvalidPlan("empty union".into()))
            .and_then(first_source_table),
        Relation::Join { left, .. } => first_source_table(left),
    }
}

pub(super) async fn large_filter_diagnostics(
    context: &SessionContext,
    request: &ExecutionRequest,
    cancel: &AtomicBool,
) -> Result<Vec<Diagnostic>> {
    use datafusion::functions_aggregate::expr_fn::count;

    let mut filters = Vec::new();
    collect_filter_nodes(&request.plan.root, &mut filters);
    let mut diagnostics = Vec::new();
    for (input, predicate) in filters {
        let frame = compile_relation(context, input, &request.inputs, request.plan.operation_id)?;
        let predicate = compile_expression(predicate)?;
        let count_frame = frame
            .filter(predicate.is_null())
            .and_then(|frame| {
                frame.aggregate(
                    Vec::<Expr>::new(),
                    vec![count(lit(1_i64)).alias("__plotx_count")],
                )
            })
            .map_err(|error| DataError::Backend(error.to_string()))?;
        let batches = collect_frame(count_frame, cancel).await?;
        let count = count_result(&batches)?;
        crate::execute::diagnostic_count(
            &mut diagnostics,
            "filter.null_predicate",
            "Filter excluded null predicates",
            count,
        );
    }
    Ok(diagnostics)
}

fn collect_filter_nodes<'a>(
    relation: &'a Relation,
    filters: &mut Vec<(&'a Relation, &'a Expression)>,
) {
    match relation {
        Relation::Filter { input, predicate } => {
            filters.push((input, predicate));
            collect_filter_nodes(input, filters);
        }
        Relation::Project { input, .. }
        | Relation::Rename { input, .. }
        | Relation::ComputedColumn { input, .. }
        | Relation::StableSort { input, .. }
        | Relation::Patch { input, .. }
        | Relation::UnitConvert { input, .. }
        | Relation::MarkMissing { input, .. }
        | Relation::Aggregate { input, .. }
        | Relation::Pivot { input, .. }
        | Relation::Unpivot { input, .. } => collect_filter_nodes(input, filters),
        Relation::Union { inputs } => inputs
            .iter()
            .for_each(|input| collect_filter_nodes(input, filters)),
        Relation::Join { left, right, .. } => {
            collect_filter_nodes(left, filters);
            collect_filter_nodes(right, filters);
        }
        Relation::SnapshotRead(_) => {}
    }
}

fn count_result(batches: &[RecordBatch]) -> Result<u64> {
    let value = batches
        .first()
        .and_then(|batch| batch.column(0).as_any().downcast_ref::<UInt64Array>())
        .map(|array| array.value(0))
        .or_else(|| {
            batches
                .first()
                .and_then(|batch| batch.column(0).as_any().downcast_ref::<Int64Array>())
                .and_then(|array| u64::try_from(array.value(0)).ok())
        });
    value.ok_or_else(|| DataError::Backend("DataFusion diagnostic count is invalid".into()))
}
