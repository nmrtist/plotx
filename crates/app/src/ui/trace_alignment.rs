use egui::{ComboBox, DragValue};
use plotx_analysis::alignment::PeakPolarity;
use plotx_core::state::{
    CanvasId, ObjectId, PlotxApp, TraceAlignmentDialogState, TraceAlignmentMethod,
    TraceAlignmentOutcome, TraceAlignmentRequest,
};

pub(crate) fn open_trace_alignment_dialog(app: &mut PlotxApp, canvas: CanvasId, object: ObjectId) {
    let Some(reference) = app.default_trace_alignment_reference(canvas, object) else {
        app.session.status = "Trace alignment needs at least two visible line series.".into();
        return;
    };
    let Some(ci) = app.doc.canvas_index(canvas) else {
        return;
    };
    let (lo, hi) = app.doc.canvases[ci]
        .object(object)
        .and_then(|object| object.plot())
        .map(|plot| (plot.figure().x.min, plot.figure().x.max))
        .unwrap_or((0.0, 1.0));
    app.session.ui.trace_alignment_dialog = Some(TraceAlignmentDialogState {
        canvas,
        object,
        reference,
        method: TraceAlignmentMethod::TraceStart,
        peak_window: (lo, hi),
        peak_polarity: PeakPolarity::Positive,
        plan: None,
        history_mark: (
            app.session.undo_stack.len(),
            app.session.redo_stack.len(),
            app.doc.edit_generation,
        ),
    });
}

pub(crate) fn open_active_trace_alignment_dialog(app: &mut PlotxApp) {
    let Some((canvas, object)) = app.trace_alignment_target() else {
        app.session.status = "Select a plot with at least two compatible line series.".into();
        return;
    };
    open_trace_alignment_dialog(app, canvas, object);
}

pub(crate) fn trace_alignment_window(app: &mut PlotxApp, ctx: &egui::Context) {
    let Some(mut state) = app.session.ui.trace_alignment_dialog.take() else {
        return;
    };
    let Some(ci) = app.doc.canvas_index(state.canvas) else {
        app.session.status = "The alignment page is no longer available.".into();
        return;
    };
    let Some((persisted, owner)) = app.doc.canvases[ci]
        .object(state.object)
        .and_then(|object| object.plot())
        .map(|plot| (plot.binding.clone(), plot.display_owner))
    else {
        app.session.status = "The alignment plot is no longer available.".into();
        return;
    };
    let binding = app.display_binding(owner, &persisted);
    let visible_lines: Vec<_> = binding
        .series
        .iter()
        .filter(|series| {
            series.visible && matches!(series.encoding, plotx_figure::SeriesEncoding::Line(_))
        })
        .collect();
    let reference_choices: Vec<_> = visible_lines
        .iter()
        .copied()
        .filter(|candidate| {
            let unit = app.line_series_x_unit(candidate);
            unit.is_some()
                && visible_lines
                    .iter()
                    .filter(|series| app.line_series_x_unit(series) == unit)
                    .count()
                    >= 2
        })
        .collect();
    if reference_choices.is_empty() {
        app.session.status = "Trace alignment needs at least two visible line series.".into();
        return;
    }
    if !reference_choices
        .iter()
        .any(|series| series.id == state.reference)
    {
        state.reference = reference_choices[0].id;
    }
    let x_unit = app
        .trace_alignment_x_unit(state.canvas, state.object, state.reference)
        .unwrap_or_else(|| "x".into());
    let mut changed = false;
    let mut apply = false;
    let mut cancel = false;
    let available = ctx.content_rect().size() - egui::vec2(32.0, 48.0);
    let size = egui::vec2(820.0, 520.0).min(available).max(egui::vec2(
        320.0_f32.min(available.x),
        280.0_f32.min(available.y),
    ));
    let modal = super::modal(ctx, "trace_alignment_modal", super::ModalKind::Dialog).show(ctx, |ui| {
        ui.set_min_size(size);
        ui.set_max_size(size);
        ui.heading("Align traces");
        ui.separator();
        ui.label("Shift visible traces along x without changing their source data.");
        ui.horizontal_wrapped(|ui| {
            ui.label("Method");
            let start = matches!(state.method, TraceAlignmentMethod::TraceStart);
            if ui.selectable_label(start, "Trace start").clicked() && !start {
                state.method = TraceAlignmentMethod::TraceStart;
                changed = true;
            }
            let peak = !start;
            if ui.selectable_label(peak, "Peak in window").clicked() && !peak {
                let (lo, hi) = state.peak_window;
                state.method = TraceAlignmentMethod::PeakInWindow {
                    lo,
                    hi,
                    polarity: state.peak_polarity,
                };
                changed = true;
            }
        });
        ui.horizontal_wrapped(|ui| {
            ui.label("Reference");
            let selected = reference_choices
                .iter()
                .find(|series| series.id == state.reference)
                .map(|series| trace_label(app, series))
                .unwrap_or_else(|| "Unavailable".into());
            ComboBox::from_id_salt("trace_alignment_reference")
                .selected_text(selected)
                .show_ui(ui, |ui| {
                    for series in &reference_choices {
                        let label = trace_label(app, series);
                        if ui.selectable_label(series.id == state.reference, &label)
                            .on_hover_text(&label)
                            .clicked()
                        {
                            state.reference = series.id;
                            changed = true;
                            ui.close();
                        }
                    }
                });
        });
        if let TraceAlignmentMethod::PeakInWindow { lo, hi, polarity } = &mut state.method {
            ui.horizontal_wrapped(|ui| {
                ui.label(format!("Window ({x_unit})"));
                changed |= ui.add(DragValue::new(lo).speed(0.01).max_decimals(6)).changed();
                ui.label("to");
                changed |= ui.add(DragValue::new(hi).speed(0.01).max_decimals(6)).changed();
                ui.label("Polarity");
                ComboBox::from_id_salt("trace_alignment_polarity")
                    .selected_text(polarity_label(*polarity))
                    .show_ui(ui, |ui| {
                        for option in [PeakPolarity::Positive, PeakPolarity::Negative, PeakPolarity::Magnitude] {
                            changed |= ui.selectable_value(polarity, option, polarity_label(option)).changed();
                        }
                    });
            });
            state.peak_window = (*lo, *hi);
            state.peak_polarity = *polarity;
            ui.weak("Positive finds upward peaks; Negative finds downward peaks; Magnitude compares positive and negative prominence.");
        } else {
            ui.weak("Trace start is the first finite plotted sample, not stimulus onset detection.");
        }

        let mark = (
            app.session.undo_stack.len(),
            app.session.redo_stack.len(),
            app.doc.edit_generation,
        );
        let request = TraceAlignmentRequest {
            canvas: state.canvas,
            object: state.object,
            reference: state.reference,
            method: state.method,
        };
        if changed || state.plan.is_none() || state.history_mark != mark {
            state.plan = Some(app.plan_trace_alignment(request));
            state.history_mark = mark;
        }
        ui.separator();
        egui::ScrollArea::both()
            .id_salt("trace_alignment_rows")
            .max_height((size.y - 250.0).max(100.0))
            .show(ui, |ui| {
                egui::Grid::new("trace_alignment_grid").striped(true).show(ui, |ui| {
                    for heading in [
                        "Series".to_owned(),
                        format!("Current shift ({x_unit})"),
                        format!("Anchor ({x_unit})"),
                        format!("Delta ({x_unit})"),
                        format!("Result ({x_unit})"),
                    ] {
                        ui.strong(heading);
                    }
                    ui.end_row();
                    if let Some(Ok(plan)) = &state.plan {
                        for row in &plan.rows {
                            ui.add_sized([250.0, 20.0], egui::Label::new(&row.label).truncate())
                                .on_hover_text(&row.label);
                            ui.monospace(format_number(row.current_shift));
                            match &row.outcome {
                                TraceAlignmentOutcome::Align { anchor, delta, resulting_shift } => {
                                    ui.monospace(format_number(*anchor));
                                    ui.monospace(format_signed(*delta));
                                    ui.monospace(format_number(*resulting_shift));
                                }
                                TraceAlignmentOutcome::Reference { anchor } => {
                                    ui.monospace(format_number(*anchor));
                                    ui.weak("Reference");
                                    ui.monospace(format_number(row.current_shift));
                                }
                                TraceAlignmentOutcome::Skipped(reason) => {
                                    ui.weak("—");
                                    ui.colored_label(ui.visuals().warn_fg_color, "Skipped");
                                    ui.weak(reason);
                                }
                            }
                            ui.end_row();
                        }
                    }
                });
            });
        if let Some(Err(error)) = &state.plan {
            ui.colored_label(ui.visuals().error_fg_color, error);
        }
        ui.separator();
        let can_apply = state.plan.as_ref().is_some_and(|plan| {
            plan.as_ref().is_ok_and(|plan| plan.alignment_count() > 0)
        });
        ui.horizontal(|ui| {
            if ui.add_enabled(can_apply, egui::Button::new("Apply")).clicked() {
                apply = true;
            }
            if ui.button("Cancel").clicked() {
                cancel = true;
            }
        });
    });

    if apply {
        let request = TraceAlignmentRequest {
            canvas: state.canvas,
            object: state.object,
            reference: state.reference,
            method: state.method,
        };
        match app.apply_trace_alignment(request) {
            Ok(count) => app.session.status = format!("Aligned {count} traces."),
            Err(error) => {
                app.session.status = error;
                app.session.ui.trace_alignment_dialog = Some(state);
            }
        }
    } else if !cancel && !modal.should_close() {
        app.session.ui.trace_alignment_dialog = Some(state);
    }
}

fn polarity_label(polarity: PeakPolarity) -> &'static str {
    match polarity {
        PeakPolarity::Positive => "Positive",
        PeakPolarity::Negative => "Negative",
        PeakPolarity::Magnitude => "Magnitude",
    }
}

fn format_number(value: f64) -> String {
    format!("{value:.6}")
}

fn format_signed(value: f64) -> String {
    format!("{value:+.6}")
}

fn trace_label(app: &PlotxApp, series: &plotx_core::state::SeriesBinding) -> String {
    let source = app
        .doc
        .dataset_by_id(series.source.resource)
        .map(plotx_core::state::Dataset::display_name)
        .unwrap_or_else(|| "Missing source".into());
    format!("{source} — {}", app.series_label(series))
}
