use super::*;

const REGION_EDGE_PX: f32 = 5.0;

enum RegionHit {
    Edge { id: u64, lo_edge: bool },
    Inside { id: u64 },
}

/// Which region band (if any) the screen x `px` lands on, edges taking priority.
fn region_hit(
    regions: &[Region],
    plot: PlotRect,
    xmin: f64,
    xspan: f64,
    xrev: bool,
    px: f32,
) -> Option<RegionHit> {
    for r in regions {
        let sxlo = x_to_screen(r.lo, plot, xmin, xspan, xrev);
        let sxhi = x_to_screen(r.hi, plot, xmin, xspan, xrev);
        if (px - sxlo).abs() <= REGION_EDGE_PX {
            return Some(RegionHit::Edge {
                id: r.id,
                lo_edge: true,
            });
        }
        if (px - sxhi).abs() <= REGION_EDGE_PX {
            return Some(RegionHit::Edge {
                id: r.id,
                lo_edge: false,
            });
        }
    }
    for r in regions {
        let left = x_to_screen(r.lo, plot, xmin, xspan, xrev)
            .min(x_to_screen(r.hi, plot, xmin, xspan, xrev));
        let right = x_to_screen(r.lo, plot, xmin, xspan, xrev)
            .max(x_to_screen(r.hi, plot, xmin, xspan, xrev));
        if px >= left && px <= right {
            return Some(RegionHit::Inside { id: r.id });
        }
    }
    None
}

pub(crate) fn handle_region_drag(
    app: &mut PlotxApp,
    ci: usize,
    object_id: ObjectId,
    dataset: usize,
    plot: PlotRect,
    ui: &Ui,
) {
    let is_series = app
        .doc
        .datasets
        .get(dataset)
        .and_then(Dataset::as_nmr2d)
        .map(|n| n.is_pseudo())
        .unwrap_or(false);
    if !is_series {
        return;
    }

    let (hover, primary_down, primary_pressed, primary_released, esc) = ui.input(|i| {
        (
            i.pointer.hover_pos(),
            i.pointer.primary_down(),
            i.pointer.primary_pressed(),
            i.pointer.primary_released(),
            i.key_pressed(egui::Key::Escape),
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

    if esc {
        if matches!(app.interaction(), Interaction::Region(_)) {
            app.cancel_interaction();
        }
        return;
    }

    if let Interaction::Region(drag) = app.interaction()
        && (drag.canvas != ci || drag.object != object_id)
    {
        return;
    }
    if matches!(app.interaction(), Interaction::Region(_)) {
        if let Some(p) = hover {
            let ppm = screen_to_x(p.x.clamp(plot.left, plot.right()), plot, xmin, xspan, xrev);
            apply_region_drag_live(app, dataset, ppm);
        }
        if primary_released || !primary_down {
            finish_region_drag(app, dataset, xspan);
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
        let regions = &app.doc.datasets[dataset].as_nmr2d().unwrap().regions;
        region_hit(regions, plot, xmin, xspan, xrev, p.x)
    };
    match hit {
        Some(RegionHit::Edge { .. }) => {
            ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal)
        }
        Some(RegionHit::Inside { .. }) => ui.ctx().set_cursor_icon(egui::CursorIcon::Grab),
        None => ui.ctx().set_cursor_icon(egui::CursorIcon::Crosshair),
    }

    if primary_pressed {
        let ppm = screen_to_x(p.x, plot, xmin, xspan, xrev);
        let before = app.doc.datasets[dataset]
            .as_nmr2d()
            .unwrap()
            .regions
            .clone();
        let mut drag = RegionDrag {
            canvas: ci,
            object: object_id,
            dataset,
            kind: RegionDragKind::NewBand,
            region_id: None,
            before,
            anchor_ppm: ppm,
            grab_lo: 0.0,
            grab_hi: 0.0,
            current_ppm: ppm,
        };
        match hit {
            Some(RegionHit::Edge { id, lo_edge }) => {
                drag.kind = if lo_edge {
                    RegionDragKind::EdgeLo
                } else {
                    RegionDragKind::EdgeHi
                };
                drag.region_id = Some(id);
                app.session.ui.selected_region = Some(id);
            }
            Some(RegionHit::Inside { id }) => {
                if let Some(r) = drag.before.iter().find(|r| r.id == id) {
                    drag.grab_lo = r.lo;
                    drag.grab_hi = r.hi;
                }
                drag.kind = RegionDragKind::Move;
                drag.region_id = Some(id);
                app.session.ui.selected_region = Some(id);
            }
            None => {
                app.session.ui.selected_region = None;
            }
        }
        app.begin_interaction(Interaction::Region(drag));
    }
}

fn apply_region_drag_live(app: &mut PlotxApp, dataset: usize, ppm: f64) {
    let Interaction::Region(drag) = app.interaction() else {
        return;
    };
    let kind = drag.kind;
    let id = drag.region_id;
    let anchor = drag.anchor_ppm;
    let (grab_lo, grab_hi) = (drag.grab_lo, drag.grab_hi);
    if kind == RegionDragKind::NewBand {
        if let Interaction::Region(drag) = &mut app.session.ui.interaction {
            drag.current_ppm = ppm;
        }
        return;
    }
    let Some(id) = id else {
        return;
    };
    let Some(d2) = app
        .doc
        .datasets
        .get_mut(dataset)
        .and_then(Dataset::as_nmr2d_mut)
    else {
        return;
    };
    let Some(r) = d2.regions.iter_mut().find(|r| r.id == id) else {
        return;
    };
    match kind {
        RegionDragKind::EdgeLo => r.lo = ppm,
        RegionDragKind::EdgeHi => r.hi = ppm,
        RegionDragKind::Move => {
            let d = ppm - anchor;
            r.lo = grab_lo + d;
            r.hi = grab_hi + d;
        }
        RegionDragKind::NewBand => {}
    }
}

fn finish_region_drag(app: &mut PlotxApp, dataset: usize, xspan: f64) {
    if !matches!(app.interaction(), Interaction::Region(_)) {
        return;
    }
    let Interaction::Region(drag) = app.take_interaction() else {
        return;
    };
    if drag.kind == RegionDragKind::NewBand {
        let lo = drag.anchor_ppm.min(drag.current_ppm);
        let hi = drag.anchor_ppm.max(drag.current_ppm);
        let min_w = (xspan.abs() * 0.002).max(f64::MIN_POSITIVE);
        if (hi - lo) <= min_w {
            return;
        }
        let Some(d2) = app
            .doc
            .datasets
            .get_mut(dataset)
            .and_then(Dataset::as_nmr2d_mut)
        else {
            return;
        };
        let id = d2.next_region_id;
        d2.next_region_id += 1;
        let idx = d2.regions.len();
        d2.regions.push(Region {
            id,
            lo,
            hi,
            name: String::new(),
            color: region_color(idx),
            metric: None,
        });
        app.session.ui.selected_region = Some(id);
    }
    let after = app.doc.datasets[dataset]
        .as_nmr2d()
        .unwrap()
        .regions
        .clone();
    app.execute_action(Action::set_regions(dataset, drag.before, after));
}
