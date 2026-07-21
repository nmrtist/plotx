use super::*;
use crate::state::{FloatSeries, materialized_float_series_table};

fn table_with_column() -> TableDataset {
    materialized_float_series_table(
        (
            "Gradient".into(),
            "mT/m".into(),
            vec![Some(0.0), Some(1.0), Some(2.0)],
        ),
        vec![FloatSeries {
            name: "a".into(),
            unit: String::new(),
            values: vec![Some(1.0), Some(2.0), Some(3.0)],
            uncertainty: None,
            fit: None,
        }],
        "plotx.test.table.v1",
    )
    .unwrap()
}

#[test]
fn sheet_rect_reflects_shape_and_position() {
    let mut table = table_with_column();
    table.board_pos = [360.0, 720.0];
    assert_eq!(table.sheet_cols(), 2);
    assert_eq!(table.visible_rows(), 3);
    let [width, height] = table.sheet_size_pt();
    assert!((width - 2.0 * SHEET_COL_W_PT).abs() < 1e-3);
    assert!((height - (SHEET_HEADER_H_PT + 3.0 * SHEET_ROW_H_PT)).abs() < 1e-3);
    assert_eq!(
        [table.board_rect_pt().left, table.board_rect_pt().top],
        [360.0, 720.0]
    );
}

#[test]
fn sheet_caps_visible_rows_and_reserves_overflow_footer() {
    let count = SHEET_MAX_ROWS + 5;
    let table = materialized_float_series_table(
        (
            "x".into(),
            "".into(),
            (0..count).map(|index| Some(index as f64)).collect(),
        ),
        Vec::new(),
        "plotx.test.long-table.v1",
    )
    .unwrap();
    assert_eq!(table.visible_rows(), SHEET_MAX_ROWS);
    assert!(table.rows_overflow());
}

#[test]
fn figure_has_one_series_per_column() {
    let dataset = table_with_column();
    assert_eq!(dataset.figure().series.len(), 1);
}

#[test]
fn figure_uses_only_valid_aligned_uncertainty() {
    let figure = materialized_float_series_table(
        (
            "Gradient".into(),
            "mT/m".into(),
            vec![Some(0.0), Some(1.0), Some(2.0)],
        ),
        vec![FloatSeries {
            name: "a".into(),
            unit: String::new(),
            values: vec![Some(1.0), Some(2.0), Some(3.0)],
            uncertainty: Some(vec![Some(0.25), None, Some(-1.0)]),
            fit: None,
        }],
        "plotx.test.uncertainty-table.v1",
    )
    .unwrap()
    .figure();
    assert_eq!(figure.error_bars.len(), 1);
    assert_eq!(figure.error_bars[0].center, [0.0, 1.0]);
}

#[test]
fn typed_table_distinguishes_null_from_special_float_values() {
    let dataset = materialized_float_series_table(
        ("x".into(), "".into(), vec![Some(0.0), None]),
        vec![FloatSeries {
            name: "signal".into(),
            unit: String::new(),
            values: vec![Some(f64::INFINITY), Some(f64::NEG_INFINITY)],
            uncertainty: None,
            fit: None,
        }],
        "plotx.test.special-floats.v1",
    )
    .unwrap();
    let values = dataset.typed_rows(2, &[]).unwrap();
    assert_eq!(
        values.columns[1].values[0],
        plotx_data::ScalarValue::Float64(f64::INFINITY)
    );
    assert_eq!(values.columns[0].values[1], plotx_data::ScalarValue::Null);
}
