use super::*;

const INTEGRAL_EDGE_PX: f32 = 5.0;

enum IntegralHit {
    Edge { id: u64, lo_edge: bool },
    Inside { id: u64 },
}

impl IntegralHit {
    fn id(&self) -> u64 {
        match self {
            IntegralHit::Edge { id, .. } | IntegralHit::Inside { id } => *id,
        }
    }
}

/// Which integral band (if any) the screen x `px` lands on, edges taking priority.
fn integral_hit(
    integrals: &[IntegralResult],
    plot: PlotRect,
    xmin: f64,
    xspan: f64,
    xrev: bool,
    px: f32,
) -> Option<IntegralHit> {
    for integ in integrals {
        let sxlo = x_to_screen(integ.start_ppm, plot, xmin, xspan, xrev);
        let sxhi = x_to_screen(integ.end_ppm, plot, xmin, xspan, xrev);
        if (px - sxlo).abs() <= INTEGRAL_EDGE_PX {
            return Some(IntegralHit::Edge {
                id: integ.id,
                lo_edge: true,
            });
        }
        if (px - sxhi).abs() <= INTEGRAL_EDGE_PX {
            return Some(IntegralHit::Edge {
                id: integ.id,
                lo_edge: false,
            });
        }
    }
    for integ in integrals {
        let a = x_to_screen(integ.start_ppm, plot, xmin, xspan, xrev);
        let b = x_to_screen(integ.end_ppm, plot, xmin, xspan, xrev);
        if px >= a.min(b) && px <= a.max(b) {
            return Some(IntegralHit::Inside { id: integ.id });
        }
    }
    None
}

pub(crate) fn handle_integral_drag(
    app: &mut PlotxApp,
    ci: usize,
    object_id: ObjectId,
    dataset: usize,
    plot: PlotRect,
    ui: &Ui,
    resp: &egui::Response,
) {
    if app
        .doc
        .datasets
        .get(dataset)
        .and_then(Dataset::as_nmr)
        .is_none()
    {
        return;
    }

    let (hover, primary_down, primary_pressed, primary_released, esc, del) = ui.input(|i| {
        (
            i.pointer.hover_pos(),
            i.pointer.primary_down(),
            i.pointer.primary_pressed(),
            i.pointer.primary_released(),
            i.key_pressed(egui::Key::Escape),
            i.key_pressed(egui::Key::Delete) || i.key_pressed(egui::Key::Backspace),
        )
    });

    let (xmin, xspan, xrev) = {
        let fig = &app.doc.canvases[ci]
            .object(object_id)
            .and_then(|object| object.plot())
            .unwrap()
            .figure;
        (fig.x.min, fig.x.span(), fig.x.reversed)
    };

    integral_context_menu(app, dataset, plot, (xmin, xspan, xrev), hover, resp);

    if del && let Some(id) = app.session.ui.selected_integral {
        app.delete_integral(dataset, id);
        return;
    }

    if esc {
        if matches!(app.interaction(), Interaction::Integral(_)) {
            app.cancel_interaction();
        }
        return;
    }

    if let Interaction::Integral(drag) = app.interaction()
        && (drag.canvas != ci || drag.object != object_id)
    {
        return;
    }
    if matches!(app.interaction(), Interaction::Integral(_)) {
        if let Some(p) = hover {
            let ppm = screen_to_x(p.x.clamp(plot.left, plot.right()), plot, xmin, xspan, xrev);
            apply_integral_drag_live(app, dataset, ppm);
        }
        if primary_released || !primary_down {
            finish_integral_drag(app, dataset, xspan);
        }
        return;
    }

    let Some(p) = hover else {
        return;
    };
    if !plot_contains(plot, p) {
        return;
    }

    let hit = {
        let integrals = &app.doc.datasets[dataset].as_nmr().unwrap().integrals;
        integral_hit(integrals, plot, xmin, xspan, xrev, p.x)
    };
    match hit {
        Some(IntegralHit::Edge { .. }) => {
            ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal)
        }
        Some(IntegralHit::Inside { .. }) => ui.ctx().set_cursor_icon(egui::CursorIcon::Grab),
        None => ui.ctx().set_cursor_icon(egui::CursorIcon::Crosshair),
    }

    if primary_pressed {
        let ppm = screen_to_x(p.x, plot, xmin, xspan, xrev);
        let before = app.doc.datasets[dataset]
            .as_nmr()
            .unwrap()
            .integrals
            .clone();
        let mut drag = IntegralDrag {
            canvas: ci,
            object: object_id,
            dataset,
            kind: RegionDragKind::NewBand,
            integral_id: None,
            before,
            anchor_ppm: ppm,
            grab_lo: 0.0,
            grab_hi: 0.0,
            current_ppm: ppm,
        };
        match hit {
            Some(IntegralHit::Edge { id, lo_edge }) => {
                drag.kind = if lo_edge {
                    RegionDragKind::EdgeLo
                } else {
                    RegionDragKind::EdgeHi
                };
                drag.integral_id = Some(id);
                app.session.ui.selected_integral = Some(id);
            }
            Some(IntegralHit::Inside { id }) => {
                if let Some(integ) = drag.before.iter().find(|i| i.id == id) {
                    drag.grab_lo = integ.start_ppm;
                    drag.grab_hi = integ.end_ppm;
                }
                drag.kind = RegionDragKind::Move;
                drag.integral_id = Some(id);
                app.session.ui.selected_integral = Some(id);
            }
            None => {
                app.session.ui.selected_integral = None;
            }
        }
        app.begin_interaction(Interaction::Integral(drag));
    }
}

fn apply_integral_drag_live(app: &mut PlotxApp, dataset: usize, ppm: f64) {
    let Interaction::Integral(drag) = app.interaction() else {
        return;
    };
    let kind = drag.kind;
    let id = drag.integral_id;
    let anchor = drag.anchor_ppm;
    let (grab_lo, grab_hi) = (drag.grab_lo, drag.grab_hi);
    if kind == RegionDragKind::NewBand {
        if let Interaction::Integral(drag) = &mut app.session.ui.interaction {
            drag.current_ppm = ppm;
        }
        return;
    }
    let Some(id) = id else {
        return;
    };
    let Some(n) = app
        .doc
        .datasets
        .get_mut(dataset)
        .and_then(Dataset::as_nmr_mut)
    else {
        return;
    };
    if let Some(integ) = n.integrals.iter_mut().find(|i| i.id == id) {
        match kind {
            RegionDragKind::EdgeLo => integ.start_ppm = ppm,
            RegionDragKind::EdgeHi => integ.end_ppm = ppm,
            RegionDragKind::Move => {
                let d = ppm - anchor;
                integ.start_ppm = grab_lo + d;
                integ.end_ppm = grab_hi + d;
            }
            RegionDragKind::NewBand => {}
        }
    }
    n.recompute_integrals();
    app.sync_integral_curves_for(dataset);
}

fn finish_integral_drag(app: &mut PlotxApp, dataset: usize, xspan: f64) {
    if !matches!(app.interaction(), Interaction::Integral(_)) {
        return;
    }
    let Interaction::Integral(drag) = app.take_interaction() else {
        return;
    };
    if drag.kind == RegionDragKind::NewBand {
        let lo = drag.anchor_ppm.min(drag.current_ppm);
        let hi = drag.anchor_ppm.max(drag.current_ppm);
        let min_w = (xspan.abs() * 0.002).max(f64::MIN_POSITIVE);
        if (hi - lo) <= min_w {
            if let Some(n) = app
                .doc
                .datasets
                .get_mut(dataset)
                .and_then(Dataset::as_nmr_mut)
            {
                n.integrals = drag.before;
            }
            return;
        }
        if let Some(n) = app
            .doc
            .datasets
            .get_mut(dataset)
            .and_then(Dataset::as_nmr_mut)
        {
            let id = n.next_integral_id;
            n.next_integral_id += 1;
            let reference_value = n.integrals.is_empty().then_some(1.0);
            n.integrals.push(IntegralResult {
                id,
                start_ppm: lo,
                end_ppm: hi,
                area: 0.0,
                normalized_area: reference_value.unwrap_or(0.0),
                mode: plotx_core::DisplayModeLabel::Real,
                reference_value,
            });
            n.recompute_integrals();
            app.session.ui.selected_integral = Some(id);
        }
    }
    let after = app.doc.datasets[dataset]
        .as_nmr()
        .unwrap()
        .integrals
        .clone();
    app.execute_action(Action::set_integrals(dataset, drag.before, after));
}

fn integral_context_menu(
    app: &mut PlotxApp,
    dataset: usize,
    plot: PlotRect,
    (xmin, xspan, xrev): (f64, f64, bool),
    hover: Option<Pos2>,
    resp: &egui::Response,
) {
    if resp.secondary_clicked() {
        app.session.ui.selected_integral = hover.and_then(|p| {
            let integrals = &app.doc.datasets[dataset].as_nmr().unwrap().integrals;
            integral_hit(integrals, plot, xmin, xspan, xrev, p.x).map(|h| h.id())
        });
    }
    resp.context_menu(|ui| {
        let Some(id) = app.session.ui.selected_integral else {
            ui.close();
            return;
        };
        let exists = app
            .doc
            .datasets
            .get(dataset)
            .and_then(Dataset::as_nmr)
            .map(|n| n.integrals.iter().any(|i| i.id == id))
            .unwrap_or(false);
        if !exists {
            ui.close();
            return;
        }
        if ui.button("Use as normalization reference").clicked() {
            app.set_integral_reference(dataset, id, 1.0);
            ui.close();
        }
        if ui.button("Delete").clicked() {
            app.delete_integral(dataset, id);
            ui.close();
        }
    });
}
