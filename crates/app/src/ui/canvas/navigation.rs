use super::*;

/// The ambient navigation layer: pan and zoom, available under every tool,
/// acting on the plot panel under the cursor (its data viewport) or on the board
/// when the cursor is over empty space. Holding Cmd/Ctrl promotes navigation to
/// the board even over a panel. Returns `true` when it consumes the gesture.
pub(crate) fn handle_navigation(app: &mut PlotxApp, ci: usize, rect: egui::Rect, ui: &Ui) -> bool {
    let (hover, scroll, zoom_delta, delta, dbl) = ui.input(|i| {
        (
            i.pointer.hover_pos(),
            i.smooth_scroll_delta,
            i.zoom_delta(),
            i.pointer.delta(),
            i.pointer
                .button_double_clicked(egui::PointerButton::Primary),
        )
    });
    let (
        primary_down,
        primary_pressed,
        primary_released,
        middle_down,
        middle_pressed,
        middle_released,
    ) = ui.input(|i| {
        (
            i.pointer.primary_down(),
            i.pointer.primary_pressed(),
            i.pointer.primary_released(),
            i.pointer.middle_down(),
            i.pointer.button_pressed(egui::PointerButton::Middle),
            i.pointer.button_released(egui::PointerButton::Middle),
        )
    });
    let (command, space_down, alt, now) = ui.input(|i| {
        (
            i.modifiers.command || i.modifiers.ctrl,
            i.key_down(egui::Key::Space),
            i.modifiers.alt,
            i.time,
        )
    });
    let typing = ui.ctx().egui_wants_keyboard_input();

    // Every viewport zoom drag is owned here start-to-finish so the ambient
    // Alt+drag box gesture and the dedicated Browse Zoom tool share one path.
    let active_zoom = match &app.session.ui.interaction {
        Interaction::Zoom(d) => Some(*d),
        _ => None,
    };
    if let Some(drag) = active_zoom {
        if let Some(pp) = hover
            && let Interaction::Zoom(d) = &mut app.session.ui.interaction
        {
            d.current = [pp.x, pp.y];
        }
        if primary_released || !primary_down {
            if let Some(plot) = plot_inner_rect(app, drag.canvas, drag.object, rect)
                && let Interaction::Zoom(d) = app.take_interaction()
            {
                if d.axis == ZoomAxis::Box {
                    finish_zoom_drag(app, drag.canvas, drag.object, plot, d);
                } else {
                    finish_axis_zoom(app, drag.canvas, drag.object, plot, d);
                }
            } else {
                app.reset_interaction();
            }
        }
        ui.ctx().request_repaint();
        return true;
    }

    let panning = matches!(app.session.ui.interaction, Interaction::Pan(_));
    let pan_input = !typing && (middle_down || (space_down && primary_down));

    // Finish an in-flight data-pan even if the pointer has left the canvas.
    let Some(p) = hover.filter(|p| rect.contains(*p)) else {
        if panning && (middle_released || primary_released || !pan_input) {
            commit_data_pan(app);
            return true;
        }
        return panning;
    };

    let data_target = if command {
        None
    } else {
        plot_under_cursor(app, ci, rect, p)
    };

    // Continue / end an in-flight data-pan; its target is locked at gesture start.
    if panning {
        let target = if let Interaction::Pan(drag) = &app.session.ui.interaction {
            Some((drag.canvas, drag.object))
        } else {
            None
        };
        if let Some((dci, did)) = target
            && pan_input
            && delta != Vec2::ZERO
            && let Some(plot) = plot_inner_rect(app, dci, did, rect)
        {
            app.session.board_fit = None;
            apply_plot_pan(app, dci, did, plot, delta);
            ui.ctx().request_repaint();
        }
        if middle_released || primary_released || !pan_input {
            commit_data_pan(app);
        }
        return true;
    }

    if dbl && let Some((id, outer, plot)) = data_target {
        reset_plot_viewport(app, ci, id, outer, plot, p);
        return true;
    }

    let pinch = (zoom_delta - 1.0).abs() > 0.001;
    let wheel = scroll.y.abs() > 0.0;
    if !typing && (pinch || wheel) {
        let consumed = match data_target {
            Some((id, outer, plot)) => {
                if pinch {
                    app.finish_pending_wheel_property(now, true);
                    let scale = (1.0 / f64::from(zoom_delta)).clamp(0.2, 5.0);
                    app.session.board_fit = None;
                    zoom_plot_viewport(app, ci, id, outer, plot, p, scale, (true, true), now, ui);
                    true
                } else if alt && hit_zone(p, outer, plot) == HitZone::Plot {
                    app.finish_pending_wheel_zoom(now, true);
                    adjust_plot_display(app, ci, id, plot, p, scroll.y, now, ui)
                } else {
                    app.finish_pending_wheel_property(now, true);
                    let axes = wheel_zoom_axes(app, ci, id, hit_zone(p, outer, plot));
                    let Some(axes) = axes else {
                        return false;
                    };
                    let scale = f64::from((-scroll.y * WHEEL_ZOOM_SPEED).exp()).clamp(0.2, 5.0);
                    app.session.board_fit = None;
                    zoom_plot_viewport(app, ci, id, outer, plot, p, scale, axes, now, ui);
                    true
                }
            }
            None => {
                let factor = if pinch {
                    zoom_delta
                } else {
                    (scroll.y * WHEEL_ZOOM_SPEED).exp()
                };
                app.session.board_fit = None;
                zoom_board_view(app, rect, p, factor);
                ui.ctx().request_repaint();
                true
            }
        };
        return consumed;
    }

    if !typing
        && primary_pressed
        && !pan_input
        && matches!(app.session.ui.interaction, Interaction::Idle)
        && let Some((id, outer, plot)) = data_target
    {
        let axis = match hit_zone(p, outer, plot) {
            HitZone::XAxis => Some(ZoomAxis::X),
            HitZone::YAxis => Some(ZoomAxis::Y),
            HitZone::Plot if alt => Some(ZoomAxis::Box),
            HitZone::Plot | HitZone::None => None,
        };
        if let Some(axis) = axis {
            freeze_board_for_gesture(app);
            app.begin_interaction(Interaction::Zoom(ZoomDrag {
                canvas: ci,
                object: id,
                start: [p.x, p.y],
                current: [p.x, p.y],
                axis,
            }));
            return true;
        }
    }

    let pan_started = middle_pressed || (space_down && primary_pressed);
    if pan_input {
        if pan_started
            && matches!(app.session.ui.interaction, Interaction::Idle)
            && let Some((id, _outer, _plot)) = data_target
            && let Some(before) = app.doc.canvases[ci]
                .object(id)
                .and_then(|object| object.plot())
                .map(|plot_object| plot_object.viewport.clone())
        {
            freeze_board_for_gesture(app);
            app.begin_interaction(Interaction::Pan(PanDrag {
                canvas: ci,
                object: id,
                before,
            }));
            return true;
        }
        if delta != Vec2::ZERO {
            app.session.board_fit = None;
            app.session.board.auto_fit = false;
            app.session.board.pan[0] += delta.x;
            app.session.board.pan[1] += delta.y;
        }
        ui.ctx().request_repaint();
        return true;
    }

    false
}

pub(crate) fn zoom_board_view(app: &mut PlotxApp, rect: egui::Rect, anchor: Pos2, factor: f32) {
    let old_zoom = app.session.board.zoom.max(0.01);
    let new_zoom = (old_zoom * factor).clamp(0.05, 8.0);
    let world_x = (anchor.x - rect.left() - app.session.board.pan[0]) / old_zoom;
    let world_y = (anchor.y - rect.top() - app.session.board.pan[1]) / old_zoom;
    app.session.board.zoom = new_zoom;
    app.session.board.pan = [
        anchor.x - rect.left() - world_x * new_zoom,
        anchor.y - rect.top() - world_y * new_zoom,
    ];
    app.session.board.auto_fit = false;
}

/// The pan gesture's undo bracket is owned by the caller (`handle_navigation`).
pub(crate) fn apply_plot_pan(
    app: &mut PlotxApp,
    ci: usize,
    object_id: ObjectId,
    plot: PlotRect,
    delta: Vec2,
) {
    let Some(plot_object) = app.doc.canvases[ci]
        .object_mut(object_id)
        .and_then(|object| object.plot_mut())
    else {
        return;
    };
    let fig = plot_object.figure();
    let x_sign = if fig.x.reversed { 1.0 } else { -1.0 };
    let y_sign = if fig.y.reversed { -1.0 } else { 1.0 };
    let dx = x_sign * f64::from(delta.x) / f64::from(plot.width.max(1.0)) * fig.x.span();
    let dy = y_sign * f64::from(delta.y) / f64::from(plot.height.max(1.0)) * fig.y.span();
    plot_object.viewport.view_x = AxisRange::new(
        plot_object.viewport.view_x.min + dx,
        plot_object.viewport.view_x.max + dx,
    )
    .clamp_to(plot_object.viewport.full_x);
    plot_object.viewport.view_y = AxisRange::new(
        plot_object.viewport.view_y.min + dy,
        plot_object.viewport.view_y.max + dy,
    )
    .clamp_to(plot_object.viewport.full_y);
    plot_object.viewport.auto_y = false;
    plot_object.apply_viewport();
    app.doc.dirty = true;
}

pub(crate) fn commit_data_pan(app: &mut PlotxApp) {
    if let Interaction::Pan(drag) = app.take_interaction()
        && let Some(object) = app.doc.canvases[drag.canvas]
            .object(drag.object)
            .and_then(|object| object.plot())
    {
        app.commit_object_viewport(
            drag.canvas,
            drag.object,
            drag.before,
            object.viewport.clone(),
        );
    }
}

pub(crate) fn finish_axis_zoom(
    app: &mut PlotxApp,
    ci: usize,
    object_id: ObjectId,
    plot: PlotRect,
    drag: ZoomDrag,
) {
    let a = clamp_to_plot(plot, pos(drag.start));
    let b = clamp_to_plot(plot, pos(drag.current));
    let Some(plot_object) = app.doc.canvases[ci]
        .object(object_id)
        .and_then(|object| object.plot())
    else {
        return;
    };
    let fig = plot_object.figure();
    let before = plot_object.viewport.clone();
    let (x, y) = match drag.axis {
        ZoomAxis::X => {
            if (a.x - b.x).abs() < SELECT_MIN_PX {
                return;
            }
            let range = AxisRange::new(
                screen_to_x(a.x, plot, fig.x.min, fig.x.span(), fig.x.reversed),
                screen_to_x(b.x, plot, fig.x.min, fig.x.span(), fig.x.reversed),
            );
            (Some(range), None)
        }
        ZoomAxis::Y => {
            if (a.y - b.y).abs() < SELECT_MIN_PX {
                return;
            }
            let range = AxisRange::new(
                screen_to_y(a.y, plot, fig.y.min, fig.y.span(), fig.y.reversed),
                screen_to_y(b.y, plot, fig.y.min, fig.y.span(), fig.y.reversed),
            );
            (None, Some(range))
        }
        ZoomAxis::Box => return,
    };
    let mut after = before.clone();
    after.select(fig, x, y);
    app.commit_object_viewport(ci, object_id, before, after);
    app.session.status = "Zoomed axis.".into();
}

pub(crate) fn reset_plot_viewport(
    app: &mut PlotxApp,
    ci: usize,
    object_id: ObjectId,
    outer_rect: EguiRect,
    plot: PlotRect,
    p: Pos2,
) {
    let Some(plot_object) = app.doc.canvases[ci]
        .object(object_id)
        .and_then(|object| object.plot())
    else {
        return;
    };
    let before = plot_object.viewport.clone();
    let mut after = before.clone();
    match hit_zone(p, outer_rect, plot) {
        HitZone::XAxis => after.reset_x(plot_object.figure()),
        HitZone::YAxis => after.reset_y(plot_object.figure()),
        HitZone::Plot => after.reset_all(),
        HitZone::None => return,
    }
    app.commit_object_viewport(ci, object_id, before, after);
}

/// The axes a plain wheel gesture addresses. A line plot is conventionally
/// navigated along its independent x coordinate; a raster-like field has two
/// spatial coordinates and navigates both. Axis strips always override that
/// body convention with the one axis they explicitly name.
fn wheel_zoom_axes(
    app: &PlotxApp,
    canvas: usize,
    object: ObjectId,
    zone: HitZone,
) -> Option<(bool, bool)> {
    match zone {
        HitZone::XAxis => Some((true, false)),
        HitZone::YAxis => Some((false, true)),
        HitZone::Plot => {
            let two_dimensional = app.doc.canvases[canvas]
                .object(object)
                .and_then(|object| object.plot())
                .is_some_and(|plot| {
                    plot.binding.series.iter().any(|series| {
                        matches!(
                            &series.encoding,
                            plotx_figure::SeriesEncoding::Contour(_)
                                | plotx_figure::SeriesEncoding::Heatmap(_)
                                | plotx_figure::SeriesEncoding::Image(_)
                        )
                    })
                });
            Some((true, two_dimensional))
        }
        HitZone::None => None,
    }
}

/// Alt+wheel changes the unique display-sensitivity property exposed by the
/// hovered series. If there is no such property, a numeric 1D plot falls back
/// to y-scale intensity. Different eligible encodings are refused explicitly.
#[allow(clippy::too_many_arguments)]
fn adjust_plot_display(
    app: &mut PlotxApp,
    canvas: usize,
    object: ObjectId,
    plot: PlotRect,
    pointer: Pos2,
    scroll_y: f32,
    now: f64,
    ui: &Ui,
) -> bool {
    use crate::ui::properties::discovery::{CanvasStepTarget, canvas_step_target};

    if app.doc.canvases[canvas]
        .object(object)
        .is_some_and(|object| object.locked)
    {
        app.session.status = crate::ui::properties::discovery::LOCKED_REASON.to_owned();
        return true;
    }

    match canvas_step_target(app, canvas, object) {
        CanvasStepTarget::Ambiguous { labels } => {
            app.finish_pending_wheel_property(now, true);
            app.session.status = format!(
                "Alt+scroll is ambiguous here ({}). Choose a layer in the Object inspector.",
                labels.join(" / ")
            );
            true
        }
        CanvasStepTarget::Unique {
            property,
            label,
            targets,
        } => {
            step_wheel_property(
                app, canvas, object, property, label, targets, scroll_y, now, ui,
            );
            true
        }
        CanvasStepTarget::None => {
            app.finish_pending_wheel_property(now, true);
            scale_line_intensity(app, canvas, object, plot, pointer, scroll_y, now, ui)
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn step_wheel_property(
    app: &mut PlotxApp,
    canvas: usize,
    object: ObjectId,
    property: plotx_core::properties::PropertyId,
    label: &'static str,
    targets: Vec<plotx_core::automation::TargetRef>,
    scroll_y: f32,
    now: f64,
    ui: &Ui,
) {
    let step_delta = ui
        .ctx()
        .options(|options| options.input_options.line_scroll_speed)
        .max(1.0);
    let changed_target = app
        .session
        .ui
        .wheel_property
        .as_ref()
        .is_some_and(|pending| {
            pending.canvas != canvas
                || pending.object != object
                || pending.property != property
                || pending.targets != targets
        });
    if changed_target {
        app.finish_pending_wheel_property(now, true);
    }
    if app.session.ui.wheel_property.is_none() {
        app.session.ui.wheel_property = Some(PendingWheelPropertyEdit {
            canvas,
            object,
            property,
            targets: targets.clone(),
            accumulator: 0.0,
            last_input_time: now,
            gesture_started: false,
        });
    }
    let pending = app.session.ui.wheel_property.as_mut().unwrap();
    pending.accumulator += scroll_y;
    pending.last_input_time = now;
    let steps = (pending.accumulator.abs() / step_delta).floor() as usize;
    if steps == 0 {
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_millis(200));
        return;
    }
    let direction = if pending.accumulator > 0.0 {
        plotx_core::properties::PropertyStep::Lower
    } else {
        plotx_core::properties::PropertyStep::Raise
    };
    let signed_step = if pending.accumulator > 0.0 {
        step_delta
    } else {
        -step_delta
    };
    pending.accumulator -= signed_step * steps as f32;

    for _ in 0..steps.min(8) {
        match app.plan_property_step(property, &targets, direction) {
            Ok(commit) => {
                if !app
                    .session
                    .ui
                    .wheel_property
                    .as_ref()
                    .is_some_and(|pending| pending.gesture_started)
                {
                    app.begin_property_gesture(property);
                    if let Some(pending) = app.session.ui.wheel_property.as_mut() {
                        pending.gesture_started = true;
                    }
                }
                let applied = app.commit_property(commit);
                app.session.status = format!(
                    "Adjusted {label} on {applied} series. Alt+scroll controls display sensitivity."
                );
            }
            Err(error) => {
                app.session.status = format!("Could not adjust {label}: {error}");
                break;
            }
        }
    }
    ui.ctx()
        .request_repaint_after(std::time::Duration::from_millis(200));
}

#[allow(clippy::too_many_arguments)]
fn scale_line_intensity(
    app: &mut PlotxApp,
    canvas: usize,
    object: ObjectId,
    plot: PlotRect,
    pointer: Pos2,
    scroll_y: f32,
    now: f64,
    ui: &Ui,
) -> bool {
    let Some(plot_object) = app.doc.canvases[canvas]
        .object(object)
        .and_then(|object| object.plot())
    else {
        return false;
    };
    if plot_object.figure().y.categories.is_some() {
        app.session.status =
            "This categorical plot has no continuous display-intensity scale.".to_owned();
        return true;
    }
    let before = plot_object.viewport.clone();
    let y_axis = plot_object.figure().y.clone();
    let view = before.view_y;
    let anchor = if view.min <= 0.0 && view.max >= 0.0 {
        0.0
    } else {
        (view.min + view.max) * 0.5
    };
    let scale = f64::from((-scroll_y * WHEEL_ZOOM_SPEED).exp()).clamp(0.2, 5.0);
    zoom_plot_viewport(
        app,
        canvas,
        object,
        plot_rect(plot),
        plot,
        Pos2::new(
            pointer.x,
            y_to_screen(anchor, plot, y_axis.min, y_axis.span(), y_axis.reversed),
        ),
        scale,
        (false, true),
        now,
        ui,
    );
    app.session.status =
        "Adjusted plot intensity; automatic Y scaling is now off. Double-click Y to reset."
            .to_owned();
    true
}

/// Zoom a plot's data viewport around the cursor on the requested axes.
/// Coalesces one stream of wheel or pinch events into one undo step.
#[allow(clippy::too_many_arguments)]
pub(crate) fn zoom_plot_viewport(
    app: &mut PlotxApp,
    ci: usize,
    object_id: ObjectId,
    _outer_rect: EguiRect,
    plot: PlotRect,
    p: Pos2,
    scale: f64,
    axes: (bool, bool),
    now: f64,
    ui: &Ui,
) {
    let (zoom_x, zoom_y) = axes;
    if !zoom_x && !zoom_y {
        return;
    }

    if app
        .session
        .ui
        .wheel_zoom
        .as_ref()
        .map(|pending| pending.canvas != ci || pending.object != object_id)
        .unwrap_or(false)
    {
        app.finish_pending_wheel_zoom(now, true);
    }
    if app.session.ui.wheel_zoom.is_none() {
        app.session.ui.wheel_zoom = Some(PendingViewportEdit {
            canvas: ci,
            object: object_id,
            before: app.doc.canvases[ci]
                .object(object_id)
                .and_then(|object| object.plot())
                .unwrap()
                .viewport
                .clone(),
            last_input_time: now,
        });
    }
    if let Some(pending) = &mut app.session.ui.wheel_zoom {
        pending.last_input_time = now;
    }

    let object = app.doc.canvases[ci].object_mut(object_id).unwrap();
    let plot_object = object.plot_mut().unwrap();
    let fig = plot_object.figure().clone();
    if zoom_x {
        let anchor = screen_to_x(p.x, plot, fig.x.min, fig.x.span(), fig.x.reversed);
        plot_object.viewport.zoom_x(&fig, anchor, scale);
    }
    if zoom_y {
        let anchor = screen_to_y(p.y, plot, fig.y.min, fig.y.span(), fig.y.reversed);
        plot_object.viewport.zoom_y(anchor, scale);
    }
    plot_object.apply_viewport();
    app.doc.dirty = true;
    ui.ctx()
        .request_repaint_after(std::time::Duration::from_millis(200));
}
