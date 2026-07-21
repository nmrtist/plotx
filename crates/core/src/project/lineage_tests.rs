use super::tests::{synthetic_1d, temp_project};
use super::*;
use crate::state::{
    FloatSeries, TableImportSource, TableMetric, TableProvenance, materialized_float_series_table,
};

#[test]
fn project_roundtrip_maps_multi_source_lineage_by_data_id() {
    let mut app = PlotxApp::new();
    app.doc
        .datasets
        .push(Dataset::Nmr(Box::new(NmrDataset::load(synthetic_1d()))));
    app.doc
        .datasets
        .push(Dataset::Nmr(Box::new(NmrDataset::load(synthetic_1d()))));
    let mut derived = Dataset::Nmr(Box::new(NmrDataset::load(synthetic_1d())));
    derived.set_lineage(Some(DatasetLineage::new(
        DerivationKind::SpectrumArithmetic,
        [1, 0, 1],
    )));
    app.doc.datasets.push(derived);

    let path = temp_project("lineage");
    let _ = std::fs::remove_file(&path);
    save_project(&app, &path, false).unwrap();
    let loaded = load_project(&path).unwrap();
    let _ = std::fs::remove_file(&path);

    assert_eq!(
        loaded.doc.datasets[2].lineage(),
        Some(&DatasetLineage::new(
            DerivationKind::SpectrumArithmetic,
            [1, 0]
        ))
    );
}

#[test]
fn provenance_without_explicit_v1_lineage_stays_unlinked() {
    let mut app = PlotxApp::new();
    app.doc
        .datasets
        .push(Dataset::Nmr(Box::new(NmrDataset::load(synthetic_1d()))));
    let source_resource = app.doc.datasets[0].resource_id().to_owned();
    let mut table = materialized_float_series_table(
        ("x".into(), "".into(), vec![Some(0.0)]),
        Vec::new(),
        "plotx.test.provenance-table.v1",
    )
    .unwrap();
    table.provenance = Some(TableProvenance {
        source_resource,
        regions: vec![(1.0, 2.0)],
        metric: TableMetric::PeakHeight,
    });
    app.doc.datasets.push(Dataset::Table(Box::new(table)));

    let path = temp_project("legacy_region_lineage");
    let _ = std::fs::remove_file(&path);
    save_project(&app, &path, false).unwrap();
    let loaded = load_project(&path).unwrap();
    let _ = std::fs::remove_file(&path);
    assert_eq!(loaded.doc.datasets[1].lineage(), None);
}

#[test]
fn lineage_resolution_rejects_missing_self_and_cycles() {
    let datasets = || {
        vec![
            Dataset::Nmr(Box::new(NmrDataset::load(synthetic_1d()))),
            Dataset::Nmr(Box::new(NmrDataset::load(synthetic_1d()))),
        ]
    };
    let binding = |data: &str, sources: &[&str]| DatasetBinding {
        data: data.to_owned(),
        recipe: format!("recipe_{data}"),
        derivation: (!sources.is_empty()).then(|| DerivationDto {
            kind: "slice".to_owned(),
            sources: sources.iter().map(|source| (*source).to_owned()).collect(),
        }),
    };
    let map = HashMap::from([("data_a".to_owned(), 0), ("data_b".to_owned(), 1)]);

    let mut missing_data = datasets();
    let missing = vec![binding("data_a", &[]), binding("data_b", &["data_missing"])];
    assert!(resolve_dataset_lineage(&mut missing_data, &missing, &map).is_err());

    let mut self_data = datasets();
    let self_ref = vec![binding("data_a", &["data_a"]), binding("data_b", &[])];
    assert!(resolve_dataset_lineage(&mut self_data, &self_ref, &map).is_err());

    let mut cycle_data = datasets();
    let cycle = vec![
        binding("data_a", &["data_b"]),
        binding("data_b", &["data_a"]),
    ];
    assert!(resolve_dataset_lineage(&mut cycle_data, &cycle, &map).is_err());
}

#[test]
fn v1_dataset_binding_without_derivation_deserializes() {
    let binding: DatasetBinding =
        serde_json::from_str(r#"{"data":"data_000000","recipe":"recipe_000000"}"#).unwrap();
    assert!(binding.derivation.is_none());
}

#[test]
fn v1_table_roundtrip_preserves_units_missing_uncertainty_and_lineage() {
    let mut app = PlotxApp::new();
    app.doc
        .datasets
        .push(Dataset::Nmr(Box::new(NmrDataset::load(synthetic_1d()))));
    let mut table = materialized_float_series_table(
        ("Time".into(), "s".into(), vec![Some(0.0), Some(1.0)]),
        vec![FloatSeries {
            name: "Signal".to_owned(),
            unit: String::new(),
            values: vec![Some(2.0), None],
            uncertainty: Some(vec![Some(0.1), None]),
            fit: None,
        }],
        "plotx.test.lineage-table.v1",
    )
    .unwrap();
    let revision_id = table.typed_state.envelope.revision.id;
    let mut source = TableImportSource::new(
        std::sync::Arc::<[u8]>::from(&b"Time,Signal\n0,2\n1,\n"[..]),
        "text/csv",
    );
    source.name = Some("measurements.csv".into());
    table.import_sources.push(source);
    table.lineage = Some(DatasetLineage::new(
        DerivationKind::WindowStatisticsTable,
        [0],
    ));
    app.doc.datasets.push(Dataset::Table(Box::new(table)));

    let path = temp_project("table_boundaries");
    let _ = std::fs::remove_file(&path);
    save_project(&app, &path, false).unwrap();
    let loaded = load_project(&path).unwrap();
    std::fs::remove_file(path).unwrap();

    let Dataset::Table(table) = &loaded.doc.datasets[1] else {
        panic!("expected a data table");
    };
    let x = table.x_binding.unwrap();
    let binding = &table.series_bindings[0];
    let uncertainty = binding.uncertainty_column.unwrap();
    assert_eq!(
        table
            .typed_state
            .envelope
            .revision
            .snapshot
            .schema
            .column(x)
            .unwrap()
            .unit
            .as_ref()
            .unwrap()
            .display_unit,
        "s"
    );
    assert_eq!(table.typed_state.envelope.revision.id, revision_id);
    let values = table
        .typed_rows(2, &[binding.value_column, uncertainty])
        .unwrap();
    assert_eq!(values.columns[0].values[1], plotx_data::ScalarValue::Null);
    assert_eq!(values.columns[1].values[1], plotx_data::ScalarValue::Null);
    assert_eq!(table.import_sources.len(), 1);
    assert_eq!(
        table.import_sources[0].name.as_deref(),
        Some("measurements.csv")
    );
    assert_eq!(table.import_sources[0].bytes(), b"Time,Signal\n0,2\n1,\n");
    assert_eq!(
        table.lineage,
        Some(DatasetLineage::new(
            DerivationKind::WindowStatisticsTable,
            [0]
        ))
    );
}
