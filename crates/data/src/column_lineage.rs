use crate::{
    ColumnId, ColumnLineage, ContentHash, DataError, ExecutionInput, Expression, RelPlanV1,
    Relation, Result, RevisionId, TableId, TableSchema,
};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone)]
struct Origin {
    inputs: BTreeSet<ColumnId>,
    expression_fingerprint: ContentHash,
}

type Catalog = BTreeMap<(TableId, RevisionId), ExecutionInput>;

/// Resolve every result column to stable source columns and the exact frozen
/// expression/operation contract that produced it.
pub fn derive_column_lineage(
    plan: &RelPlanV1,
    inputs: &Catalog,
    output_schema: &TableSchema,
) -> Result<Vec<ColumnLineage>> {
    let mut origins = relation_origins(&plan.root, inputs)?;
    complete_pivot_origins(
        plan.operation_id,
        &plan.root,
        inputs,
        &mut origins,
        output_schema,
    )?;
    output_schema
        .columns
        .iter()
        .map(|column| {
            let origin = origins.get(&column.id).ok_or_else(|| {
                DataError::InvalidPlan(format!("result column {} has no frozen lineage", column.id))
            })?;
            Ok(ColumnLineage {
                output: column.id,
                inputs: origin.inputs.iter().copied().collect(),
                expression_fingerprint: origin.expression_fingerprint,
            })
        })
        .collect()
}

fn relation_origins(relation: &Relation, catalog: &Catalog) -> Result<BTreeMap<ColumnId, Origin>> {
    match relation {
        Relation::SnapshotRead(read) => {
            let input = catalog
                .get(&(read.table, read.revision))
                .ok_or_else(|| DataError::InvalidPlan("lineage input is unavailable".into()))?;
            if input.snapshot_fingerprint() != read.fingerprint {
                return Err(DataError::InvalidPlan(
                    "lineage input fingerprint differs from the plan".into(),
                ));
            }
            Ok(input
                .schema()
                .columns
                .iter()
                .map(|column| (column.id, identity(column.id)))
                .collect())
        }
        Relation::Project { input, columns } => {
            let mut origins = relation_origins(input, catalog)?;
            origins.retain(|column, _| columns.contains(column));
            Ok(origins)
        }
        Relation::Rename { input, .. }
        | Relation::Filter { input, .. }
        | Relation::StableSort { input, .. }
        | Relation::Patch { input, .. }
        | Relation::MarkMissing { input, .. } => relation_origins(input, catalog),
        Relation::ComputedColumn {
            input,
            column,
            expression,
        } => {
            let mut origins = relation_origins(input, catalog)?;
            let mut origin = expression_origin(expression, b"computed-column.v1")?;
            origin.inputs = resolve_inputs(&origin.inputs, &origins)?;
            origins.insert(column.id, origin);
            Ok(origins)
        }
        Relation::UnitConvert { input, column, .. } => {
            let mut origins = relation_origins(input, catalog)?;
            let source = origins
                .get(column)
                .cloned()
                .ok_or(DataError::MissingColumn(*column))?;
            origins.insert(
                *column,
                Origin {
                    inputs: source.inputs,
                    expression_fingerprint: fingerprint(relation)?,
                },
            );
            Ok(origins)
        }
        Relation::Aggregate {
            input,
            groups,
            measures,
        } => {
            let sources = relation_origins(input, catalog)?;
            let mut origins = groups
                .iter()
                .map(|column| {
                    sources
                        .get(column)
                        .cloned()
                        .map(|origin| (*column, origin))
                        .ok_or(DataError::MissingColumn(*column))
                })
                .collect::<Result<BTreeMap<_, _>>>()?;
            for measure in measures {
                let referenced = measure
                    .input
                    .as_ref()
                    .map_or_else(BTreeSet::new, expression_columns);
                let inputs = resolve_inputs(&referenced, &sources)?;
                origins.insert(
                    measure.output.id,
                    Origin {
                        inputs,
                        expression_fingerprint: fingerprint(measure)?,
                    },
                );
            }
            Ok(origins)
        }
        Relation::Pivot {
            input,
            groups,
            names_from,
            values_from,
            ..
        } => {
            let mut origins = relation_origins(input, catalog)?;
            origins.retain(|column, _| groups.contains(column));
            // Dynamic pivot output IDs are defined by the operation and name;
            // the caller resolves those IDs from the checked output schema.
            let _ = (names_from, values_from);
            Ok(origins)
        }
        Relation::Unpivot {
            input,
            ids,
            values,
            name_column,
            value_column,
        } => {
            let sources = relation_origins(input, catalog)?;
            let mut origins = ids
                .iter()
                .map(|column| {
                    sources
                        .get(column)
                        .cloned()
                        .map(|origin| (*column, origin))
                        .ok_or(DataError::MissingColumn(*column))
                })
                .collect::<Result<BTreeMap<_, _>>>()?;
            let referenced: BTreeSet<_> = values.iter().copied().collect();
            let inputs = resolve_inputs(&referenced, &sources)?;
            let expression_fingerprint = fingerprint(relation)?;
            for output in [name_column.id, value_column.id] {
                origins.insert(
                    output,
                    Origin {
                        inputs: inputs.clone(),
                        expression_fingerprint,
                    },
                );
            }
            Ok(origins)
        }
        Relation::Union { inputs } => {
            let mut merged = BTreeMap::new();
            for input in inputs {
                for (column, origin) in relation_origins(input, catalog)? {
                    merged
                        .entry(column)
                        .and_modify(|existing: &mut Origin| {
                            existing.inputs.extend(origin.inputs.iter().copied())
                        })
                        .or_insert(origin);
                }
            }
            Ok(merged)
        }
        Relation::Join { left, right, .. } => {
            let mut origins = relation_origins(left, catalog)?;
            for (column, origin) in relation_origins(right, catalog)? {
                if origins.insert(column, origin).is_some() {
                    return Err(DataError::InvalidPlan(
                        "joined column identities collide".into(),
                    ));
                }
            }
            Ok(origins)
        }
    }
}

/// Pivot columns are dynamic, so fill their shared value/name origin after the
/// checked schema is known.
fn complete_pivot_origins(
    operation: crate::OperationId,
    relation: &Relation,
    catalog: &Catalog,
    origins: &mut BTreeMap<ColumnId, Origin>,
    output_schema: &TableSchema,
) -> Result<()> {
    match relation {
        Relation::Pivot {
            input,
            names_from,
            values_from,
            ..
        } => {
            let sources = relation_origins(input, catalog)?;
            let mut inputs = BTreeSet::new();
            for column in [names_from, values_from] {
                inputs.extend(
                    sources
                        .get(column)
                        .ok_or(DataError::MissingColumn(*column))?
                        .inputs
                        .iter()
                        .copied(),
                );
            }
            let marker = Origin {
                inputs,
                expression_fingerprint: fingerprint(relation)?,
            };
            for column in &output_schema.columns {
                if column.id == ColumnId::derived(operation, column.name.as_bytes()) {
                    origins.insert(column.id, marker.clone());
                }
            }
            complete_pivot_origins(operation, input, catalog, origins, output_schema)?;
        }
        Relation::Project { input, .. }
        | Relation::Rename { input, .. }
        | Relation::ComputedColumn { input, .. }
        | Relation::Filter { input, .. }
        | Relation::StableSort { input, .. }
        | Relation::Aggregate { input, .. }
        | Relation::Unpivot { input, .. }
        | Relation::Patch { input, .. }
        | Relation::UnitConvert { input, .. }
        | Relation::MarkMissing { input, .. } => {
            complete_pivot_origins(operation, input, catalog, origins, output_schema)?;
        }
        Relation::Union { inputs } => {
            for input in inputs {
                complete_pivot_origins(operation, input, catalog, origins, output_schema)?;
            }
        }
        Relation::Join { left, right, .. } => {
            complete_pivot_origins(operation, left, catalog, origins, output_schema)?;
            complete_pivot_origins(operation, right, catalog, origins, output_schema)?;
        }
        Relation::SnapshotRead(_) => {}
    }
    Ok(())
}

fn identity(column: ColumnId) -> Origin {
    Origin {
        inputs: BTreeSet::from([column]),
        expression_fingerprint: ContentHash::of(b"plotx.column.identity.v1"),
    }
}

fn expression_origin(expression: &Expression, prefix: &[u8]) -> Result<Origin> {
    let mut bytes = prefix.to_vec();
    bytes.extend(
        serde_json::to_vec(expression).map_err(|error| DataError::Backend(error.to_string()))?,
    );
    Ok(Origin {
        inputs: expression_columns(expression),
        expression_fingerprint: ContentHash::of(&bytes),
    })
}

fn expression_columns(expression: &Expression) -> BTreeSet<ColumnId> {
    let mut columns = BTreeSet::new();
    collect_expression_columns(expression, &mut columns);
    columns
}

fn resolve_inputs(
    columns: &BTreeSet<ColumnId>,
    origins: &BTreeMap<ColumnId, Origin>,
) -> Result<BTreeSet<ColumnId>> {
    let mut resolved = BTreeSet::new();
    for column in columns {
        resolved.extend(
            origins
                .get(column)
                .ok_or(DataError::MissingColumn(*column))?
                .inputs
                .iter()
                .copied(),
        );
    }
    Ok(resolved)
}

fn collect_expression_columns(expression: &Expression, columns: &mut BTreeSet<ColumnId>) {
    match expression {
        Expression::Column { column } => {
            columns.insert(*column);
        }
        Expression::Literal { .. } => {}
        Expression::Call { args, .. } => {
            args.iter()
                .for_each(|argument| collect_expression_columns(argument, columns));
        }
        Expression::Cast { input, .. } => collect_expression_columns(input, columns),
    }
}

fn fingerprint(value: &impl serde::Serialize) -> Result<ContentHash> {
    let bytes = serde_json::to_vec(value).map_err(|error| DataError::Backend(error.to_string()))?;
    Ok(ContentHash::of(&bytes))
}
