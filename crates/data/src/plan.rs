use crate::{
    ColumnId, ColumnSchema, ContentHash, DataError, LogicalType, OperationId, Result, RevisionId,
    RowId, TableId, UnitSpec,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RelPlanV1 {
    pub version: u32,
    pub operation_id: OperationId,
    pub root: Relation,
}

impl RelPlanV1 {
    pub fn new(root: Relation) -> Self {
        Self {
            version: 1,
            operation_id: OperationId::new(),
            root,
        }
    }

    pub fn validate(&self) -> Result<()> {
        if self.version != 1 {
            return Err(DataError::InvalidPlan(format!(
                "unsupported PlotX relation plan version {}",
                self.version
            )));
        }
        validate_relation(&self.root)
    }

    pub fn fingerprint(&self) -> Result<ContentHash> {
        self.validate()?;
        let bytes =
            serde_json::to_vec(self).map_err(|error| DataError::Backend(error.to_string()))?;
        Ok(ContentHash::of(&bytes))
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Relation {
    SnapshotRead(SnapshotRead),
    Project {
        input: Box<Relation>,
        columns: Vec<ColumnId>,
    },
    Rename {
        input: Box<Relation>,
        renames: Vec<ColumnRename>,
    },
    ComputedColumn {
        input: Box<Relation>,
        column: ColumnSchema,
        expression: Expression,
    },
    Filter {
        input: Box<Relation>,
        predicate: Expression,
    },
    StableSort {
        input: Box<Relation>,
        keys: Vec<SortKey>,
    },
    Aggregate {
        input: Box<Relation>,
        groups: Vec<ColumnId>,
        measures: Vec<AggregateMeasure>,
    },
    Pivot {
        input: Box<Relation>,
        groups: Vec<ColumnId>,
        names_from: ColumnId,
        values_from: ColumnId,
        aggregate: AggregateFunction,
    },
    Unpivot {
        input: Box<Relation>,
        ids: Vec<ColumnId>,
        values: Vec<ColumnId>,
        name_column: Box<ColumnSchema>,
        value_column: Box<ColumnSchema>,
    },
    Union {
        inputs: Vec<Relation>,
    },
    Join {
        left: Box<Relation>,
        right: Box<Relation>,
        kind: JoinKind,
        keys: Vec<JoinKey>,
        cardinality: JoinCardinality,
    },
    Patch {
        input: Box<Relation>,
        edits: Vec<CellPatch>,
    },
    UnitConvert {
        input: Box<Relation>,
        column: ColumnId,
        source: UnitSpec,
        target: UnitSpec,
    },
    MarkMissing {
        input: Box<Relation>,
        columns: Vec<ColumnId>,
        predicate: Expression,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SnapshotRead {
    pub table: TableId,
    pub revision: RevisionId,
    pub fingerprint: ContentHash,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ColumnRename {
    pub column: ColumnId,
    pub name: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "expr", rename_all = "snake_case")]
pub enum Expression {
    Column {
        column: ColumnId,
    },
    Literal {
        value: LiteralValue,
    },
    Call {
        function: String,
        args: Vec<Expression>,
    },
    Cast {
        input: Box<Expression>,
        target: LogicalType,
        failure: CastFailure,
    },
}

impl Expression {
    pub fn column(column: ColumnId) -> Self {
        Self::Column { column }
    }

    pub fn call(function: impl Into<String>, args: Vec<Self>) -> Self {
        Self::Call {
            function: function.into(),
            args,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
pub enum LiteralValue {
    Null,
    Boolean(bool),
    Int64(i64),
    Float64(FiniteOrSpecial),
    Utf8(String),
    Categorical(u32),
    Date(i32),
    Time(i64),
    Timestamp(i64),
    Duration(i64),
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum FiniteOrSpecial {
    Finite(f64),
    Nan,
    PositiveInfinity,
    NegativeInfinity,
}

impl FiniteOrSpecial {
    pub fn new(value: f64) -> Self {
        if value.is_nan() {
            Self::Nan
        } else if value == f64::INFINITY {
            Self::PositiveInfinity
        } else if value == f64::NEG_INFINITY {
            Self::NegativeInfinity
        } else {
            Self::Finite(value)
        }
    }

    pub fn get(self) -> f64 {
        match self {
            Self::Finite(value) => value,
            Self::Nan => f64::NAN,
            Self::PositiveInfinity => f64::INFINITY,
            Self::NegativeInfinity => f64::NEG_INFINITY,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CastFailure {
    Error,
    Null,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SortKey {
    pub column: ColumnId,
    pub direction: SortDirection,
    /// Null is last by default. Non-null floats always use the frozen total
    /// order `-Inf < finite < +Inf < NaN`.
    pub nulls: NullPlacement,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SortDirection {
    Ascending,
    Descending,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NullPlacement {
    First,
    Last,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AggregateMeasure {
    pub output: ColumnSchema,
    pub function: AggregateFunction,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input: Option<Expression>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AggregateFunction {
    CountAll,
    Count,
    SumV1,
    MeanV1,
    MinimumV1,
    MaximumV1,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct JoinKey {
    pub left: ColumnId,
    pub right: ColumnId,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JoinKind {
    Inner,
    Left,
    Full,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JoinCardinality {
    OneToOne,
    OneToMany,
    ManyToOne,
    ManyToMany,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CellPatch {
    pub row: RowId,
    pub column: ColumnId,
    pub value: LiteralValue,
}

fn validate_relation(relation: &Relation) -> Result<()> {
    match relation {
        Relation::SnapshotRead(_) => Ok(()),
        Relation::Project { input, columns } => {
            require_unique_nonempty(columns, "project columns")?;
            validate_relation(input)
        }
        Relation::Rename { input, renames } => {
            if renames.iter().any(|rename| rename.name.trim().is_empty()) {
                return Err(DataError::InvalidPlan("rename target is empty".into()));
            }
            validate_relation(input)
        }
        Relation::ComputedColumn {
            input, expression, ..
        } => {
            validate_expression(expression)?;
            validate_relation(input)
        }
        Relation::Filter { input, predicate }
        | Relation::MarkMissing {
            input, predicate, ..
        } => {
            validate_expression(predicate)?;
            validate_relation(input)
        }
        Relation::StableSort { input, keys } => {
            if keys.is_empty() {
                return Err(DataError::InvalidPlan("stable sort has no key".into()));
            }
            validate_relation(input)
        }
        Relation::Aggregate {
            input, measures, ..
        } => {
            if measures.is_empty() {
                return Err(DataError::InvalidPlan("aggregate has no measure".into()));
            }
            for measure in measures {
                match (measure.function, &measure.input) {
                    (AggregateFunction::CountAll, None) => {}
                    (AggregateFunction::CountAll, Some(_)) | (_, None) => {
                        return Err(DataError::InvalidPlan(
                            "count(*) alone omits an input expression".into(),
                        ));
                    }
                    (_, Some(expression)) => validate_expression(expression)?,
                }
            }
            validate_relation(input)
        }
        Relation::Pivot { input, .. } | Relation::Unpivot { input, .. } => validate_relation(input),
        Relation::Union { inputs } => {
            if inputs.is_empty() {
                return Err(DataError::InvalidPlan("union has no input".into()));
            }
            inputs.iter().try_for_each(validate_relation)
        }
        Relation::Join {
            left, right, keys, ..
        } => {
            if keys.is_empty() {
                return Err(DataError::InvalidPlan(
                    "cross joins and keyless joins are not part of v1".into(),
                ));
            }
            validate_relation(left)?;
            validate_relation(right)
        }
        Relation::Patch { input, edits } => {
            let mut cells = BTreeSet::new();
            if edits
                .iter()
                .any(|edit| !cells.insert((edit.row, edit.column)))
            {
                return Err(DataError::InvalidPlan(
                    "a patch addresses the same cell more than once".into(),
                ));
            }
            validate_relation(input)
        }
        Relation::UnitConvert {
            input,
            source,
            target,
            ..
        } => {
            source.validate()?;
            target.validate()?;
            if !source.is_compatible_with(target) {
                return Err(DataError::IncompatibleUnits {
                    left: source.display_unit.clone(),
                    right: target.display_unit.clone(),
                });
            }
            validate_relation(input)
        }
    }
}

fn require_unique_nonempty(values: &[ColumnId], label: &str) -> Result<()> {
    let unique: BTreeSet<_> = values.iter().collect();
    if values.is_empty() || unique.len() != values.len() {
        return Err(DataError::InvalidPlan(format!(
            "{label} must be non-empty and unique"
        )));
    }
    Ok(())
}

fn validate_expression(expression: &Expression) -> Result<()> {
    match expression {
        Expression::Call { function, args } => {
            if function.trim().is_empty()
                || matches!(
                    function.as_str(),
                    "now" | "current_time" | "random" | "uuid" | "clock"
                )
            {
                return Err(DataError::InvalidPlan(format!(
                    "non-deterministic or unversioned function {function:?}"
                )));
            }
            args.iter().try_for_each(validate_expression)
        }
        Expression::Cast { input, .. } => validate_expression(input),
        Expression::Literal {
            value: LiteralValue::Float64(FiniteOrSpecial::Finite(value)),
        } if !value.is_finite() => Err(DataError::InvalidPlan(
            "non-finite literals must use their explicit representation".into(),
        )),
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plan_serialization_preserves_special_floats() {
        let plan = RelPlanV1::new(Relation::Filter {
            input: Box::new(Relation::SnapshotRead(SnapshotRead {
                table: TableId::new(),
                revision: RevisionId::new(),
                fingerprint: ContentHash::of(b"snapshot"),
            })),
            predicate: Expression::call(
                "eq.v1",
                vec![Expression::Literal {
                    value: LiteralValue::Float64(FiniteOrSpecial::Nan),
                }],
            ),
        });
        let json = serde_json::to_string(&plan).unwrap();
        assert!(json.contains("nan"));
        assert_eq!(serde_json::from_str::<RelPlanV1>(&json).unwrap(), plan);
    }

    #[test]
    fn plans_reject_nondeterministic_functions() {
        let expression = Expression::call("random", Vec::new());
        assert!(validate_expression(&expression).is_err());
    }
}
