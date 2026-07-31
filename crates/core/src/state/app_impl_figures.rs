use super::*;
use plotx_figure::{Color, Figure, RangeAnnotation};
use std::sync::Arc;

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
        if let Some(state) = self.doc.datasets[dataset]
            .region_analysis()
            .filter(|state| state.show_annotations)
        {
            let unit = self.doc.datasets[dataset].region_axis_unit().unwrap_or("");
            figure
                .range_annotations
                .extend(state.regions.iter().map(|region| {
                    let [r, g, b] = region.color;
                    RangeAnnotation {
                        source_id: region.id.get(),
                        x0: region.lo,
                        x1: region.hi,
                        label: region.column_name(unit),
                        label_position: region.label_position,
                        color: Color::rgb(r, g, b),
                        fill_opacity: 0.12,
                        width: 1.0,
                    }
                }));
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
        &mut self,
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
                let semantic_colors = fig.series_colors_are_semantic;
                for series in &mut fig.series {
                    if !semantic_colors {
                        series.color = color;
                    }
                    series.width = line.width.get();
                    for point in &mut series.points {
                        point[1] *= line.scale;
                    }
                }
                for error_bar in &mut fig.error_bars {
                    if !semantic_colors {
                        error_bar.color = color;
                    }
                    error_bar.center[1] *= line.scale;
                    error_bar.negative *= line.scale.abs();
                    error_bar.positive *= line.scale.abs();
                }
                // Bar/box bodies live in `polygons` and must follow the traces.
                // Value-mapped figures (heatmap cells, colormap surfaces, pie
                // wedges) keep their own colours — one override would erase the
                // encoding they carry.
                if !semantic_colors
                    && fig.heatmap.is_none()
                    && fig.axis_frame != plotx_figure::AxisFrame::Hidden
                {
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
            let fits_apply = (domain != DataDomain::Table || selected_chart.id == "table_line")
                && !self.doc.datasets[primary]
                    .as_nmr()
                    .is_some_and(|nmr| nmr.output_domain() == plotx_io::Domain::Time);
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

    pub(super) fn build_encoded_series_figure(&mut self, series: &SeriesBinding) -> Option<Figure> {
        let dataset = self.doc.dataset_by_id(series.source.resource)?;
        if !dataset.supports_encoding(series.source.field, &series.encoding) {
            return None;
        }
        let plotx_figure::SeriesEncoding::Contour(contour) = &series.encoding else {
            return dataset.encoded_field_figure(series.source.field, &series.encoding);
        };

        let field = FieldRef {
            resource: series.source.resource,
            field: series.source.field,
        };
        let version = match self.session.compute.field_version_for(field) {
            Ok(version) => version,
            Err(error) => {
                self.session.status = field_enqueue_error_status(error);
                return dataset.encoded_field_figure(series.source.field, &series.encoding);
            }
        };
        // A cache hit must not touch the values: the summary is looked up first,
        // and the payload is materialized only on the paths that actually hand a
        // grid to a worker.
        let source = VersionedFieldRef { field, version };
        let summary = match self.session.compute.cached_field_summary(source) {
            Some(summary) => summary,
            None => {
                let snapshot = dataset.field_snapshot(series.source.field, version, None)?;
                self.session.compute.remember_field_summary(&snapshot);
                snapshot.summary?
            }
        };
        let resolution = resolve_contour_levels(source, contour, summary, |key| {
            self.session.compute.estimate_for(key).cloned()
        });
        match resolution {
            ContourResolution::Ready {
                levels,
                unreachable,
            } => {
                // A threshold the field never reaches draws nothing, which is
                // exactly what the user asked for — but silently is how a
                // mistyped magnitude becomes an unexplained blank plot.
                if !unreachable.is_empty() {
                    self.session.status = unreachable_threshold_status(&unreachable);
                }
                let key = ContourGeometryCacheKey { source, levels };
                if let Some(geometry) = self.session.compute.geometry_for(&key) {
                    // A capped build drew fewer levels than the panel lists.
                    // Saying so is the difference between a contour the user
                    // chose and one the renderer silently cut down.
                    if let Some(omitted) = geometry.omitted {
                        self.session.status = omitted_levels_status(&omitted);
                    }
                    return dataset.contour_figure_from_geometry(
                        series.source.field,
                        &geometry,
                        &contour.style,
                    );
                }
                // Ask whether the build is already running *before* building
                // its input. A miss is resolved on every frame for as long as
                // the job runs, and materializing the grid first would read and
                // clone the whole plane each time only for the enqueue to
                // recognize the duplicate and drop it.
                if !self.session.compute.geometry_in_flight(&key) {
                    let grid = self.contour_grid(dataset, series.source.field, version, summary)?;
                    if let Err(error) = self.session.compute.enqueue_contour(key, grid) {
                        self.session.status = field_enqueue_error_status(error);
                        return dataset.encoded_field_figure(series.source.field, &series.encoding);
                    }
                }
                // The plot is empty and will stay empty until the worker
                // answers. An unexplained blank plot is indistinguishable from
                // a broken one, so the wait is stated where every other slow
                // operation states it — but never over an unreachable
                // threshold, which explains a half that will still be blank
                // once the wait is over and names the edit that fixes it.
                if unreachable.is_empty() {
                    self.session.status = CONTOUR_GEOMETRY_PENDING_STATUS.to_owned();
                }
            }
            ContourResolution::Pending(keys) => {
                let pending_status = estimate_pending_status(&keys);
                // Same order for the same reason: only the keys that are not
                // already running need a grid, and when none of them do the
                // payload is never touched at all.
                if keys
                    .iter()
                    .any(|key| !self.session.compute.estimate_in_flight(key))
                {
                    let grid = self.contour_grid(dataset, series.source.field, version, summary)?;
                    for key in keys {
                        if let Err(error) = self
                            .session
                            .compute
                            .enqueue_estimate(key, Arc::clone(&grid))
                        {
                            self.session.status = field_enqueue_error_status(error);
                            return dataset
                                .encoded_field_figure(series.source.field, &series.encoding);
                        }
                    }
                }
                self.session.status = pending_status;
            }
            ContourResolution::Unavailable => {
                self.session.status = "Contour levels are unavailable for this field.".into();
            }
        }
        dataset.encoded_field_figure(series.source.field, &series.encoding)
    }

    /// Materialize the worker-owned grid. Only the enqueue paths call this;
    /// resolving against a warm geometry cache never does.
    fn contour_grid(
        &self,
        dataset: &Dataset,
        field: FieldId,
        version: FieldVersion,
        summary: FieldSummary,
    ) -> Option<Arc<ScalarGrid2D>> {
        let snapshot = dataset.field_snapshot(field, version, Some(summary))?;
        Some(Arc::new(snapshot.payload.scalar_grid()?.clone()))
    }
}

/// What the user is told while a field's derived work is still running.
///
/// Contour geometry and the estimates it depends on are computed off the
/// interface thread, and until they land the plot is genuinely empty. Nothing
/// used to say so: the frame that would have explained the wait drew a blank
/// axis box, which reads exactly like a plot that failed. These are worded as
/// progress rather than as failure, and name the work rather than the object —
/// derived field work is content-addressed and shared between every plot
/// resolving the same key (§5–§6), so there is no per-object state to report and
/// none is introduced here.
const CONTOUR_GEOMETRY_PENDING_STATUS: &str = "Building contour geometry…";

/// Name the measurement being waited on rather than "an estimate": the two kinds
/// take visibly different times, and a user who knows which one is running knows
/// whether the anchor they just chose is the reason.
fn estimate_pending_status(keys: &[EstimateKey]) -> String {
    let noise = keys.iter().any(|key| key.kind == EstimateKind::Noise);
    let background = keys.iter().any(|key| key.kind == EstimateKind::Background);
    match (noise, background) {
        (true, true) => "Measuring this field's noise scale and background…".to_owned(),
        (false, true) => "Measuring this field's background…".to_owned(),
        // An empty list cannot reach here: resolution reports `Pending` only
        // when it has a key to wait for.
        (true, false) | (false, false) => "Measuring this field's noise scale…".to_owned(),
    }
}

/// Word the thresholds a field never reaches.
///
/// Both numbers appear side by side because that is what makes the common
/// mistake legible: a threshold of 20 against a peak of 10 reads as an extra
/// zero at a glance, where "no contours available" reads as a broken plot. The
/// message ends on the action that fixes it rather than on the failure.
fn unreachable_threshold_status(unreachable: &[UnreachableContourThreshold]) -> String {
    unreachable
        .iter()
        .map(|report| {
            let half = if report.negative {
                "negative"
            } else {
                "positive"
            };
            // The negative half's threshold and peak are magnitudes, so name the
            // peak as one instead of implying a signed comparison.
            let peak_label = if report.negative {
                "peak magnitude"
            } else {
                "peak"
            };
            let peak = format_level(report.peak.get());
            format!(
                "The {half} contour threshold {threshold} is above this field's {half} \
                 {peak_label} {peak}, so no {half} contours are drawn. \
                 Lower the threshold below {peak}.",
                threshold = format_level(report.threshold.get()),
            )
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Word the levels the renderer's segment budget left undrawn.
///
/// The count says how much of the ladder is missing, the magnitude says where
/// the plot stops being what the panel describes, and the sentence ends on the
/// one edit that recovers a complete picture. Naming the lowest level that *was*
/// drawn makes the fix concrete: set the ladder's floor there and every level it
/// then lists is a level actually on screen.
fn omitted_levels_status(omitted: &OmittedContourLevels) -> String {
    let count = usize::from(omitted.positive) + usize::from(omitted.negative);
    match omitted.lowest_drawn {
        Some(lowest) => format!(
            "The lowest {count} contour levels were not drawn: at {highest} and below, this \
             field crosses more of the grid than one plot can render. Raise the lowest level \
             to {lowest} or above to see every level the panel lists.",
            highest = format_magnitude(omitted.highest_omitted.get()),
            lowest = format_magnitude(lowest.get()),
        ),
        None => format!(
            "No contour levels were drawn: even the highest, {highest}, crosses more of the \
             field than one plot can render. Raise the lowest level well above {highest}, or \
             draw fewer levels.",
            highest = format_magnitude(omitted.highest_omitted.get()),
        ),
    }
}

/// Print a level a ladder computed, as opposed to one a user typed.
///
/// `format_level` echoes a threshold back verbatim because the user chose the
/// digits. A rung of a geometric ladder has no chosen digits — its exact value
/// is `base·ratio^k` and prints as sixteen of them — so it is shown to four
/// significant figures, which is both readable and enough to type back in.
fn format_magnitude(value: f64) -> String {
    format!("{value:.3e}")
}

/// Print a contour level so a mistyped magnitude stays legible: plain decimals
/// across the range users type by hand, scientific notation once a digit count
/// stops being readable. Only strictly positive magnitudes reach this.
fn format_level(value: f64) -> String {
    if value.abs() >= 1e5 || value.abs() < 1e-3 {
        format!("{value:.3e}")
    } else {
        format!("{value}")
    }
}

fn field_enqueue_error_status(error: FieldEnqueueError) -> String {
    match error {
        FieldEnqueueError::WorkersUnavailable => {
            "Background contour computation is unavailable in this session.".into()
        }
        FieldEnqueueError::VersionExhausted => {
            "Field runtime versions are exhausted; reopen PlotX to continue.".into()
        }
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
