use super::{Dataset, Trace1d};

impl Dataset {
    pub fn trace_x_unit(&self) -> String {
        match self {
            Self::Nmr(_) => "ppm".into(),
            Self::Table(table) => table
                .x_binding
                .and_then(|id| {
                    table
                        .typed_state
                        .envelope
                        .revision
                        .snapshot
                        .schema
                        .column(id)
                })
                .and_then(|column| column.unit.as_ref())
                .map(|unit| unit.display_unit.clone())
                .unwrap_or_default(),
            Self::Nmr2D(_) => String::new(),
            Self::Electrophysiology(_) => "s".into(),
            Self::Afm(_) => String::new(),
        }
    }

    pub fn has_displayed_trace(&self, column: Option<plotx_data::ColumnId>) -> bool {
        match self {
            Self::Nmr(_) => true,
            Self::Table(table) => {
                table.x_binding.is_some()
                    && column.map_or(!table.series_bindings.is_empty(), |column| {
                        table
                            .series_bindings
                            .iter()
                            .any(|binding| binding.value_column == column)
                    })
            }
            Self::Nmr2D(_) => false,
            Self::Electrophysiology(data) => !data.data.sweeps.is_empty(),
            Self::Afm(_) => false,
        }
    }

    pub fn displayed_trace(&self, column: Option<plotx_data::ColumnId>) -> Option<Trace1d> {
        match self {
            Self::Nmr(data) => Some(Trace1d {
                xs: data.spectrum.ppm.clone(),
                ys: data.spectrum.real(),
                x_reversed: true,
            }),
            Self::Table(table) => typed_table_trace(table, column),
            Self::Nmr2D(_) => None,
            Self::Electrophysiology(data) => {
                let ys = data.processed_trace(0, data.selected_channel).ok()?;
                let xs = (0..ys.len())
                    .map(|index| index as f64 / data.data.sample_rate_hz)
                    .collect();
                Some(Trace1d {
                    xs,
                    ys,
                    x_reversed: false,
                })
            }
            Self::Afm(_) => None,
        }
    }
}

fn typed_table_trace(
    table: &super::TableDataset,
    column: Option<plotx_data::ColumnId>,
) -> Option<Trace1d> {
    let x = table.x_binding?;
    let value = column.or_else(|| {
        table
            .series_bindings
            .first()
            .map(|binding| binding.value_column)
    })?;
    table
        .series_bindings
        .iter()
        .find(|binding| binding.value_column == value)?;
    let count = usize::try_from(table.typed_state.envelope.revision.snapshot.row_count).ok()?;
    let rows = table.typed_rows(count, &[x, value]).ok()?;
    let numbers = |values: &[plotx_data::ScalarValue]| {
        values
            .iter()
            .map(|value| match value {
                plotx_data::ScalarValue::Int64(value) => *value as f64,
                plotx_data::ScalarValue::Float64(value) => *value,
                _ => f64::NAN,
            })
            .collect()
    };
    Some(Trace1d {
        xs: numbers(&rows.columns[0].values),
        ys: numbers(&rows.columns[1].values),
        x_reversed: false,
    })
}
