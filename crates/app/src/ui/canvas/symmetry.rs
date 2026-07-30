use super::*;
use plotx_analysis::symmetry::{ArtifactLikelihood, PartnerStatus};
use plotx_core::state::{Peak2DId, Peak2DReview, Peak2DSelection, SymmetryCursorReading};

const CROSS_HALF_PX: f32 = 8.0;
const PEAK_HIT_PX: f32 = 7.0;

pub(crate) struct SymmetryPaintTarget {
    pub canvas: usize,
    pub object: ObjectId,
    pub dataset: usize,
    pub plot: PlotRect,
}

pub(crate) fn handle_symmetry(
    app: &mut PlotxApp,
    canvas: usize,
    object: ObjectId,
    dataset: usize,
    plot: PlotRect,
    ui: &Ui,
) {
    if app.symmetry_audit_needs_start(dataset)
        && let Err(error) = app.start_symmetry_audit(dataset)
    {
        app.session.status = error;
    }
    let (escape, delete, pressed, pointer) = ui.input(|input| {
        (
            input.key_pressed(egui::Key::Escape),
            input.key_pressed(egui::Key::Delete) || input.key_pressed(egui::Key::Backspace),
            input.pointer.primary_pressed(),
            input.pointer.hover_pos(),
        )
    });
    if escape {
        app.session.ui.symmetry_pin = None;
        app.session.ui.selected_peak_2d = None;
        return;
    }
    let dataset_id = app.doc.datasets[dataset].resource_id();
    if delete
        && let Some(id) = app
            .session
            .ui
            .selected_peak_2d
            .and_then(|selection| selection.in_dataset(dataset_id))
    {
        app.remove_peak_2d(dataset, id);
        return;
    }
    let Some(pointer) = pointer.filter(|pointer| plot_contains(plot, *pointer)) else {
        return;
    };
    if !pressed {
        return;
    }
    let axes = plot_axes(app, canvas, object);
    if let Some(id) = hit_peak(app, dataset, plot, axes, pointer) {
        app.session.ui.selected_peak_2d = Some(Peak2DSelection::new(dataset_id, id));
        let Some(mark) = app.doc.datasets[dataset]
            .as_nmr2d()
            .and_then(|nmr| nmr.peaks.mark(id))
        else {
            return;
        };
        app.session.ui.symmetry_pin = app.symmetry_reading(dataset, mark.f2, mark.f1, true);
        return;
    }
    let snap = app.session.ui.symmetry_snap || ui.input(|input| input.modifiers.shift);
    app.session.ui.symmetry_pin = reading_at_pointer(app, dataset, plot, axes, pointer, snap);
    app.session.ui.selected_peak_2d = None;
}

pub(crate) fn paint_symmetry(
    app: &PlotxApp,
    target: SymmetryPaintTarget,
    ui: &Ui,
    painter: &egui::Painter,
    chrome: ChromeStyle,
) {
    if app.session.tool != Tool::Symmetry {
        return;
    }
    let SymmetryPaintTarget {
        canvas,
        object,
        dataset,
        plot,
    } = target;
    let axes = plot_axes(app, canvas, object);
    paint_peak_marks(app, dataset, plot, axes, painter, chrome);

    let hover = ui.input(|input| {
        (
            input.pointer.hover_pos(),
            app.session.ui.symmetry_snap || input.modifiers.shift,
        )
    });
    let reading = hover
        .0
        .filter(|pointer| plot_contains(plot, *pointer))
        .and_then(|pointer| reading_at_pointer(app, dataset, plot, axes, pointer, hover.1));
    if let Some(pin) = app
        .session
        .ui
        .symmetry_pin
        .as_ref()
        .filter(|pin| pin.dataset == app.doc.datasets[dataset].resource_id())
    {
        paint_reading(pin, plot, axes, painter, chrome, true);
    }
    if let Some(reading) = &reading {
        paint_reading(reading, plot, axes, painter, chrome, false);
        paint_readout(
            reading,
            plot,
            painter,
            chrome,
            ui.visuals().dark_mode,
            app.symmetry_audit_progress().is_some(),
        );
    } else if let Some(pin) = app
        .session
        .ui
        .symmetry_pin
        .as_ref()
        .filter(|pin| pin.dataset == app.doc.datasets[dataset].resource_id())
    {
        paint_readout(
            pin,
            plot,
            painter,
            chrome,
            ui.visuals().dark_mode,
            app.symmetry_audit_progress().is_some(),
        );
    }
}

#[derive(Clone, Copy)]
struct Axes {
    x_min: f64,
    x_span: f64,
    x_reversed: bool,
    y_min: f64,
    y_span: f64,
    y_reversed: bool,
}

fn plot_axes(app: &PlotxApp, canvas: usize, object: ObjectId) -> Axes {
    let figure = app.doc.canvases[canvas]
        .object(object)
        .and_then(|object| object.plot())
        .expect("active symmetry target is a plot")
        .figure();
    Axes {
        x_min: figure.x.min,
        x_span: figure.x.span(),
        x_reversed: figure.x.reversed,
        y_min: figure.y.min,
        y_span: figure.y.span(),
        y_reversed: figure.y.reversed,
    }
}

fn reading_at_pointer(
    app: &PlotxApp,
    dataset: usize,
    plot: PlotRect,
    axes: Axes,
    pointer: Pos2,
    snap: bool,
) -> Option<SymmetryCursorReading> {
    let f2 = screen_to_x(pointer.x, plot, axes.x_min, axes.x_span, axes.x_reversed);
    let f1 = screen_to_y(pointer.y, plot, axes.y_min, axes.y_span, axes.y_reversed);
    app.symmetry_reading(dataset, f2, f1, snap)
}

fn hit_peak(
    app: &PlotxApp,
    dataset: usize,
    plot: PlotRect,
    axes: Axes,
    pointer: Pos2,
) -> Option<Peak2DId> {
    app.doc.datasets[dataset]
        .as_nmr2d()?
        .peaks
        .marks
        .iter()
        .filter_map(|mark| {
            let position = point_to_screen(mark.f2, mark.f1, plot, axes);
            let distance = position.distance(pointer);
            (distance <= PEAK_HIT_PX).then_some((distance, mark.id))
        })
        .min_by(|left, right| left.0.total_cmp(&right.0))
        .map(|(_, id)| id)
}

fn paint_peak_marks(
    app: &PlotxApp,
    dataset: usize,
    plot: PlotRect,
    axes: Axes,
    painter: &egui::Painter,
    chrome: ChromeStyle,
) {
    let Some(nmr) = app.doc.datasets.get(dataset).and_then(Dataset::as_nmr2d) else {
        return;
    };
    let selected = app.session.ui.selected_peak_2d;
    for mark in &nmr.peaks.marks {
        let position = point_to_screen(mark.f2, mark.f1, plot, axes);
        if !plot_contains(plot, position) {
            continue;
        }
        let selected = selected == Some(Peak2DSelection::new(nmr.resource_id, mark.id));
        let color = match mark.review {
            Peak2DReview::Confirmed => chrome.selection_active,
            Peak2DReview::Uncertain => chrome.snap_guide,
            Peak2DReview::PossibleArtifact => chrome.pivot,
            Peak2DReview::Unreviewed => chrome.selection_stroke,
        };
        let stroke = Stroke::new(if selected { 2.5_f32 } else { 1.5_f32 }, color);
        match mark.review {
            Peak2DReview::PossibleArtifact => {
                painter.line_segment(
                    [
                        position + egui::vec2(-4.0, -4.0),
                        position + egui::vec2(4.0, 4.0),
                    ],
                    stroke,
                );
                painter.line_segment(
                    [
                        position + egui::vec2(-4.0, 4.0),
                        position + egui::vec2(4.0, -4.0),
                    ],
                    stroke,
                );
            }
            Peak2DReview::Uncertain => {
                painter.add(egui::Shape::convex_polygon(
                    vec![
                        position + egui::vec2(0.0, -5.0),
                        position + egui::vec2(5.0, 0.0),
                        position + egui::vec2(0.0, 5.0),
                        position + egui::vec2(-5.0, 0.0),
                    ],
                    Color32::TRANSPARENT,
                    stroke,
                ));
            }
            Peak2DReview::Confirmed | Peak2DReview::Unreviewed => {
                painter.circle_stroke(position, if selected { 6.0 } else { 4.5 }, stroke);
            }
        }
    }
}

fn paint_reading(
    reading: &SymmetryCursorReading,
    plot: PlotRect,
    axes: Axes,
    painter: &egui::Painter,
    chrome: ChromeStyle,
    pinned: bool,
) {
    let primary = point_to_screen(reading.current.f2, reading.current.f1, plot, axes);
    let partner_point = reading.partner.unwrap_or(plotx_core::state::Peak2DPoint {
        f2: reading.partner_target[0],
        f1: reading.partner_target[1],
        intensity: 0.0,
    });
    let partner = point_to_screen(partner_point.f2, partner_point.f1, plot, axes);
    let primary_stroke = Stroke::new(
        if pinned { 2.0_f32 } else { 1.25_f32 },
        chrome.selection_active,
    );
    let partner_stroke = Stroke::new(if pinned { 2.0_f32 } else { 1.25_f32 }, chrome.snap_guide);

    if pinned
        && plot_contains(plot, primary)
        && plot_contains(plot, partner)
        && !reading.on_diagonal
    {
        painter.line_segment(
            [primary, partner],
            Stroke::new(1.0_f32, Color32::from_white_alpha(90)),
        );
    }
    paint_short_cross(painter, primary, primary_stroke, false);
    painter.text(
        primary + egui::vec2(7.0, -7.0),
        egui::Align2::LEFT_BOTTOM,
        if pinned { "P" } else { "A" },
        egui::FontId::proportional(10.0),
        primary_stroke.color,
    );

    if reading.on_diagonal {
        painter.circle_stroke(primary, 6.0, partner_stroke);
        return;
    }
    if plot_contains(plot, partner) {
        paint_short_cross(painter, partner, partner_stroke, true);
        painter.circle_stroke(partner, 6.0, partner_stroke);
        painter.text(
            partner + egui::vec2(7.0, -7.0),
            egui::Align2::LEFT_BOTTOM,
            if pinned { "P'" } else { "A'" },
            egui::FontId::proportional(10.0),
            partner_stroke.color,
        );
    } else {
        paint_edge_badge(partner, plot, painter, partner_stroke);
    }
}

fn paint_short_cross(painter: &egui::Painter, center: Pos2, stroke: Stroke, dashed: bool) {
    for (start, end) in [
        (
            center + egui::vec2(-CROSS_HALF_PX, 0.0),
            center + egui::vec2(CROSS_HALF_PX, 0.0),
        ),
        (
            center + egui::vec2(0.0, -CROSS_HALF_PX),
            center + egui::vec2(0.0, CROSS_HALF_PX),
        ),
    ] {
        if dashed {
            paint_dashed_segment(painter, start, end, stroke);
        } else {
            painter.line_segment([start, end], stroke);
        }
    }
}

fn paint_dashed_segment(painter: &egui::Painter, start: Pos2, end: Pos2, stroke: Stroke) {
    let vector = end - start;
    let length = vector.length();
    if length <= f32::EPSILON {
        return;
    }
    let direction = vector / length;
    let mut offset = 0.0;
    while offset < length {
        let dash_end = (offset + 3.0).min(length);
        painter.line_segment(
            [start + direction * offset, start + direction * dash_end],
            stroke,
        );
        offset += 5.0;
    }
}

fn paint_edge_badge(target: Pos2, plot: PlotRect, painter: &egui::Painter, stroke: Stroke) {
    let edge = Pos2::new(
        target.x.clamp(plot.left + 6.0, plot.right() - 6.0),
        target.y.clamp(plot.top + 6.0, plot.bottom() - 6.0),
    );
    painter.circle_filled(edge, 4.0, stroke.color);
    painter.text(
        edge + egui::vec2(6.0, -6.0),
        egui::Align2::LEFT_BOTTOM,
        "A'",
        egui::FontId::proportional(10.0),
        stroke.color,
    );
}

fn paint_readout(
    reading: &SymmetryCursorReading,
    plot: PlotRect,
    painter: &egui::Painter,
    chrome: ChromeStyle,
    dark_mode: bool,
    running: bool,
) {
    let text = reading_text(reading, running);
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

fn reading_text(reading: &SymmetryCursorReading, running: bool) -> String {
    let coordinates = format!(
        "A {:.3}, {:.3}  ->  A' {:.3}, {:.3}",
        reading.current.f2,
        reading.current.f1,
        reading.partner_target[0],
        reading.partner_target[1],
    );
    let intensities = match reading.partner {
        Some(partner) => format!(
            " · I(A) {} · I(A') {} · |I(A)/I(A')| {}",
            fmt_intensity(reading.current.intensity),
            fmt_intensity(partner.intensity),
            fmt_intensity_ratio(reading.current.intensity, partner.intensity),
        ),
        None => format!(" · I(A) {}", fmt_intensity(reading.current.intensity)),
    };
    if reading.on_diagonal {
        return format!("{coordinates}{intensities} · on diagonal");
    }
    let evidence = match reading.status {
        Some(PartnerStatus::Matched) => format!(
            "partner found · S/N {} / {}",
            fmt_snr(reading.current_signal_to_noise),
            fmt_snr(reading.partner_signal_to_noise),
        ),
        Some(PartnerStatus::Ambiguous) => {
            format!(
                "ambiguous · {} nearby candidates",
                reading.alternatives.max(1)
            )
        }
        Some(PartnerStatus::Missing) => "no counterpart detected".to_owned(),
        Some(PartnerStatus::OutsideRange) => "partner outside acquired range".to_owned(),
        None if running => "checking symmetry…".to_owned(),
        None => "exact transposed position".to_owned(),
    };
    let suggestion = match reading.likelihood {
        Some(ArtifactLikelihood::High) => " · review suggested",
        Some(ArtifactLikelihood::Medium) => " · uncertain",
        Some(ArtifactLikelihood::Low) | None => "",
    };
    format!("{coordinates}{intensities} · {evidence}{suggestion}")
}

fn fmt_snr(value: Option<f64>) -> String {
    value.map_or("—".to_owned(), |value| format!("{value:.1}"))
}

fn fmt_intensity(value: f64) -> String {
    let magnitude = value.abs();
    if value == 0.0 {
        "0".to_owned()
    } else if !(1e-3..1e4).contains(&magnitude) {
        format!("{value:.3e}")
    } else {
        format!("{value:.3}")
    }
}

fn fmt_intensity_ratio(current: f64, partner: f64) -> String {
    if partner == 0.0 {
        return "—".to_owned();
    }
    let ratio = (current / partner).abs();
    if ratio.is_finite() {
        fmt_intensity(ratio)
    } else {
        "—".to_owned()
    }
}

fn point_to_screen(f2: f64, f1: f64, plot: PlotRect, axes: Axes) -> Pos2 {
    Pos2::new(
        x_to_screen(f2, plot, axes.x_min, axes.x_span, axes.x_reversed),
        y_to_screen(f1, plot, axes.y_min, axes.y_span, axes.y_reversed),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matched_readout_compares_both_intensities_and_their_ratio() {
        let reading = SymmetryCursorReading {
            dataset: plotx_core::state::DatasetId::new(),
            current: plotx_core::state::Peak2DPoint {
                f2: 7.0,
                f1: 3.0,
                intensity: 10.0,
            },
            partner_target: [3.0, 7.0],
            partner: Some(plotx_core::state::Peak2DPoint {
                f2: 3.0,
                f1: 7.0,
                intensity: -5.0,
            }),
            current_key: None,
            partner_key: None,
            alternatives: 0,
            status: Some(PartnerStatus::Matched),
            likelihood: Some(ArtifactLikelihood::Low),
            reasons: Vec::new(),
            current_signal_to_noise: Some(12.0),
            partner_signal_to_noise: Some(8.0),
            on_diagonal: false,
        };

        let text = reading_text(&reading, false);
        assert!(text.contains("I(A) 10.000"));
        assert!(text.contains("I(A') -5.000"));
        assert!(text.contains("|I(A)/I(A')| 2.000"));
    }
}
