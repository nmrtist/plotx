use std::{collections::BTreeMap, sync::Arc};

use plotx_data::{
    CodecRegistry, ColumnChunk, ColumnId, ColumnSchema, LogicalType, MemoryBlockStore,
    RevisionReason, RowId, SnapshotBuilder, TableId, TableRevision, TableSchema, UncertaintyKind,
    UncertaintyMeaning, UncertaintyRelation, UnitSpec,
};

/// Exact bytes and source metadata retained for reproducible table imports.
/// Cloning a dataset shares the immutable bytes; project persistence stores
/// them once by content hash even when multiple tables use the same source.
#[derive(Clone, Debug, PartialEq)]
pub struct TableImportSource {
    bytes: Arc<[u8]>,
    pub media_type: String,
    pub name: Option<String>,
    pub metadata: BTreeMap<String, serde_json::Value>,
}

impl TableImportSource {
    pub fn new(bytes: impl Into<Arc<[u8]>>, media_type: impl Into<String>) -> Self {
        Self {
            bytes: bytes.into(),
            media_type: media_type.into(),
            name: None,
            metadata: BTreeMap::new(),
        }
    }

    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
}

/// Native typed snapshot retained behind the transitional numerical table
/// view. Consumers can migrate independently without losing text, null, or
/// logical-type information at import time.
#[derive(Clone)]
pub struct TypedTableState {
    pub envelope: plotx_data::TableEnvelopeV1,
    pub store: Arc<plotx_data::MemoryBlockStore>,
}

/// A fully parsed table import awaiting explicit user confirmation.
///
/// Parsing and type inference happen before this state is created, but the
/// document remains unchanged until the preview is committed. Keeping the
/// typed snapshot here also means confirmation cannot accidentally reparse a
/// different file or infer a different schema.
pub struct TableImportPreviewState {
    pub candidates: Vec<TableImportCandidate>,
    pub selected: usize,
    pub report: crate::operation::OperationReport<()>,
    pub recent_path: Option<std::path::PathBuf>,
}

pub struct TableImportCandidate {
    pub name: String,
    pub retained_sources: Vec<TableImportSource>,
    pub typed_state: TypedTableState,
    pub x_binding: Option<ColumnId>,
    pub series_bindings: Vec<super::TableSeriesBinding>,
}

impl TypedTableState {
    /// Materialize an in-process producer directly into the common typed
    /// snapshot boundary. Callers provide logical schemas and validity-aware
    /// arrays; no x/y-shaped compatibility object is involved.
    pub fn materialized(
        columns: Vec<(ColumnSchema, ColumnChunk)>,
        uncertainty: Vec<UncertaintyRelation>,
        operation_id: &str,
    ) -> plotx_data::Result<Self> {
        let row_count = columns.first().map_or(0, |(_, values)| values.len());
        if columns.iter().any(|(_, values)| values.len() != row_count) {
            return Err(plotx_data::DataError::InvalidArray(
                "materialized columns have different row counts".into(),
            ));
        }
        let table_id = TableId::new();
        let store = Arc::new(MemoryBlockStore::default());
        let codecs = CodecRegistry::with_arrow_ipc();
        let schema = TableSchema::new(columns.iter().map(|(schema, _)| schema.clone()).collect())?;
        let mut builder = SnapshotBuilder::new(table_id, schema, store.as_ref(), &codecs)?;
        builder.set_uncertainty(uncertainty)?;
        if row_count > 0 {
            let rows = (0..row_count).map(|_| RowId::new()).collect::<Vec<_>>();
            let values = columns
                .into_iter()
                .map(|(_, values)| values)
                .collect::<Vec<_>>();
            builder.push_batch(&rows, &values)?;
        }
        let revision = TableRevision::initial(
            builder.finish()?,
            RevisionReason::Import,
            operation_id,
            env!("CARGO_PKG_VERSION"),
        )?;
        Ok(Self {
            envelope: plotx_data::TableEnvelopeV1::new(revision),
            store,
        })
    }

    pub fn imported(
        snapshot: plotx_data::TableSnapshot,
        store: Arc<plotx_data::MemoryBlockStore>,
    ) -> plotx_data::Result<Self> {
        Self::imported_with_operation(snapshot, store, "plotx.import.delimited.v1")
    }

    pub fn imported_with_operation(
        snapshot: plotx_data::TableSnapshot,
        store: Arc<plotx_data::MemoryBlockStore>,
        operation_id: &str,
    ) -> plotx_data::Result<Self> {
        let revision = plotx_data::TableRevision::initial(
            snapshot,
            plotx_data::RevisionReason::Import,
            operation_id,
            env!("CARGO_PKG_VERSION"),
        )?;
        Ok(Self {
            envelope: plotx_data::TableEnvelopeV1::new(revision),
            store,
        })
    }

    pub fn execution_input(&self) -> plotx_data::Result<plotx_data::ExecutionInput> {
        plotx_data::ExecutionInput::snapshot(
            self.envelope.revision.snapshot.clone(),
            self.store.clone(),
        )
    }
}

impl super::TableDataset {
    /// Construct a dataset directly from aligned typed arrays and explicit
    /// presentation bindings. This is the common boundary for domain
    /// producers; the table model itself has no distinguished x/y columns.
    pub fn from_materialized(
        columns: Vec<(ColumnSchema, ColumnChunk)>,
        uncertainty: Vec<UncertaintyRelation>,
        x_binding: Option<ColumnId>,
        series_bindings: Vec<super::TableSeriesBinding>,
        operation_id: &str,
    ) -> plotx_data::Result<Self> {
        let typed_state = TypedTableState::materialized(columns, uncertainty, operation_id)?;
        let schema = &typed_state.envelope.revision.snapshot.schema;
        if x_binding.is_some_and(|column| schema.column(column).is_none())
            || series_bindings.iter().any(|binding| {
                schema.column(binding.value_column).is_none()
                    || binding
                        .uncertainty_column
                        .is_some_and(|column| schema.column(column).is_none())
            })
        {
            return Err(plotx_data::DataError::InvalidSchema(
                "presentation binding references an absent column".into(),
            ));
        }
        let mut dataset = Self::from_typed(typed_state);
        dataset.x_binding = x_binding;
        dataset.series_bindings = series_bindings;
        Ok(dataset)
    }

    /// Read at most `limit` rows from the immutable typed snapshot. An empty
    /// projection selects every column. This is the bounded common path for
    /// previews, automation and consumers that do not need a relation plan.
    pub fn typed_rows(
        &self,
        limit: usize,
        projection: &[ColumnId],
    ) -> Result<TypedTableRows, String> {
        let snapshot = &self.typed_state.envelope.revision.snapshot;
        let selected = if projection.is_empty() {
            snapshot
                .schema
                .columns
                .iter()
                .map(|column| column.id)
                .collect::<Vec<_>>()
        } else {
            projection.to_vec()
        };
        let schemas = selected
            .iter()
            .map(|column| {
                snapshot
                    .schema
                    .column(*column)
                    .cloned()
                    .ok_or_else(|| format!("Column {column} is absent from the table schema."))
            })
            .collect::<Result<Vec<_>, _>>()?;
        let mut result = TypedTableRows {
            row_ids: Vec::with_capacity(
                limit.min(usize::try_from(snapshot.row_count).unwrap_or(usize::MAX)),
            ),
            columns: schemas
                .into_iter()
                .map(|schema| TypedTableColumn {
                    schema,
                    values: Vec::new(),
                })
                .collect(),
            total_rows: snapshot.row_count,
        };
        if limit == 0 {
            return Ok(result);
        }
        let codecs = CodecRegistry::with_arrow_ipc();
        let reader =
            plotx_data::SnapshotReader::new(snapshot, self.typed_state.store.as_ref(), &codecs)
                .map_err(|error| error.to_string())?;
        for batch_index in 0..snapshot.batch_count() {
            let batch = reader
                .read_batch(batch_index, &selected)
                .map_err(|error| error.to_string())?;
            let take = (limit - result.row_ids.len()).min(batch.row_ids.len());
            result.row_ids.extend_from_slice(&batch.row_ids[..take]);
            for (target, (_, chunk)) in result.columns.iter_mut().zip(batch.columns) {
                target.values.extend(
                    (0..take).map(|row| chunk.value(row).unwrap_or(plotx_data::ScalarValue::Null)),
                );
            }
            if result.row_ids.len() == limit {
                break;
            }
        }
        Ok(result)
    }

    pub(crate) fn typed_plot_data(&self, max_points: usize) -> Result<TypedPlotData, String> {
        if max_points == 0 {
            return Err("Plot sample limit must be positive.".into());
        }
        let snapshot = &self.typed_state.envelope.revision.snapshot;
        let Some(x_column) = self.x_binding else {
            return Ok(TypedPlotData::default());
        };
        let x_schema = snapshot
            .schema
            .column(x_column)
            .ok_or_else(|| "The chart x binding is absent from the table schema.".to_owned())?;
        require_numeric_plot_column(x_schema)?;
        let mut projection = vec![x_column];
        for binding in &self.series_bindings {
            let schema = snapshot
                .schema
                .column(binding.value_column)
                .ok_or_else(|| {
                    format!(
                        "Chart column {} is absent from the schema.",
                        binding.value_column
                    )
                })?;
            require_numeric_plot_column(schema)?;
            projection.push(binding.value_column);
            if let Some(uncertainty) = binding.uncertainty_column {
                let schema = snapshot.schema.column(uncertainty).ok_or_else(|| {
                    format!("Uncertainty column {uncertainty} is absent from the schema.")
                })?;
                require_numeric_plot_column(schema)?;
                projection.push(uncertainty);
            }
        }
        let stride = snapshot.row_count.div_ceil(max_points as u64).max(1);
        let capacity = usize::try_from(snapshot.row_count)
            .unwrap_or(usize::MAX)
            .min(max_points);
        let mut plot = TypedPlotData {
            x_label: schema_axis_label(x_schema),
            x: Vec::with_capacity(capacity),
            series: self
                .series_bindings
                .iter()
                .map(|binding| TypedPlotSeries {
                    binding: binding.clone(),
                    name: snapshot
                        .schema
                        .column(binding.value_column)
                        .map(|column| column.name.clone())
                        .unwrap_or_default(),
                    y: Vec::with_capacity(capacity),
                    uncertainty: binding
                        .uncertainty_column
                        .map(|_| Vec::with_capacity(capacity)),
                })
                .collect(),
        };
        let codecs = CodecRegistry::with_arrow_ipc();
        let reader =
            plotx_data::SnapshotReader::new(snapshot, self.typed_state.store.as_ref(), &codecs)
                .map_err(|error| error.to_string())?;
        for batch_index in 0..snapshot.batch_count() {
            let batch = reader
                .read_batch(batch_index, &projection)
                .map_err(|error| error.to_string())?;
            for local in 0..batch.row_ids.len() {
                let position = batch.row_start + local as u64;
                if !position.is_multiple_of(stride) {
                    continue;
                }
                let mut chunks = batch.columns.iter();
                let x = chunks
                    .next()
                    .and_then(|(_, chunk)| chunk.value(local))
                    .ok_or_else(|| "Typed chart x chunk is incomplete.".to_owned())?;
                plot.x.push(plot_number(x)?);
                for series in &mut plot.series {
                    let value = chunks
                        .next()
                        .and_then(|(_, chunk)| chunk.value(local))
                        .ok_or_else(|| "Typed chart value chunk is incomplete.".to_owned())?;
                    series.y.push(plot_number(value)?);
                    if let Some(uncertainty) = &mut series.uncertainty {
                        let value = chunks
                            .next()
                            .and_then(|(_, chunk)| chunk.value(local))
                            .ok_or_else(|| {
                                "Typed chart uncertainty chunk is incomplete.".to_owned()
                            })?;
                        uncertainty.push(plot_number(value)?);
                    }
                }
            }
        }
        Ok(plot)
    }
}

#[derive(Default)]
pub(crate) struct TypedPlotData {
    pub x_label: String,
    pub x: Vec<f64>,
    pub series: Vec<TypedPlotSeries>,
}

pub struct TypedTableRows {
    pub row_ids: Vec<RowId>,
    pub columns: Vec<TypedTableColumn>,
    pub total_rows: u64,
}

pub struct TypedTableColumn {
    pub schema: ColumnSchema,
    pub values: Vec<plotx_data::ScalarValue>,
}

pub(crate) struct TypedPlotSeries {
    pub binding: super::TableSeriesBinding,
    pub name: String,
    pub y: Vec<f64>,
    pub uncertainty: Option<Vec<f64>>,
}

fn require_numeric_plot_column(schema: &ColumnSchema) -> Result<(), String> {
    matches!(
        schema.logical_type,
        LogicalType::Int64 | LogicalType::Float64
    )
    .then_some(())
    .ok_or_else(|| format!("Chart column {:?} is not numeric.", schema.name))
}

fn plot_number(value: plotx_data::ScalarValue) -> Result<f64, String> {
    match value {
        plotx_data::ScalarValue::Null => Ok(f64::NAN),
        plotx_data::ScalarValue::Int64(value) => Ok(value as f64),
        plotx_data::ScalarValue::Float64(value) => Ok(value),
        value => Err(format!(
            "Chart value {:?} is not numeric.",
            value.logical_type()
        )),
    }
}

fn schema_axis_label(schema: &ColumnSchema) -> String {
    schema.unit.as_ref().map_or_else(
        || schema.name.clone(),
        |unit| format!("{} ({})", schema.name, unit.display_unit),
    )
}

pub(crate) fn unit_from_label(unit: &str) -> Option<UnitSpec> {
    if unit.is_empty() {
        return None;
    }
    if let Ok(spec) = plotx_data::UnitRegistry::plotx_v1().resolve(unit) {
        return Some(spec);
    }
    let mut spec = UnitSpec::dimensionless(unit);
    spec.quantity = "domain_quantity".into();
    spec.canonical_unit = unit.into();
    spec.extension_id = Some(format!(
        "space.nmrtist.plotx.unit.{}",
        unit.chars()
            .map(|character| {
                if character.is_ascii_alphanumeric() {
                    character
                } else {
                    '_'
                }
            })
            .collect::<String>()
            .to_ascii_lowercase()
    ));
    Some(spec)
}

pub(crate) fn materialized_float_column(
    name: impl Into<String>,
    unit: &str,
    values: impl IntoIterator<Item = Option<f64>>,
) -> (ColumnSchema, ColumnChunk) {
    let chunk = ColumnChunk::optional_f64(values);
    let mut schema = ColumnSchema::new(name, LogicalType::Float64);
    schema.nullable = chunk.validity().null_count() != 0;
    schema.unit = unit_from_label(unit);
    (schema, chunk)
}

/// Convenience input for domain producers that emit numeric presentation
/// series. It is immediately compiled into ordinary typed columns and
/// uncertainty relations and is never persisted as a table model.
pub struct FloatSeries {
    pub name: String,
    pub unit: String,
    pub values: Vec<Option<f64>>,
    pub uncertainty: Option<Vec<Option<f64>>>,
    pub fit: Option<super::CurveFitReference>,
}

pub fn materialized_float_series_table(
    x: (String, String, Vec<Option<f64>>),
    series: Vec<FloatSeries>,
    operation_id: &str,
) -> plotx_data::Result<super::TableDataset> {
    let (mut x_schema, x_values) = materialized_float_column(x.0, &x.1, x.2);
    x_schema.role = plotx_data::SemanticRole::Custom("space.nmrtist.plotx.axis.x".into());
    let x_binding = x_schema.id;
    let mut columns = vec![(x_schema, x_values)];
    let mut bindings = Vec::with_capacity(series.len());
    let mut relations = Vec::new();
    for series in series {
        let (schema, values) = materialized_float_column(series.name, &series.unit, series.values);
        let value_column = schema.id;
        columns.push((schema, values));
        let uncertainty_column = if let Some(uncertainty) = series.uncertainty {
            let (mut schema, values) = materialized_float_column(
                format!("{} uncertainty", columns.last().unwrap().0.name),
                &series.unit,
                uncertainty,
            );
            schema.role = plotx_data::SemanticRole::Custom(
                "space.nmrtist.plotx.uncertainty.symmetric".into(),
            );
            let column = schema.id;
            columns.push((schema, values));
            relations.push(UncertaintyRelation {
                value: value_column,
                kind: UncertaintyKind::Symmetric {
                    column,
                    meaning: UncertaintyMeaning::MeasurementStandardDeviation,
                },
            });
            Some(column)
        } else {
            None
        };
        bindings.push(super::TableSeriesBinding {
            value_column,
            uncertainty_column,
            fit: series.fit,
        });
    }
    super::TableDataset::from_materialized(
        columns,
        relations,
        Some(x_binding),
        bindings,
        operation_id,
    )
}
