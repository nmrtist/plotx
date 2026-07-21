use crate::{
    DataError, ExecutionInput, ExecutionRequest, Expression, LiteralValue, MaterializedTable,
    Relation, Result, TableSchema,
};
use datafusion::{
    common::ScalarValue as DataFusionScalar,
    logical_expr::{Expr, expr_fn::not},
    prelude::{col, isnan, lit},
};
use std::{collections::BTreeMap, sync::atomic::AtomicBool};

pub(super) fn reference_relation(
    relation: &Relation,
    inputs: &BTreeMap<(crate::TableId, crate::RevisionId), ExecutionInput>,
    operation: crate::OperationId,
) -> Result<MaterializedTable> {
    let request = ExecutionRequest {
        plan: crate::RelPlanV1 {
            version: 1,
            operation_id: operation,
            root: relation.clone(),
        },
        inputs: inputs.clone(),
        memory_limit_bytes: u64::MAX,
    };
    Ok(crate::execute_reference(&request, &AtomicBool::new(false))?.table)
}

pub(super) fn checked_schema(
    relation: &Relation,
    inputs: &BTreeMap<(crate::TableId, crate::RevisionId), ExecutionInput>,
) -> Result<TableSchema> {
    let catalog = inputs
        .iter()
        .map(|(key, input)| (*key, input.schema().clone()))
        .collect::<BTreeMap<_, _>>();
    Ok(crate::typecheck_plan(&crate::RelPlanV1::new(relation.clone()), &catalog)?.schema)
}

pub(super) fn compile_expression(expression: &Expression) -> Result<Expr> {
    match expression {
        Expression::Column { column } => Ok(col(column_field(*column))),
        Expression::Literal { value } => Ok(compile_literal(value)),
        Expression::Call { function, args } => {
            let mut args = args
                .iter()
                .map(compile_expression)
                .collect::<Result<Vec<_>>>()?;
            match (function.as_str(), args.as_mut_slice()) {
                ("is_null.v1", [value]) => Ok(value.clone().is_null()),
                ("is_finite.v1", [value]) => {
                    let value = value.clone();
                    Ok(not(isnan(value.clone()))
                        .and(value.clone().not_eq(lit(f64::INFINITY)))
                        .and(value.not_eq(lit(f64::NEG_INFINITY))))
                }
                ("not.v1", [value]) => Ok(not(value.clone())),
                ("and.v1", [left, right]) => Ok(left.clone().and(right.clone())),
                ("or.v1", [left, right]) => Ok(left.clone().or(right.clone())),
                ("eq.v1", [left, right]) => Ok(left.clone().eq(right.clone())),
                ("add.v1", [left, right]) => Ok(left.clone() + right.clone()),
                ("subtract.v1", [left, right]) => Ok(left.clone() - right.clone()),
                ("multiply.v1", [left, right]) => Ok(left.clone() * right.clone()),
                ("divide.v1", [left, right]) => Ok(left.clone() / right.clone()),
                _ => Err(DataError::Unsupported(format!("function {function}"))),
            }
        }
        Expression::Cast { .. } => Err(DataError::Unsupported("DataFusion cast policy".into())),
    }
}

pub(super) fn compile_literal(value: &LiteralValue) -> Expr {
    match value {
        LiteralValue::Null => lit(DataFusionScalar::Null),
        LiteralValue::Boolean(value) => lit(*value),
        LiteralValue::Int64(value) => lit(*value),
        LiteralValue::Float64(value) => lit(value.get()),
        LiteralValue::Utf8(value) => lit(value.clone()),
        LiteralValue::Categorical(value) => lit(*value),
        LiteralValue::Date(value) => lit(DataFusionScalar::Date32(Some(*value))),
        LiteralValue::Time(value) => lit(DataFusionScalar::Time64Nanosecond(Some(*value))),
        LiteralValue::Timestamp(value) => {
            lit(DataFusionScalar::TimestampNanosecond(Some(*value), None))
        }
        LiteralValue::Duration(value) => lit(DataFusionScalar::DurationNanosecond(Some(*value))),
    }
}

pub(super) fn column_field(column: crate::ColumnId) -> String {
    format!("c_{}", column.to_string().replace('-', ""))
}
