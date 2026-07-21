use crate::{
    ColumnId, ColumnSchema, DataError, Diagnostic, ExecutionInput, LogicalType, OperationId,
    RelPlanV1, Relation, Result, RevisionId, RowId, TableId, TableSchema,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering as AtomicOrdering},
        mpsc,
    },
    thread,
};

#[derive(Clone, Debug, PartialEq)]
pub struct MaterializedColumn {
    pub schema: ColumnSchema,
    pub values: Vec<crate::ScalarValue>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct MaterializedTable {
    pub table_id: TableId,
    pub schema: TableSchema,
    pub row_ids: Vec<RowId>,
    pub columns: Vec<MaterializedColumn>,
}

impl MaterializedTable {
    pub fn validate(&self) -> Result<()> {
        self.schema.validate()?;
        if self.columns.len() != self.schema.columns.len()
            || self
                .columns
                .iter()
                .zip(&self.schema.columns)
                .any(|(column, schema)| {
                    column.schema != *schema || column.values.len() != self.row_ids.len()
                })
        {
            return Err(DataError::InvalidArray(
                "materialized columns are not aligned with schema and rows".into(),
            ));
        }
        if self.row_ids.iter().copied().collect::<BTreeSet<_>>().len() != self.row_ids.len() {
            return Err(DataError::InvalidArray("duplicate stable row id".into()));
        }
        for column in &self.columns {
            for value in &column.values {
                validate_scalar(value, &column.schema)?;
            }
        }
        self.validate_business_keys()?;
        Ok(())
    }

    fn validate_business_keys(&self) -> Result<()> {
        for key in &self.schema.business_keys {
            let columns = key
                .columns
                .iter()
                .map(|column| self.column(*column))
                .collect::<Result<Vec<_>>>()?;
            let mut seen = BTreeSet::new();
            for row in 0..self.row_ids.len() {
                let encoded = columns
                    .iter()
                    .map(|column| scalar_key(&column.values[row]))
                    .collect::<Vec<_>>();
                if !seen.insert(encoded) {
                    return Err(DataError::InvalidArray(format!(
                        "business key {:?} is not unique",
                        key.name
                    )));
                }
            }
        }
        Ok(())
    }

    pub fn column(&self, id: ColumnId) -> Result<&MaterializedColumn> {
        self.columns
            .iter()
            .find(|column| column.schema.id == id)
            .ok_or(DataError::MissingColumn(id))
    }

    pub(crate) fn row(&self, id: RowId) -> Option<usize> {
        self.row_ids.iter().position(|row| *row == id)
    }

    pub(crate) fn reorder(&mut self, order: &[usize]) {
        self.row_ids = order.iter().map(|index| self.row_ids[*index]).collect();
        for column in &mut self.columns {
            column.values = order
                .iter()
                .map(|index| column.values[*index].clone())
                .collect();
        }
    }
}

pub(crate) fn scalar_key(value: &crate::ScalarValue) -> Vec<u8> {
    use crate::ScalarValue as S;
    let mut key = Vec::new();
    match value {
        S::Null => key.push(0),
        S::Boolean(value) => key.extend([1, u8::from(*value)]),
        S::Int64(value) => {
            key.push(2);
            key.extend(value.to_le_bytes());
        }
        S::Float64(value) => {
            key.push(3);
            let bits = if value.is_nan() {
                f64::NAN.to_bits()
            } else if *value == 0.0 {
                0
            } else {
                value.to_bits()
            };
            key.extend(bits.to_le_bytes());
        }
        S::Utf8(value) => {
            key.push(4);
            key.extend(value.as_bytes());
        }
        S::Categorical(value) => {
            key.push(5);
            key.extend(value.to_le_bytes());
        }
        S::Date(value) => {
            key.push(6);
            key.extend(value.to_le_bytes());
        }
        S::Time(value) => {
            key.push(7);
            key.extend(value.to_le_bytes());
        }
        S::Timestamp(value) => {
            key.push(8);
            key.extend(value.to_le_bytes());
        }
        S::Duration(value) => {
            key.push(9);
            key.extend(value.to_le_bytes());
        }
        S::Extension { type_id, storage } => {
            key.push(10);
            key.extend((type_id.len() as u64).to_le_bytes());
            key.extend(type_id.as_bytes());
            key.extend(scalar_key(storage));
        }
    }
    key
}

pub(crate) fn validate_scalar(value: &crate::ScalarValue, column: &ColumnSchema) -> Result<()> {
    use crate::ScalarValue as S;
    if matches!(value, S::Null) {
        return column.nullable.then_some(()).ok_or_else(|| {
            DataError::InvalidArray(format!("non-null column {:?} contains null", column.name))
        });
    }
    let compatible = matches!(
        (&column.logical_type, value),
        (LogicalType::Boolean, S::Boolean(_))
            | (LogicalType::Int64, S::Int64(_))
            | (LogicalType::Float64, S::Float64(_))
            | (LogicalType::Utf8, S::Utf8(_))
            | (LogicalType::Categorical { .. }, S::Categorical(_))
            | (LogicalType::Date, S::Date(_))
            | (LogicalType::Time, S::Time(_))
            | (LogicalType::Timestamp { .. }, S::Timestamp(_))
            | (LogicalType::Duration, S::Duration(_))
    ) || matches!(
        (&column.logical_type, value),
        (LogicalType::Extension(extension), S::Extension { type_id, .. })
            if extension.id == *type_id
    );
    if compatible {
        Ok(())
    } else {
        Err(DataError::TypeMismatch {
            expected: column.logical_type.clone(),
            actual: value.logical_type(),
        })
    }
}

#[derive(Clone, Debug)]
pub struct ExecutionRequest {
    pub plan: RelPlanV1,
    pub inputs: BTreeMap<(TableId, RevisionId), ExecutionInput>,
    pub memory_limit_bytes: u64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ExecutionOutput {
    pub table: MaterializedTable,
    pub diagnostics: Vec<Diagnostic>,
    pub backend: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ExecutionProgress {
    pub completed_units: u64,
    pub total_units: Option<u64>,
    pub message: String,
}

pub enum ExecutionEvent {
    Progress(ExecutionProgress),
    Completed(ExecutionOutput),
    Failed(DataError),
}

pub struct ExecutionHandle {
    cancel: Arc<AtomicBool>,
    events: mpsc::Receiver<ExecutionEvent>,
}

impl ExecutionHandle {
    #[doc(hidden)]
    pub fn new(cancel: Arc<AtomicBool>, events: mpsc::Receiver<ExecutionEvent>) -> Self {
        Self { cancel, events }
    }

    pub fn cancel(&self) {
        self.cancel.store(true, AtomicOrdering::Relaxed);
    }

    pub fn recv(&self) -> std::result::Result<ExecutionEvent, mpsc::RecvError> {
        self.events.recv()
    }

    /// Poll without blocking the UI thread. A disconnected channel means the
    /// worker exited without another event and is surfaced distinctly from an
    /// idle worker.
    pub fn try_recv(&self) -> std::result::Result<ExecutionEvent, mpsc::TryRecvError> {
        self.events.try_recv()
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancel.load(AtomicOrdering::Relaxed)
    }
}

pub trait ExecutionService: Send + Sync {
    fn execute(&self, request: ExecutionRequest) -> ExecutionHandle;
}

#[derive(Default)]
pub struct ReferenceExecutionService;

impl ExecutionService for ReferenceExecutionService {
    fn execute(&self, request: ExecutionRequest) -> ExecutionHandle {
        let cancel = Arc::new(AtomicBool::new(false));
        let worker_cancel = cancel.clone();
        let (sender, events) = mpsc::channel();
        thread::spawn(move || {
            let _ = sender.send(ExecutionEvent::Progress(ExecutionProgress {
                completed_units: 0,
                total_units: None,
                message: "validating PlotX relation plan".into(),
            }));
            let event = match execute_reference(&request, &worker_cancel) {
                Ok(output) => ExecutionEvent::Completed(output),
                Err(error) => ExecutionEvent::Failed(error),
            };
            let _ = sender.send(event);
        });
        ExecutionHandle { cancel, events }
    }
}

pub fn execute_reference(
    request: &ExecutionRequest,
    cancel: &AtomicBool,
) -> Result<ExecutionOutput> {
    request.plan.validate()?;
    let schemas: BTreeMap<(TableId, RevisionId), TableSchema> = request
        .inputs
        .iter()
        .map(|(identity, input)| (*identity, input.schema().clone()))
        .collect();
    crate::typecheck_plan(&request.plan, &schemas)?;
    check_cancel(cancel)?;
    let mut diagnostics = Vec::new();
    let table = eval_relation(
        &request.plan.root,
        request.plan.operation_id,
        &request.inputs,
        cancel,
        &mut diagnostics,
    )?;
    table.validate()?;
    let requested = estimate_bytes(&table);
    if requested > request.memory_limit_bytes {
        return Err(DataError::MemoryBudget {
            requested,
            limit: request.memory_limit_bytes,
        });
    }
    Ok(ExecutionOutput {
        table,
        diagnostics,
        backend: "plotx.reference.v1".into(),
    })
}

pub(crate) fn eval_relation(
    relation: &Relation,
    operation: OperationId,
    inputs: &BTreeMap<(TableId, RevisionId), ExecutionInput>,
    cancel: &AtomicBool,
    diagnostics: &mut Vec<Diagnostic>,
) -> Result<MaterializedTable> {
    check_cancel(cancel)?;
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
            input.materialize()
        }
        Relation::Project { input, columns } => {
            let input = eval_relation(input, operation, inputs, cancel, diagnostics)?;
            let selected = columns
                .iter()
                .map(|id| input.column(*id).cloned())
                .collect::<Result<Vec<_>>>()?;
            Ok(MaterializedTable {
                schema: TableSchema::new(
                    selected
                        .iter()
                        .map(|column| column.schema.clone())
                        .collect(),
                )?,
                columns: selected,
                ..input
            })
        }
        Relation::Rename { input, renames } => {
            let mut input = eval_relation(input, operation, inputs, cancel, diagnostics)?;
            for rename in renames {
                input
                    .columns
                    .iter_mut()
                    .find(|column| column.schema.id == rename.column)
                    .ok_or(DataError::MissingColumn(rename.column))?
                    .schema
                    .name
                    .clone_from(&rename.name);
            }
            input.schema.columns = input
                .columns
                .iter()
                .map(|column| column.schema.clone())
                .collect();
            input.schema.validate()?;
            Ok(input)
        }
        Relation::ComputedColumn {
            input,
            column,
            expression,
        } => {
            let mut input = eval_relation(input, operation, inputs, cancel, diagnostics)?;
            let mut values = Vec::with_capacity(input.row_ids.len());
            for row in 0..input.row_ids.len() {
                check_periodic(cancel, row)?;
                values.push(crate::execute_expr::eval_expression(
                    expression, &input, row,
                )?);
            }
            input.schema.columns.push(column.clone());
            input.columns.push(MaterializedColumn {
                schema: column.clone(),
                values,
            });
            Ok(input)
        }
        Relation::Filter { input, predicate } => {
            let mut input = eval_relation(input, operation, inputs, cancel, diagnostics)?;
            let mut keep = Vec::new();
            let mut nulls = 0;
            for row in 0..input.row_ids.len() {
                check_periodic(cancel, row)?;
                match crate::execute_expr::eval_expression(predicate, &input, row)? {
                    crate::ScalarValue::Boolean(true) => keep.push(row),
                    crate::ScalarValue::Boolean(false) => {}
                    crate::ScalarValue::Null => nulls += 1,
                    value => {
                        return Err(crate::execute_expr::type_error(LogicalType::Boolean, value));
                    }
                }
            }
            input.reorder(&keep);
            diagnostic_count(
                diagnostics,
                "filter.null_predicate",
                "Filter excluded null predicates",
                nulls,
            );
            Ok(input)
        }
        Relation::StableSort { input, keys } => {
            let mut input = eval_relation(input, operation, inputs, cancel, diagnostics)?;
            let mut order: Vec<usize> = (0..input.row_ids.len()).collect();
            order.sort_by(|left, right| {
                crate::execute_expr::compare_rows(&input, *left, *right, keys)
                    .unwrap_or(std::cmp::Ordering::Equal)
                    .then_with(|| input.row_ids[*left].cmp(&input.row_ids[*right]))
            });
            input.reorder(&order);
            Ok(input)
        }
        Relation::Patch { input, edits } => {
            let mut input = eval_relation(input, operation, inputs, cancel, diagnostics)?;
            crate::execute_expr::apply_patches(&mut input, edits)?;
            Ok(input)
        }
        Relation::UnitConvert {
            input,
            column,
            source,
            target,
        } => {
            let mut input = eval_relation(input, operation, inputs, cancel, diagnostics)?;
            let data = input
                .columns
                .iter_mut()
                .find(|item| item.schema.id == *column)
                .ok_or(DataError::MissingColumn(*column))?;
            for value in &mut data.values {
                match value {
                    crate::ScalarValue::Float64(value) => {
                        *value = source.convert_value(*value, target)?;
                    }
                    crate::ScalarValue::Null => {}
                    _ => {
                        return Err(DataError::Unsupported(
                            "unit conversion requires Float64".into(),
                        ));
                    }
                }
            }
            data.schema.unit = Some(target.clone());
            input.schema.columns = input
                .columns
                .iter()
                .map(|item| item.schema.clone())
                .collect();
            Ok(input)
        }
        Relation::MarkMissing {
            input,
            columns,
            predicate,
        } => {
            let mut input = eval_relation(input, operation, inputs, cancel, diagnostics)?;
            let marked = (0..input.row_ids.len())
                .map(
                    |row| match crate::execute_expr::eval_expression(predicate, &input, row) {
                        Ok(crate::ScalarValue::Boolean(value)) => Ok(value),
                        Ok(crate::ScalarValue::Null) => Ok(false),
                        Ok(value) => {
                            Err(crate::execute_expr::type_error(LogicalType::Boolean, value))
                        }
                        Err(error) => Err(error),
                    },
                )
                .collect::<Result<Vec<_>>>()?;
            for id in columns {
                let column = input
                    .columns
                    .iter_mut()
                    .find(|column| column.schema.id == *id)
                    .ok_or(DataError::MissingColumn(*id))?;
                for (value, mark) in column.values.iter_mut().zip(&marked) {
                    if *mark {
                        *value = crate::ScalarValue::Null;
                    }
                }
            }
            Ok(input)
        }
        Relation::Union { inputs: relations } => {
            crate::execute_relations::union(relations, operation, inputs, cancel, diagnostics)
        }
        Relation::Join {
            left,
            right,
            kind,
            keys,
            cardinality,
        } => {
            let left = eval_relation(left, operation, inputs, cancel, diagnostics)?;
            let right = eval_relation(right, operation, inputs, cancel, diagnostics)?;
            crate::execute_relations::join(
                left,
                right,
                *kind,
                keys,
                *cardinality,
                operation,
                cancel,
            )
        }
        Relation::Aggregate {
            input,
            groups,
            measures,
        } => {
            let input = eval_relation(input, operation, inputs, cancel, diagnostics)?;
            crate::execute_relations::aggregate(
                input,
                groups,
                measures,
                operation,
                diagnostics,
                cancel,
            )
        }
        Relation::Pivot {
            input,
            groups,
            names_from,
            values_from,
            aggregate,
        } => {
            let input = eval_relation(input, operation, inputs, cancel, diagnostics)?;
            crate::execute_reshape::pivot(
                input,
                groups,
                *names_from,
                *values_from,
                *aggregate,
                operation,
                diagnostics,
            )
        }
        Relation::Unpivot {
            input,
            ids,
            values,
            name_column,
            value_column,
        } => {
            let input = eval_relation(input, operation, inputs, cancel, diagnostics)?;
            crate::execute_reshape::unpivot(
                input,
                ids,
                values,
                name_column,
                value_column,
                operation,
            )
        }
    }
}

pub(crate) fn check_cancel(cancel: &AtomicBool) -> Result<()> {
    if cancel.load(AtomicOrdering::Relaxed) {
        Err(DataError::Cancelled)
    } else {
        Ok(())
    }
}

#[doc(hidden)]
pub fn check_periodic(cancel: &AtomicBool, index: usize) -> Result<()> {
    if index.is_multiple_of(1024) {
        check_cancel(cancel)
    } else {
        Ok(())
    }
}

pub(crate) fn estimate_bytes(table: &MaterializedTable) -> u64 {
    table
        .columns
        .iter()
        .flat_map(|column| &column.values)
        .map(|value| match value {
            crate::ScalarValue::Utf8(value) => value.len() as u64,
            _ => 16,
        })
        .sum::<u64>()
        + table.row_ids.len() as u64 * 16
}

#[doc(hidden)]
pub fn diagnostic_count(diagnostics: &mut Vec<Diagnostic>, code: &str, message: &str, count: u64) {
    if count > 0 {
        diagnostics.push(Diagnostic {
            severity: crate::DiagnosticSeverity::Info,
            code: code.into(),
            message: message.into(),
            counts: BTreeMap::from([("rows".into(), count)]),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        AggregateFunction, AggregateMeasure, BusinessKey, ContentHash, Expression, SnapshotRead,
    };

    fn input() -> (ExecutionInput, RevisionId) {
        let group = ColumnSchema::new("group", LogicalType::Utf8);
        let value = ColumnSchema::new("value", LogicalType::Float64);
        let schema = TableSchema::new(vec![group.clone(), value.clone()]).unwrap();
        let table = MaterializedTable {
            table_id: TableId::new(),
            schema,
            row_ids: vec![RowId::new(), RowId::new(), RowId::new()],
            columns: vec![
                MaterializedColumn {
                    schema: group,
                    values: ["a", "a", "b"]
                        .map(|value| crate::ScalarValue::Utf8(value.into()))
                        .into(),
                },
                MaterializedColumn {
                    schema: value,
                    values: vec![
                        crate::ScalarValue::Float64(1.0),
                        crate::ScalarValue::Null,
                        crate::ScalarValue::Float64(3.0),
                    ],
                },
            ],
        };
        (
            ExecutionInput::materialized(table, ContentHash::of(b"input")),
            RevisionId::new(),
        )
    }

    #[test]
    fn filter_uses_sql_three_value_logic() {
        let (input, revision) = input();
        let table_id = input.materialized_table().unwrap().table_id;
        let value = input.materialized_table().unwrap().schema.columns[1].id;
        let read = Relation::SnapshotRead(SnapshotRead {
            table: table_id,
            revision,
            fingerprint: input.snapshot_fingerprint(),
        });
        let request = ExecutionRequest {
            plan: RelPlanV1::new(Relation::Filter {
                input: Box::new(read),
                predicate: Expression::call("is_finite.v1", vec![Expression::column(value)]),
            }),
            inputs: BTreeMap::from([((table_id, revision), input)]),
            memory_limit_bytes: 1_000_000,
        };
        assert_eq!(
            execute_reference(&request, &AtomicBool::new(false))
                .unwrap()
                .table
                .row_ids
                .len(),
            2
        );
    }

    #[test]
    fn aggregate_ignores_null_and_reports_it() {
        let (input, revision) = input();
        let table_id = input.materialized_table().unwrap().table_id;
        let group = input.materialized_table().unwrap().schema.columns[0].id;
        let value = input.materialized_table().unwrap().schema.columns[1].id;
        let read = Relation::SnapshotRead(SnapshotRead {
            table: table_id,
            revision,
            fingerprint: input.snapshot_fingerprint(),
        });
        let request = ExecutionRequest {
            plan: RelPlanV1::new(Relation::Aggregate {
                input: Box::new(read),
                groups: vec![group],
                measures: vec![AggregateMeasure {
                    output: ColumnSchema::new("mean", LogicalType::Float64),
                    function: AggregateFunction::MeanV1,
                    input: Some(Expression::column(value)),
                }],
            }),
            inputs: BTreeMap::from([((table_id, revision), input)]),
            memory_limit_bytes: 1_000_000,
        };
        let output = execute_reference(&request, &AtomicBool::new(false)).unwrap();
        assert_eq!(output.table.row_ids.len(), 2);
        assert!(!output.diagnostics.is_empty());
    }

    #[test]
    fn materialized_business_keys_are_checked_for_value_uniqueness() {
        let (mut input, _) = input();
        let input = input.materialized_table_mut().unwrap();
        let group = input.schema.columns[0].id;
        input.schema.columns[0].nullable = false;
        input.columns[0].schema.nullable = false;
        input.schema.business_keys.push(BusinessKey {
            name: "sample".into(),
            columns: vec![group],
        });
        let error = input.validate().unwrap_err();
        assert!(error.to_string().contains("not unique"));
    }

    #[test]
    fn execution_handle_can_be_polled_and_cancelled_without_blocking() {
        let (sender, events) = mpsc::channel();
        let cancelled = Arc::new(AtomicBool::new(false));
        let handle = ExecutionHandle::new(cancelled, events);
        assert!(matches!(handle.try_recv(), Err(mpsc::TryRecvError::Empty)));
        sender
            .send(ExecutionEvent::Progress(ExecutionProgress {
                completed_units: 1,
                total_units: Some(2),
                message: "working".into(),
            }))
            .unwrap();
        assert!(matches!(
            handle.try_recv(),
            Ok(ExecutionEvent::Progress(ExecutionProgress {
                completed_units: 1,
                total_units: Some(2),
                ..
            }))
        ));
        handle.cancel();
        assert!(handle.is_cancelled());
    }
}
