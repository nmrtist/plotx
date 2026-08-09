use super::*;
use plotx_analysis::alignment::{PeakPolarity, trace_peak_anchor};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TraceAlignmentMethod {
    TraceStart,
    PeakInWindow {
        lo: f64,
        hi: f64,
        polarity: PeakPolarity,
    },
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TraceAlignmentRequest {
    pub canvas: CanvasId,
    pub object: ObjectId,
    pub reference: SeriesId,
    pub method: TraceAlignmentMethod,
}

#[derive(Clone, Debug, PartialEq)]
pub enum TraceAlignmentOutcome {
    Align {
        anchor: f64,
        delta: f64,
        resulting_shift: f64,
    },
    Reference {
        anchor: f64,
    },
    Skipped(String),
}

#[derive(Clone, Debug, PartialEq)]
pub struct TraceAlignmentRow {
    pub series: SeriesId,
    pub label: String,
    pub current_shift: f64,
    pub outcome: TraceAlignmentOutcome,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TraceAlignmentPlan {
    pub request: TraceAlignmentRequest,
    pub x_unit: String,
    pub reference_anchor: Option<f64>,
    pub rows: Vec<TraceAlignmentRow>,
}

impl TraceAlignmentPlan {
    pub fn alignment_count(&self) -> usize {
        self.rows
            .iter()
            .filter(|row| matches!(row.outcome, TraceAlignmentOutcome::Align { .. }))
            .count()
    }
}

impl PlotxApp {
    /// Resolve the plot targeted by global alignment entry points such as the Ribbon.
    pub fn trace_alignment_target(&self) -> Option<(CanvasId, ObjectId)> {
        let ci = self.session.active_canvas?;
        let canvas = self.doc.canvases.get(ci)?;
        let object = if let Some(selected) = canvas.selected_object {
            canvas.object(selected)?.plot().map(|_| selected)?
        } else {
            let mut plots = canvas
                .objects
                .iter()
                .filter(|object| object.plot().is_some())
                .map(|object| object.id);
            let only = plots.next()?;
            plots.next().is_none().then_some(only)?
        };
        self.can_align_plot_traces(canvas.resource_id, object)
            .then_some((canvas.resource_id, object))
    }

    pub fn line_series_x_unit(&self, series: &SeriesBinding) -> Option<String> {
        line_x_unit(self, series)
    }

    pub fn trace_alignment_x_unit(
        &self,
        canvas: CanvasId,
        object: ObjectId,
        series: SeriesId,
    ) -> Option<String> {
        let ci = self.doc.canvas_index(canvas)?;
        let plot = self.doc.canvases[ci].object(object)?.plot()?;
        let displayed = self.display_binding(plot.display_owner, &plot.binding);
        let series = displayed
            .series
            .iter()
            .find(|candidate| candidate.id == series)?;
        line_x_unit(self, series)
    }

    pub fn default_trace_alignment_reference(
        &self,
        canvas: CanvasId,
        object: ObjectId,
    ) -> Option<SeriesId> {
        let ci = self.doc.canvas_index(canvas)?;
        let plot = self.doc.canvases[ci].object(object)?.plot()?;
        let displayed = self.display_binding(plot.display_owner, &plot.binding);
        displayed.series.iter().find_map(|candidate| {
            let unit = eligible_trace_unit(self, candidate)?;
            (displayed
                .series
                .iter()
                .filter(|series| eligible_trace_unit(self, series).as_ref() == Some(&unit))
                .count()
                >= 2)
                .then_some(candidate.id)
        })
    }

    pub fn can_align_plot_traces(&self, canvas: CanvasId, object: ObjectId) -> bool {
        self.default_trace_alignment_reference(canvas, object)
            .is_some()
    }

    pub fn plan_trace_alignment(
        &mut self,
        request: TraceAlignmentRequest,
    ) -> Result<TraceAlignmentPlan, String> {
        if !trace_alignment_method_valid(request.method) {
            return Err("Alignment settings must be finite and the window must have width.".into());
        }
        let ci = self
            .doc
            .canvas_index(request.canvas)
            .ok_or_else(|| "The alignment page is no longer available.".to_owned())?;
        let (persisted, display_owner, chart, frame) = self.doc.canvases[ci]
            .object(request.object)
            .and_then(|object| {
                object.plot().map(|plot| {
                    (
                        plot.binding.clone(),
                        plot.display_owner,
                        plot.chart.clone(),
                        object.frame,
                    )
                })
            })
            .ok_or_else(|| "The alignment plot is no longer available.".to_owned())?;
        let binding = self.display_binding(display_owner, &persisted);
        let reference = binding
            .series
            .iter()
            .find(|series| series.id == request.reference)
            .ok_or_else(|| "The reference series is no longer available.".to_owned())?;
        if !reference.visible
            || !matches!(reference.encoding, plotx_figure::SeriesEncoding::Line(_))
        {
            return Err("The reference must be a visible line series.".into());
        }
        let reference_unit = line_x_unit(self, reference)
            .ok_or_else(|| "The reference trace has no x-axis unit contract.".to_owned())?;
        for series in &binding.series {
            validate_live_line_source(self, series)?;
        }

        let size = [frame.width / MM_TO_PT, frame.height / MM_TO_PT];
        let mut anchors = std::collections::BTreeMap::new();
        for series in binding.series.iter().filter(|series| {
            series.visible && matches!(series.encoding, plotx_figure::SeriesEncoding::Line(_))
        }) {
            let mut materialized = series.clone();
            materialized.visible = true;
            let figure = self.build_binding_figure(
                &DataBinding {
                    series: vec![materialized],
                },
                &chart,
                &StackSpec::default(),
                size,
            );
            anchors.insert(series.id, detected_anchor(&figure, request.method));
        }
        let reference_anchor = anchors.get(&request.reference).copied().flatten();
        let mut rows = Vec::with_capacity(binding.series.len());
        for series in &binding.series {
            let current_shift = series.line_x_shift().unwrap_or(0.0);
            let label = alignment_series_label(self, series);
            let outcome = if !matches!(series.encoding, plotx_figure::SeriesEncoding::Line(_)) {
                TraceAlignmentOutcome::Skipped("Not a line series.".into())
            } else if !series.visible {
                TraceAlignmentOutcome::Skipped(
                    "Hidden series are not aligned automatically.".into(),
                )
            } else if line_x_unit(self, series).as_ref() != Some(&reference_unit) {
                TraceAlignmentOutcome::Skipped("The x-axis unit differs from the reference.".into())
            } else {
                match (reference_anchor, anchors.get(&series.id).copied().flatten()) {
                    (None, _) => {
                        TraceAlignmentOutcome::Skipped("The reference has no usable anchor.".into())
                    }
                    (_, None) => TraceAlignmentOutcome::Skipped(match request.method {
                        TraceAlignmentMethod::TraceStart => "No finite plotted sample.".into(),
                        TraceAlignmentMethod::PeakInWindow { .. } => {
                            "No significant peak in the window.".into()
                        }
                    }),
                    (Some(anchor), Some(_)) if series.id == request.reference => {
                        TraceAlignmentOutcome::Reference { anchor }
                    }
                    (Some(reference), Some(anchor)) => {
                        let delta = reference - anchor;
                        let resulting_shift = current_shift + delta;
                        if !delta.is_finite() || !resulting_shift.is_finite() {
                            return Err("Alignment produced a non-finite shift.".into());
                        }
                        TraceAlignmentOutcome::Align {
                            anchor,
                            delta,
                            resulting_shift,
                        }
                    }
                }
            };
            rows.push(TraceAlignmentRow {
                series: series.id,
                label,
                current_shift,
                outcome,
            });
        }
        Ok(TraceAlignmentPlan {
            request,
            x_unit: reference_unit,
            reference_anchor,
            rows,
        })
    }

    pub fn apply_trace_alignment(
        &mut self,
        request: TraceAlignmentRequest,
    ) -> Result<usize, String> {
        // Always recompute from current absolute shifts. The preview is advisory
        // and cannot smuggle stale series identities into a document action.
        let plan = self.plan_trace_alignment(request)?;
        if plan.alignment_count() == 0 {
            return Err("No non-reference series can be aligned.".into());
        }
        let ci = self
            .doc
            .canvas_index(request.canvas)
            .ok_or_else(|| "The alignment page is no longer available.".to_owned())?;
        let (before, display_owner) = self.doc.canvases[ci]
            .object(request.object)
            .and_then(CanvasObject::plot)
            .map(|plot| (plot.binding.clone(), plot.display_owner))
            .ok_or_else(|| "The alignment plot is no longer available.".to_owned())?;
        let mut displayed_after = self.display_binding(display_owner, &before);
        for row in &plan.rows {
            let TraceAlignmentOutcome::Align {
                resulting_shift, ..
            } = row.outcome
            else {
                continue;
            };
            let series = displayed_after
                .series
                .iter_mut()
                .find(|series| series.id == row.series)
                .ok_or_else(|| "A series changed before alignment could be applied.".to_owned())?;
            if !series.set_line_x_shift(resulting_shift) {
                return Err("Alignment produced an invalid line shift.".into());
            }
        }
        let after = self.merge_display_binding(display_owner, &before, displayed_after);
        self.execute_action(Action::set_series_presentation(
            ci,
            request.object,
            before,
            after,
        ));
        Ok(plan.alignment_count())
    }
}

fn trace_alignment_method_valid(method: TraceAlignmentMethod) -> bool {
    match method {
        TraceAlignmentMethod::TraceStart => true,
        TraceAlignmentMethod::PeakInWindow { lo, hi, .. } => {
            lo.is_finite() && hi.is_finite() && lo != hi
        }
    }
}

fn line_x_unit(app: &PlotxApp, series: &SeriesBinding) -> Option<String> {
    app.doc
        .dataset_by_id(series.source.resource)
        .and_then(|dataset| dataset.field_descriptor(series.source.field))
        .and_then(|descriptor| descriptor.line_x_unit().map(str::to_owned))
}

fn eligible_trace_unit(app: &PlotxApp, series: &SeriesBinding) -> Option<String> {
    (series.visible && matches!(series.encoding, plotx_figure::SeriesEncoding::Line(_)))
        .then(|| line_x_unit(app, series))?
}

fn validate_live_line_source(app: &PlotxApp, series: &SeriesBinding) -> Result<(), String> {
    if !matches!(series.encoding, plotx_figure::SeriesEncoding::Line(_)) {
        return Ok(());
    }
    let dataset = app
        .doc
        .dataset_by_id(series.source.resource)
        .ok_or_else(|| format!("Series {} references a missing dataset.", series.id))?;
    if !dataset.has_field(series.source.field)
        || !dataset.supports_encoding(series.source.field, &series.encoding)
    {
        return Err(format!(
            "Series {} references an unavailable line field.",
            series.id
        ));
    }
    match (
        series.source.item,
        dataset.trace_collection(series.source.field),
    ) {
        (Some(item), Some(collection)) if collection.item(item).is_some() => Ok(()),
        (Some(_), _) => Err(format!("Series {} references a missing trace.", series.id)),
        (None, Some(_)) => Err(format!(
            "Series {} does not identify a trace item.",
            series.id
        )),
        (None, None) => Ok(()),
    }
}

fn detected_anchor(figure: &plotx_figure::Figure, method: TraceAlignmentMethod) -> Option<f64> {
    match method {
        TraceAlignmentMethod::TraceStart => figure
            .series
            .iter()
            .flat_map(|series| series.points.iter())
            .find_map(|point| (point[0].is_finite() && point[1].is_finite()).then_some(point[0])),
        TraceAlignmentMethod::PeakInWindow { lo, hi, polarity } => {
            let mut x = Vec::new();
            let mut y = Vec::new();
            for point in figure.series.iter().flat_map(|series| &series.points) {
                if point[0].is_finite() && point[1].is_finite() {
                    x.push(point[0]);
                    y.push(point[1]);
                }
            }
            trace_peak_anchor(&x, &y, lo, hi, polarity)
        }
    }
}

fn alignment_series_label(app: &PlotxApp, series: &SeriesBinding) -> String {
    let item = app.series_label(series);
    let source = app
        .doc
        .dataset_by_id(series.source.resource)
        .map(Dataset::display_name)
        .unwrap_or_else(|| "Missing source".into());
    format!("{source} — {item}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trace_start_requires_one_finite_plotted_sample() {
        let figure = plotx_figure::Figure::new(
            "trace",
            plotx_figure::Axis::new("x", 0.0, 2.0),
            plotx_figure::Axis::new("y", 0.0, 2.0),
        )
        .with_series(plotx_figure::Series::line(
            "trace",
            vec![[0.0, f64::NAN], [1.0, 2.0]],
        ));
        assert_eq!(
            detected_anchor(&figure, TraceAlignmentMethod::TraceStart),
            Some(1.0)
        );
    }
}
