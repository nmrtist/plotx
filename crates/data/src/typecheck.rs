use crate::{
    AggregateFunction, CastFailure, ColumnId, ColumnSchema, DataError, Expression, JoinKind,
    LiteralValue, LogicalType, OperationId, RelPlanV1, Relation, Result, RevisionId, SnapshotRead,
    TableId, TableSchema,
};
use std::collections::{BTreeMap, BTreeSet};

pub trait SchemaCatalog {
    fn schema(&self, table: TableId, revision: RevisionId) -> Result<TableSchema>;
}

impl SchemaCatalog for BTreeMap<(TableId, RevisionId), TableSchema> {
    fn schema(&self, table: TableId, revision: RevisionId) -> Result<TableSchema> {
        self.get(&(table, revision))
            .cloned()
            .ok_or_else(|| DataError::InvalidPlan("snapshot schema is unavailable".into()))
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct CheckedPlan {
    pub schema: TableSchema,
    /// UTF-8 pivot names are data-dependent. Categorical pivot names are fully
    /// known during checking and leave this false.
    pub dynamic_columns: bool,
}

pub fn typecheck_plan(plan: &RelPlanV1, catalog: &dyn SchemaCatalog) -> Result<CheckedPlan> {
    plan.validate()?;
    check_relation(&plan.root, catalog, plan.operation_id)
}

fn check_relation(
    relation: &Relation,
    catalog: &dyn SchemaCatalog,
    operation: OperationId,
) -> Result<CheckedPlan> {
    match relation {
        Relation::SnapshotRead(SnapshotRead {
            table, revision, ..
        }) => Ok(CheckedPlan {
            schema: catalog.schema(*table, *revision)?,
            dynamic_columns: false,
        }),
        Relation::Project { input, columns } => {
            let checked = check_relation(input, catalog, operation)?;
            let selected = columns
                .iter()
                .map(|id| {
                    checked
                        .schema
                        .column(*id)
                        .cloned()
                        .ok_or(DataError::MissingColumn(*id))
                })
                .collect::<Result<Vec<_>>>()?;
            Ok(CheckedPlan {
                schema: TableSchema::new(selected)?,
                dynamic_columns: checked.dynamic_columns,
            })
        }
        Relation::Rename { input, renames } => {
            let mut checked = check_relation(input, catalog, operation)?;
            for rename in renames {
                checked
                    .schema
                    .columns
                    .iter_mut()
                    .find(|column| column.id == rename.column)
                    .ok_or(DataError::MissingColumn(rename.column))?
                    .name
                    .clone_from(&rename.name);
            }
            checked.schema.validate()?;
            Ok(checked)
        }
        Relation::ComputedColumn {
            input,
            column,
            expression,
        } => {
            let mut checked = check_relation(input, catalog, operation)?;
            if checked.schema.column(column.id).is_some() {
                return Err(DataError::InvalidPlan(
                    "computed column id already exists".into(),
                ));
            }
            let expression_type = expression_type(expression, &checked.schema)?;
            require_assignable(&expression_type, column)?;
            checked.schema.columns.push(column.clone());
            checked.schema.validate()?;
            Ok(checked)
        }
        Relation::Filter { input, predicate } => {
            let checked = check_relation(input, catalog, operation)?;
            require_type(
                &expression_type(predicate, &checked.schema)?,
                &LogicalType::Boolean,
            )?;
            Ok(checked)
        }
        Relation::StableSort { input, keys } => {
            let checked = check_relation(input, catalog, operation)?;
            for key in keys {
                computable(
                    checked
                        .schema
                        .column(key.column)
                        .ok_or(DataError::MissingColumn(key.column))?,
                )?;
            }
            Ok(checked)
        }
        Relation::Aggregate {
            input,
            groups,
            measures,
        } => {
            let checked = check_relation(input, catalog, operation)?;
            let mut columns = groups
                .iter()
                .map(|id| {
                    checked
                        .schema
                        .column(*id)
                        .cloned()
                        .ok_or(DataError::MissingColumn(*id))
                })
                .collect::<Result<Vec<_>>>()?;
            for measure in measures {
                let expected = match measure.function {
                    AggregateFunction::CountAll | AggregateFunction::Count => LogicalType::Int64,
                    _ => LogicalType::Float64,
                };
                if let Some(input) = &measure.input {
                    require_type(
                        &expression_type(input, &checked.schema)?,
                        &LogicalType::Float64,
                    )?;
                }
                if measure.output.logical_type != expected {
                    return Err(DataError::TypeMismatch {
                        expected,
                        actual: measure.output.logical_type.clone(),
                    });
                }
                columns.push(measure.output.clone());
            }
            Ok(CheckedPlan {
                schema: TableSchema::new(columns)?,
                dynamic_columns: false,
            })
        }
        Relation::Pivot {
            input,
            groups,
            names_from,
            values_from,
            aggregate,
        } => check_pivot(
            input,
            groups,
            *names_from,
            *values_from,
            *aggregate,
            catalog,
            operation,
        ),
        Relation::Unpivot {
            input,
            ids,
            values,
            name_column,
            value_column,
        } => {
            let checked = check_relation(input, catalog, operation)?;
            if name_column.logical_type != LogicalType::Utf8 {
                return Err(DataError::TypeMismatch {
                    expected: LogicalType::Utf8,
                    actual: name_column.logical_type.clone(),
                });
            }
            for id in values {
                let source = checked
                    .schema
                    .column(*id)
                    .ok_or(DataError::MissingColumn(*id))?;
                if source.logical_type != value_column.logical_type
                    || source.unit != value_column.unit
                {
                    return Err(DataError::InvalidPlan(
                        "unpivot value columns have incompatible contracts".into(),
                    ));
                }
            }
            let mut columns = ids
                .iter()
                .map(|id| {
                    checked
                        .schema
                        .column(*id)
                        .cloned()
                        .ok_or(DataError::MissingColumn(*id))
                })
                .collect::<Result<Vec<_>>>()?;
            columns.push((**name_column).clone());
            columns.push((**value_column).clone());
            Ok(CheckedPlan {
                schema: TableSchema::new(columns)?,
                dynamic_columns: false,
            })
        }
        Relation::Union { inputs } => {
            let mut checked = inputs
                .iter()
                .map(|input| check_relation(input, catalog, operation));
            let first = checked
                .next()
                .transpose()?
                .ok_or_else(|| DataError::InvalidPlan("empty union".into()))?;
            for input in checked {
                let input = input?;
                if input.schema != first.schema {
                    return Err(DataError::InvalidPlan("union schemas differ".into()));
                }
            }
            Ok(first)
        }
        Relation::Join {
            left,
            right,
            kind,
            keys,
            ..
        } => {
            let left = check_relation(left, catalog, operation)?;
            let right = check_relation(right, catalog, operation)?;
            for key in keys {
                let left_key = left
                    .schema
                    .column(key.left)
                    .ok_or(DataError::MissingColumn(key.left))?;
                let right_key = right
                    .schema
                    .column(key.right)
                    .ok_or(DataError::MissingColumn(key.right))?;
                if left_key.logical_type != right_key.logical_type
                    || left_key.unit != right_key.unit
                {
                    return Err(DataError::InvalidPlan("join key contracts differ".into()));
                }
                computable(left_key)?;
                computable(right_key)?;
            }
            let mut columns = left.schema.columns;
            let left_ids: BTreeSet<_> = columns.iter().map(|column| column.id).collect();
            if right
                .schema
                .columns
                .iter()
                .any(|column| left_ids.contains(&column.id))
            {
                return Err(DataError::InvalidPlan("join column ids collide".into()));
            }
            if *kind == JoinKind::Full {
                columns.iter_mut().for_each(|column| column.nullable = true);
            }
            columns.extend(right.schema.columns.into_iter().map(|mut column| {
                column.nullable |= matches!(kind, JoinKind::Left | JoinKind::Full);
                column
            }));
            Ok(CheckedPlan {
                schema: TableSchema::new(columns)?,
                dynamic_columns: left.dynamic_columns || right.dynamic_columns,
            })
        }
        Relation::Patch { input, edits } => {
            let checked = check_relation(input, catalog, operation)?;
            for edit in edits {
                let column = checked
                    .schema
                    .column(edit.column)
                    .ok_or(DataError::MissingColumn(edit.column))?;
                require_assignable(&literal_type(&edit.value), column)?;
            }
            Ok(checked)
        }
        Relation::UnitConvert {
            input,
            column,
            source,
            target,
        } => {
            let mut checked = check_relation(input, catalog, operation)?;
            let converted = checked
                .schema
                .columns
                .iter_mut()
                .find(|item| item.id == *column)
                .ok_or(DataError::MissingColumn(*column))?;
            if converted.logical_type != LogicalType::Float64
                || converted.unit.as_ref() != Some(source)
            {
                return Err(DataError::InvalidPlan(
                    "unit conversion source does not match column".into(),
                ));
            }
            if !source.is_compatible_with(target) {
                return Err(DataError::IncompatibleUnits {
                    left: source.display_unit.clone(),
                    right: target.display_unit.clone(),
                });
            }
            converted.unit = Some(target.clone());
            Ok(checked)
        }
        Relation::MarkMissing {
            input,
            columns,
            predicate,
        } => {
            let checked = check_relation(input, catalog, operation)?;
            require_type(
                &expression_type(predicate, &checked.schema)?,
                &LogicalType::Boolean,
            )?;
            for id in columns {
                if !checked
                    .schema
                    .column(*id)
                    .ok_or(DataError::MissingColumn(*id))?
                    .nullable
                {
                    return Err(DataError::InvalidPlan(
                        "missing marking targets a non-null column".into(),
                    ));
                }
            }
            Ok(checked)
        }
    }
}

fn check_pivot(
    input: &Relation,
    groups: &[ColumnId],
    names_from: ColumnId,
    values_from: ColumnId,
    aggregate: AggregateFunction,
    catalog: &dyn SchemaCatalog,
    operation: OperationId,
) -> Result<CheckedPlan> {
    let checked = check_relation(input, catalog, operation)?;
    let names = checked
        .schema
        .column(names_from)
        .ok_or(DataError::MissingColumn(names_from))?;
    let values = checked
        .schema
        .column(values_from)
        .ok_or(DataError::MissingColumn(values_from))?;
    if values.logical_type != LogicalType::Float64 {
        return Err(DataError::TypeMismatch {
            expected: LogicalType::Float64,
            actual: values.logical_type.clone(),
        });
    }
    let mut columns = groups
        .iter()
        .map(|id| {
            checked
                .schema
                .column(*id)
                .cloned()
                .ok_or(DataError::MissingColumn(*id))
        })
        .collect::<Result<Vec<_>>>()?;
    let dynamic_columns = match &names.logical_type {
        LogicalType::Categorical { levels } => {
            for level in levels {
                let mut column = ColumnSchema::new(&level.value, pivot_aggregate_type(aggregate));
                column.id = ColumnId::derived(operation, level.value.as_bytes());
                if !matches!(
                    aggregate,
                    AggregateFunction::CountAll | AggregateFunction::Count
                ) {
                    column.unit = values.unit.clone();
                }
                columns.push(column);
            }
            false
        }
        LogicalType::Utf8 => true,
        actual => {
            return Err(DataError::TypeMismatch {
                expected: LogicalType::Utf8,
                actual: actual.clone(),
            });
        }
    };
    Ok(CheckedPlan {
        schema: TableSchema::new(columns)?,
        dynamic_columns,
    })
}

fn pivot_aggregate_type(function: AggregateFunction) -> LogicalType {
    if matches!(
        function,
        AggregateFunction::CountAll | AggregateFunction::Count
    ) {
        LogicalType::Int64
    } else {
        LogicalType::Float64
    }
}

#[derive(Clone, Debug)]
struct ExpressionType {
    logical: Option<LogicalType>,
    nullable: bool,
}

fn expression_type(expression: &Expression, schema: &TableSchema) -> Result<ExpressionType> {
    match expression {
        Expression::Column { column } => {
            let column = schema
                .column(*column)
                .ok_or(DataError::MissingColumn(*column))?;
            computable(column)?;
            Ok(ExpressionType {
                logical: Some(column.logical_type.clone()),
                nullable: column.nullable,
            })
        }
        Expression::Literal { value } => Ok(literal_type(value)),
        Expression::Cast {
            input,
            target,
            failure,
        } => {
            expression_type(input, schema)?;
            Ok(ExpressionType {
                logical: Some(target.clone()),
                nullable: *failure == CastFailure::Null,
            })
        }
        Expression::Call { function, args } => {
            let args = args
                .iter()
                .map(|arg| expression_type(arg, schema))
                .collect::<Result<Vec<_>>>()?;
            let inputs_nullable = args.iter().any(|arg| arg.nullable);
            let (logical, nullable) = match function.as_str() {
                "is_null.v1" => {
                    require_arity(function, &args, 1)?;
                    (LogicalType::Boolean, false)
                }
                "is_finite.v1" => {
                    require_arity(function, &args, 1)?;
                    require_type(&args[0], &LogicalType::Float64)?;
                    (LogicalType::Boolean, inputs_nullable)
                }
                "not.v1" | "and.v1" | "or.v1" => {
                    let arity = if function == "not.v1" { 1 } else { 2 };
                    require_arity(function, &args, arity)?;
                    for arg in &args {
                        require_type(arg, &LogicalType::Boolean)?;
                    }
                    (LogicalType::Boolean, inputs_nullable)
                }
                "eq.v1" => {
                    require_arity(function, &args, 2)?;
                    if let (Some(left), Some(right)) = (&args[0].logical, &args[1].logical)
                        && !logical_types_compatible(left, right)
                    {
                        return Err(DataError::InvalidPlan(
                            "eq.v1 operands have different types".into(),
                        ));
                    }
                    (LogicalType::Boolean, inputs_nullable)
                }
                "add.v1" | "subtract.v1" | "multiply.v1" | "divide.v1" => {
                    require_arity(function, &args, 2)?;
                    for arg in &args {
                        require_type(arg, &LogicalType::Float64)?;
                    }
                    (LogicalType::Float64, inputs_nullable)
                }
                _ => return Err(DataError::Unsupported(format!("function {function}"))),
            };
            Ok(ExpressionType {
                logical: Some(logical),
                nullable,
            })
        }
    }
}

fn require_arity(function: &str, args: &[ExpressionType], expected: usize) -> Result<()> {
    if args.len() == expected {
        Ok(())
    } else {
        Err(DataError::InvalidPlan(format!(
            "{function} expects {expected} argument(s), found {}",
            args.len()
        )))
    }
}

fn literal_type(value: &LiteralValue) -> ExpressionType {
    let logical = match value {
        LiteralValue::Null => None,
        LiteralValue::Boolean(_) => Some(LogicalType::Boolean),
        LiteralValue::Int64(_) => Some(LogicalType::Int64),
        LiteralValue::Float64(_) => Some(LogicalType::Float64),
        LiteralValue::Utf8(_) => Some(LogicalType::Utf8),
        LiteralValue::Categorical(_) => Some(LogicalType::Categorical { levels: Vec::new() }),
        LiteralValue::Date(_) => Some(LogicalType::Date),
        LiteralValue::Time(_) => Some(LogicalType::Time),
        LiteralValue::Timestamp(_) => Some(LogicalType::Timestamp {
            display_timezone: "UTC".into(),
        }),
        LiteralValue::Duration(_) => Some(LogicalType::Duration),
    };
    ExpressionType {
        logical,
        nullable: matches!(value, LiteralValue::Null),
    }
}

fn require_type(actual: &ExpressionType, expected: &LogicalType) -> Result<()> {
    if actual
        .logical
        .as_ref()
        .is_none_or(|actual| logical_types_compatible(actual, expected))
    {
        Ok(())
    } else {
        Err(DataError::TypeMismatch {
            expected: expected.clone(),
            actual: actual.logical.clone().expect("checked as present"),
        })
    }
}

fn logical_types_compatible(actual: &LogicalType, expected: &LogicalType) -> bool {
    actual == expected
        || matches!(
            (actual, expected),
            (
                LogicalType::Categorical { .. },
                LogicalType::Categorical { .. }
            ) | (LogicalType::Timestamp { .. }, LogicalType::Timestamp { .. })
        )
}

fn require_assignable(actual: &ExpressionType, column: &ColumnSchema) -> Result<()> {
    require_type(actual, &column.logical_type)?;
    if actual.nullable && !column.nullable {
        return Err(DataError::InvalidPlan(format!(
            "nullable expression assigned to non-null column {:?}",
            column.name
        )));
    }
    Ok(())
}

fn computable(column: &ColumnSchema) -> Result<()> {
    if matches!(&column.logical_type, LogicalType::Extension(extension) if extension.semantics_critical)
    {
        Err(DataError::Unsupported(format!(
            "column {:?} uses an unregistered semantic extension",
            column.name
        )))
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ContentHash, OperationId};

    #[test]
    fn filter_predicates_are_statically_boolean() {
        let column = ColumnSchema::new("value", LogicalType::Float64);
        let table = TableId::new();
        let revision = RevisionId::new();
        let read = Relation::SnapshotRead(SnapshotRead {
            table,
            revision,
            fingerprint: ContentHash::of(b"x"),
        });
        let plan = RelPlanV1 {
            version: 1,
            operation_id: OperationId::new(),
            root: Relation::Filter {
                input: Box::new(read),
                predicate: Expression::column(column.id),
            },
        };
        let catalog =
            BTreeMap::from([((table, revision), TableSchema::new(vec![column]).unwrap())]);
        assert!(typecheck_plan(&plan, &catalog).is_err());
    }
}
