use plotx_data::{CodecRegistry, MemoryBlockStore, ScalarValue, SnapshotReader};
use plotx_io::origin::{
    OriginByteOrder, OriginCell, OriginColumn, OriginColumnType, OriginFormat, OriginHeaderVersion,
    OriginLimits, OriginProbe, OriginProfile, OriginProject, OriginResourceUsage, OriginSupport,
    OriginWorkbook, OriginWorksheet,
};

use crate::origin::{OriginImportError, import_origin_project};

const ORIGINAL_NAME_KEY: &str = "space.nmrtist.plotx.import.origin.original_name";

fn project(worksheets: Vec<OriginWorksheet>) -> OriginProject {
    let mut project = OriginProject {
        probe: OriginProbe {
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
        },
        parameters: Vec::new(),
        notes: Vec::new(),
        workbooks: vec![OriginWorkbook {
            name: "Book".to_owned(),
            worksheets,
        }],
        diagnostics: Vec::new(),
        unsupported_objects: Vec::new(),
        resource_usage: OriginResourceUsage::default(),
    };
    sync_resource_counts(&mut project);
    project
}

fn sync_resource_counts(project: &mut OriginProject) {
    project.resource_usage.workbooks = project.workbooks.len();
    project.resource_usage.worksheets = project
        .workbooks
        .iter()
        .map(|workbook| workbook.worksheets.len())
        .sum();
    project.resource_usage.columns = project
        .workbooks
        .iter()
        .flat_map(|workbook| &workbook.worksheets)
        .map(|worksheet| worksheet.columns.len())
        .sum();
    project.resource_usage.cells = project
        .workbooks
        .iter()
        .flat_map(|workbook| &workbook.worksheets)
        .flat_map(|worksheet| &worksheet.columns)
        .map(|column| column.cells.len())
        .sum();
}

fn worksheet(name: &str, columns: Vec<OriginColumn>) -> OriginWorksheet {
    OriginWorksheet {
        name: name.to_owned(),
        columns,
        row_count: 1,
        metadata: Vec::new(),
    }
}

fn columns(names: &[&str]) -> Vec<OriginColumn> {
    names
        .iter()
        .map(|name| OriginColumn {
            name: (*name).to_owned(),
            long_name: None,
            role: None,
            units: None,
            comments: None,
            column_type: OriginColumnType::Integer,
            cells: vec![OriginCell::Integer(1)],
        })
        .collect()
}

fn imported_names(names: &[&str]) -> Vec<String> {
    let imported = import_origin_project(
        project(vec![worksheet("Sheet", columns(names))]),
        &MemoryBlockStore::default(),
        &CodecRegistry::with_arrow_ipc(),
        OriginLimits::default(),
    )
    .expect("test project should convert");
    imported[0]
        .snapshot
        .schema
        .columns
        .iter()
        .map(|column| column.name.clone())
        .collect()
}

#[test]
fn generated_duplicate_names_do_not_steal_later_source_names() {
    assert_eq!(
        imported_names(&["name", "name", "name (2)"]),
        ["name", "name (3)", "name (2)"]
    );
}

#[test]
fn generated_blank_names_do_not_steal_real_position_names() {
    assert_eq!(
        imported_names(&["", "Column 1"]),
        ["Column 1 (2)", "Column 1"]
    );
    assert_eq!(imported_names(&["Column 1", ""]), ["Column 1", "Column 2"]);
}

#[test]
fn every_changed_name_retains_its_source_name() {
    let imported = import_origin_project(
        project(vec![worksheet(
            "Sheet",
            columns(&["name", "name", "name (2)", ""]),
        )]),
        &MemoryBlockStore::default(),
        &CodecRegistry::with_arrow_ipc(),
        OriginLimits::default(),
    )
    .expect("test project should convert");
    let schemas = &imported[0].snapshot.schema.columns;

    assert_eq!(schemas[1].metadata[ORIGINAL_NAME_KEY], "name");
    assert_eq!(schemas[3].metadata[ORIGINAL_NAME_KEY], "");
    assert!(!schemas[0].metadata.contains_key(ORIGINAL_NAME_KEY));
    assert!(!schemas[2].metadata.contains_key(ORIGINAL_NAME_KEY));
}

#[test]
fn prepares_every_candidate_before_writing_any_blocks() {
    let project = project(vec![
        worksheet("First", columns(&["x"])),
        worksheet("Second", columns(&["1234567890123456", "1234567890123456"])),
    ]);
    let store = MemoryBlockStore::default();
    let limits = OriginLimits {
        max_string_bytes: 16,
        ..OriginLimits::default()
    };

    let error = import_origin_project(project, &store, &CodecRegistry::with_arrow_ipc(), limits)
        .expect_err("all candidate names must be prepared before block writes");

    assert!(matches!(
        error,
        OriginImportError::LimitExceeded {
            resource: "string bytes",
            ..
        }
    ));
    assert_eq!(store.block_count(), 0);
}

#[test]
fn pads_short_columns_across_the_first_batch_and_into_the_second() {
    const ROWS: usize = 65_537;
    let mut sheet = worksheet(
        "Sheet",
        vec![
            OriginColumn {
                name: "anchor".to_owned(),
                long_name: None,
                role: None,
                units: None,
                comments: None,
                column_type: OriginColumnType::Integer,
                cells: (0..ROWS)
                    .map(|value| OriginCell::Integer(value as i64))
                    .collect(),
            },
            OriginColumn {
                name: "short".to_owned(),
                long_name: None,
                role: None,
                units: None,
                comments: None,
                column_type: OriginColumnType::Integer,
                cells: vec![OriginCell::Integer(10), OriginCell::Integer(20)],
            },
        ],
    );
    sheet.row_count = ROWS;
    let store = MemoryBlockStore::default();
    let codecs = CodecRegistry::with_arrow_ipc();
    let imported = import_origin_project(
        project(vec![sheet]),
        &store,
        &codecs,
        OriginLimits::default(),
    )
    .expect("short columns should be padded with nulls");
    let reader = SnapshotReader::new(&imported[0].snapshot, &store, &codecs).unwrap();
    let first = reader.read_batch(0, &[]).unwrap();
    let second = reader.read_batch(1, &[]).unwrap();

    assert_eq!(first.columns[1].1.value(1), Some(ScalarValue::Int64(20)));
    assert_eq!(first.columns[1].1.value(2), Some(ScalarValue::Null));
    assert_eq!(first.columns[1].1.value(65_535), Some(ScalarValue::Null));
    assert_eq!(second.columns[1].1.value(0), Some(ScalarValue::Null));
}

#[test]
fn pads_a_short_column_that_ends_exactly_on_a_batch_boundary() {
    const ROWS: usize = 65_537;
    let mut sheet = worksheet(
        "Sheet",
        vec![
            OriginColumn {
                name: "anchor".to_owned(),
                long_name: None,
                role: None,
                units: None,
                comments: None,
                column_type: OriginColumnType::Integer,
                cells: (0..ROWS)
                    .map(|value| OriginCell::Integer(value as i64))
                    .collect(),
            },
            OriginColumn {
                name: "short".to_owned(),
                long_name: None,
                role: None,
                units: None,
                comments: None,
                column_type: OriginColumnType::Integer,
                cells: (0..65_536)
                    .map(|value| OriginCell::Integer(value as i64))
                    .collect(),
            },
        ],
    );
    sheet.row_count = ROWS;
    let store = MemoryBlockStore::default();
    let codecs = CodecRegistry::with_arrow_ipc();
    let imported = import_origin_project(
        project(vec![sheet]),
        &store,
        &codecs,
        OriginLimits::default(),
    )
    .expect("a batch-boundary short column should be padded with nulls");
    let second = SnapshotReader::new(&imported[0].snapshot, &store, &codecs)
        .unwrap()
        .read_batch(1, &[])
        .unwrap();

    assert_eq!(second.columns[1].1.value(0), Some(ScalarValue::Null));
}
