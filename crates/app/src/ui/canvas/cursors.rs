use super::*;
use plotx_core::state::{CursorDelta, CursorPoint};
use plotx_processing::Processed2D;

const CROSS_HALF_PX: f32 = 7.0;

pub(crate) fn handle_cursor_tool(
    app: &mut PlotxApp,
    canvas: usize,
    object: ObjectId,
    dataset: usize,
    plot: PlotRect,
    ui: &Ui,
) {
    let (pressed, pointer) =
        ui.input(|input| (input.pointer.primary_pressed(), input.pointer.hover_pos()));
    if !pressed {
        return;
    }
    let Some(pointer) = pointer.filter(|pointer| plot_contains(plot, *pointer)) else {
        return;
    };
    let Some(point) = point_at_pointer(app, canvas, object, dataset, plot, pointer) else {
        return;
    };
    match app.session.tool {
        Tool::InspectCursor => {
            app.session.ui.inspect_cursor_pin = Some(point);
            app.session.status = "Inspect cursor pinned. Click another position to move it.".into();
        }
        Tool::DeltaCursor => {
            let same_target = app
                .session
                .ui
                .delta_cursor_anchor
                .is_some_and(|anchor| point_matches_target(anchor, point));
            if same_target {
                let first = app
                    .session
                    .ui
                    .delta_cursor_anchor
                    .take()
                    .expect("same-target Delta anchor exists");
                app.session.ui.delta_cursor_pin = Some(CursorDelta {
                    first,
                    second: point,
                });
                app.session.status =
                    "Delta measurement pinned. Click to start another measurement.".into();
            } else {
                app.session.ui.delta_cursor_anchor = Some(point);
                app.session.ui.delta_cursor_pin = None;
                app.session.status =
                    "Delta cursor: first point set; click the second point.".into();
            }
        }
        _ => {}
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn paint_cursor_tool(
    app: &PlotxApp,
    canvas: usize,
    object: ObjectId,
    dataset: usize,
    plot: PlotRect,
    ui: &Ui,
    painter: &egui::Painter,
    chrome: ChromeStyle,
) {
    if !matches!(app.session.tool, Tool::InspectCursor | Tool::DeltaCursor) {
        return;
    }
    let target = target_ids(app, canvas, object, dataset);
    let hover = ui
        .input(|input| input.pointer.hover_pos())
        .filter(|pointer| plot_contains(plot, *pointer))
        .and_then(|pointer| point_at_pointer(app, canvas, object, dataset, plot, pointer));

    match app.session.tool {
        Tool::InspectCursor => {
            let pin = app
                .session
                .ui
                .inspect_cursor_pin
                .filter(|point| target.is_some_and(|target| point_target(*point) == target));
            if let Some(pin) = pin {
                paint_point(app, canvas, object, plot, pin, painter, chrome, true, "P");
            }
            if let Some(hover) = hover {
                paint_point(app, canvas, object, plot, hover, painter, chrome, false, "");
            }
            if let Some(reading) = hover.or(pin) {
                paint_readout(
                    inspect_text(app, dataset, reading),
                    plot,
                    painter,
                    chrome,
                    ui.visuals().dark_mode,
                );
            }
        }
        Tool::DeltaCursor => {
            let anchor = app
                .session
                .ui
                .delta_cursor_anchor
                .filter(|point| target.is_some_and(|target| point_target(*point) == target));
            let pin = app.session.ui.delta_cursor_pin.filter(|delta| {
                target.is_some_and(|target| {
                    point_target(delta.first) == target && point_target(delta.second) == target
                })
            });
            let pair = anchor
                .zip(hover)
                .map(|(first, second)| CursorDelta { first, second })
                .or(pin);
            if let Some(pair) = pair {
                if let (Some(first), Some(second)) = (
                    point_to_screen(app, canvas, object, plot, pair.first),
                    point_to_screen(app, canvas, object, plot, pair.second),
                ) {
                    paint_delta(first, second, painter, chrome, pin.is_some());
                }
                paint_readout(
                    delta_text(app, dataset, pair),
                    plot,
                    painter,
                    chrome,
                    ui.visuals().dark_mode,
                );
            } else if let Some(hover) = hover {
                paint_point(app, canvas, object, plot, hover, painter, chrome, false, "");
                paint_readout(
                    format!("{} · click first point", inspect_text(app, dataset, hover)),
                    plot,
                    painter,
                    chrome,
                    ui.visuals().dark_mode,
                );
            }
        }
        _ => {}
    }
}

type CursorTarget = (
    plotx_core::state::CanvasId,
    ObjectId,
    plotx_core::state::DatasetId,
);

fn target_ids(
    app: &PlotxApp,
    canvas: usize,
    object: ObjectId,
    dataset: usize,
) -> Option<CursorTarget> {
    Some((
        app.doc.canvases.get(canvas)?.resource_id,
        object,
        app.doc.datasets.get(dataset)?.resource_id(),
    ))
}

fn point_target(point: CursorPoint) -> CursorTarget {
    (point.canvas, point.object, point.dataset)
}

fn point_matches_target(left: CursorPoint, right: CursorPoint) -> bool {
    point_target(left) == point_target(right)
}

fn point_at_pointer(
    app: &PlotxApp,
    canvas: usize,
    object: ObjectId,
    dataset: usize,
    plot: PlotRect,
    pointer: Pos2,
) -> Option<CursorPoint> {
    let figure = app
        .doc
        .canvases
        .get(canvas)?
        .object(object)?
        .plot()?
        .figure();
    let x = screen_to_x(
        pointer.x,
        plot,
        figure.x.min,
        figure.x.span(),
        figure.x.reversed,
    );
    let data = app.doc.datasets.get(dataset)?;
    let (y, intensity) = match data {
        Dataset::Nmr(_) => {
            let trace = data.displayed_trace(None)?;
            let index = nearest_index(&trace.xs, x)?;
            (None, *trace.ys.get(index)?)
        }
        Dataset::Nmr2D(nmr) => {
            let Processed2D::Ft(spectrum) = &nmr.processed else {
                return None;
            };
            let y = screen_to_y(
                pointer.y,
                plot,
                figure.y.min,
                figure.y.span(),
                figure.y.reversed,
            );
            let col = spectrum.nearest_f2(x);
            let row = spectrum.nearest_f1(y);
            let value = spectrum
                .data
                .get(row.checked_mul(spectrum.f2_size)? + col)?;
            (Some(y), nmr.display_mode().reduce(value))
        }
        _ => return None,
    };
    Some(CursorPoint {
        canvas: app.doc.canvases[canvas].resource_id,
        object,
        dataset: data.resource_id(),
        x,
        y,
        intensity,
    })
}

fn nearest_index(values: &[f64], target: f64) -> Option<usize> {
    values
        .iter()
        .enumerate()
        .filter(|(_, value)| value.is_finite())
        .min_by(|(_, left), (_, right)| (*left - target).abs().total_cmp(&(*right - target).abs()))
        .map(|(index, _)| index)
}

fn point_to_screen(
    app: &PlotxApp,
    canvas: usize,
    object: ObjectId,
    plot: PlotRect,
    point: CursorPoint,
) -> Option<Pos2> {
    let figure = app
        .doc
        .canvases
        .get(canvas)?
        .object(object)?
        .plot()?
        .figure();
    Some(Pos2::new(
        x_to_screen(
            point.x,
            plot,
            figure.x.min,
            figure.x.span(),
            figure.x.reversed,
        ),
        y_to_screen(
            point.y.unwrap_or(point.intensity),
            plot,
            figure.y.min,
            figure.y.span(),
            figure.y.reversed,
        ),
    ))
}

#[allow(clippy::too_many_arguments)]
fn paint_point(
    app: &PlotxApp,
    canvas: usize,
    object: ObjectId,
    plot: PlotRect,
    point: CursorPoint,
    painter: &egui::Painter,
    chrome: ChromeStyle,
    pinned: bool,
    label: &str,
) {
    let Some(position) = point_to_screen(app, canvas, object, plot, point) else {
        return;
    };
    let stroke = Stroke::new(
        if pinned { 2.0_f32 } else { 1.25_f32 },
        chrome.selection_active,
    );
    painter.line_segment(
        [
            Pos2::new(position.x, plot.top),
            Pos2::new(position.x, plot.bottom()),
        ],
        stroke,
    );
    if point.y.is_some() {
        painter.line_segment(
            [
                Pos2::new(plot.left, position.y),
                Pos2::new(plot.right(), position.y),
            ],
            stroke,
        );
    } else {
        painter.circle_filled(position, 3.5, stroke.color);
    }
    if !label.is_empty() {
        painter.text(
            position + egui::vec2(CROSS_HALF_PX, -CROSS_HALF_PX),
            egui::Align2::LEFT_BOTTOM,
            label,
            egui::FontId::proportional(10.0),
            stroke.color,
        );
    }
}

fn paint_delta(
    first: Pos2,
    second: Pos2,
    painter: &egui::Painter,
    chrome: ChromeStyle,
    pinned: bool,
) {
    let stroke = Stroke::new(
        if pinned { 2.0_f32 } else { 1.5_f32 },
        chrome.selection_active,
    );
    painter.line_segment([first, second], Stroke::new(1.2_f32, chrome.snap_guide));
    for (position, label) in [(first, "A"), (second, "B")] {
        painter.line_segment(
            [
                position + egui::vec2(-CROSS_HALF_PX, 0.0),
                position + egui::vec2(CROSS_HALF_PX, 0.0),
            ],
            stroke,
        );
        painter.line_segment(
            [
                position + egui::vec2(0.0, -CROSS_HALF_PX),
                position + egui::vec2(0.0, CROSS_HALF_PX),
            ],
            stroke,
        );
        painter.text(
            position + egui::vec2(CROSS_HALF_PX, -CROSS_HALF_PX),
            egui::Align2::LEFT_BOTTOM,
            label,
            egui::FontId::proportional(10.0),
            stroke.color,
        );
    }
}

fn inspect_text(app: &PlotxApp, dataset: usize, point: CursorPoint) -> String {
    let Some(data) = app.doc.datasets.get(dataset) else {
        return String::new();
    };
    match data {
        Dataset::Nmr(_) => format!("x {:.4} ppm · I {}", point.x, fmt_number(point.intensity)),
        Dataset::Nmr2D(_) => format!(
            "F2 {:.4} ppm · F1 {:.4} ppm · I {}",
            point.x,
            point.y.unwrap_or_default(),
            fmt_number(point.intensity),
        ),
        _ => String::new(),
    }
}

fn delta_text(app: &PlotxApp, dataset: usize, delta: CursorDelta) -> String {
    let Some(data) = app.doc.datasets.get(dataset) else {
        return String::new();
    };
    let dx = delta.second.x - delta.first.x;
    let di = delta.second.intensity - delta.first.intensity;
    match data {
        Dataset::Nmr(nmr) => format!(
            "Δx {} ppm ({} Hz) · ΔI {}",
            fmt_delta(dx),
            fmt_delta(dx * nmr.data.observe_freq_mhz),
            fmt_number(di),
        ),
        Dataset::Nmr2D(nmr) => {
            let Processed2D::Ft(spectrum) = &nmr.processed else {
                return String::new();
            };
            let dy = delta.second.y.unwrap_or_default() - delta.first.y.unwrap_or_default();
            format!(
                "ΔF2 {} ppm ({} Hz) · ΔF1 {} ppm ({} Hz) · ΔI {}",
                fmt_delta(dx),
                fmt_delta(dx * spectrum.direct.observe_freq_mhz),
                fmt_delta(dy),
                fmt_delta(dy * spectrum.indirect.observe_freq_mhz),
                fmt_number(di),
            )
        }
        _ => String::new(),
    }
}

fn paint_readout(
    text: String,
    plot: PlotRect,
    painter: &egui::Painter,
    chrome: ChromeStyle,
    dark_mode: bool,
) {
    let galley = painter.layout_no_wrap(
        text,
        egui::FontId::proportional(11.0),
        chrome.selection_stroke,
    );
    let anchor = Pos2::new(plot.left + 6.0, plot.bottom() - 6.0);
    let rect = egui::Align2::LEFT_BOTTOM.anchor_size(anchor, galley.size());
    painter.rect_filled(
        rect.expand(4.0),
        3.0,
        Color32::from_black_alpha(if dark_mode { 175 } else { 32 }),
    );
    painter.galley(rect.min, galley, chrome.selection_stroke);
}

fn fmt_delta(value: f64) -> String {
    format!("{value:+.4}")
}

fn fmt_number(value: f64) -> String {
    let magnitude = value.abs();
    if value == 0.0 {
        "0".into()
    } else if !(1e-3..1e4).contains(&magnitude) {
        format!("{value:.3e}")
    } else {
        format!("{value:.3}")
    }
}
