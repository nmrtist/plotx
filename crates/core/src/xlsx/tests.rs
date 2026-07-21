use super::*;
use plotx_io::xlsx::{XlsxCell, XlsxSheet, XlsxValue, XlsxWorkbook};

#[test]
fn imports_mixed_null_and_iso_columns_without_discarding_them() {
    let workbook = XlsxWorkbook {
        sheets: vec![XlsxSheet {
            name: "Data".into(),
            hidden: false,
            rows: vec![
                vec![text("sample"), text("value"), text("when")],
                vec![text("a"), number(1.0), text("2026-07-20")],
                vec![
                    text("b"),
                    XlsxCell::value(XlsxValue::Empty),
                    text("2026-07-21"),
                ],
            ],
        }],
        uses_1904_date_system: false,
        plotx_schema: None,
        diagnostics: Vec::new(),
    };
    let store = plotx_data::MemoryBlockStore::default();
    let codecs = CodecRegistry::with_arrow_ipc();
    let imported = import_xlsx_workbook(&workbook, &store, &codecs).unwrap();
    assert_eq!(imported.len(), 1);
    assert_eq!(imported[0].snapshot.row_count, 2);
    assert_eq!(
        imported[0]
            .snapshot
            .schema
            .columns
            .iter()
            .map(|column| column.logical_type.clone())
            .collect::<Vec<_>>(),
        vec![LogicalType::Utf8, LogicalType::Float64, LogicalType::Date]
    );
}

#[test]
fn civil_dates_use_arrow_date32_epoch() {
    assert_eq!(parse_iso_date("1970-01-01"), Some(0));
    assert_eq!(parse_iso_date("2000-01-01"), Some(10_957));
    assert!(parse_iso_date("2025-02-29").is_none());
    assert!(parse_iso_date("2024-02-29").is_some());
    assert!(parse_iso_date("2026-04-31").is_none());
}

fn text(value: &str) -> XlsxCell {
    XlsxCell::value(XlsxValue::Utf8(value.into()))
}

fn number(value: f64) -> XlsxCell {
    XlsxCell::value(XlsxValue::Float64(value))
}
