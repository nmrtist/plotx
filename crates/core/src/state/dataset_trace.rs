use super::{Dataset, Trace1d};

impl Dataset {
    pub fn trace_item_figure(
        &self,
        field: super::FieldId,
        item: plotx_data::TraceItemId,
    ) -> Option<plotx_figure::Figure> {
        let collection = self.trace_collection(field)?;
        let index = collection.items.iter().position(|entry| entry.id == item)?;
        let label = collection.item(item)?.automatic_label()?;
        match self {
            Self::Nmr2D(data) => {
                let plotx_processing::Processed2D::Stack(stack) = &data.processed else {
                    return None;
                };
                let values = stack.traces.get(index)?;
                let points = stack
                    .ppm
                    .iter()
                    .copied()
                    .zip(values.iter().map(|value| value.re))
                    .map(|(x, y)| [x, y])
                    .collect::<Vec<_>>();
                let (x0, x1) = stack.ppm_bounds();
                let (mut y0, mut y1) = points
                    .iter()
                    .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), point| {
                        (lo.min(point[1]), hi.max(point[1]))
                    });
                if !y0.is_finite() || y0 == y1 {
                    y0 = -0.5;
                    y1 = 0.5;
                }
                let x_name = if stack.direct_domain == plotx_io::Domain::Frequency {
                    crate::figures::axis_label(&stack.direct.nucleus)
                } else {
                    "Time (s)".to_owned()
                };
                let x_axis = plotx_figure::Axis::new(x_name, x0, x1)
                    .reversed(stack.direct_domain == plotx_io::Domain::Frequency);
                Some(
                    plotx_figure::Figure::new(
                        "",
                        x_axis,
                        plotx_figure::Axis::new("Intensity", y0, y1),
                    )
                    .with_series(plotx_figure::Series::line(label, points)),
                )
            }
            Self::Electrophysiology(data) => {
                let channel = (0..data.data.channels.len()).find(|&channel| {
                    data.field_key(channel)
                        .and_then(|key| data.field_catalog.id_for_key(key))
                        == Some(field)
                })?;
                let ys = data.processed_trace(index, channel).ok()?;
                let points = ys
                    .iter()
                    .enumerate()
                    .filter_map(|(i, y)| {
                        y.is_finite()
                            .then_some([i as f64 / data.data.sample_rate_hz, *y])
                    })
                    .collect::<Vec<_>>();
                let (mut y0, mut y1) = ys
                    .iter()
                    .copied()
                    .filter(|value| value.is_finite())
                    .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), value| {
                        (lo.min(value), hi.max(value))
                    });
                if !y0.is_finite() || y0 == y1 {
                    y0 = -0.5;
                    y1 = 0.5;
                }
                let meta = data.data.channels.get(channel)?;
                Some(
                    plotx_figure::Figure::new(
                        "",
                        plotx_figure::Axis::new(
                            "Time (s)",
                            0.0,
                            ys.len() as f64 / data.data.sample_rate_hz,
                        ),
                        plotx_figure::Axis::new(
                            format!("{} ({})", meta.name, meta.unit.symbol),
                            y0,
                            y1,
                        ),
                    )
                    .with_series(plotx_figure::Series::line(label, points)),
                )
            }
            _ => None,
        }
    }

    pub fn trace_item_label(
        &self,
        field: super::FieldId,
        item: plotx_data::TraceItemId,
    ) -> Option<String> {
        self.trace_collection(field)?.item(item)?.automatic_label()
    }

    pub fn trace_x_unit(&self) -> String {
        match self {
            Self::Nmr(data) => match data.output_domain() {
                plotx_io::Domain::Time => "s".into(),
                plotx_io::Domain::Frequency => "ppm".into(),
            },
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
            Self::MassSpec(_) => "min".into(),
            Self::Xrd(_) => "deg".into(),
            Self::Xps(_) => "eV".into(),
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
            Self::MassSpec(_) => true,
            Self::Xrd(_) => true,
            Self::Xps(data) => data.displayed_region(data.active_region).is_some(),
        }
    }

    pub fn displayed_trace(&self, column: Option<plotx_data::ColumnId>) -> Option<Trace1d> {
        match self {
            Self::Nmr(data) => Some(match &data.processed {
                plotx_processing::Processed1D::Time(trace) => Trace1d {
                    xs: trace.time_s.clone(),
                    ys: trace.real(),
                    x_reversed: false,
                },
                plotx_processing::Processed1D::Frequency(spectrum) => Trace1d {
                    xs: spectrum.ppm.clone(),
                    ys: spectrum.real(),
                    x_reversed: true,
                },
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
            Self::MassSpec(data) => {
                let stream = data.run.stream(data.active_stream)?;
                Some(Trace1d {
                    xs: stream
                        .spectra
                        .iter()
                        .map(|scan| scan.retention_time_min)
                        .collect(),
                    ys: stream.spectra.iter().map(|scan| scan.tic).collect(),
                    x_reversed: false,
                })
            }
            Self::Xrd(data) => Some(Trace1d {
                xs: data.data.two_theta_deg.clone(),
                ys: data.processed.intensity.clone(),
                x_reversed: false,
            }),
            Self::Xps(data) => {
                let processed = data.displayed_region(data.active_region)?;
                Some(Trace1d {
                    xs: processed.binding_energy_ev,
                    ys: processed.intensity,
                    x_reversed: data.active_region().binding_energy_ev.is_some(),
                })
            }
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
