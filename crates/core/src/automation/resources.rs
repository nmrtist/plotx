use super::{
    AutomationError, CapabilityId, DataPreview, DocumentRevision, FrozenTargetSet,
    ProjectBlueprint, ResourceDescriptor, ResourceKindId, ResourceQuery, ResourceRef,
    SelectionReason, TargetSelector,
};
use crate::state::{Dataset, PlotxApp};
use std::collections::BTreeMap;

pub const KIND_DATASET: &str = "plotx.dataset";
pub const KIND_CANVAS: &str = "plotx.canvas";
pub const KIND_TABLE_ROW: &str = "plotx.table.row";
pub const KIND_TABLE_COLUMN: &str = "plotx.table.column";
pub const KIND_CANVAS_OBJECT: &str = "plotx.canvas.object";
pub const KIND_EXTERNAL_INPUT: &str = "plotx.external_input";

pub const CAP_RENAME: &str = "resource.rename";
pub const CAP_RENDER: &str = "figure.render";
pub const CAP_THEME: &str = "figure.theme";
pub const CAP_EXPORT: &str = "figure.export";
pub const CAP_PREVIEW: &str = "data.preview";
pub const CAP_TRANSFORM: &str = "data.transform";
pub const CAP_PROCESSING_SCHEME: &str = "processing.scheme";

/// Capability-oriented resource access. New resource types can participate by
/// implementing this trait; query and tool orchestration do not dispatch on a
/// Rust data-domain enum.
pub trait ResourceProvider {
    fn revision(&self) -> DocumentRevision;
    fn blueprint(&self) -> ProjectBlueprint;
    fn descriptors(&self) -> Vec<ResourceDescriptor>;
    fn current_selection(&self) -> Vec<ResourceRef>;
    fn preview(&self, target: &ResourceRef, limit: usize) -> Result<DataPreview, AutomationError>;
    fn render_preview(&self, target: &ResourceRef) -> Result<String, AutomationError>;

    fn inspect(&self, id: &str) -> Option<ResourceDescriptor> {
        self.descriptors()
            .into_iter()
            .find(|descriptor| descriptor.resource.id == id)
    }
}

pub struct ProjectResourceProvider<'a> {
    app: &'a PlotxApp,
}

impl<'a> ProjectResourceProvider<'a> {
    pub fn new(app: &'a PlotxApp) -> Self {
        Self { app }
    }

    fn dataset_descriptor(&self, index: usize, dataset: &Dataset) -> ResourceDescriptor {
        let id = dataset.resource_id().to_owned();
        let lineage = dataset
            .lineage()
            .into_iter()
            .flat_map(|lineage| lineage.sources.iter())
            .filter_map(|source| self.app.doc.datasets.get(*source))
            .map(|source| source.resource_id().to_owned())
            .collect();
        let mut capabilities = vec![cap(CAP_RENAME), cap(CAP_PREVIEW)];
        if matches!(dataset, Dataset::Nmr(_) | Dataset::Nmr2D(_)) {
            capabilities.push(cap(CAP_PROCESSING_SCHEME));
        }
        let mut metadata = BTreeMap::new();
        metadata.insert("domain".to_owned(), format!("{:?}", dataset.domain()));
        metadata.insert("index_hint".to_owned(), index.to_string());
        let (dimensions, units, children) = match dataset {
            Dataset::Table(table) => {
                capabilities.push(cap(CAP_TRANSFORM));
                let snapshot = &table.typed_state.envelope.revision.snapshot;
                let columns = snapshot
                    .schema
                    .columns
                    .iter()
                    .map(|column| child_ref(&id, &column.id.to_string(), KIND_TABLE_COLUMN));
                metadata.insert("table_id".into(), snapshot.table_id.to_string());
                metadata.insert(
                    "table_revision".into(),
                    table.typed_state.envelope.revision.id.to_string(),
                );
                (
                    vec![
                        usize::try_from(snapshot.row_count).unwrap_or(usize::MAX),
                        snapshot.schema.columns.len(),
                    ],
                    snapshot
                        .schema
                        .columns
                        .iter()
                        .filter_map(|column| {
                            column.unit.as_ref().map(|unit| unit.display_unit.clone())
                        })
                        .collect(),
                    columns.collect(),
                )
            }
            Dataset::Nmr(nmr) => (
                vec![nmr.spectrum.values.len()],
                vec!["ppm".to_owned()],
                Vec::new(),
            ),
            Dataset::Nmr2D(nmr) => (
                vec![nmr.data.rows, nmr.data.cols],
                vec!["ppm".to_owned(), "ppm".to_owned()],
                Vec::new(),
            ),
            Dataset::Electrophysiology(recording) => (
                vec![recording.data.sweeps.len(), recording.data.channels.len()],
                vec!["s".to_owned()],
                Vec::new(),
            ),
        };
        ResourceDescriptor {
            resource: top_ref(&id, KIND_DATASET),
            name: dataset.display_name(),
            capabilities,
            children,
            dimensions,
            units,
            metadata,
            lineage,
            revision: self.revision(),
        }
    }

    fn canvas_descriptor(&self, index: usize) -> ResourceDescriptor {
        let canvas = &self.app.doc.canvases[index];
        let children = canvas
            .objects
            .iter()
            .map(|object| {
                child_ref(
                    &canvas.resource_id,
                    &object.id.to_string(),
                    KIND_CANVAS_OBJECT,
                )
            })
            .collect();
        let mut metadata = BTreeMap::new();
        metadata.insert("index_hint".to_owned(), index.to_string());
        ResourceDescriptor {
            resource: top_ref(&canvas.resource_id, KIND_CANVAS),
            name: canvas.name.clone(),
            capabilities: vec![
                cap(CAP_RENAME),
                cap(CAP_RENDER),
                cap(CAP_THEME),
                cap(CAP_EXPORT),
            ],
            children,
            dimensions: vec![canvas.objects.len()],
            units: vec!["mm".to_owned()],
            metadata,
            lineage: canvas
                .dataset_indices()
                .into_iter()
                .filter_map(|dataset| self.app.doc.datasets.get(dataset))
                .map(|dataset| dataset.resource_id().to_owned())
                .collect(),
            revision: self.revision(),
        }
    }
}

impl ResourceProvider for ProjectResourceProvider<'_> {
    fn revision(&self) -> DocumentRevision {
        DocumentRevision(self.app.doc.automation_revision)
    }

    fn blueprint(&self) -> ProjectBlueprint {
        let mut counts = BTreeMap::new();
        for descriptor in self.descriptors() {
            *counts.entry(descriptor.resource.kind.0).or_insert(0) += 1;
        }
        let relationships = self
            .app
            .doc
            .datasets
            .iter()
            .flat_map(|dataset| {
                let child = dataset.resource_id().to_owned();
                dataset
                    .lineage()
                    .into_iter()
                    .flat_map(|lineage| lineage.sources.iter())
                    .filter_map(|source| self.app.doc.datasets.get(*source))
                    .map(move |source| format!("{} derives_from {}", child, source.resource_id()))
            })
            .collect();
        ProjectBlueprint {
            revision: self.revision(),
            resource_counts: counts,
            relationships,
            warnings: Vec::new(),
        }
    }

    fn descriptors(&self) -> Vec<ResourceDescriptor> {
        let mut descriptors = Vec::new();
        for (index, dataset) in self.app.doc.datasets.iter().enumerate() {
            let parent = self.dataset_descriptor(index, dataset);
            if let Dataset::Table(table) = dataset {
                let snapshot = &table.typed_state.envelope.revision.snapshot;
                for column in &snapshot.schema.columns {
                    descriptors.push(ResourceDescriptor {
                        resource: child_ref(
                            dataset.resource_id(),
                            &column.id.to_string(),
                            KIND_TABLE_COLUMN,
                        ),
                        name: column.name.clone(),
                        capabilities: vec![cap(CAP_PREVIEW)],
                        children: Vec::new(),
                        dimensions: vec![usize::try_from(snapshot.row_count).unwrap_or(usize::MAX)],
                        units: column
                            .unit
                            .as_ref()
                            .map(|unit| vec![unit.display_unit.clone()])
                            .unwrap_or_default(),
                        metadata: BTreeMap::from([
                            ("logical_type".into(), format!("{:?}", column.logical_type)),
                            ("nullable".into(), column.nullable.to_string()),
                        ]),
                        lineage: Vec::new(),
                        revision: self.revision(),
                    });
                }
            }
            descriptors.push(parent);
        }
        for index in 0..self.app.doc.canvases.len() {
            let parent = self.canvas_descriptor(index);
            let canvas = &self.app.doc.canvases[index];
            for object in &canvas.objects {
                descriptors.push(ResourceDescriptor {
                    resource: child_ref(
                        &canvas.resource_id,
                        &object.id.to_string(),
                        KIND_CANVAS_OBJECT,
                    ),
                    name: object.name.clone(),
                    capabilities: vec![cap(CAP_RENAME)],
                    children: Vec::new(),
                    dimensions: Vec::new(),
                    units: Vec::new(),
                    metadata: BTreeMap::new(),
                    lineage: object
                        .dataset_indices()
                        .into_iter()
                        .filter_map(|dataset| self.app.doc.datasets.get(dataset))
                        .map(|dataset| dataset.resource_id().to_owned())
                        .collect(),
                    revision: self.revision(),
                });
            }
            descriptors.push(parent);
        }
        descriptors
    }

    fn current_selection(&self) -> Vec<ResourceRef> {
        let mut selected = Vec::new();
        if let Some(dataset) = self
            .app
            .active_dataset()
            .and_then(|index| self.app.doc.datasets.get(index))
        {
            selected.push(top_ref(dataset.resource_id(), KIND_DATASET));
        }
        if let Some(canvas) = self
            .app
            .session
            .active_canvas
            .and_then(|index| self.app.doc.canvases.get(index))
        {
            selected.push(top_ref(&canvas.resource_id, KIND_CANVAS));
        }
        selected
    }

    fn preview(&self, target: &ResourceRef, limit: usize) -> Result<DataPreview, AutomationError> {
        let descriptor = self.inspect(&target.id).ok_or_else(|| {
            AutomationError::InvalidSelector(format!("resource {} does not exist", target.id))
        })?;
        let Some(dataset) = self.app.doc.datasets.iter().find(|dataset| {
            dataset.resource_id() == target.id
                || target.parent_id.as_deref() == Some(dataset.resource_id())
        }) else {
            return Ok(DataPreview {
                target: target.clone(),
                shape: descriptor.dimensions,
                values: serde_json::Value::Null,
                returned: 0,
                total: 0,
                truncated: false,
                statistics: BTreeMap::new(),
            });
        };
        preview_dataset(dataset, target, limit)
    }

    fn render_preview(&self, target: &ResourceRef) -> Result<String, AutomationError> {
        let canvas_id = if target.kind.0 == KIND_CANVAS {
            target.id.as_str()
        } else {
            target.parent_id.as_deref().unwrap_or("")
        };
        let canvas = self
            .app
            .doc
            .canvases
            .iter()
            .find(|canvas| canvas.resource_id == canvas_id)
            .ok_or_else(|| {
                AutomationError::InvalidSelector("render.preview requires a canvas".to_owned())
            })?;
        Ok(crate::state::render_document_svg(canvas))
    }
}

pub fn freeze_targets(
    provider: &impl ResourceProvider,
    selector: &TargetSelector,
) -> Result<FrozenTargetSet, AutomationError> {
    match selector {
        TargetSelector::Explicit { ids } => {
            let mut targets = Vec::new();
            let mut reasons = Vec::new();
            for id in ids {
                let descriptor = provider.inspect(id).ok_or_else(|| {
                    AutomationError::InvalidSelector(format!("resource {id} does not exist"))
                })?;
                reasons.push(SelectionReason {
                    resource_id: id.clone(),
                    reason: "explicit stable id".to_owned(),
                });
                targets.push(descriptor.resource);
            }
            Ok(FrozenTargetSet {
                revision: provider.revision(),
                total_matches: targets.len(),
                targets,
                reasons,
                truncated: false,
            })
        }
        TargetSelector::CurrentSelection => {
            let targets = provider.current_selection();
            let reasons = targets
                .iter()
                .map(|target| SelectionReason {
                    resource_id: target.id.clone(),
                    reason: "current GUI selection".to_owned(),
                })
                .collect();
            Ok(FrozenTargetSet {
                revision: provider.revision(),
                total_matches: targets.len(),
                targets,
                reasons,
                truncated: false,
            })
        }
        TargetSelector::Query { query } => Ok(search_resources(provider, query)),
        TargetSelector::WorkflowInput { name } => Err(AutomationError::InvalidSelector(format!(
            "workflow input '{name}' must be bound by the workflow executor"
        ))),
        TargetSelector::NodeOutput { node, port } => Err(AutomationError::InvalidSelector(
            format!("node output {node}.{port} must be bound by the workflow executor"),
        )),
    }
}

pub fn search_resources(
    provider: &impl ResourceProvider,
    query: &ResourceQuery,
) -> FrozenTargetSet {
    let limit = query.limit.clamp(1, 500);
    let mut matches = provider
        .descriptors()
        .into_iter()
        .filter(|descriptor| matches_query(descriptor, query))
        .collect::<Vec<_>>();
    matches.sort_by(|a, b| a.resource.id.cmp(&b.resource.id));
    let total_matches = matches.len();
    let selected = matches
        .into_iter()
        .skip(query.offset)
        .take(limit)
        .collect::<Vec<_>>();
    FrozenTargetSet {
        revision: provider.revision(),
        targets: selected
            .iter()
            .map(|descriptor| descriptor.resource.clone())
            .collect(),
        reasons: selected
            .iter()
            .map(|descriptor| SelectionReason {
                resource_id: descriptor.resource.id.clone(),
                reason: query_reason(query),
            })
            .collect(),
        total_matches,
        truncated: query.offset + selected.len() < total_matches,
    }
}

fn matches_query(descriptor: &ResourceDescriptor, query: &ResourceQuery) -> bool {
    (query.kinds.is_empty() || query.kinds.contains(&descriptor.resource.kind))
        && query
            .capabilities
            .iter()
            .all(|required| descriptor.capabilities.contains(required))
        && query.name_contains.as_ref().is_none_or(|needle| {
            descriptor
                .name
                .to_lowercase()
                .contains(&needle.to_lowercase())
        })
        && query
            .units
            .iter()
            .all(|unit| descriptor.units.contains(unit))
        && query
            .metadata
            .iter()
            .all(|(key, value)| descriptor.metadata.get(key) == Some(value))
        && query
            .lineage_source
            .as_ref()
            .is_none_or(|source| descriptor.lineage.contains(source))
}

fn query_reason(query: &ResourceQuery) -> String {
    let mut clauses = Vec::new();
    if !query.kinds.is_empty() {
        clauses.push("kind");
    }
    if !query.capabilities.is_empty() {
        clauses.push("capability");
    }
    if query.name_contains.is_some() {
        clauses.push("name");
    }
    if !query.units.is_empty() {
        clauses.push("unit");
    }
    if !query.metadata.is_empty() {
        clauses.push("metadata");
    }
    if query.lineage_source.is_some() {
        clauses.push("lineage");
    }
    if clauses.is_empty() {
        "matched all resources".to_owned()
    } else {
        format!("matched {} query", clauses.join(" + "))
    }
}

fn preview_dataset(
    dataset: &Dataset,
    target: &ResourceRef,
    limit: usize,
) -> Result<DataPreview, AutomationError> {
    let limit = limit.clamp(1, 10_000);
    let mut statistics = BTreeMap::new();
    let (shape, values, total) = match dataset {
        Dataset::Table(table) => {
            if target.kind.0 == KIND_TABLE_COLUMN {
                let column = target
                    .local_id
                    .as_deref()
                    .and_then(|id| id.parse::<plotx_data::ColumnId>().ok())
                    .ok_or_else(|| {
                        AutomationError::InvalidSelector("invalid table column id".to_owned())
                    })?;
                let preview = table
                    .typed_rows(limit, &[column])
                    .map_err(AutomationError::Execution)?;
                let values = &preview.columns[0].values;
                add_typed_statistics(&mut statistics, values);
                (
                    vec![usize::try_from(preview.total_rows).unwrap_or(usize::MAX)],
                    serde_json::Value::Array(values.iter().map(scalar_json).collect()),
                    usize::try_from(preview.total_rows).unwrap_or(usize::MAX),
                )
            } else {
                let preview = table
                    .typed_rows(limit, &[])
                    .map_err(AutomationError::Execution)?;
                let values = (0..preview.row_ids.len())
                    .map(|row| {
                        let mut record =
                            vec![serde_json::Value::String(preview.row_ids[row].to_string())];
                        record.extend(preview.columns.iter().map(|column| {
                            column
                                .values
                                .get(row)
                                .map(scalar_json)
                                .unwrap_or(serde_json::Value::Null)
                        }));
                        record
                    })
                    .collect::<Vec<_>>();
                (
                    vec![
                        usize::try_from(preview.total_rows).unwrap_or(usize::MAX),
                        preview.columns.len(),
                    ],
                    serde_json::json!(values),
                    usize::try_from(preview.total_rows).unwrap_or(usize::MAX),
                )
            }
        }
        Dataset::Nmr(nmr) => {
            let values = nmr
                .spectrum
                .values
                .iter()
                .map(|value| value.re)
                .collect::<Vec<_>>();
            add_statistics(&mut statistics, &values);
            (
                vec![values.len()],
                serde_json::json!(finite_slice(&values, limit)),
                values.len(),
            )
        }
        Dataset::Nmr2D(nmr) => (
            vec![nmr.data.rows, nmr.data.cols],
            serde_json::json!({"summary": dataset.summary()}),
            nmr.data.rows.saturating_mul(nmr.data.cols),
        ),
        Dataset::Electrophysiology(recording) => (
            vec![recording.data.sweeps.len(), recording.data.channels.len()],
            serde_json::json!({"summary": dataset.summary()}),
            recording.data.sweeps.len(),
        ),
    };
    let returned = total.min(limit);
    Ok(DataPreview {
        target: target.clone(),
        shape,
        values,
        returned,
        total,
        truncated: returned < total,
        statistics,
    })
}

fn add_statistics(out: &mut BTreeMap<String, f64>, values: &[f64]) {
    let finite = values
        .iter()
        .copied()
        .filter(|value| value.is_finite())
        .collect::<Vec<_>>();
    if finite.is_empty() {
        return;
    }
    out.insert(
        "min".to_owned(),
        finite.iter().copied().fold(f64::INFINITY, f64::min),
    );
    out.insert(
        "max".to_owned(),
        finite.iter().copied().fold(f64::NEG_INFINITY, f64::max),
    );
    out.insert(
        "mean".to_owned(),
        finite.iter().sum::<f64>() / finite.len() as f64,
    );
}

fn add_typed_statistics(out: &mut BTreeMap<String, f64>, values: &[plotx_data::ScalarValue]) {
    let finite = values.iter().filter_map(|value| match value {
        plotx_data::ScalarValue::Int64(value) => Some(*value as f64),
        plotx_data::ScalarValue::Float64(value) if value.is_finite() => Some(*value),
        _ => None,
    });
    add_statistics(out, &finite.collect::<Vec<_>>());
}

fn scalar_json(value: &plotx_data::ScalarValue) -> serde_json::Value {
    use plotx_data::ScalarValue;
    match value {
        ScalarValue::Null => serde_json::Value::Null,
        ScalarValue::Boolean(value) => serde_json::json!(value),
        ScalarValue::Int64(value) => serde_json::json!(value),
        ScalarValue::Float64(value) if value.is_nan() => serde_json::json!({"float": "NaN"}),
        ScalarValue::Float64(value) if *value == f64::INFINITY => {
            serde_json::json!({"float": "+Inf"})
        }
        ScalarValue::Float64(value) if *value == f64::NEG_INFINITY => {
            serde_json::json!({"float": "-Inf"})
        }
        ScalarValue::Float64(value) => serde_json::json!(value),
        ScalarValue::Utf8(value) => serde_json::json!(value),
        ScalarValue::Categorical(value) => serde_json::json!({"category_index": value}),
        ScalarValue::Date(value) => serde_json::json!({"date_days": value}),
        ScalarValue::Time(value) => serde_json::json!({"time_ns": value}),
        ScalarValue::Timestamp(value) => serde_json::json!({"timestamp_ns_utc": value}),
        ScalarValue::Duration(value) => serde_json::json!({"duration_ns": value}),
        ScalarValue::Extension { type_id, storage } => {
            serde_json::json!({"extension": type_id, "storage": scalar_json(storage)})
        }
    }
}

fn finite_slice(values: &[f64], limit: usize) -> Vec<serde_json::Value> {
    values
        .iter()
        .take(limit)
        .copied()
        .map(finite_json)
        .collect()
}

fn finite_json(value: f64) -> serde_json::Value {
    if value.is_finite() {
        serde_json::json!(value)
    } else {
        serde_json::Value::Null
    }
}

fn top_ref(id: &str, kind: &str) -> ResourceRef {
    ResourceRef {
        id: id.to_owned(),
        kind: ResourceKindId::new(kind),
        parent_id: None,
        local_id: None,
    }
}

fn child_ref(parent: &str, local: &str, kind: &str) -> ResourceRef {
    ResourceRef {
        id: format!("{parent}/{local}"),
        kind: ResourceKindId::new(kind),
        parent_id: Some(parent.to_owned()),
        local_id: Some(local.to_owned()),
    }
}

fn cap(id: &str) -> CapabilityId {
    CapabilityId::new(id)
}
