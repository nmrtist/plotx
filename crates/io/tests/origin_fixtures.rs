use plotx_io::origin::{
    OriginCell, OriginError, OriginFormat, OriginLimits, OriginProfile, OriginProject,
    probe_origin, read_origin,
};

const MIXED_NUMERIC_VALUE: f64 = 314.0 / 100.0;

fn cell<'a>(
    project: &'a OriginProject,
    workbook_name: &str,
    column_name: &str,
    row: usize,
) -> Option<&'a OriginCell> {
    project
        .workbooks
        .iter()
        .find(|workbook| workbook.name == workbook_name)?
        .worksheets
        .iter()
        .flat_map(|worksheet| &worksheet.columns)
        .find(|column| column.name == column_name)?
        .cells
        .get(row)
}

fn assert_float_cell(
    project: &OriginProject,
    workbook: &str,
    column: &str,
    row: usize,
    expected: f64,
) {
    let Some(OriginCell::Float(actual)) = cell(project, workbook, column, row) else {
        panic!("expected a floating-point cell at {workbook}/{column}/{row}");
    };
    assert_eq!(*actual, expected);
}

fn parameter(project: &OriginProject, key: &str) -> f64 {
    project
        .parameters
        .iter()
        .find(|entry| entry.key == key)
        .unwrap_or_else(|| panic!("missing project parameter {key}"))
        .value
        .parse()
        .unwrap_or_else(|error| panic!("parameter {key} is not numeric: {error}"))
}

#[test]
fn imports_openopj_origin_7_v552_fixture() {
    let bytes = include_bytes!("fixtures/origin/test-origin-7.0552.opj");
    let project = read_origin(bytes, OriginLimits::default())
        .expect("the licensed OpenOPJ fixture should import");

    assert_eq!(project.probe.profile, Some(OriginProfile::Origin7V552));
    assert_eq!(
        project
            .workbooks
            .iter()
            .map(|workbook| workbook.name.as_str())
            .collect::<Vec<_>>(),
        ["Data1", "Data1Coeff", "Data1spline", "TestW"]
    );
    assert!(project.workbooks.iter().all(|workbook| {
        workbook.worksheets.len() == 1 && workbook.worksheets[0].name == "Sheet1"
    }));
    assert_eq!(
        project
            .workbooks
            .iter()
            .map(|workbook| {
                let worksheet = &workbook.worksheets[0];
                (
                    workbook.name.as_str(),
                    worksheet.row_count,
                    worksheet.columns.len(),
                )
            })
            .collect::<Vec<_>>(),
        [
            ("Data1", 21, 5),
            ("Data1Coeff", 774, 4),
            ("Data1spline", 481, 1),
            ("TestW", 3, 6),
        ]
    );
    assert_eq!(
        cell(&project, "Data1", "INJV", 0),
        Some(&OriginCell::Float(0.4))
    );
    assert_eq!(
        cell(&project, "Data1", "INJV", 19),
        Some(&OriginCell::Float(2.0))
    );
    assert_eq!(cell(&project, "Data1", "INJV", 20), Some(&OriginCell::Null));
    assert_eq!(
        cell(&project, "TestW", "TextNumeric", 0),
        Some(&OriginCell::Text("text".to_owned()))
    );
    assert_eq!(
        cell(&project, "TestW", "TextNumeric", 1),
        Some(&OriginCell::Float(MIXED_NUMERIC_VALUE))
    );

    assert_float_cell(
        &project,
        "TestW",
        "Float",
        0,
        f64::from(f32::from_bits(0x43ac_cccd)),
    );
    assert_float_cell(
        &project,
        "TestW",
        "Float",
        1,
        f64::from(f32::from_bits(0xc7c3_501a)),
    );
    assert_eq!(
        cell(&project, "TestW", "Long", 0),
        Some(&OriginCell::Integer(345))
    );
    assert_eq!(
        cell(&project, "TestW", "Long", 1),
        Some(&OriginCell::Integer(-100000))
    );
    assert_eq!(
        cell(&project, "TestW", "Integer", 0),
        Some(&OriginCell::Integer(34))
    );
    assert_eq!(
        cell(&project, "TestW", "Integer", 1),
        Some(&OriginCell::Integer(-1000))
    );
    assert_eq!(
        cell(&project, "TestW", "Text", 0),
        Some(&OriginCell::Text("test string 123".to_owned()))
    );
    assert_eq!(
        cell(&project, "TestW", "Text", 1),
        Some(&OriginCell::Text("only text".to_owned()))
    );
    assert_eq!(
        cell(&project, "TestW", "firstRow", 0),
        Some(&OriginCell::Null)
    );
    assert_float_cell(&project, "TestW", "firstRow", 1, 5.23);
    assert_float_cell(&project, "TestW", "firstRow", 2, -7.0);

    assert_eq!(project.parameters.len(), 41);
    assert_eq!(parameter(&project, "ERR"), 1.0);
    assert_eq!(parameter(&project, "SYRNG_C_DATA1"), 1.25);
    assert_eq!(parameter(&project, "CELL_C_DATA1"), 0.1246);
    assert!((parameter(&project, "S") - 1.28889201142965).abs() < 1.0e-14);

    assert_eq!(
        project
            .notes
            .iter()
            .find(|note| note.name == "Results")
            .map(|note| note.content.as_str()),
        Some("Data1 Temperature:\t25.10242\r\n\r\n")
    );
    assert_eq!(
        project
            .notes
            .iter()
            .find(|note| note.name == "ResultsLog")
            .map(|note| note.content.as_str()),
        Some(
            "[3/5/2009 13:32 \"/DeltaH\" (2454895)]\r\n\
Data: Data1_NDH\r\n\
Model: OneSites\r\n\
Chi^2/DoF = 3008\r\n\
N\t0.800\t0.0346\r\n\
K\t1.75E4\t1.86E3\r\n\
H\t-5406\t340.5\r\n\
S\t1.29\r\n\r\n"
        )
    );

    assert_eq!(project.resource_usage.workbooks, 4);
    assert_eq!(project.resource_usage.worksheets, 4);
    assert_eq!(project.resource_usage.columns, 16);
    assert_eq!(project.resource_usage.cells, 1889);
    assert_eq!(project.resource_usage.metadata_records, 362);
    assert_eq!(
        project
            .unsupported_objects
            .iter()
            .find(|summary| summary.kind == "worksheet columns")
            .map(|summary| summary.count),
        Some(23)
    );
    assert!(project.diagnostics.iter().any(|diagnostic| {
        diagnostic.code == plotx_io::origin::OriginDiagnosticCode::UnsupportedColumnSkipped
    }));
    assert!(
        project
            .unsupported_objects
            .iter()
            .any(|summary| { summary.kind == "project tree" && summary.count == 1 })
    );
    assert!(
        project
            .unsupported_objects
            .iter()
            .any(|summary| { summary.kind == "embedded attachments" && summary.count == 1 })
    );
    assert!(
        project
            .unsupported_objects
            .iter()
            .any(|summary| { summary.kind == "window presentation records" && summary.count > 0 })
    );
    assert!(
        project
            .unsupported_objects
            .iter()
            .any(|summary| { summary.kind == "note properties" && summary.count == 2 })
    );
}

#[test]
fn recognizes_public_opju_fixture_without_partial_output() {
    let bytes = include_bytes!("fixtures/origin/RawData_Locust_Revision1_TIS_Mechanism.opju");
    let probe = probe_origin(bytes).expect("the public fixture has a recognized OPJU header");
    assert_eq!(probe.format, OriginFormat::Opju);
    let error = read_origin(bytes, OriginLimits::default()).unwrap_err();
    assert_eq!(
        error,
        OriginError::UnsupportedOpjuVariant {
            message: "This OPJU file uses a record layout that PlotX does not support yet. No data was imported."
                .to_owned(),
        }
    );
}
