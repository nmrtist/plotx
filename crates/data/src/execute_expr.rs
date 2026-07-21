use crate::{
    CastFailure, CellPatch, DataError, Expression, LiteralValue, LogicalType, MaterializedTable,
    NullPlacement, Result, ScalarValue, SortDirection, SortKey,
};
use std::cmp::Ordering;

#[doc(hidden)]
pub fn eval_expression(
    expression: &Expression,
    table: &MaterializedTable,
    row: usize,
) -> Result<ScalarValue> {
    match expression {
        Expression::Column { column } => Ok(table.column(*column)?.values[row].clone()),
        Expression::Literal { value } => Ok(literal_scalar(value)),
        Expression::Cast {
            input,
            target,
            failure,
        } => {
            let value = eval_expression(input, table, row)?;
            cast(value, target).or_else(|error| match failure {
                CastFailure::Error => Err(error),
                CastFailure::Null => Ok(ScalarValue::Null),
            })
        }
        Expression::Call { function, args } => {
            let args = args
                .iter()
                .map(|arg| eval_expression(arg, table, row))
                .collect::<Result<Vec<_>>>()?;
            call(function, &args)
        }
    }
}

fn call(function: &str, args: &[ScalarValue]) -> Result<ScalarValue> {
    use ScalarValue as S;
    match (function, args) {
        ("is_null.v1", [value]) => Ok(S::Boolean(matches!(value, S::Null))),
        ("is_finite.v1", [S::Float64(value)]) => Ok(S::Boolean(value.is_finite())),
        ("not.v1", [S::Boolean(value)]) => Ok(S::Boolean(!value)),
        ("not.v1", [S::Null]) => Ok(S::Null),
        ("and.v1", [left, right]) => sql_and(left, right),
        ("or.v1", [left, right]) => sql_or(left, right),
        ("eq.v1", [S::Null, _] | [_, S::Null]) => Ok(S::Null),
        ("eq.v1", [left, right]) => Ok(S::Boolean(compare_scalar(left, right)? == Ordering::Equal)),
        ("add.v1", [left, right]) => numeric_binary(left, right, |a, b| a + b),
        ("subtract.v1", [left, right]) => numeric_binary(left, right, |a, b| a - b),
        ("multiply.v1", [left, right]) => numeric_binary(left, right, |a, b| a * b),
        ("divide.v1", [left, right]) => numeric_binary(left, right, |a, b| a / b),
        (_, values) if values.iter().any(|value| matches!(value, S::Null)) => Ok(S::Null),
        _ => Err(DataError::Unsupported(format!("function {function}"))),
    }
}

fn numeric_binary(
    left: &ScalarValue,
    right: &ScalarValue,
    operation: impl FnOnce(f64, f64) -> f64,
) -> Result<ScalarValue> {
    match (left, right) {
        (ScalarValue::Float64(left), ScalarValue::Float64(right)) => {
            Ok(ScalarValue::Float64(operation(*left, *right)))
        }
        (ScalarValue::Null, _) | (_, ScalarValue::Null) => Ok(ScalarValue::Null),
        _ => Err(DataError::InvalidPlan(
            "numeric function requires Float64".into(),
        )),
    }
}

fn sql_and(left: &ScalarValue, right: &ScalarValue) -> Result<ScalarValue> {
    use ScalarValue::{Boolean as B, Null};
    match (left, right) {
        (B(false), _) | (_, B(false)) => Ok(B(false)),
        (B(true), B(true)) => Ok(B(true)),
        (B(true), Null) | (Null, B(true)) | (Null, Null) => Ok(Null),
        _ => Err(DataError::InvalidPlan("and requires Boolean".into())),
    }
}

fn sql_or(left: &ScalarValue, right: &ScalarValue) -> Result<ScalarValue> {
    use ScalarValue::{Boolean as B, Null};
    match (left, right) {
        (B(true), _) | (_, B(true)) => Ok(B(true)),
        (B(false), B(false)) => Ok(B(false)),
        (B(false), Null) | (Null, B(false)) | (Null, Null) => Ok(Null),
        _ => Err(DataError::InvalidPlan("or requires Boolean".into())),
    }
}

fn cast(value: ScalarValue, target: &LogicalType) -> Result<ScalarValue> {
    use ScalarValue as S;
    if matches!(value, S::Null) {
        return Ok(S::Null);
    }
    match (value, target) {
        (value @ S::Boolean(_), LogicalType::Boolean)
        | (value @ S::Int64(_), LogicalType::Int64)
        | (value @ S::Float64(_), LogicalType::Float64)
        | (value @ S::Utf8(_), LogicalType::Utf8) => Ok(value),
        (S::Int64(value), LogicalType::Float64) => Ok(S::Float64(value as f64)),
        (S::Float64(value), LogicalType::Int64) if value.is_finite() && value.fract() == 0.0 => {
            Ok(S::Int64(value as i64))
        }
        (S::Utf8(value), LogicalType::Float64) => value
            .parse()
            .map(S::Float64)
            .map_err(|_| DataError::InvalidArray("text is not Float64".into())),
        (value, LogicalType::Utf8) => Ok(S::Utf8(format_scalar(&value))),
        (value, expected) => Err(DataError::TypeMismatch {
            expected: expected.clone(),
            actual: value.logical_type(),
        }),
    }
}

pub(crate) fn apply_patches(table: &mut MaterializedTable, edits: &[CellPatch]) -> Result<()> {
    for edit in edits {
        let row = table.row(edit.row).ok_or(DataError::MissingRow(edit.row))?;
        let column = table
            .columns
            .iter_mut()
            .find(|column| column.schema.id == edit.column)
            .ok_or(DataError::MissingColumn(edit.column))?;
        let value = literal_scalar(&edit.value);
        crate::execute::validate_scalar(&value, &column.schema)?;
        column.values[row] = value;
    }
    Ok(())
}

pub(crate) fn compare_rows(
    table: &MaterializedTable,
    left: usize,
    right: usize,
    keys: &[SortKey],
) -> Result<Ordering> {
    for key in keys {
        let column = table.column(key.column)?;
        let mut order = compare_with_nulls(&column.values[left], &column.values[right], key.nulls)?;
        let either_null = matches!(column.values[left], ScalarValue::Null)
            || matches!(column.values[right], ScalarValue::Null);
        // Null placement is an absolute instruction, independent of key
        // direction. Only the non-null value order is reversed for DESC.
        if key.direction == SortDirection::Descending && !either_null {
            order = order.reverse();
        }
        if order != Ordering::Equal {
            return Ok(order);
        }
    }
    Ok(Ordering::Equal)
}

fn compare_with_nulls(
    left: &ScalarValue,
    right: &ScalarValue,
    nulls: NullPlacement,
) -> Result<Ordering> {
    match (left, right) {
        (ScalarValue::Null, ScalarValue::Null) => Ok(Ordering::Equal),
        (ScalarValue::Null, _) => Ok(if nulls == NullPlacement::First {
            Ordering::Less
        } else {
            Ordering::Greater
        }),
        (_, ScalarValue::Null) => Ok(if nulls == NullPlacement::First {
            Ordering::Greater
        } else {
            Ordering::Less
        }),
        _ => compare_scalar(left, right),
    }
}

pub(crate) fn compare_scalar(left: &ScalarValue, right: &ScalarValue) -> Result<Ordering> {
    use ScalarValue as S;
    match (left, right) {
        (S::Boolean(a), S::Boolean(b)) => Ok(a.cmp(b)),
        (S::Int64(a), S::Int64(b)) => Ok(a.cmp(b)),
        (S::Float64(a), S::Float64(b)) => Ok(float_order(*a, *b)),
        (S::Utf8(a), S::Utf8(b)) => Ok(a.cmp(b)),
        (S::Categorical(a), S::Categorical(b)) => Ok(a.cmp(b)),
        (S::Date(a), S::Date(b)) => Ok(a.cmp(b)),
        (S::Time(a), S::Time(b))
        | (S::Timestamp(a), S::Timestamp(b))
        | (S::Duration(a), S::Duration(b)) => Ok(a.cmp(b)),
        _ => Err(DataError::InvalidPlan(
            "comparison operands have different types".into(),
        )),
    }
}

pub(crate) fn float_order(left: f64, right: f64) -> Ordering {
    match (left.is_nan(), right.is_nan()) {
        (true, true) => Ordering::Equal,
        (true, false) => Ordering::Greater,
        (false, true) => Ordering::Less,
        (false, false) => left.total_cmp(&right),
    }
}

pub(crate) fn literal_scalar(value: &LiteralValue) -> ScalarValue {
    match value {
        LiteralValue::Null => ScalarValue::Null,
        LiteralValue::Boolean(value) => ScalarValue::Boolean(*value),
        LiteralValue::Int64(value) => ScalarValue::Int64(*value),
        LiteralValue::Float64(value) => ScalarValue::Float64(value.get()),
        LiteralValue::Utf8(value) => ScalarValue::Utf8(value.clone()),
        LiteralValue::Categorical(value) => ScalarValue::Categorical(*value),
        LiteralValue::Date(value) => ScalarValue::Date(*value),
        LiteralValue::Time(value) => ScalarValue::Time(*value),
        LiteralValue::Timestamp(value) => ScalarValue::Timestamp(*value),
        LiteralValue::Duration(value) => ScalarValue::Duration(*value),
    }
}

pub(crate) fn type_error(expected: LogicalType, actual: ScalarValue) -> DataError {
    DataError::TypeMismatch {
        expected,
        actual: actual.logical_type(),
    }
}

fn format_scalar(value: &ScalarValue) -> String {
    match value {
        ScalarValue::Null => String::new(),
        ScalarValue::Boolean(value) => value.to_string(),
        ScalarValue::Int64(value) => value.to_string(),
        ScalarValue::Float64(value) => value.to_string(),
        ScalarValue::Utf8(value) => value.clone(),
        ScalarValue::Categorical(value) => value.to_string(),
        ScalarValue::Date(value) => value.to_string(),
        ScalarValue::Time(value) | ScalarValue::Timestamp(value) | ScalarValue::Duration(value) => {
            value.to_string()
        }
        ScalarValue::Extension { type_id, .. } => format!("<{type_id}>"),
    }
}
