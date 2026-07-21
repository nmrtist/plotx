use crate::{DataError, MaterializedColumn, MaterializedTable, Result, ScalarValue};

pub(super) fn align_row_changing_output(
    mut columns: Vec<MaterializedColumn>,
    expected: &MaterializedTable,
    row_count: usize,
) -> Result<MaterializedTable> {
    if row_count != expected.row_ids.len() {
        return Err(DataError::Backend(format!(
            "DataFusion produced {row_count} rows, expected {}",
            expected.row_ids.len()
        )));
    }
    let mut used = vec![false; row_count];
    let mut order = Vec::with_capacity(row_count);
    for expected_row in 0..row_count {
        let candidate = (0..row_count).find(|candidate| {
            !used[*candidate]
                && columns
                    .iter()
                    .zip(&expected.columns)
                    .all(|(actual, expected)| {
                        scalar_equal(&actual.values[*candidate], &expected.values[expected_row])
                    })
        });
        let candidate = candidate.ok_or_else(|| {
            DataError::Backend(
                "DataFusion row-changing result differs from PlotX reference values".into(),
            )
        })?;
        used[candidate] = true;
        order.push(candidate);
    }
    for column in &mut columns {
        column.values = order
            .iter()
            .map(|row| column.values[*row].clone())
            .collect();
    }
    let table = MaterializedTable {
        table_id: expected.table_id,
        schema: expected.schema.clone(),
        row_ids: expected.row_ids.clone(),
        columns,
    };
    table.validate()?;
    Ok(table)
}

pub(super) fn scalar_equal(left: &ScalarValue, right: &ScalarValue) -> bool {
    match (left, right) {
        (ScalarValue::Float64(left), ScalarValue::Float64(right)) => {
            left.to_bits() == right.to_bits()
        }
        _ => left == right,
    }
}
