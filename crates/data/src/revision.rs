use crate::{
    CellPatch, ColumnId, ContentHash, DATA_SCHEMA_VERSION, DataError, LiteralValue, OperationId,
    RelPlanV1, Relation, Result, RevisionId, RowId, SnapshotRead, TableId, TableSchema,
    TableSnapshot,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TableRevision {
    pub id: RevisionId,
    pub table_id: TableId,
    pub snapshot: TableSnapshot,
    #[serde(default)]
    pub parents: Vec<RevisionId>,
    pub operation: RevisionOperation,
    pub reason: RevisionReason,
    /// Explicitly supplied UTC instant. The data layer never reads a clock,
    /// keeping plan evaluation deterministic.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_at_utc: Option<String>,
}

impl TableRevision {
    pub fn initial(
        snapshot: TableSnapshot,
        reason: RevisionReason,
        operation_name: impl Into<String>,
        software_version: impl Into<String>,
    ) -> Result<Self> {
        let operation = RevisionOperation {
            id: OperationId::new(),
            name: operation_name.into(),
            inputs: Vec::new(),
            plan: None,
            plan_fingerprint: ContentHash::of(b"plotx.no-plan.v1"),
            result_fingerprint: snapshot.fingerprint,
            software_version: software_version.into(),
            function_versions: BTreeMap::new(),
            parameters: BTreeMap::new(),
            diagnostics: Vec::new(),
            column_lineage: Vec::new(),
            row_mapping: None,
        };
        let revision = Self {
            id: RevisionId::new(),
            table_id: snapshot.table_id,
            snapshot,
            parents: Vec::new(),
            operation,
            reason,
            created_at_utc: None,
        };
        revision.validate()?;
        Ok(revision)
    }

    pub fn validate(&self) -> Result<()> {
        if self.table_id != self.snapshot.table_id {
            return Err(DataError::InvalidSchema(
                "revision and snapshot table identities differ".into(),
            ));
        }
        if self.parents.iter().copied().collect::<BTreeSet<_>>().len() != self.parents.len() {
            return Err(DataError::InvalidSchema(
                "revision repeats a parent identity".into(),
            ));
        }
        self.snapshot.validate()?;
        if let Some(plan) = &self.operation.plan {
            plan.validate()?;
            if plan.fingerprint()? != self.operation.plan_fingerprint {
                return Err(DataError::CorruptBlock(
                    "revision plan fingerprint mismatch".into(),
                ));
            }
        }
        if self.snapshot.fingerprint != self.operation.result_fingerprint {
            return Err(DataError::CorruptBlock(
                "revision result fingerprint mismatch".into(),
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RevisionOperation {
    pub id: OperationId,
    pub name: String,
    #[serde(default)]
    pub inputs: Vec<RevisionInput>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plan: Option<RelPlanV1>,
    pub plan_fingerprint: ContentHash,
    pub result_fingerprint: ContentHash,
    pub software_version: String,
    #[serde(default)]
    pub function_versions: BTreeMap<String, String>,
    #[serde(default)]
    pub parameters: BTreeMap<String, serde_json::Value>,
    #[serde(default)]
    pub diagnostics: Vec<Diagnostic>,
    #[serde(default)]
    pub column_lineage: Vec<ColumnLineage>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub row_mapping: Option<RowMapping>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RevisionInput {
    pub table: TableId,
    pub revision: RevisionId,
    pub snapshot_fingerprint: ContentHash,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RevisionReason {
    Import,
    ManualEdit,
    Transform,
    Refresh,
    Rebase,
    Automation,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Diagnostic {
    pub severity: DiagnosticSeverity,
    pub code: String,
    pub message: String,
    #[serde(default)]
    pub counts: BTreeMap<String, u64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiagnosticSeverity {
    Info,
    Warning,
    Error,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ColumnLineage {
    pub output: ColumnId,
    pub inputs: Vec<ColumnId>,
    pub expression_fingerprint: ContentHash,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RowMapping {
    Identity,
    Selection {
        /// Inclusive input runs `(start, length)` retained in output order.
        runs: Vec<(u64, u64)>,
    },
    Derived {
        mapping_block: ContentHash,
        codec: String,
    },
    /// Union preserves every source identity under its stable table namespace;
    /// this compact rule replaces one mapping entry per output row.
    UnionNamespaces {
        sources: Vec<TableId>,
    },
    /// Unpivot derives one row per source row/value-column pair.
    Unpivot {
        source: TableId,
        value_columns: Vec<ColumnId>,
    },
}

/// Codec payload used by `plotx.row-map.runs-json.v1`. Sources point into the
/// ordered `RevisionOperation.inputs` list and use input-position runs, which
/// is substantially smaller than repeating UUIDs for contiguous groups.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DerivedRowMappingV1 {
    pub version: u32,
    pub outputs: Vec<DerivedRowSourcesV1>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DerivedRowSourcesV1 {
    pub output: RowId,
    pub sources: Vec<InputRowRunsV1>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct InputRowRunsV1 {
    pub input: u32,
    pub runs: Vec<(u64, u64)>,
}

impl DerivedRowMappingV1 {
    pub const CODEC: &'static str = "plotx.row-map.runs-json.v1";

    pub fn encode(&self) -> Result<Vec<u8>> {
        if self.version != 1 {
            return Err(DataError::Unsupported(format!(
                "derived row mapping v{}",
                self.version
            )));
        }
        serde_json::to_vec(self).map_err(|error| DataError::Backend(error.to_string()))
    }

    pub fn decode(bytes: &[u8]) -> Result<Self> {
        let mapping: Self = serde_json::from_slice(bytes)
            .map_err(|error| DataError::CorruptBlock(error.to_string()))?;
        if mapping.version != 1 {
            return Err(DataError::Unsupported(format!(
                "derived row mapping v{}",
                mapping.version
            )));
        }
        Ok(mapping)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TableEnvelopeV1 {
    pub schema_version: u32,
    pub revision: TableRevision,
    /// Immutable ancestors retained for audit, undo after reopen, and block
    /// reachability. The selected current revision remains the field above.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub history: Vec<TableRevision>,
    #[serde(default)]
    pub raw_inputs: Vec<crate::RawInputObject>,
    #[serde(default)]
    pub extensions: BTreeMap<String, ExtensionBlock>,
}

impl TableEnvelopeV1 {
    pub fn new(revision: TableRevision) -> Self {
        Self {
            schema_version: DATA_SCHEMA_VERSION,
            revision,
            history: Vec::new(),
            raw_inputs: Vec::new(),
            extensions: BTreeMap::new(),
        }
    }

    pub fn validate(&self, understood_extensions: &BTreeSet<String>) -> Result<()> {
        self.validate_structure()?;
        for (id, extension) in &self.extensions {
            if extension.semantics_critical && !understood_extensions.contains(id) {
                return Err(DataError::Unsupported(format!(
                    "calculation requires unknown semantic extension {id}"
                )));
            }
        }
        Ok(())
    }

    /// Validate an envelope for lossless preservation without claiming that
    /// the current environment understands every semantic extension.
    pub fn validate_structure(&self) -> Result<()> {
        if self.schema_version != 1 {
            return Err(DataError::Unsupported(format!(
                "table envelope schema v{}",
                self.schema_version
            )));
        }
        self.revision.validate()?;
        let mut revisions = BTreeSet::new();
        revisions.insert(self.revision.id);
        for revision in &self.history {
            revision.validate()?;
            if revision.table_id != self.revision.table_id {
                return Err(DataError::InvalidSchema(
                    "table envelope history crosses table identities".into(),
                ));
            }
            if !revisions.insert(revision.id) {
                return Err(DataError::InvalidSchema(
                    "table envelope repeats a revision identity".into(),
                ));
            }
        }
        for revision in self.history.iter().chain(std::iter::once(&self.revision)) {
            if revision
                .parents
                .iter()
                .any(|parent| !revisions.contains(parent))
            {
                return Err(DataError::InvalidSchema(
                    "table envelope omits a parent revision".into(),
                ));
            }
        }
        for input in &self.raw_inputs {
            input.validate()?;
        }
        for (id, extension) in &self.extensions {
            if extension.version != 1 || !id.contains('.') {
                return Err(DataError::InvalidSchema(format!(
                    "invalid extension contract {id:?}"
                )));
            }
        }
        Ok(())
    }

    pub fn referenced_blocks(&self) -> BTreeSet<ContentHash> {
        let mut hashes = BTreeSet::new();
        for revision in self.history.iter().chain(std::iter::once(&self.revision)) {
            hashes.extend(
                revision
                    .snapshot
                    .row_id_chunks
                    .iter()
                    .chain(
                        revision
                            .snapshot
                            .columns
                            .iter()
                            .flat_map(|column| column.chunks.iter()),
                    )
                    .map(|chunk| chunk.byte_hash),
            );
            if let Some(RowMapping::Derived { mapping_block, .. }) = &revision.operation.row_mapping
            {
                hashes.insert(*mapping_block);
            }
        }
        hashes.extend(self.raw_inputs.iter().map(|input| input.byte_hash));
        hashes
    }

    pub fn advance(&mut self, revision: TableRevision) -> Result<()> {
        if revision.table_id != self.revision.table_id
            || !revision.parents.contains(&self.revision.id)
        {
            return Err(DataError::InvalidSchema(
                "new table revision is not a child of the selected revision".into(),
            ));
        }
        if !self
            .history
            .iter()
            .any(|ancestor| ancestor.id == self.revision.id)
        {
            self.history.push(self.revision.clone());
        }
        self.revision = revision;
        self.validate_structure()
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ExtensionBlock {
    pub version: u32,
    pub semantics_critical: bool,
    pub payload: serde_json::Value,
}

/// Lightweight history stores immutable manifests. Undo/redo changes the
/// selected revision; it does not clone data blocks.
#[derive(Clone, Debug, Default)]
pub struct RevisionHistory {
    revisions: BTreeMap<RevisionId, TableRevision>,
    current: Option<RevisionId>,
    redo: Vec<RevisionId>,
}

impl RevisionHistory {
    pub fn insert(&mut self, revision: TableRevision) -> Result<()> {
        revision.validate()?;
        if self.revisions.contains_key(&revision.id) {
            return Err(DataError::InvalidSchema(format!(
                "duplicate revision {}",
                revision.id
            )));
        }
        if revision
            .parents
            .iter()
            .any(|parent| !self.revisions.contains_key(parent))
        {
            return Err(DataError::InvalidSchema(
                "revision references an unknown parent".into(),
            ));
        }
        self.current = Some(revision.id);
        self.redo.clear();
        self.revisions.insert(revision.id, revision);
        Ok(())
    }

    pub fn current(&self) -> Option<&TableRevision> {
        self.current.and_then(|id| self.revisions.get(&id))
    }

    pub fn switch_to(&mut self, revision: RevisionId) -> Result<()> {
        if !self.revisions.contains_key(&revision) {
            return Err(DataError::InvalidSchema(format!(
                "revision {revision} does not exist"
            )));
        }
        self.current = Some(revision);
        self.redo.clear();
        Ok(())
    }

    pub fn undo(&mut self) -> Result<bool> {
        let Some(current) = self.current else {
            return Ok(false);
        };
        let revision = &self.revisions[&current];
        let Some(parent) = revision.parents.first().copied() else {
            return Ok(false);
        };
        self.redo.push(current);
        self.current = Some(parent);
        Ok(true)
    }

    pub fn redo(&mut self) -> bool {
        let Some(revision) = self.redo.pop() else {
            return false;
        };
        self.current = Some(revision);
        true
    }
}

#[derive(Clone, Debug)]
pub struct TableTransaction {
    base: RevisionInput,
    edits: BTreeMap<(RowId, ColumnId), LiteralValue>,
}

impl TableTransaction {
    pub fn new(base: &TableRevision) -> Self {
        Self {
            base: RevisionInput {
                table: base.table_id,
                revision: base.id,
                snapshot_fingerprint: base.snapshot.fingerprint,
            },
            edits: BTreeMap::new(),
        }
    }

    pub fn set(&mut self, row: RowId, column: ColumnId, value: LiteralValue) {
        self.edits.insert((row, column), value);
    }

    pub fn clear(&mut self, row: RowId, column: ColumnId) {
        self.set(row, column, LiteralValue::Null);
    }

    pub fn is_empty(&self) -> bool {
        self.edits.is_empty()
    }

    pub fn plan(&self) -> RelPlanV1 {
        let read = Relation::SnapshotRead(SnapshotRead {
            table: self.base.table,
            revision: self.base.revision,
            fingerprint: self.base.snapshot_fingerprint,
        });
        RelPlanV1::new(Relation::Patch {
            input: Box::new(read),
            edits: self
                .edits
                .iter()
                .map(|(&(row, column), value)| CellPatch {
                    row,
                    column,
                    value: value.clone(),
                })
                .collect(),
        })
    }

    pub fn execute_and_commit(
        self,
        base: &TableRevision,
        store: &dyn crate::BlockStore,
        codecs: &crate::CodecRegistry,
        software_version: impl Into<String>,
    ) -> Result<TableRevision> {
        if base.table_id != self.base.table
            || base.id != self.base.revision
            || base.snapshot.fingerprint != self.base.snapshot_fingerprint
        {
            return Err(DataError::PatchConflict(
                "transaction base revision no longer matches".into(),
            ));
        }
        let edits = self
            .edits
            .iter()
            .map(|(&(row, column), value)| CellPatch {
                row,
                column,
                value: value.clone(),
            })
            .collect::<Vec<_>>();
        let result = crate::patch_snapshot(&base.snapshot, &edits, store, codecs)?;
        self.commit(result, software_version)
    }

    pub fn commit(
        self,
        result: TableSnapshot,
        software_version: impl Into<String>,
    ) -> Result<TableRevision> {
        if result.table_id != self.base.table {
            return Err(DataError::InvalidSchema(
                "transaction result changed the table identity".into(),
            ));
        }
        let plan = self.plan();
        let operation = RevisionOperation {
            id: plan.operation_id,
            name: "patch.v1".into(),
            inputs: vec![self.base.clone()],
            plan_fingerprint: plan.fingerprint()?,
            result_fingerprint: result.fingerprint,
            plan: Some(plan),
            software_version: software_version.into(),
            function_versions: BTreeMap::new(),
            parameters: BTreeMap::new(),
            diagnostics: Vec::new(),
            column_lineage: Vec::new(),
            row_mapping: Some(RowMapping::Identity),
        };
        let revision = TableRevision {
            id: RevisionId::new(),
            table_id: result.table_id,
            snapshot: result,
            parents: vec![self.base.revision],
            operation,
            reason: RevisionReason::ManualEdit,
            created_at_utc: None,
        };
        revision.validate()?;
        Ok(revision)
    }
}

/// Strictly rebase patches by an explicit provenance/business-key resolution.
/// Positional or similarity-based matching is intentionally impossible.
pub fn rebase_patches(
    patches: &[CellPatch],
    rows: &BTreeMap<RowId, RowId>,
    old_schema: &TableSchema,
    new_schema: &TableSchema,
) -> Result<Vec<CellPatch>> {
    patches
        .iter()
        .map(|patch| {
            let row = rows.get(&patch.row).copied().ok_or_else(|| {
                DataError::PatchConflict(format!("row {} has no exact match", patch.row))
            })?;
            let old = old_schema
                .column(patch.column)
                .ok_or(DataError::MissingColumn(patch.column))?;
            let new = new_schema.column(patch.column).ok_or_else(|| {
                DataError::PatchConflict(format!("column {} was removed", patch.column))
            })?;
            if old.logical_type != new.logical_type {
                return Err(DataError::PatchConflict(format!(
                    "column {:?} changed type",
                    old.name
                )));
            }
            if old.unit != new.unit {
                return Err(DataError::PatchConflict(format!(
                    "column {:?} changed unit",
                    old.name
                )));
            }
            Ok(CellPatch {
                row,
                column: patch.column,
                value: patch.value.clone(),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ColumnSchema, LogicalType, TableSchema};

    #[test]
    fn rebase_requires_explicit_row_mapping_and_unchanged_column_contract() {
        let column = ColumnSchema::new("value", LogicalType::Float64);
        let schema = TableSchema::new(vec![column.clone()]).unwrap();
        let source = RowId::new();
        let target = RowId::new();
        let patch = CellPatch {
            row: source,
            column: column.id,
            value: LiteralValue::Float64(crate::FiniteOrSpecial::new(3.0)),
        };
        assert!(
            rebase_patches(
                std::slice::from_ref(&patch),
                &BTreeMap::new(),
                &schema,
                &schema
            )
            .is_err()
        );
        let rebased = rebase_patches(
            &[patch],
            &BTreeMap::from([(source, target)]),
            &schema,
            &schema,
        )
        .unwrap();
        assert_eq!(rebased[0].row, target);
    }

    #[test]
    fn rebase_blocks_removed_type_changed_and_unit_changed_columns() {
        let mut column = ColumnSchema::new("value", LogicalType::Float64);
        let source = RowId::new();
        let target = RowId::new();
        let patch = CellPatch {
            row: source,
            column: column.id,
            value: LiteralValue::Float64(crate::FiniteOrSpecial::new(3.0)),
        };
        let old_schema = TableSchema::new(vec![column.clone()]).unwrap();
        let mapping = BTreeMap::from([(source, target)]);

        let removed = TableSchema::new(Vec::new()).unwrap();
        assert!(
            rebase_patches(
                std::slice::from_ref(&patch),
                &mapping,
                &old_schema,
                &removed
            )
            .unwrap_err()
            .to_string()
            .contains("removed")
        );

        let mut changed_type = column.clone();
        changed_type.logical_type = LogicalType::Int64;
        assert!(
            rebase_patches(
                std::slice::from_ref(&patch),
                &mapping,
                &old_schema,
                &TableSchema::new(vec![changed_type]).unwrap(),
            )
            .unwrap_err()
            .to_string()
            .contains("changed type")
        );

        column.unit = Some(crate::UnitSpec::ppm());
        assert!(
            rebase_patches(
                &[patch],
                &mapping,
                &old_schema,
                &TableSchema::new(vec![column]).unwrap(),
            )
            .unwrap_err()
            .to_string()
            .contains("changed unit")
        );
    }
}
