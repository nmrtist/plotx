use super::*;
use plotx_core::{BaselineMode, DisplayModeLabel, IntegralMethod};
use plotx_processing::Processed2D;

const HANDLE_PX: f32 = 6.0;
const MIN_RECT_PX: f32 = 3.0;

#[derive(Clone, Copy)]
struct AxisMap {
    min: f64,
    span: f64,
    reversed: bool,
}

#[derive(Clone, Copy)]
struct Integral2DHit {
    id: u64,
    kind: Integral2DDragKind,
}

fn integral_screen_rect(integral: &Integral2D, plot: PlotRect, x: AxisMap, y: AxisMap) -> EguiRect {
    let x0 = x_to_screen(integral.f2.0, plot, x.min, x.span, x.reversed);
    let x1 = x_to_screen(integral.f2.1, plot, x.min, x.span, x.reversed);
    let y0 = y_to_screen(integral.f1.0, plot, y.min, y.span, y.reversed);
    let y1 = y_to_screen(integral.f1.1, plot, y.min, y.span, y.reversed);
    EguiRect::from_two_pos(Pos2::new(x0, y0), Pos2::new(x1, y1))
}

/// Hit-test rectangles in corner, edge, then interior priority.
fn integral_2d_hit(
    integrals: &[Integral2D],
    plot: PlotRect,
    x: AxisMap,
    y: AxisMap,
    pointer: Pos2,
) -> Option<Integral2DHit> {
    for integral in integrals {
        let r = integral_screen_rect(integral, plot, x, y);
        let f2_lo_x = x_to_screen(integral.f2.0, plot, x.min, x.span, x.reversed);
        let f2_hi_x = x_to_screen(integral.f2.1, plot, x.min, x.span, x.reversed);
        let f1_lo_y = y_to_screen(integral.f1.0, plot, y.min, y.span, y.reversed);
        let f1_hi_y = y_to_screen(integral.f1.1, plot, y.min, y.span, y.reversed);
        for (at, kind) in [
            (
                Pos2::new(f2_lo_x, f1_lo_y),
                Integral2DDragKind::CornerF2LoF1Lo,
            ),
            (
                Pos2::new(f2_lo_x, f1_hi_y),
                Integral2DDragKind::CornerF2LoF1Hi,
            ),
            (
                Pos2::new(f2_hi_x, f1_lo_y),
                Integral2DDragKind::CornerF2HiF1Lo,
            ),
            (
                Pos2::new(f2_hi_x, f1_hi_y),
                Integral2DDragKind::CornerF2HiF1Hi,
            ),
        ] {
            if (pointer.x - at.x).abs() <= HANDLE_PX && (pointer.y - at.y).abs() <= HANDLE_PX {
                return Some(Integral2DHit {
                    id: integral.id,
                    kind,
                });
            }
        }
        if pointer.y >= r.top() - HANDLE_PX && pointer.y <= r.bottom() + HANDLE_PX {
            if (pointer.x - f2_lo_x).abs() <= HANDLE_PX {
                return Some(Integral2DHit {
                    id: integral.id,
                    kind: Integral2DDragKind::EdgeF2Lo,
                });
            }
            if (pointer.x - f2_hi_x).abs() <= HANDLE_PX {
                return Some(Integral2DHit {
                    id: integral.id,
                    kind: Integral2DDragKind::EdgeF2Hi,
                });
            }
        }
        if pointer.x >= r.left() - HANDLE_PX && pointer.x <= r.right() + HANDLE_PX {
            if (pointer.y - f1_lo_y).abs() <= HANDLE_PX {
                return Some(Integral2DHit {
                    id: integral.id,
                    kind: Integral2DDragKind::EdgeF1Lo,
                });
            }
            if (pointer.y - f1_hi_y).abs() <= HANDLE_PX {
                return Some(Integral2DHit {
                    id: integral.id,
                    kind: Integral2DDragKind::EdgeF1Hi,
                });
            }
        }
    }
    integrals.iter().find_map(|integral| {
        integral_screen_rect(integral, plot, x, y)
            .contains(pointer)
            .then_some(Integral2DHit {
                id: integral.id,
                kind: Integral2DDragKind::Move,
            })
    })
}

fn spectrum_bounds(app: &PlotxApp, dataset: usize) -> Option<((f64, f64), (f64, f64))> {
    let n = app.doc.datasets.get(dataset)?.as_nmr2d()?;
    let Processed2D::Ft(spectrum) = &n.processed else {
        return None;
    };
    let f2 = (*spectrum.f2_ppm.first()?, *spectrum.f2_ppm.last()?);
    let f1 = (*spectrum.f1_ppm.first()?, *spectrum.f1_ppm.last()?);
    Some((
        (f2.0.min(f2.1), f2.0.max(f2.1)),
        (f1.0.min(f1.1), f1.0.max(f1.1)),
    ))
}

/// Translate a stored interval without letting it escape the current axis. A
/// stale interval wider than the axis is deliberately clipped to the full axis
/// instead of constructing invalid `clamp` bounds.
fn shifted_range(range: (f64, f64), delta: f64, full: (f64, f64)) -> (f64, f64) {
    let full = (full.0.min(full.1), full.0.max(full.1));
    let range = (range.0.min(range.1), range.0.max(range.1));
    let full_span = full.1 - full.0;
    let span = range.1 - range.0;
    if !delta.is_finite() || !span.is_finite() || span >= full_span {
        return full;
    }
    let lo = (range.0 + delta).clamp(full.0, full.1 - span);
    (lo, lo + span)
}

pub(crate) fn handle_integral_2d_drag(
    app: &mut PlotxApp,
    ci: usize,
    object_id: ObjectId,
    dataset: usize,
    plot: PlotRect,
    ui: &Ui,
    response: &egui::Response,
) {
    let Some((f2_full, f1_full)) = spectrum_bounds(app, dataset) else {
        return;
    };
    let (x, y) = {
        let figure = &app.doc.canvases[ci]
            .object(object_id)
            .and_then(|o| o.plot())
            .unwrap()
            .figure;
        (
            AxisMap {
                min: figure.x.min,
                span: figure.x.span(),
                reversed: figure.x.reversed,
            },
            AxisMap {
                min: figure.y.min,
                span: figure.y.span(),
                reversed: figure.y.reversed,
            },
        )
    };
    let (hover, down, pressed, released, escape, delete) = ui.input(|input| {
        (
            input.pointer.hover_pos(),
            input.pointer.primary_down(),
            input.pointer.primary_pressed(),
            input.pointer.primary_released(),
            input.key_pressed(egui::Key::Escape),
            input.key_pressed(egui::Key::Delete) || input.key_pressed(egui::Key::Backspace),
        )
    });

    integral_2d_context_menu(app, dataset, plot, x, y, hover, response);
    if delete && let Some(id) = app.session.ui.selected_integral {
        app.delete_integral_2d(dataset, id);
        app.session.ui.selected_integral = None;
        return;
    }
    if escape {
        if matches!(app.interaction(), Interaction::Integral2D(_)) {
            app.cancel_interaction();
        }
        return;
    }
    if let Interaction::Integral2D(drag) = app.interaction()
        && (drag.canvas != ci || drag.object != object_id)
    {
        return;
    }
    if matches!(app.interaction(), Interaction::Integral2D(_)) {
        if let Some(pointer) = hover {
            let current = [
                screen_to_x(
                    pointer.x.clamp(plot.left, plot.right()),
                    plot,
                    x.min,
                    x.span,
                    x.reversed,
                )
                .clamp(f2_full.0, f2_full.1),
                screen_to_y(
                    pointer.y.clamp(plot.top, plot.bottom()),
                    plot,
                    y.min,
                    y.span,
                    y.reversed,
                )
                .clamp(f1_full.0, f1_full.1),
            ];
            apply_integral_2d_drag_live(app, dataset, current, f2_full, f1_full);
        }
        if released || !down {
            finish_integral_2d_drag(app, dataset, plot, x, y);
        }
        return;
    }

    let Some(pointer) = hover.filter(|pointer| plot_contains(plot, *pointer)) else {
        return;
    };
    let integrals = &app.doc.datasets[dataset].as_nmr2d().unwrap().integrals;
    let hit = integral_2d_hit(integrals, plot, x, y, pointer);
    ui.ctx().set_cursor_icon(match hit.map(|hit| hit.kind) {
        Some(Integral2DDragKind::Move) => egui::CursorIcon::Grab,
        Some(Integral2DDragKind::EdgeF2Lo | Integral2DDragKind::EdgeF2Hi) => {
            egui::CursorIcon::ResizeHorizontal
        }
        Some(Integral2DDragKind::EdgeF1Lo | Integral2DDragKind::EdgeF1Hi) => {
            egui::CursorIcon::ResizeVertical
        }
        Some(Integral2DDragKind::CornerF2LoF1Lo | Integral2DDragKind::CornerF2HiF1Hi) => {
            egui::CursorIcon::ResizeNwSe
        }
        Some(Integral2DDragKind::CornerF2LoF1Hi | Integral2DDragKind::CornerF2HiF1Lo) => {
            egui::CursorIcon::ResizeNeSw
        }
        _ => egui::CursorIcon::Crosshair,
    });
    if !pressed {
        return;
    }

    let at = [
        screen_to_x(pointer.x, plot, x.min, x.span, x.reversed).clamp(f2_full.0, f2_full.1),
        screen_to_y(pointer.y, plot, y.min, y.span, y.reversed).clamp(f1_full.0, f1_full.1),
    ];
    let before = integrals.clone();
    let mut drag = Integral2DDrag {
        canvas: ci,
        object: object_id,
        dataset,
        kind: Integral2DDragKind::NewRect,
        integral_id: None,
        before,
        anchor: at,
        grab_f2: (0.0, 0.0),
        grab_f1: (0.0, 0.0),
        current: at,
    };
    if let Some(hit) = hit {
        drag.kind = hit.kind;
        drag.integral_id = Some(hit.id);
        if let Some(integral) = drag.before.iter().find(|integral| integral.id == hit.id) {
            drag.grab_f2 = integral.f2;
            drag.grab_f1 = integral.f1;
        }
        app.session.ui.selected_integral = Some(hit.id);
    } else {
        app.session.ui.selected_integral = None;
    }
    app.begin_interaction(Interaction::Integral2D(drag));
}

fn apply_integral_2d_drag_live(
    app: &mut PlotxApp,
    dataset: usize,
    current: [f64; 2],
    f2_full: (f64, f64),
    f1_full: (f64, f64),
) {
    let Interaction::Integral2D(drag) = app.interaction() else {
        return;
    };
    let (kind, id, anchor, grab_f2, grab_f1) = (
        drag.kind,
        drag.integral_id,
        drag.anchor,
        drag.grab_f2,
        drag.grab_f1,
    );
    if kind == Integral2DDragKind::NewRect {
        if let Interaction::Integral2D(drag) = &mut app.session.ui.interaction {
            drag.current = current;
        }
        return;
    }
    let Some(integral) = id.and_then(|id| {
        app.doc
            .datasets
            .get_mut(dataset)?
            .as_nmr2d_mut()?
            .integrals
            .iter_mut()
            .find(|integral| integral.id == id)
    }) else {
        return;
    };
    let ordered = |a: f64, b: f64| (a.min(b), a.max(b));
    match kind {
        Integral2DDragKind::Move => {
            integral.f2 = shifted_range(grab_f2, current[0] - anchor[0], f2_full);
            integral.f1 = shifted_range(grab_f1, current[1] - anchor[1], f1_full);
        }
        Integral2DDragKind::EdgeF2Lo => integral.f2 = ordered(current[0], grab_f2.1),
        Integral2DDragKind::EdgeF2Hi => integral.f2 = ordered(grab_f2.0, current[0]),
        Integral2DDragKind::EdgeF1Lo => integral.f1 = ordered(current[1], grab_f1.1),
        Integral2DDragKind::EdgeF1Hi => integral.f1 = ordered(grab_f1.0, current[1]),
        Integral2DDragKind::CornerF2LoF1Lo => {
            integral.f2 = ordered(current[0], grab_f2.1);
            integral.f1 = ordered(current[1], grab_f1.1);
        }
        Integral2DDragKind::CornerF2LoF1Hi => {
            integral.f2 = ordered(current[0], grab_f2.1);
            integral.f1 = ordered(grab_f1.0, current[1]);
        }
        Integral2DDragKind::CornerF2HiF1Lo => {
            integral.f2 = ordered(grab_f2.0, current[0]);
            integral.f1 = ordered(current[1], grab_f1.1);
        }
        Integral2DDragKind::CornerF2HiF1Hi => {
            integral.f2 = ordered(grab_f2.0, current[0]);
            integral.f1 = ordered(grab_f1.0, current[1]);
        }
        Integral2DDragKind::NewRect => {}
    }
}

fn finish_integral_2d_drag(
    app: &mut PlotxApp,
    dataset: usize,
    plot: PlotRect,
    x: AxisMap,
    y: AxisMap,
) {
    let Interaction::Integral2D(drag) = app.take_interaction() else {
        return;
    };
    if drag.kind == Integral2DDragKind::NewRect {
        let f2 = (
            drag.anchor[0].min(drag.current[0]),
            drag.anchor[0].max(drag.current[0]),
        );
        let f1 = (
            drag.anchor[1].min(drag.current[1]),
            drag.anchor[1].max(drag.current[1]),
        );
        let width = (x_to_screen(f2.0, plot, x.min, x.span, x.reversed)
            - x_to_screen(f2.1, plot, x.min, x.span, x.reversed))
        .abs();
        let height = (y_to_screen(f1.0, plot, y.min, y.span, y.reversed)
            - y_to_screen(f1.1, plot, y.min, y.span, y.reversed))
        .abs();
        if width < MIN_RECT_PX || height < MIN_RECT_PX {
            if let Some(n) = app
                .doc
                .datasets
                .get_mut(dataset)
                .and_then(Dataset::as_nmr2d_mut)
            {
                n.integrals = drag.before;
            }
            return;
        }
        if let Some(n) = app
            .doc
            .datasets
            .get_mut(dataset)
            .and_then(Dataset::as_nmr2d_mut)
        {
            let id = n.next_integral_id();
            let reference_value = n.integrals.is_empty().then_some(1.0);
            let mode = n.display_mode().into();
            n.integrals.push(Integral2D {
                id,
                name: format!("Integral {}", n.integrals.len() + 1),
                f2,
                f1,
                volume: 0.0,
                normalized_volume: None,
                reference_value,
                mode,
                method: IntegralMethod::Sum,
                baseline: BaselineMode::None,
            });
            app.session.ui.selected_integral = Some(id);
        }
    }
    if let Some(n) = app
        .doc
        .datasets
        .get_mut(dataset)
        .and_then(Dataset::as_nmr2d_mut)
        && let Err(error) = n.recompute_integrals()
    {
        app.session.status = format!("Could not recompute 2D integrals: {error}");
    }
    let after = app.doc.datasets[dataset]
        .as_nmr2d()
        .unwrap()
        .integrals
        .clone();
    app.execute_action(Action::set_integrals_2d(
        app.doc.datasets[dataset].resource_id(),
        drag.before,
        after,
    ));
}

fn integral_2d_context_menu(
    app: &mut PlotxApp,
    dataset: usize,
    plot: PlotRect,
    x: AxisMap,
    y: AxisMap,
    hover: Option<Pos2>,
    response: &egui::Response,
) {
    if response.secondary_clicked() {
        app.session.ui.selected_integral = hover.and_then(|pointer| {
            let integrals = &app.doc.datasets[dataset].as_nmr2d()?.integrals;
            integral_2d_hit(integrals, plot, x, y, pointer).map(|hit| hit.id)
        });
    }
    response.context_menu(|ui| {
        let Some(id) = app.session.ui.selected_integral else {
            ui.close();
            return;
        };
        if ui.button("Use as normalization reference").clicked() {
            app.set_integral_2d_reference(dataset, id, 1.0);
            ui.close();
        }
        if ui.button("Delete").clicked() {
            app.delete_integral_2d(dataset, id);
            app.session.ui.selected_integral = None;
            ui.close();
        }
    });
}

pub(crate) fn paint_integrals_2d(
    app: &PlotxApp,
    ci: usize,
    object_id: ObjectId,
    dataset: usize,
    plot: PlotRect,
    painter: &egui::Painter,
    chrome: ChromeStyle,
) {
    let Some(n) = app
        .doc
        .datasets
        .get(dataset)
        .and_then(Dataset::as_nmr2d)
        .filter(|n| n.is_true_2d())
    else {
        return;
    };
    let Some(figure) = app.doc.canvases[ci]
        .object(object_id)
        .and_then(|o| o.plot())
        .map(|p| &p.figure)
    else {
        return;
    };
    let x = AxisMap {
        min: figure.x.min,
        span: figure.x.span(),
        reversed: figure.x.reversed,
    };
    let y = AxisMap {
        min: figure.y.min,
        span: figure.y.span(),
        reversed: figure.y.reversed,
    };
    for integral in &n.integrals {
        let r = integral_screen_rect(integral, plot, x, y).intersect(plot_rect(plot));
        if r.width() < 1.0 || r.height() < 1.0 {
            continue;
        }
        let color = chrome.integral;
        let [red, green, blue, _] = color.to_array();
        painter.rect_filled(
            r,
            0.0,
            Color32::from_rgba_unmultiplied(red, green, blue, 30),
        );
        let selected = app.session.ui.selected_integral == Some(integral.id);
        painter.rect_stroke(
            r,
            0.0,
            Stroke::new(if selected { 2.0_f32 } else { 1.0_f32 }, color),
            StrokeKind::Inside,
        );
        let value = integral
            .normalized_volume
            .map_or_else(|| "—".to_owned(), |v| format!("{v:.3}"));
        painter.text(
            r.left_top() + egui::vec2(3.0, 2.0),
            egui::Align2::LEFT_TOP,
            format!("{}: {}", integral.name, value),
            egui::FontId::proportional(11.0),
            color,
        );
        if selected {
            for point in [
                r.left_top(),
                r.right_top(),
                r.left_bottom(),
                r.right_bottom(),
                r.center_top(),
                r.center_bottom(),
                r.left_center(),
                r.right_center(),
            ] {
                painter.rect_filled(
                    EguiRect::from_center_size(point, egui::vec2(5.0, 5.0)),
                    0.0,
                    color,
                );
            }
        }
    }
    if let Interaction::Integral2D(drag) = &app.session.ui.interaction
        && drag.dataset == dataset
        && drag.canvas == ci
        && drag.kind == Integral2DDragKind::NewRect
    {
        let preview = Integral2D {
            id: 0,
            name: String::new(),
            f2: (drag.anchor[0], drag.current[0]),
            f1: (drag.anchor[1], drag.current[1]),
            volume: 0.0,
            normalized_volume: None,
            reference_value: None,
            mode: DisplayModeLabel::Real,
            method: IntegralMethod::Sum,
            baseline: BaselineMode::None,
        };
        let r = integral_screen_rect(&preview, plot, x, y).intersect(plot_rect(plot));
        painter.rect_filled(r, 0.0, chrome.selection_fill);
        painter.rect_stroke(
            r,
            0.0,
            Stroke::new(1.0_f32, chrome.selection_stroke),
            StrokeKind::Inside,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn integral() -> Integral2D {
        Integral2D {
            id: 7,
            name: "A".into(),
            f2: (2.0, 4.0),
            f1: (3.0, 6.0),
            volume: 0.0,
            normalized_volume: None,
            reference_value: None,
            mode: DisplayModeLabel::Real,
            method: IntegralMethod::Sum,
            baseline: BaselineMode::None,
        }
    }

    #[test]
    fn corners_take_priority_over_edges_and_interior() {
        let plot = PlotRect::new(0.0, 0.0, 100.0, 100.0);
        let map = AxisMap {
            min: 0.0,
            span: 10.0,
            reversed: false,
        };
        let corner = Pos2::new(20.0, 70.0);
        assert_eq!(
            integral_2d_hit(&[integral()], plot, map, map, corner).map(|h| h.kind),
            Some(Integral2DDragKind::CornerF2LoF1Lo)
        );
        assert_eq!(
            integral_2d_hit(&[integral()], plot, map, map, Pos2::new(20.0, 50.0)).map(|h| h.kind),
            Some(Integral2DDragKind::EdgeF2Lo)
        );
        assert_eq!(
            integral_2d_hit(&[integral()], plot, map, map, Pos2::new(30.0, 50.0)).map(|h| h.kind),
            Some(Integral2DDragKind::Move)
        );
    }

    #[test]
    fn moving_stale_oversized_range_clips_without_panicking() {
        assert_eq!(shifted_range((-5.0, 15.0), 1.0, (0.0, 10.0)), (0.0, 10.0));
        assert_eq!(shifted_range((8.0, 6.0), 10.0, (0.0, 10.0)), (8.0, 10.0));
    }
}
