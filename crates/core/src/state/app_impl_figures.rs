use super::*;
use plotx_figure::Figure;

impl PlotxApp {
    /// Build a dataset's figure through the chart registry: resolve `chart`'s
    /// type for the dataset's domain (falling back to the domain default when the
    /// recorded id doesn't apply), then dispatch to its builder. The default chart
    /// of each domain calls the same builder as before, so figures are unchanged.
    pub fn build_full_canvas_figure(
        &self,
        dataset: usize,
        chart: &ChartSpec,
        size_mm: [f32; 2],
    ) -> Figure {
        let mut figure =
            crate::workflow::build_dataset_figure(&self.doc.datasets[dataset], chart, size_mm);
        if let Some(nmr) = self.doc.datasets[dataset].as_nmr() {
            figure.integral_curves = nmr.integral_curves();
        }
        // Every figure build stamps the document's typography, so a doc-level
        // edit reaches each plot on its next rebuild without per-plot state.
        figure.typography = self.doc.style_library.figure_typography;
        figure
    }

    /// Build the figure for a plot's data binding. A multi-series binding whose
    /// datasets share one stackable (line-series) domain is combined into one
    /// figure honouring `stack`; any other binding renders the primary alone.
    pub fn build_binding_figure(
        &self,
        binding: &DataBinding,
        chart: &ChartSpec,
        stack: &StackSpec,
        size_mm: [f32; 2],
    ) -> Figure {
        if binding.series.len() > 1 && self.series_stackable(binding) {
            self.build_stacked_figure(binding, stack, size_mm)
        } else {
            if let Some(series) = binding.series.first()
                && !matches!(series.encoding, plotx_figure::SeriesEncoding::Line(_))
            {
                let figure = self
                    .build_encoded_series_figure(series)
                    .unwrap_or_else(|| unsupported_series_figure(series));
                return self.normalize_binding_figure(figure, size_mm);
            }
            let Some(primary_id) = binding.primary_dataset() else {
                return Figure::new(
                    "",
                    plotx_figure::Axis::new("x", 0.0, 1.0),
                    plotx_figure::Axis::new("y", 0.0, 1.0),
                );
            };
            let Some(primary) = self.doc.dataset_index(primary_id) else {
                return Figure::new(
                    "",
                    plotx_figure::Axis::new("x", 0.0, 1.0),
                    plotx_figure::Axis::new("y", 0.0, 1.0),
                );
            };
            let domain = self.doc.datasets[primary].domain();
            let mut fig = self.build_full_canvas_figure(primary, chart, size_mm);
            // A single-series colour override (e.g. a theme's primary trace colour)
            // recolours the built traces, so it survives figure rebuilds and export.
            // Applied before the line-fit overlays so those keep their own colours;
            // stacked figures never get overlays (each trace stays a single series).
            if let Some(line) = binding
                .series
                .first()
                .and_then(|series| match &series.encoding {
                    plotx_figure::SeriesEncoding::Line(line) => Some(line),
                    plotx_figure::SeriesEncoding::Contour(_)
                    | plotx_figure::SeriesEncoding::Heatmap(_)
                    | plotx_figure::SeriesEncoding::Image(_) => None,
                })
            {
                let color = line.color.resolve();
                for series in &mut fig.series {
                    series.color = color;
                    series.width = line.width.get();
                    for point in &mut series.points {
                        point[1] *= line.scale;
                    }
                }
                for error_bar in &mut fig.error_bars {
                    error_bar.color = color;
                    error_bar.center[1] *= line.scale;
                    error_bar.negative *= line.scale.abs();
                    error_bar.positive *= line.scale.abs();
                }
                // Bar/box bodies live in `polygons` and must follow the traces.
                // Value-mapped figures (heatmap cells, colormap surfaces, pie
                // wedges) keep their own colours — one override would erase the
                // encoding they carry.
                if fig.heatmap.is_none() && fig.axis_frame != plotx_figure::AxisFrame::Hidden {
                    let background = fig.background;
                    for polygon in &mut fig.polygons {
                        polygon.fill = color;
                        if let Some((stroke, _)) = &mut polygon.stroke
                            && *stroke != background
                        {
                            *stroke = color;
                        }
                    }
                }
            }
            // Stored fits are curves in the table's native x/y space; every
            // other table chart (histogram, box, heatmap, …) draws in different
            // coordinates where those curves would be unrelated ink.
            let selected_chart = binding
                .series
                .first()
                .and_then(|series| self.doc.dataset_by_id(series.source.resource))
                .and_then(|dataset| {
                    dataset
                        .field_descriptor(binding.series[0].source.field)
                        .map(|field| {
                            resolved_chart_type_for_field(
                                &field.capabilities,
                                domain,
                                &chart.type_id,
                            )
                        })
                })
                .unwrap_or_else(|| default_chart_type(domain));
            let fits_apply = domain != DataDomain::Table || selected_chart.id == "table_line";
            if fits_apply {
                fig = apply_line_fit_overlays(fig, self.doc.datasets[primary].line_fits());
            }
            fig
        }
    }

    pub(super) fn normalize_binding_figure(&self, mut figure: Figure, size_mm: [f32; 2]) -> Figure {
        figure.title.clear();
        figure.width = size_mm[0] * MM_TO_PT;
        figure.height = size_mm[1] * MM_TO_PT;
        figure.typography = self.doc.style_library.figure_typography;
        figure
    }

    pub(super) fn build_encoded_series_figure(&self, series: &SeriesBinding) -> Option<Figure> {
        let dataset = self.doc.dataset_by_id(series.source.resource)?;
        if !dataset.supports_encoding(series.source.field, &series.encoding) {
            return None;
        }
        dataset.encoded_field_figure(series.source.field, &series.encoding)
    }
}

fn unsupported_series_figure(series: &SeriesBinding) -> Figure {
    let mut figure = Figure::new(
        "Unavailable field encoding",
        plotx_figure::Axis::new("x", 0.0, 1.0),
        plotx_figure::Axis::new("y", 0.0, 1.0),
    );
    figure.annotations.push(plotx_figure::Annotation {
        text: format!(
            "The selected field cannot be rendered with {}.",
            match series.encoding {
                plotx_figure::SeriesEncoding::Line(_) => "line",
                plotx_figure::SeriesEncoding::Contour(_) => "contour",
                plotx_figure::SeriesEncoding::Heatmap(_) => "heatmap",
                plotx_figure::SeriesEncoding::Image(_) => "image",
            }
        ),
        at: [0.5, 0.5],
        color: plotx_figure::Color::rgb(0xd1, 0x24, 0x2a),
        size: 12.0,
    });
    figure
}
