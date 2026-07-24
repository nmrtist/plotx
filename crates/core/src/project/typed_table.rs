use super::*;
use plotx_data::{
    BlockStore, CodecRegistry, ColumnId, ContentHash, MemoryBlockStore, RawInputObject,
    SnapshotReader, TableEnvelopeV1,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

const TABLE_ENVELOPE_FILE: &str = "table-envelope-v1.json";

/// Write a typed table envelope and each of its referenced content-addressed
/// blocks into a PlotX project archive. `written_blocks` deduplicates blocks
/// across tables and revisions in the same save.
pub fn write_table_envelope_v1(
    zip: &mut zip::ZipWriter<File>,
    options: SimpleFileOptions,
    object_id: &str,
    envelope: &TableEnvelopeV1,
    store: &dyn BlockStore,
    written_blocks: &mut BTreeSet<ContentHash>,
) -> Result<String> {
    envelope.validate_structure()?;
    let envelope_path = format!("objects/{object_id}/{TABLE_ENVELOPE_FILE}");
    write_json(zip, options, &envelope_path, envelope)?;
    for hash in envelope.referenced_blocks() {
        if written_blocks.insert(hash) {
            let bytes = store.get(hash)?;
            if ContentHash::of(&bytes) != hash {
                return Err(ProjectError::Invalid(format!(
                    "typed table block {hash} failed byte-hash validation before save"
                )));
            }
            write_bytes(zip, options, &block_path(hash), &bytes)?;
        }
    }
    Ok(envelope_path)
}

/// Load a typed table envelope and only the blocks reachable from that exact
/// revision. Unknown codecs and missing/corrupt blocks fail before callers can
/// expose a partially recovered table.
pub fn read_table_envelope_v1(
    zip: &mut zip::ZipArchive<File>,
    envelope_path: &str,
    _understood_extensions: &BTreeSet<String>,
    codecs: &CodecRegistry,
) -> Result<(TableEnvelopeV1, MemoryBlockStore)> {
    let envelope: TableEnvelopeV1 = read_json(zip, envelope_path)?;
    // Opening a project is a preservation operation. Unknown semantic
    // extensions remain intact; execution paths call `validate` with their
    // actual registry before calculating with them.
    envelope.validate_structure()?;
    for revision in envelope
        .history
        .iter()
        .chain(std::iter::once(&envelope.revision))
    {
        for descriptor in revision.snapshot.row_id_chunks.iter().chain(
            revision
                .snapshot
                .columns
                .iter()
                .flat_map(|column| column.chunks.iter()),
        ) {
            codecs.get(&descriptor.codec)?;
        }
    }
    let store = MemoryBlockStore::default();
    for hash in envelope.referenced_blocks() {
        let bytes = read_bytes(zip, &block_path(hash)).map_err(|error| match error {
            ProjectError::Zip(zip::result::ZipError::FileNotFound) => {
                ProjectError::Invalid(format!("typed table block {hash} is missing"))
            }
            other => other,
        })?;
        if ContentHash::of(&bytes) != hash {
            return Err(ProjectError::Invalid(format!(
                "typed table block {hash} is corrupt"
            )));
        }
        let stored = store.put(bytes)?;
        if stored != hash {
            return Err(ProjectError::Invalid(format!(
                "typed table block {hash} changed while loading"
            )));
        }
    }
    for revision in envelope
        .history
        .iter()
        .chain(std::iter::once(&envelope.revision))
    {
        SnapshotReader::new(&revision.snapshot, &store, codecs)?.validate_business_keys()?;
    }
    Ok((envelope, store))
}

fn block_path(hash: ContentHash) -> String {
    format!("objects/table-blocks/{hash}.bin")
}

#[derive(Serialize, Deserialize)]
struct TableSidecarV1 {
    #[serde(default)]
    x_column: Option<ColumnId>,
    series: Vec<crate::state::TableSeriesBinding>,
    provenance: Option<crate::state::TableProvenance>,
    meta: crate::state::TableMeta,
    curve_fit_analyses: Vec<crate::state::StoredCurveFitAnalysis>,
    board_pos: [f32; 2],
    peaks: crate::state::PeakSet,
    line_fits: Vec<crate::state::StoredLineFit>,
    statistics: Vec<crate::state::StatAnalysis>,
}

pub(crate) fn table_dataset_to_v1(
    table: &crate::state::TableDataset,
    data_id: &str,
    recipe_id: &str,
) -> Result<(
    DataObject,
    RecipeObject,
    TableEnvelopeV1,
    std::sync::Arc<MemoryBlockStore>,
)> {
    let typed = &table.typed_state;
    typed.envelope.validate_structure()?;
    let mut envelope = typed.envelope.clone();
    let store = typed.store.clone();
    let snapshot = &envelope.revision.snapshot;
    if table
        .x_binding
        .is_some_and(|column| snapshot.schema.column(column).is_none())
    {
        return Err(ProjectError::Invalid(
            "table x binding is absent from its typed schema".into(),
        ));
    }
    for binding in &table.series_bindings {
        if snapshot.schema.column(binding.value_column).is_none()
            || binding
                .uncertainty_column
                .is_some_and(|column| snapshot.schema.column(column).is_none())
        {
            return Err(ProjectError::Invalid(
                "table series binding is absent from its typed schema".into(),
            ));
        }
    }
    if !table.import_sources.is_empty() {
        envelope.raw_inputs.clear();
        for source in &table.import_sources {
            let mut raw = RawInputObject::embed(
                source.bytes().to_vec(),
                source.media_type.clone(),
                source.name.clone(),
                store.as_ref(),
            )?;
            raw.metadata.clone_from(&source.metadata);
            envelope.raw_inputs.push(raw);
        }
    }
    let sidecar = TableSidecarV1 {
        x_column: table.x_binding,
        series: table.series_bindings.clone(),
        provenance: table.provenance.clone(),
        meta: table.meta,
        curve_fit_analyses: table.curve_fit_analyses.clone(),
        board_pos: table.board_pos,
        peaks: table.peaks.clone(),
        line_fits: table.line_fits.clone(),
        statistics: table.statistics.clone(),
    };
    let typed_shape = vec![
        usize::try_from(envelope.revision.snapshot.row_count).unwrap_or(usize::MAX),
        envelope.revision.snapshot.schema.columns.len(),
    ];
    let data = DataObject {
        id: data_id.to_owned(),
        role: "data".into(),
        classification: table_classification(),
        label: table.name.clone(),
        dimensions: Vec::new(),
        payload: Payload {
            storage: STORAGE_TABLE_V1.into(),
            blob: format!("objects/{data_id}/{TABLE_ENVELOPE_FILE}"),
            shape: typed_shape,
            domain: "table".into(),
        },
        extensions: serde_json::json!({ "plotx.table.v1": sidecar }),
    };
    let recipe = RecipeObject {
        id: recipe_id.to_owned(),
        role: "recipe".into(),
        classification: table_recipe_classification(),
        input: data_id.to_owned(),
        parameters: RecipeParameters::default(),
        extensions: serde_json::Value::Null,
    };
    Ok((data, recipe, envelope, store))
}

pub(crate) fn table_dataset_from_v1(
    zip: &mut zip::ZipArchive<File>,
    data: &DataObject,
) -> Result<crate::state::TableDataset> {
    let sidecar: TableSidecarV1 = serde_json::from_value(
        data.extensions
            .get("plotx.table.v1")
            .cloned()
            .ok_or_else(|| ProjectError::Invalid("typed table sidecar is missing".into()))?,
    )
    .map_err(|error| ProjectError::Invalid(format!("invalid typed table sidecar: {error}")))?;
    let codecs = CodecRegistry::with_arrow_ipc();
    let (envelope, store) =
        read_table_envelope_v1(zip, &data.payload.blob, &BTreeSet::new(), &codecs)?;
    let import_sources = envelope
        .raw_inputs
        .iter()
        .map(|raw| {
            let mut source = crate::state::TableImportSource::new(
                std::sync::Arc::<[u8]>::from(raw.read(&store)?),
                raw.media_type.clone(),
            );
            source.name.clone_from(&raw.name);
            source.metadata.clone_from(&raw.metadata);
            Ok(source)
        })
        .collect::<plotx_data::Result<Vec<_>>>()?;
    let mut dataset = crate::state::TableDataset {
        resource_id: data.id.parse().map_err(|_| {
            ProjectError::Invalid(format!("table has invalid stable id {}", data.id))
        })?,
        provenance: sidecar.provenance,
        meta: sidecar.meta,
        curve_fit_analyses: sidecar.curve_fit_analyses,
        x_binding: sidecar.x_column,
        series_bindings: sidecar.series.clone(),
        name: data.label.clone(),
        lineage: None,
        board_pos: sidecar.board_pos,
        peaks: sidecar.peaks,
        line_fits: sidecar.line_fits,
        next_line_fit_id: 0,
        statistics: sidecar.statistics,
        import_sources,
        typed_state: crate::state::TypedTableState {
            envelope,
            store: std::sync::Arc::new(store),
        },
        next_stat_id: 0,
    };
    dataset.next_line_fit_id = dataset
        .line_fits
        .iter()
        .map(|fit| fit.id.saturating_add(1))
        .max()
        .unwrap_or(0);
    dataset.next_stat_id = dataset
        .statistics
        .iter()
        .map(|analysis| analysis.id.saturating_add(1))
        .max()
        .unwrap_or(0);
    Ok(dataset)
}

#[cfg(test)]
mod tests {
    use super::*;
    use plotx_data::{
        ColumnChunk, ColumnSchema, ColumnValues, ExtensionBlock, LogicalType, RevisionReason,
        RowId, SnapshotBuilder, SnapshotReader, TableId, TableRevision, TableSchema, Validity,
    };

    #[test]
    fn typed_table_archive_round_trip_verifies_referenced_blocks() {
        let store = MemoryBlockStore::default();
        let codecs = CodecRegistry::with_arrow_ipc();
        let value = ColumnSchema::new("value", LogicalType::Float64);
        let schema = TableSchema::new(vec![value.clone()]).unwrap();
        let mut builder = SnapshotBuilder::new(TableId::new(), schema, &store, &codecs).unwrap();
        builder
            .push_batch(
                &[RowId::new(), RowId::new()],
                &[ColumnChunk::all_valid(ColumnValues::Float64(vec![
                    1.0, 2.0,
                ]))],
            )
            .unwrap();
        let revision = TableRevision::initial(
            builder.finish().unwrap(),
            RevisionReason::Import,
            "import.delimited.v1",
            "test",
        )
        .unwrap();
        let mut envelope = TableEnvelopeV1::new(revision);
        let mut child = TableRevision::initial(
            envelope.revision.snapshot.clone(),
            RevisionReason::ManualEdit,
            "patch.v1",
            "test",
        )
        .unwrap();
        child.parents = vec![envelope.revision.id];
        envelope.advance(child).unwrap();
        envelope.extensions.insert(
            "space.vendor.instrument".into(),
            ExtensionBlock {
                version: 1,
                semantics_critical: true,
                payload: serde_json::json!({"mode": "proprietary"}),
            },
        );
        envelope.raw_inputs.push(
            RawInputObject::embed(
                b"group,value\na,1\n".to_vec(),
                "text/csv",
                Some("source.csv".into()),
                &store,
            )
            .unwrap(),
        );

        let path =
            std::env::temp_dir().join(format!("plotx-typed-table-{}.zip", uuid::Uuid::new_v4()));
        let file = File::create(&path).unwrap();
        let mut writer = zip::ZipWriter::new(file);
        let options =
            SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        write_table_envelope_v1(
            &mut writer,
            options,
            "table-1",
            &envelope,
            &store,
            &mut BTreeSet::new(),
        )
        .unwrap();
        writer.finish().unwrap();

        let mut archive = zip::ZipArchive::new(File::open(&path).unwrap()).unwrap();
        let (loaded, loaded_store) = read_table_envelope_v1(
            &mut archive,
            "objects/table-1/table-envelope-v1.json",
            &BTreeSet::new(),
            &codecs,
        )
        .unwrap();
        let batch = SnapshotReader::new(&loaded.revision.snapshot, &loaded_store, &codecs)
            .unwrap()
            .read_batch(0, &[value.id])
            .unwrap();
        assert_eq!(
            batch.columns[0].1.value(1),
            Some(plotx_data::ScalarValue::Float64(2.0))
        );
        assert_eq!(
            loaded.raw_inputs[0].read(&loaded_store).unwrap(),
            b"group,value\na,1\n"
        );
        assert_eq!(loaded.history.len(), 1);
        assert_eq!(loaded.history[0].id, loaded.revision.parents[0]);
        assert!(loaded.extensions.contains_key("space.vendor.instrument"));
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn lazy_project_projection_does_not_materialize_snapshot_values() {
        let store = std::sync::Arc::new(MemoryBlockStore::default());
        let codecs = CodecRegistry::with_arrow_ipc();
        let x = ColumnSchema::new("time", LogicalType::Float64);
        let y = ColumnSchema::new("signal", LogicalType::Float64);
        let schema = TableSchema::new(vec![x.clone(), y.clone()]).unwrap();
        let mut builder =
            SnapshotBuilder::new(TableId::new(), schema, store.as_ref(), &codecs).unwrap();
        builder
            .push_batch(
                &[RowId::new(), RowId::new(), RowId::new()],
                &[
                    ColumnChunk::all_valid(ColumnValues::Float64(vec![0.0, 1.0, 2.0])),
                    ColumnChunk::new(
                        ColumnValues::Float64(vec![2.0, f64::NAN, 0.0]),
                        Validity::from_valid([true, true, false]),
                    )
                    .unwrap(),
                ],
            )
            .unwrap();
        let snapshot = builder.finish().unwrap();
        let typed = crate::state::TypedTableState::imported(snapshot, store).unwrap();
        let dataset = crate::state::TableDataset::from_typed(typed);
        let preview = dataset.typed_rows(3, &[x.id, y.id]).unwrap();
        assert_eq!(
            preview.columns[0].values,
            vec![
                plotx_data::ScalarValue::Float64(0.0),
                plotx_data::ScalarValue::Float64(1.0),
                plotx_data::ScalarValue::Float64(2.0),
            ]
        );
        assert_eq!(
            preview.columns[1].values[0],
            plotx_data::ScalarValue::Float64(2.0)
        );
        assert!(matches!(
            preview.columns[1].values[1],
            plotx_data::ScalarValue::Float64(value) if value.is_nan()
        ));
        assert_eq!(preview.columns[1].values[2], plotx_data::ScalarValue::Null);
        assert_eq!(preview.row_ids.len(), 3);
    }
}
