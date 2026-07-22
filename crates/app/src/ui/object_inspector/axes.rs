use egui::{DragValue, Response, TextEdit, Ui};
use plotx_core::actions::Action;
use plotx_core::state::{AxisOverrides, AxisRange, ObjectId, PlotxApp};
use plotx_figure::AxisFrame;

#[derive(Clone, Copy)]
enum AxisKind {
    X,
    Y,
}

pub(super) fn commit_if_target_changed(app: &mut PlotxApp, target: Option<(usize, ObjectId)>) {
    let pending_target = app
        .session
        .ui
        .axis_overrides_before
        .as_ref()
        .map(|(canvas, object, _)| (*canvas, *object));
    if pending_target.is_some() && pending_target != target {
        commit_edit(app);
    }
}

pub(super) fn axes_section(
    app: &mut PlotxApp,
    canvas: usize,
    object: ObjectId,
    ui: &mut Ui,
) -> bool {
    let Some((x_auto, y_auto, hidden, x_categorical, y_categorical)) = app.doc.canvases[canvas]
        .object(object)
        .and_then(|object| object.plot())
        .map(|plot| {
            (
                plot.figure.x.label.clone(),
                plot.figure.y.label.clone(),
                plot.figure.axis_frame == AxisFrame::Hidden,
                plot.figure.x.categories.is_some(),
                plot.figure.y.categories.is_some(),
            )
        })
    else {
        return false;
    };

    ui.strong("Axes");
    let hidden_reason = "Choose a chart with visible axes to edit axis settings.";
    let mut focused = false;
    egui::Grid::new("object_axis_labels")
        .num_columns(2)
        .spacing([8.0, 4.0])
        .show(ui, |ui| {
            focused |= label_row(
                app,
                canvas,
                object,
                AxisKind::X,
                "X title",
                &x_auto,
                !hidden,
                hidden_reason,
                ui,
            );
            ui.end_row();
            focused |= label_row(
                app,
                canvas,
                object,
                AxisKind::Y,
                "Y title",
                &y_auto,
                !hidden,
                hidden_reason,
                ui,
            );
            ui.end_row();
        });

    let x_reason = if hidden {
        hidden_reason
    } else {
        "Choose a chart with a numeric x axis to set its range."
    };
    let y_reason = if hidden {
        hidden_reason
    } else {
        "Choose a chart with a numeric y axis to set its range."
    };
    range_row(
        app,
        canvas,
        object,
        AxisKind::X,
        "X range",
        !hidden && !x_categorical,
        x_reason,
        ui,
    );
    range_row(
        app,
        canvas,
        object,
        AxisKind::Y,
        "Y range",
        !hidden && !y_categorical,
        y_reason,
        ui,
    );

    focused
}

#[allow(clippy::too_many_arguments)]
fn label_row(
    app: &mut PlotxApp,
    canvas: usize,
    object: ObjectId,
    axis: AxisKind,
    label: &str,
    automatic: &str,
    enabled: bool,
    disabled_reason: &str,
    ui: &mut Ui,
) -> bool {
    let current = current_overrides(app, canvas, object);
    let mut text = match axis {
        AxisKind::X => current.x_label.clone(),
        AxisKind::Y => current.y_label.clone(),
    }
    .unwrap_or_default();
    ui.label(label);
    let response = ui
        .add_enabled(
            enabled,
            TextEdit::singleline(&mut text)
                .hint_text(automatic)
                .desired_width(132.0),
        )
        .on_disabled_hover_text(disabled_reason);
    if response.gained_focus() {
        begin_edit(app, canvas, object);
    }
    if response.changed() {
        let value = (!text.trim().is_empty()).then_some(text);
        let mut after = current_overrides(app, canvas, object);
        match axis {
            AxisKind::X => after.x_label = value,
            AxisKind::Y => after.y_label = value,
        }
        apply_live(app, canvas, object, after);
    }
    if response.lost_focus() {
        commit_edit(app);
    }
    response.has_focus()
}

#[allow(clippy::too_many_arguments)]
fn range_row(
    app: &mut PlotxApp,
    canvas: usize,
    object: ObjectId,
    axis: AxisKind,
    label: &str,
    enabled: bool,
    disabled_reason: &str,
    ui: &mut Ui,
) {
    let Some((overrides, view)) = app.doc.canvases[canvas]
        .object(object)
        .and_then(|object| object.plot())
        .map(|plot| {
            let view = match axis {
                AxisKind::X => plot.viewport.view_x,
                AxisKind::Y => plot.viewport.view_y,
            };
            (plot.axis_overrides.clone(), view)
        })
    else {
        return;
    };
    let manual = match axis {
        AxisKind::X => overrides.x_range,
        AxisKind::Y => overrides.y_range,
    };
    let mut automatic = manual.is_none();
    ui.horizontal(|ui| {
        ui.label(label);
        // A range retained across a switch to a categorical/hidden axis stays
        // inactive, but its Auto control remains available so it can be cleared.
        let auto_enabled = enabled || manual.is_some();
        let auto_response = ui
            .add_enabled(auto_enabled, egui::Checkbox::new(&mut automatic, "Auto"))
            .on_disabled_hover_text(disabled_reason);
        if auto_response.changed() {
            commit_edit(app);
            let before = current_overrides(app, canvas, object);
            let mut after = before.clone();
            let value = (!automatic).then_some(view);
            match axis {
                AxisKind::X => after.x_range = value,
                AxisKind::Y => after.y_range = value,
            }
            app.execute_action(Action::set_axis_overrides(canvas, object, before, after));
        }
    });

    let mut range = manual.unwrap_or(view);
    let span = range.span();
    let min_span = range.min.abs().max(range.max.abs()).max(span).max(1.0) * 1e-12;
    let min_upper = range.max - min_span;
    let max_lower = range.min + min_span;
    let auto_reason = match axis {
        AxisKind::X => "Turn off Auto to set a manual x range.",
        AxisKind::Y => "Turn off Auto to set a manual y range.",
    };
    let drag_reason = if enabled && automatic {
        auto_reason
    } else {
        disabled_reason
    };
    ui.horizontal(|ui| {
        ui.add_space(12.0);
        let min_response = ui
            .add_enabled(
                enabled && !automatic,
                range_drag(&mut range.min, span).range(f64::MIN..=min_upper),
            )
            .on_hover_text("Minimum")
            .on_disabled_hover_text(drag_reason);
        let max_response = ui
            .add_enabled(
                enabled && !automatic,
                range_drag(&mut range.max, span).range(max_lower..=f64::MAX),
            )
            .on_hover_text("Maximum")
            .on_disabled_hover_text(drag_reason);
        handle_range_response(
            app,
            canvas,
            object,
            axis,
            range,
            &min_response,
            &max_response,
        );
    });
}

fn range_drag(value: &mut f64, span: f64) -> DragValue<'_> {
    DragValue::new(value)
        .speed((span / 200.0).max(1e-9))
        .max_decimals(8)
}

fn handle_range_response(
    app: &mut PlotxApp,
    canvas: usize,
    object: ObjectId,
    axis: AxisKind,
    range: AxisRange,
    min_response: &Response,
    max_response: &Response,
) {
    if min_response.drag_started() || max_response.drag_started() {
        begin_edit(app, canvas, object);
    }
    let changed = min_response.changed() || max_response.changed();
    if changed && range.is_valid() {
        let mut after = current_overrides(app, canvas, object);
        match axis {
            AxisKind::X => after.x_range = Some(range),
            AxisKind::Y => after.y_range = Some(range),
        }
        apply_live(app, canvas, object, after);
    }
    let drag_stopped = min_response.drag_stopped() || max_response.drag_stopped();
    let typed_value = changed && !min_response.dragged() && !max_response.dragged();
    if drag_stopped || typed_value {
        commit_edit(app);
    }
}

fn current_overrides(app: &PlotxApp, canvas: usize, object: ObjectId) -> AxisOverrides {
    app.doc
        .canvases
        .get(canvas)
        .and_then(|canvas| canvas.object(object))
        .and_then(|object| object.plot())
        .map(|plot| plot.axis_overrides.clone())
        .unwrap_or_default()
}

fn begin_edit(app: &mut PlotxApp, canvas: usize, object: ObjectId) {
    if app.session.ui.axis_overrides_before.is_none() {
        app.session.ui.axis_overrides_before =
            Some((canvas, object, current_overrides(app, canvas, object)));
    }
}

fn apply_live(app: &mut PlotxApp, canvas: usize, object: ObjectId, after: AxisOverrides) {
    begin_edit(app, canvas, object);
    app.set_axis_overrides_value(canvas, object, &after);
    app.doc.dirty = true;
}

fn commit_edit(app: &mut PlotxApp) {
    app.finish_axis_overrides_edit();
}
