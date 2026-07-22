use super::*;
use std::path::PathBuf;

#[test]
fn mixed_columns_import_as_typed_text_without_being_discarded() {
    let mut app = PlotxApp::new();
    let input = "x,mixed,note\n0,1,ok\n1,oops,review\n";
    import_delimited_text(&mut app, input, DelimitedTableSource::Clipboard);
    assert!(app.doc.datasets.is_empty());
    assert!(commit_table_import_preview(&mut app));
    let table = app.doc.datasets[0].as_table().unwrap();
    assert!(table.series_bindings.is_empty());
    assert_eq!(table.import_sources[0].bytes(), input.as_bytes());
    let typed = &table.typed_state;
    assert_eq!(typed.envelope.revision.snapshot.schema.columns.len(), 3);
    assert_eq!(
        typed.envelope.revision.snapshot.schema.columns[1].logical_type,
        plotx_core::data::LogicalType::Utf8
    );
    let codecs = plotx_core::data::CodecRegistry::with_arrow_ipc();
    let batch = plotx_core::data::SnapshotReader::new(
        &typed.envelope.revision.snapshot,
        typed.store.as_ref(),
        &codecs,
    )
    .unwrap()
    .read_batch(0, &[])
    .unwrap();
    assert_eq!(
        batch.columns[1].1.value(1),
        Some(plotx_core::data::ScalarValue::Utf8("oops".into()))
    );

    let original_fingerprint = typed.envelope.revision.snapshot.fingerprint;
    let delta = {
        let table = app.doc.datasets[0].as_table().unwrap();
        let preview = table.typed_rows(1, &[]).unwrap();
        let row = preview.row_ids[0];
        let column = preview.columns[0].schema.id;
        let mut delta = plotx_core::state::TableEditDelta::new_dataset(table);
        delta.record_typed_value(
            row,
            column,
            plotx_core::data::LiteralValue::Int64(0),
            plotx_core::data::LiteralValue::Int64(5),
        );
        delta.finish_dataset(table);
        assert!(delta.typed_diagnostic.is_none());
        delta
    };
    app.execute_action(plotx_core::actions::Action::edit_table(0, delta));
    let edited_fingerprint = app.doc.datasets[0]
        .as_table()
        .unwrap()
        .typed_state
        .envelope
        .revision
        .snapshot
        .fingerprint;
    assert_ne!(edited_fingerprint, original_fingerprint);
    app.undo();
    assert_eq!(typed_fingerprint(&app), original_fingerprint);
    app.redo();
    assert_eq!(typed_fingerprint(&app), edited_fingerprint);

    let path = std::env::temp_dir().join(format!(
        "plotx-mixed-typed-import-{}.plotx",
        uuid::Uuid::new_v4()
    ));
    plotx_core::project::save_project(&app, &path, false).unwrap();
    let loaded = plotx_core::project::load_project(&path).unwrap();
    std::fs::remove_file(path).unwrap();
    let loaded = loaded.doc.datasets[0].as_table().unwrap();
    assert_eq!(loaded.import_sources[0].bytes(), input.as_bytes());
    assert_eq!(
        loaded
            .typed_state
            .envelope
            .revision
            .snapshot
            .schema
            .columns
            .len(),
        3
    );
}

fn typed_fingerprint(app: &PlotxApp) -> plotx_core::data::ContentHash {
    app.doc.datasets[0]
        .as_table()
        .unwrap()
        .typed_state
        .envelope
        .revision
        .snapshot
        .fingerprint
}

#[test]
fn clipboard_schema_restores_typed_contract_and_is_retained() {
    let id = plotx_core::data::ColumnId::new();
    let mut value =
        plotx_core::data::ColumnSchema::new("count", plotx_core::data::LogicalType::Int64);
    value.id = id;
    let label = plotx_core::data::ColumnSchema::new("label", plotx_core::data::LogicalType::Utf8);
    let contract = plotx_core::xlsx::PlotxDelimitedSchemaV1 {
        schema_version: 1,
        table_id: plotx_core::data::TableId::new(),
        schema: plotx_core::data::TableSchema::new(vec![value, label]).unwrap(),
        uncertainty: Vec::new(),
    };
    let schema_json = serde_json::to_string(&contract).unwrap();
    let mut app = PlotxApp::new();
    import_delimited_text_with_schema(
        &mut app,
        "count,label\n9007199254740993,exact\n",
        DelimitedTableSource::Clipboard,
        Some(&schema_json),
    );
    assert!(app.doc.datasets.is_empty());
    assert!(commit_table_import_preview(&mut app));

    let table = app.doc.datasets[0].as_table().unwrap();
    let snapshot = &table.typed_state.envelope.revision.snapshot;
    assert_eq!(snapshot.schema.columns[0].id, id);
    assert_eq!(
        snapshot.schema.columns[0].logical_type,
        plotx_core::data::LogicalType::Int64
    );
    assert_eq!(table.import_sources.len(), 2);
    assert_eq!(table.import_sources[1].bytes(), schema_json.as_bytes());
}

/// Every recent entry must reopen through the loader that imported it; a
/// delimited table routed to the acquisition loader fails on every click.
#[test]
fn recent_entries_route_to_their_import_path() {
    let file = |name: &str| PathBuf::from(format!("C:/data/{name}"));
    assert_eq!(
        recent_open_kind(&file("session.PLOTX")),
        RecentOpenKind::Project
    );
    assert_eq!(
        recent_open_kind(&file("results.csv")),
        RecentOpenKind::DelimitedTable
    );
    assert_eq!(
        recent_open_kind(&file("results.tsv")),
        RecentOpenKind::DelimitedTable
    );
    assert_eq!(
        recent_open_kind(&file("results.txt")),
        RecentOpenKind::DelimitedTable
    );
    assert_eq!(
        recent_open_kind(&file("results.XLSX")),
        RecentOpenKind::XlsxTable
    );
    assert_eq!(recent_open_kind(&file("run.abf")), RecentOpenKind::DataFile);
    assert_eq!(recent_open_kind(&file("fid")), RecentOpenKind::DataFile);
    assert_eq!(
        format!("{:?}", recent_open_kind(&file("project.opj"))),
        "OriginProject"
    );
    assert_eq!(
        format!("{:?}", recent_open_kind(&file("project.OPJU"))),
        "OriginProject"
    );
    assert_eq!(
        recent_open_kind(&std::env::temp_dir()),
        RecentOpenKind::Folder
    );

    let root = std::env::temp_dir().join(format!(
        "plotx-recent-open-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |duration| duration.as_nanos())
    ));
    std::fs::create_dir(&root).expect("create recent-open test directory");
    let csv_directory = root.join("sample.csv");
    let plotx_directory = root.join("sample.plotx");
    std::fs::create_dir(&csv_directory).expect("create CSV-named directory");
    std::fs::create_dir(&plotx_directory).expect("create PlotX-named directory");
    let kinds = (
        recent_open_kind(&csv_directory),
        recent_open_kind(&plotx_directory),
    );
    std::fs::remove_dir_all(&root).expect("remove recent-open test directory");
    assert_eq!(kinds, (RecentOpenKind::Folder, RecentOpenKind::Folder));
}
