//! Capability-checked Substrait interchange. Substrait protobuf objects never
//! cross PlotX's public data boundary and are never persisted in projects.

use crate::{ContentHash, DataError, ExecutionRequest, Expression, RelPlanV1, Relation, Result};
use prost::Message;
use serde::{Deserialize, Serialize};

const PLOTX_IR_TYPE_URL: &str = "type.googleapis.com/space.nmrtist.plotx.RelPlanV1Exchange";

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SubstraitCapability {
    Equivalent,
    Rejected { reason: String },
}

pub fn substrait_capability(plan: &RelPlanV1) -> SubstraitCapability {
    match supported_relation(&plan.root) {
        Ok(()) => SubstraitCapability::Equivalent,
        Err(reason) => SubstraitCapability::Rejected { reason },
    }
}

/// Produce a protobuf Substrait plan only when the complete PlotX semantics
/// are representable by the current adapter. Exact PlotX identity metadata is
/// embedded as ignorable round-trip metadata and bound to the relation body.
pub fn export_substrait(request: &ExecutionRequest) -> Result<Vec<u8>> {
    request.plan.validate()?;
    require_capability(&request.plan)?;
    let (logical, state) = crate::compile_for_interop(request)?;
    let mut plan =
        datafusion_substrait::logical_plan::producer::to_substrait_plan(&logical, &state)
            .map_err(|error| DataError::Backend(format!("Substrait export failed: {error}")))?;
    plan.advanced_extensions = None;
    let body_hash = ContentHash::of(&plan.encode_to_vec());
    let envelope = RoundTripEnvelope {
        contract: 1,
        body_hash,
        plotx_plan: request.plan.clone(),
    };
    let value = serde_json::to_vec(&envelope)
        .map_err(|error| DataError::Backend(format!("Substrait metadata failed: {error}")))?;
    plan.advanced_extensions = Some(substrait::proto::extensions::AdvancedExtension {
        optimization: vec![pbjson_types::Any {
            type_url: PLOTX_IR_TYPE_URL.into(),
            value: value.into(),
        }],
        enhancement: None,
    });
    Ok(plan.encode_to_vec())
}

/// Recover a PlotX plan from a lossless PlotX-produced Substrait exchange.
/// Arbitrary Substrait cannot supply stable revision/column identity and is
/// therefore rejected instead of being guessed into a non-equivalent plan.
pub fn import_substrait(bytes: &[u8]) -> Result<RelPlanV1> {
    let mut plan = substrait::proto::Plan::decode(bytes)
        .map_err(|error| DataError::InvalidPlan(format!("invalid Substrait protobuf: {error}")))?;
    let extension = plan
        .advanced_extensions
        .as_ref()
        .and_then(|extensions| {
            extensions
                .optimization
                .iter()
                .find(|value| value.type_url == PLOTX_IR_TYPE_URL)
        })
        .ok_or_else(|| {
            DataError::Unsupported(
                "Substrait plan lacks lossless PlotX revision and column identity metadata".into(),
            )
        })?;
    let envelope: RoundTripEnvelope = serde_json::from_slice(&extension.value)
        .map_err(|error| DataError::InvalidPlan(format!("invalid PlotX metadata: {error}")))?;
    if envelope.contract != 1 {
        return Err(DataError::Unsupported(format!(
            "Substrait PlotX exchange contract {}",
            envelope.contract
        )));
    }
    plan.advanced_extensions = None;
    if ContentHash::of(&plan.encode_to_vec()) != envelope.body_hash {
        return Err(DataError::InvalidPlan(
            "Substrait relation body changed after PlotX capability validation".into(),
        ));
    }
    envelope.plotx_plan.validate()?;
    require_capability(&envelope.plotx_plan)?;
    Ok(envelope.plotx_plan)
}

fn require_capability(plan: &RelPlanV1) -> Result<()> {
    match substrait_capability(plan) {
        SubstraitCapability::Equivalent => Ok(()),
        SubstraitCapability::Rejected { reason } => Err(DataError::Unsupported(format!(
            "Substrait cannot represent this PlotX plan equivalently: {reason}"
        ))),
    }
}

fn supported_relation(relation: &Relation) -> std::result::Result<(), String> {
    match relation {
        Relation::SnapshotRead(_) => Ok(()),
        Relation::Project { input, .. }
        | Relation::Rename { input, .. }
        | Relation::StableSort { input, .. } => supported_relation(input),
        Relation::ComputedColumn {
            input, expression, ..
        } => {
            supported_relation(input)?;
            supported_expression(expression)
        }
        Relation::Filter { input, predicate } => {
            supported_relation(input)?;
            supported_expression(predicate)
        }
        Relation::Union { inputs } => inputs.iter().try_for_each(supported_relation),
        Relation::Aggregate {
            input, measures, ..
        } => {
            supported_relation(input)?;
            measures
                .iter()
                .filter_map(|measure| measure.input.as_ref())
                .try_for_each(supported_expression)
        }
        Relation::Join { left, right, .. } => {
            supported_relation(left)?;
            supported_relation(right)
        }
        Relation::Unpivot { input, .. } => supported_relation(input),
        Relation::Pivot { .. } => Err(
            "Pivot output columns are data-dependent and have no lossless Substrait relation"
                .into(),
        ),
        Relation::Patch { .. } => Err("Patch has no lossless Substrait relation".into()),
        Relation::UnitConvert { .. } => {
            Err("unit conversion semantics are not carried by Substrait".into())
        }
        Relation::MarkMissing { .. } => {
            Err("explicit missing-value marking is not carried by Substrait".into())
        }
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
        Expression::Call { function, .. } => Err(format!(
            "function '{function}' has no equivalent Substrait mapping"
        )),
        Expression::Cast { .. } => Err("PlotX cast failure policy is not representable".into()),
    }
}

#[derive(Serialize, Deserialize)]
struct RoundTripEnvelope {
    contract: u32,
    body_hash: ContentHash,
    plotx_plan: RelPlanV1,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        ColumnId, ColumnSchema, ExecutionInput, FiniteOrSpecial, LiteralValue, LogicalType,
        MaterializedColumn, MaterializedTable, RevisionId, RowId, SnapshotRead, TableId,
        TableSchema,
    };
    use std::collections::BTreeMap;

    #[test]
    fn supported_plan_round_trips_as_real_protobuf() {
        let (request, column) = request();
        let filtered = RelPlanV1::new(Relation::Filter {
            input: Box::new(request.plan.root.clone()),
            predicate: Expression::call(
                "eq.v1",
                vec![
                    Expression::column(column),
                    Expression::Literal {
                        value: LiteralValue::Float64(FiniteOrSpecial::Finite(1.0)),
                    },
                ],
            ),
        });
        let request = ExecutionRequest {
            plan: filtered.clone(),
            ..request
        };
        let bytes = export_substrait(&request).unwrap();
        let decoded = substrait::proto::Plan::decode(bytes.as_slice()).unwrap();
        assert!(!decoded.relations.is_empty());
        assert_eq!(import_substrait(&bytes).unwrap(), filtered);
    }

    #[test]
    fn semantic_extensions_and_tampering_are_rejected() {
        let (mut marked_request, column) = request();
        marked_request.plan = RelPlanV1::new(Relation::MarkMissing {
            input: Box::new(marked_request.plan.root),
            columns: vec![column],
            predicate: Expression::Literal {
                value: LiteralValue::Boolean(true),
            },
        });
        assert!(matches!(
            substrait_capability(&marked_request.plan),
            SubstraitCapability::Rejected { .. }
        ));

        let (request, _) = request();
        let bytes = export_substrait(&request).unwrap();
        let mut decoded = substrait::proto::Plan::decode(bytes.as_slice()).unwrap();
        decoded
            .version
            .as_mut()
            .unwrap()
            .producer
            .push_str("-tampered");
        assert!(import_substrait(&decoded.encode_to_vec()).is_err());
    }

    fn request() -> (ExecutionRequest, ColumnId) {
        let table = TableId::new();
        let revision = RevisionId::new();
        let column = ColumnSchema::new("value", LogicalType::Float64);
        let schema = TableSchema::new(vec![column.clone()]).unwrap();
        let materialized = MaterializedTable {
            table_id: table,
            schema,
            row_ids: vec![RowId::new(), RowId::new()],
            columns: vec![MaterializedColumn {
                schema: column.clone(),
                values: vec![crate::ScalarValue::Float64(1.0), crate::ScalarValue::Null],
            }],
        };
        let fingerprint = ContentHash::of(b"input");
        let plan = RelPlanV1::new(Relation::SnapshotRead(SnapshotRead {
            table,
            revision,
            fingerprint,
        }));
        let mut inputs = BTreeMap::new();
        inputs.insert(
            (table, revision),
            ExecutionInput::materialized(materialized, fingerprint),
        );
        (
            ExecutionRequest {
                plan,
                inputs,
                memory_limit_bytes: 1024 * 1024,
            },
            column.id,
        )
    }
}
