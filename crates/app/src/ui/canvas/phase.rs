use super::*;
use plotx_core::actions::DatasetProcessingState;
use plotx_core::state::{PhaseAxis, PhaseOrient};

pub(crate) struct PhaseDragCtx {
    pub axis: PhaseAxis,
    pub orient: PhaseOrient,
    pub piv_px: f32,
    pub pivot_ppm: f64,
    pub map_min: f64,
    pub map_span: f64,
    pub map_rev: bool,
}

/// The committed pivot, overridden by the canvas-only position while its handle
/// is being dragged.
pub(crate) fn displayed_phase_pivot_ppm(
    app: &PlotxApp,
    dataset: usize,
    axis: PhaseAxis,
) -> Option<f64> {
    match app.interaction() {
        Interaction::Phase(drag)
            if drag.dataset == dataset
                && drag.axis == axis
                && drag.kind == PhaseDragKind::Pivot =>
        {
            drag.preview_pivot_ppm
        }
        _ => None,
    }
    .or_else(|| app.doc.datasets[dataset].pivot_ppm(axis))
}

pub(crate) fn handle_phase_before_paint(
    app: &mut PlotxApp,
    ci: usize,
    board_rect: egui::Rect,
    ui: &Ui,
) {
    let Some(object_id) =
        data_edit_target(app, ci).or_else(|| app.doc.canvases[ci].active_plot_object_id())
    else {
        return;
    };
    if data_edit_target(app, ci) != Some(object_id)
        || matches!(app.session.ui.interaction, Interaction::Pan(_))
    {
        return;
    }
    let Some(di) = app.doc.canvases[ci]
        .object(object_id)
        .and_then(|object| object.dataset())
    else {
        return;
    };
    let Some(outer) = object_screen_rect(
        app.session.board,
        &app.doc.canvases[ci],
        object_id,
        board_rect,
    ) else {
        return;
    };
    let outer_rect = plot_rect(outer);
    let Some(figure) = app.doc.canvases[ci]
        .object(object_id)
        .and_then(|object| object.plot())
        .map(|plot| &plot.figure)
    else {
        return;
    };
    let plot = plotx_render::Projector::new(
        figure,
        outer,
        &plotx_render::Margins::for_figure(figure).scaled(app.session.board.zoom),
    )
    .plot;
    let axis = app.doc.datasets[di].active_phase_axis(app.session.ui.phase_axis);
    let Some(pivot_ppm) = displayed_phase_pivot_ppm(app, di, axis) else {
        return;
    };
    let ctx = match axis.orient() {
        PhaseOrient::Vertical => {
            let (map_min, map_span, map_rev) = (figure.x.min, figure.x.span(), figure.x.reversed);
            PhaseDragCtx {
                axis,
                orient: PhaseOrient::Vertical,
                piv_px: x_to_screen(pivot_ppm, plot, map_min, map_span, map_rev)
                    .clamp(plot.left, plot.right()),
                pivot_ppm,
                map_min,
                map_span,
                map_rev,
            }
        }
        PhaseOrient::Horizontal => {
            let (map_min, map_span, map_rev) = (figure.y.min, figure.y.span(), figure.y.reversed);
            PhaseDragCtx {
                axis,
                orient: PhaseOrient::Horizontal,
                piv_px: y_to_screen(pivot_ppm, plot, map_min, map_span, map_rev)
                    .clamp(plot.top, plot.bottom()),
                pivot_ppm,
                map_min,
                map_span,
                map_rev,
            }
        }
    };
    if handle_phase_drag(app, di, outer_rect, plot, ctx, ui) {
        app.apply_dataset_edit(di);
    }
}

pub(crate) fn handle_phase_drag(
    app: &mut PlotxApp,
    di: usize,
    rect: egui::Rect,
    plot: PlotRect,
    ctx: PhaseDragCtx,
    ui: &Ui,
) -> bool {
    let (hover, p_down, s_down, p_pressed, s_pressed, delta) = ui.input(|i| {
        (
            i.pointer.hover_pos(),
            i.pointer.primary_down(),
            i.pointer.secondary_down(),
            i.pointer.primary_pressed(),
            i.pointer.secondary_pressed(),
            i.pointer.delta(),
        )
    });

    if !matches!(app.interaction(), Interaction::Phase(_))
        && (p_pressed || s_pressed)
        && let Some(p) = hover
        && rect.contains(p)
    {
        let cursor = match ctx.orient {
            PhaseOrient::Vertical => p.x,
            PhaseOrient::Horizontal => p.y,
        };
        let near = (cursor - ctx.piv_px).abs() <= PIVOT_GRAB_PX;
        let kind = if near {
            PhaseDragKind::Pivot
        } else if s_pressed {
            PhaseDragKind::Ph1
        } else {
            PhaseDragKind::Ph0
        };
        let gesture_before = DatasetProcessingState::from_dataset(&app.doc.datasets[di]);
        app.begin_interaction(Interaction::Phase(PhaseDrag {
            kind,
            dataset: di,
            axis: ctx.axis,
            preview_pivot_ppm: (kind == PhaseDragKind::Pivot).then_some(ctx.pivot_ppm),
            gesture_before,
        }));
        // Grabbing phase on the plot takes over from any auto-phase, seeded so the
        // trace does not jump on the first pointer move.
        app.seed_manual_phase(di, ctx.axis);
    }

    if let Interaction::Phase(drag) = app.interaction()
        && drag.dataset != di
    {
        return false;
    }

    let mut dirty = false;
    // The axis is locked in at grab time, so a mid-drag panel change can't retarget.
    let kind_axis = match app.interaction() {
        Interaction::Phase(drag) => Some((drag.kind, drag.axis)),
        _ => None,
    };
    match kind_axis {
        Some((PhaseDragKind::Pivot, axis)) => {
            if let Some(p) = hover {
                let ppm = match ctx.orient {
                    PhaseOrient::Vertical => {
                        screen_to_x(p.x, plot, ctx.map_min, ctx.map_span, ctx.map_rev)
                    }
                    PhaseOrient::Horizontal => {
                        screen_to_y(p.y, plot, ctx.map_min, ctx.map_span, ctx.map_rev)
                    }
                };
                if let Interaction::Phase(drag) = &mut app.session.ui.interaction {
                    drag.preview_pivot_ppm = Some(ppm);
                }
                ui.ctx().request_repaint();
            }
            if !p_down && !s_down {
                let preview = match app.interaction() {
                    Interaction::Phase(drag) => drag.preview_pivot_ppm,
                    _ => None,
                };
                if let Some(ppm) = preview
                    && app.doc.datasets[di].repivot_ppm(axis, ppm)
                {
                    app.doc.dirty = true;
                }
                finish_phase_drag(app);
            }
        }
        Some((PhaseDragKind::Ph0, axis)) => {
            if delta.y != 0.0 {
                if let Some(p) = app.doc.datasets[di].phase_params_mut(axis) {
                    p.auto = None;
                    p.phase0 += -(delta.y as f64) * PH0_PER_PX;
                }
                dirty = true;
            }
            if !p_down {
                finish_phase_drag(app);
            }
        }
        Some((PhaseDragKind::Ph1, axis)) => {
            if delta.y != 0.0 {
                if let Some(p) = app.doc.datasets[di].phase_params_mut(axis) {
                    p.auto = None;
                    p.phase1 += -(delta.y as f64) * PH1_PER_PX;
                }
                dirty = true;
            }
            if !s_down {
                finish_phase_drag(app);
            }
        }
        None => {}
    }
    if dirty {
        ui.ctx().request_repaint();
    }
    dirty
}

pub(crate) fn finish_phase_drag(app: &mut PlotxApp) {
    if !matches!(app.interaction(), Interaction::Phase(_)) {
        return;
    }
    let Interaction::Phase(_) = app.take_interaction() else {
        return;
    };
}
