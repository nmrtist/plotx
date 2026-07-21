use std::collections::BTreeMap;

use plotx_data::{CodecRegistry, LogicalType, MemoryBlockStore, ScalarValue, SnapshotReader};
use plotx_io::origin::{
    OriginByteOrder, OriginCell, OriginColumn, OriginColumnType, OriginDiagnostic,
    OriginDiagnosticCode, OriginDiagnosticSeverity, OriginFormat, OriginHeaderVersion,
    OriginLimits, OriginMetadataEntry, OriginNote, OriginObjectLocation, OriginProbe,
    OriginProfile, OriginProject, OriginResourceUsage, OriginSupport,
    OriginUnsupportedObjectSummary, OriginWorkbook, OriginWorksheet,
};

use crate::origin::{ORIGIN_IMPORT_OPERATION, OriginImportError, import_origin_project};

fn probe() -> OriginProbe {
    OriginProbe {
        format: OriginFormat::Opj,
        raw_version: "4.2673 552".to_owned(),
        version: OriginHeaderVersion {
            major: 4,
            minor: 2673,
            build: 552,
        },
        byte_order: OriginByteOrder::LittleEndian,
        profile: Some(OriginProfile::Origin7V552),
        support: OriginSupport::Supported,
    }
}

fn column(name: &str, column_type: OriginColumnType, cells: Vec<OriginCell>) -> OriginColumn {
    OriginColumn {
        name: name.to_owned(),
        long_name: None,
        role: None,
        units: None,
        comments: None,
        column_type,
        cells,
    }
}

fn worksheet(name: &str, row_count: usize, columns: Vec<OriginColumn>) -> OriginWorksheet {
    OriginWorksheet {
        name: name.to_owned(),
        columns,
        row_count,
        metadata: Vec::new(),
    }
}

fn project(workbooks: Vec<OriginWorkbook>) -> OriginProject {
    OriginProject {
        probe: probe(),
        parameters: Vec::new(),
        notes: Vec::new(),
        workbooks,
        diagnostics: Vec::new(),
        unsupported_objects: Vec::new(),
        resource_usage: OriginResourceUsage::default(),
    }
}

fn workbook(name: &str, worksheets: Vec<OriginWorksheet>) -> OriginWorkbook {
    OriginWorkbook {
        name: name.to_owned(),
        worksheets,
    }
}

fn imported(
    project: OriginProject,
) -> (
    Vec<crate::origin::ImportedOriginWorksheet>,
    MemoryBlockStore,
    CodecRegistry,
) {
    let store = MemoryBlockStore::default();
    let codecs = CodecRegistry::with_arrow_ipc();
    let imported = import_origin_project(project, &store, &codecs, OriginLimits::default())
        .expect("test project should convert");
    (imported, store, codecs)
}

#[test]
fn exposes_the_stable_origin_import_operation() {
    assert_eq!(ORIGIN_IMPORT_OPERATION, "plotx.import.origin.v1");
}

#[test]
fn converts_supported_types_and_pads_short_columns_with_nulls() {
    let columns = vec![
        column(
            "double",
            OriginColumnType::Float,
            vec![OriginCell::Float(1.25), OriginCell::Null],
        ),
        column(
            "float",
            OriginColumnType::Float,
            vec![OriginCell::Float(f32::from_bits(0x43ac_cccd) as f64)],
        ),
        column(
            "integer",
            OriginColumnType::Integer,
            vec![OriginCell::Integer(-1000), OriginCell::Integer(34)],
        ),
        column(
            "text",
            OriginColumnType::Text,
            vec![OriginCell::Text("alpha".to_owned()), OriginCell::Null],
        ),
    ];
    let (imported, store, codecs) = imported(project(vec![workbook(
        "Book1",
        vec![worksheet("Sheet1", 2, columns)],
    )]));

    assert_eq!(imported.len(), 1);
    assert_eq!(imported[0].name, "Book1 / Sheet1");
    assert_eq!(imported[0].snapshot.row_count, 2);
    assert_eq!(
        imported[0]
            .snapshot
            .schema
            .columns
            .iter()
            .map(|column| column.logical_type.clone())
            .collect::<Vec<_>>(),
        vec![
            LogicalType::Float64,
            LogicalType::Float64,
            LogicalType::Int64,
            LogicalType::Utf8,
        ]
    );
    assert!(
        imported[0].snapshot.schema.columns[3].unit.is_none(),
        "text columns must never acquire a numeric unit"
    );

    let batch = SnapshotReader::new(&imported[0].snapshot, &store, &codecs)
        .unwrap()
        .read_batch(0, &[])
        .unwrap();
    assert_eq!(
        batch.columns[0].1.value(0),
        Some(ScalarValue::Float64(1.25))
    );
    assert_eq!(batch.columns[0].1.value(1), Some(ScalarValue::Null));
    assert_eq!(
        batch.columns[1].1.value(0),
        Some(ScalarValue::Float64(f32::from_bits(0x43ac_cccd) as f64))
    );
    assert_eq!(batch.columns[1].1.value(1), Some(ScalarValue::Null));
    assert_eq!(batch.columns[2].1.value(0), Some(ScalarValue::Int64(-1000)));
    assert_eq!(
        batch.columns[3].1.value(0),
        Some(ScalarValue::Utf8("alpha".to_owned()))
    );
    assert_eq!(batch.columns[3].1.value(1), Some(ScalarValue::Null));
}

#[test]
fn converts_mixed_cells_without_dropping_numbers() {
    let mixed_number = "3.14".parse::<f64>().unwrap();
    let mixed = column(
        "mixed",
        OriginColumnType::Mixed,
        vec![
            OriginCell::Text("text".to_owned()),
            OriginCell::Float(mixed_number),
            OriginCell::Integer(-7),
            OriginCell::Null,
        ],
    );
    let (imported, store, codecs) = imported(project(vec![workbook(
        "Book",
        vec![worksheet("Sheet", 4, vec![mixed])],
    )]));

    assert_eq!(
        imported[0].snapshot.schema.columns[0].logical_type,
        LogicalType::Utf8
    );
    let batch = SnapshotReader::new(&imported[0].snapshot, &store, &codecs)
        .unwrap()
        .read_batch(0, &[])
        .unwrap();
    assert_eq!(
        (0..4)
            .map(|row| batch.columns[0].1.value(row))
            .collect::<Vec<_>>(),
        vec![
            Some(ScalarValue::Utf8("text".to_owned())),
            Some(ScalarValue::Utf8("3.14".to_owned())),
            Some(ScalarValue::Utf8("-7".to_owned())),
            Some(ScalarValue::Null),
        ]
    );
}

#[test]
fn generates_empty_names_and_disambiguates_exact_duplicates_case_sensitively() {
    let names = ["", "", "name", "name", "name", "Name"];
    let columns = names
        .into_iter()
        .map(|name| {
            column(
                name,
                OriginColumnType::Integer,
                vec![OriginCell::Integer(1)],
            )
        })
        .collect();
    let (imported, _, _) = imported(project(vec![workbook(
        "Book",
        vec![worksheet("Sheet", 1, columns)],
    )]));

    assert_eq!(
        imported[0]
            .snapshot
            .schema
            .columns
            .iter()
            .map(|column| column.name.as_str())
            .collect::<Vec<_>>(),
        [
            "Column 1", "Column 2", "name", "name (2)", "name (3)", "Name"
        ]
    );
    let source_columns = imported[0].source_metadata["space.nmrtist.plotx.import.origin.columns"]
        .as_array()
        .unwrap();
    assert_eq!(source_columns[0]["source_name"], "");
    assert_eq!(source_columns[0]["imported_name"], "Column 1");
    assert_eq!(source_columns[3]["source_name"], "name");
    assert_eq!(source_columns[3]["imported_name"], "name (2)");
    assert_eq!(
        imported[0].snapshot.schema.columns[3].metadata["space.nmrtist.plotx.import.origin.original_name"],
        "name"
    );
}

#[test]
fn creates_one_candidate_for_each_nonempty_worksheet() {
    let value = || column("x", OriginColumnType::Float, vec![OriginCell::Float(1.0)]);
    let project = project(vec![
        workbook(
            "Book1",
            vec![
                worksheet("Empty", 0, Vec::new()),
                worksheet("Data", 1, vec![value()]),
            ],
        ),
        workbook("Book2", vec![worksheet("More", 1, vec![value()])]),
    ]);
    let (imported, _, _) = imported(project);

    assert_eq!(
        imported
            .iter()
            .map(|item| item.name.as_str())
            .collect::<Vec<_>>(),
        ["Book1 / Data", "Book2 / More"]
    );
    assert_eq!(
        imported[0].resource_usage.total_owned_bytes,
        imported[1].resource_usage.total_owned_bytes
    );
    assert_eq!(
        imported[0].source_metadata["space.nmrtist.plotx.import.origin.resource_usage"]["total_owned_bytes"],
        imported[1].resource_usage.total_owned_bytes
    );
}

#[test]
fn preserves_project_column_and_parser_metadata() {
    let mut data = column(
        "signal",
        OriginColumnType::Float,
        vec![OriginCell::Float(2.0)],
    );
    data.long_name = Some("Detector signal".to_owned());
    data.role = Some("Y".to_owned());
    data.units = Some("mV".to_owned());
    data.comments = Some("calibrated".to_owned());
    let mut sheet = worksheet("Sheet1", 1, vec![data]);
    sheet.metadata = vec![OriginMetadataEntry {
        key: "layer".to_owned(),
        value: "raw".to_owned(),
    }];
    let mut project = project(vec![workbook("Book1", vec![sheet])]);
    project.parameters = vec![OriginMetadataEntry {
        key: "temperature".to_owned(),
        value: "298".to_owned(),
    }];
    project.notes = vec![OriginNote {
        name: "Methods".to_owned(),
        content: "Prepared under nitrogen.".to_owned(),
    }];
    project.diagnostics = vec![OriginDiagnostic {
        code: OriginDiagnosticCode::UnsupportedObjectSkipped,
        severity: OriginDiagnosticSeverity::Warning,
        location: Some(OriginObjectLocation {
            workbook: Some("Book1".to_owned()),
            worksheet: Some("Sheet1".to_owned()),
            column: None,
            byte_offset: Some(42),
        }),
        message: "PlotX skipped a graph.".to_owned(),
    }];
    project.unsupported_objects = vec![OriginUnsupportedObjectSummary {
        kind: "graphs".to_owned(),
        count: 1,
    }];
    project.resource_usage = OriginResourceUsage {
        input_bytes: 100,
        parser_bytes: 50,
        decoded_text_bytes: 20,
        total_owned_bytes: 150,
        workbooks: 1,
        worksheets: 1,
        columns: 1,
        cells: 1,
        metadata_records: 3,
    };
    let (imported, _, _) = imported(project);
    let item = &imported[0];

    assert_eq!(item.diagnostics.len(), 1);
    assert!(item.resource_usage.total_owned_bytes > 150);
    assert_eq!(
        item.source_metadata["space.nmrtist.plotx.import.format"],
        "opj"
    );
    assert_eq!(
        item.source_metadata["space.nmrtist.plotx.import.origin.producer_version"],
        "4.2673 552"
    );
    assert_eq!(
        item.source_metadata["space.nmrtist.plotx.import.origin.parameters"][0]["key"],
        "temperature"
    );
    assert_eq!(
        item.source_metadata["space.nmrtist.plotx.import.origin.notes"][0]["content"],
        "Prepared under nitrogen."
    );
    assert_eq!(
        item.source_metadata["space.nmrtist.plotx.import.origin.unsupported_objects"][0]["kind"],
        "graphs"
    );
    assert_eq!(
        item.source_metadata["space.nmrtist.plotx.import.origin.resource_usage"]["total_owned_bytes"],
        item.resource_usage.total_owned_bytes
    );
    assert_eq!(
        item.source_metadata["space.nmrtist.plotx.import.origin.diagnostics"][0]["message"],
        "PlotX skipped a graph."
    );
    assert_eq!(
        item.snapshot.metadata["space.nmrtist.plotx.import.origin.workbook"],
        "Book1"
    );
    assert_eq!(
        item.snapshot.metadata["space.nmrtist.plotx.import.origin.worksheet"],
        "Sheet1"
    );
    assert_eq!(
        item.snapshot.metadata["space.nmrtist.plotx.import.origin.diagnostics"][0]["message"],
        "PlotX skipped a graph."
    );
    let schema_metadata = &item.snapshot.schema.columns[0].metadata;
    assert_eq!(
        schema_metadata["space.nmrtist.plotx.import.origin.long_name"],
        "Detector signal"
    );
    assert_eq!(
        schema_metadata["space.nmrtist.plotx.import.origin.role"],
        "Y"
    );
    assert_eq!(
        schema_metadata["space.nmrtist.plotx.import.origin.units"],
        "mV"
    );
    assert_eq!(
        schema_metadata["space.nmrtist.plotx.import.origin.comments"],
        "calibrated"
    );
}

#[test]
fn rejects_empty_projects_and_projects_with_only_empty_worksheets() {
    let store = MemoryBlockStore::default();
    let codecs = CodecRegistry::with_arrow_ipc();
    for project in [
        project(Vec::new()),
        project(vec![workbook(
            "Book",
            vec![worksheet("Empty", 0, Vec::new())],
        )]),
    ] {
        assert!(matches!(
            import_origin_project(project, &store, &codecs, OriginLimits::default()),
            Err(OriginImportError::NoSupportedWorksheet)
        ));
    }
}

#[test]
fn rejects_snapshot_capacity_before_writing_any_blocks() {
    let mut project = project(vec![workbook(
        "Book",
        vec![worksheet(
            "Sheet",
            1,
            vec![column(
                "value",
                OriginColumnType::Float,
                vec![OriginCell::Float(1.0)],
            )],
        )],
    )]);
    project.resource_usage.input_bytes = 1;
    project.resource_usage.total_owned_bytes = 1;
    let limits = OriginLimits {
        max_total_owned_bytes: 1,
        ..OriginLimits::default()
    };
    let store = MemoryBlockStore::default();
    let codecs = CodecRegistry::with_arrow_ipc();

    let error = import_origin_project(project, &store, &codecs, limits).unwrap_err();

    assert!(matches!(
        error,
        OriginImportError::LimitExceeded {
            resource: "total owned bytes",
            limit: 1,
            actual: _,
        }
    ));
    assert_eq!(store.block_count(), 0);
}

#[test]
fn rejects_declared_cell_type_mismatches_without_panicking() {
    let invalid = project(vec![workbook(
        "Book",
        vec![worksheet(
            "Sheet",
            1,
            vec![column(
                "value",
                OriginColumnType::Float,
                vec![OriginCell::Text("not a float".to_owned())],
            )],
        )],
    )]);
    let store = MemoryBlockStore::default();
    let codecs = CodecRegistry::with_arrow_ipc();
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        import_origin_project(invalid, &store, &codecs, OriginLimits::default())
    }));

    assert!(result.is_ok());
    assert!(matches!(
        result.unwrap(),
        Err(OriginImportError::InvalidCellType {
            column,
            row: 0,
            expected: OriginColumnType::Float,
            ..
        }) if column == "value"
    ));
}

#[test]
fn streams_large_worksheets_in_fixed_size_batches() {
    const ROWS: usize = 65_537;
    let values = (0..ROWS)
        .map(|value| OriginCell::Integer(value as i64))
        .collect();
    let (imported, store, codecs) = imported(project(vec![workbook(
        "Book",
        vec![worksheet(
            "Sheet",
            ROWS,
            vec![column("index", OriginColumnType::Integer, values)],
        )],
    )]));

    assert_eq!(imported[0].snapshot.batch_count(), 2);
    let second = SnapshotReader::new(&imported[0].snapshot, &store, &codecs)
        .unwrap()
        .read_batch(1, &[])
        .unwrap();
    assert_eq!(second.row_ids.len(), 1);
    assert_eq!(
        second.columns[0].1.value(0),
        Some(ScalarValue::Int64(65_536))
    );
}

#[test]
fn generated_names_skip_existing_generated_and_suffixed_names() {
    let names = ["name", "name (2)", "name", "", "Column 4"];
    let columns = names
        .into_iter()
        .map(|name| {
            column(
                name,
                OriginColumnType::Integer,
                vec![OriginCell::Integer(1)],
            )
        })
        .collect();
    let (imported, _, _) = imported(project(vec![workbook(
        "Book",
        vec![worksheet("Sheet", 1, columns)],
    )]));

    assert_eq!(
        imported[0]
            .snapshot
            .schema
            .columns
            .iter()
            .map(|column| column.name.as_str())
            .collect::<Vec<_>>(),
        ["name", "name (2)", "name (3)", "Column 4", "Column 4 (2)"]
    );
}

#[test]
fn all_blank_column_names_receive_position_based_names() {
    let columns = ["", " ", "\t"]
        .into_iter()
        .map(|name| column(name, OriginColumnType::Text, vec![OriginCell::Null]))
        .collect();
    let (imported, _, _) = imported(project(vec![workbook(
        "Book",
        vec![worksheet("Sheet", 1, columns)],
    )]));

    assert_eq!(
        imported[0]
            .snapshot
            .schema
            .columns
            .iter()
            .map(|column| column.name.as_str())
            .collect::<Vec<_>>(),
        ["Column 1", "Column 2", "Column 3"]
    );
}

#[test]
fn source_metadata_is_a_direct_table_import_source_map() {
    let (imported, _, _) = imported(project(vec![workbook(
        "Book",
        vec![worksheet(
            "Sheet",
            1,
            vec![column(
                "value",
                OriginColumnType::Integer,
                vec![OriginCell::Integer(1)],
            )],
        )],
    )]));

    let _: &BTreeMap<String, serde_json::Value> = &imported[0].source_metadata;
}
