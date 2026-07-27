//! The in-place readout for canvas-steppable settings (§8.5 channel 3).
//!
//! "The best parameter is the one you never have to look for" only holds if the
//! plot says what its threshold currently is. This paints that value in the
//! corner of every plot the `+` / `-` gesture would act on, so the number the
//! keys move is visible where the keys are pressed.
//!
//! It is a label and nothing more: it reads a cached value through
//! [`PlotxApp::property_readout`] and never resolves, measures or queues
//! anything. A plot whose estimate has not arrived says so.

use super::{HitZone, hit_zone, object_screen_rect, plot_rect, plot_under_cursor};
use crate::ui::properties;
use egui::{Align2, Color32, FontId};
use plotx_core::properties::PropertyReadout;
use plotx_core::state::PlotxApp;

const READOUT_FONT_PT: f32 = 11.0;
const READOUT_INSET_PX: f32 = 6.0;

/// Paint the corner readout for each plot the canvas gesture currently targets.
pub(crate) fn paint_property_readouts(
    app: &PlotxApp,
    ci: usize,
    rect: egui::Rect,
    painter: &egui::Painter,
    chrome: super::ChromeStyle,
    dark_mode: bool,
) {
    // Nothing to read out unless a steppable property applies right now — the
    // same condition that enables the gesture, asked once, so the label cannot
    // appear on a plot the keys would not move.
    let Some((property, _)) = properties::discovery::step_target(app) else {
        return;
    };
    let Some(canvas) = app.doc.canvases.get(ci) else {
        return;
    };
    for object in properties::discovery::selection_objects(app) {
        // The series the gesture edits, not the first one the plot happens to
        // hold. A contour stacked over a heatmap is drawn second, and reading
        // the first series would caption the plot with a heatmap that has no
        // level while the keys moved the contour that does — or, with several
        // contours, present one of them as the whole plot's threshold.
        let targets = app.series_targets(ci, object);
        let readouts: Vec<PropertyReadout> = app
            .resolve_property_set(property, &targets)
            .applicable_targets
            .iter()
            .filter_map(|address| app.property_readout(address).ok())
            .collect();
        let Some(text) = properties::readout::aggregate_property_summary(&readouts) else {
            continue;
        };
        let Some(frame) = object_screen_rect(app.session.board, canvas, object, rect) else {
            continue;
        };
        let frame = plot_rect(frame);
        if !frame.is_positive() {
            continue;
        }
        let anchor = frame.right_top() + egui::vec2(-READOUT_INSET_PX, READOUT_INSET_PX);
        let galley = painter.layout_no_wrap(
            text,
            FontId::proportional(READOUT_FONT_PT),
            chrome.selection_stroke,
        );
        let text_rect = Align2::RIGHT_TOP.anchor_size(anchor, galley.size());
        // A plot's own ink runs right up to its frame, so the label needs a
        // backing plate to stay legible over dense contours.
        painter.rect_filled(
            text_rect.expand(3.0),
            3.0,
            Color32::from_black_alpha(if dark_mode { 150 } else { 20 }),
        );
        painter.galley(text_rect.min, galley, chrome.selection_stroke);
    }
}

/// Paint the exact wheel target under the pointer. The hint is transient editor
/// chrome and never enters exported figures.
pub(crate) fn paint_wheel_target_hint(
    app: &PlotxApp,
    ci: usize,
    rect: egui::Rect,
    ui: &egui::Ui,
    painter: &egui::Painter,
    chrome: super::ChromeStyle,
    dark_mode: bool,
) {
    let Some(pointer) = ui.input(|input| input.pointer.hover_pos()) else {
        return;
    };
    let Some((object, outer, plot)) = plot_under_cursor(app, ci, rect, pointer) else {
        return;
    };
    let zone = hit_zone(pointer, outer, plot);
    let target_rect = match zone {
        HitZone::Plot => plot_rect(plot),
        HitZone::XAxis => egui::Rect::from_min_max(
            egui::pos2(plot.left, plot.bottom()),
            egui::pos2(plot.right(), outer.bottom()),
        ),
        HitZone::YAxis => egui::Rect::from_min_max(
            egui::pos2(outer.left(), plot.top),
            egui::pos2(plot.left, plot.bottom()),
        ),
        HitZone::None => return,
    };
    painter.rect_filled(target_rect, 0.0, chrome.selection_fill);
    painter.rect_stroke(
        target_rect,
        0.0,
        egui::Stroke::new(1.0_f32, chrome.selection_stroke),
        egui::StrokeKind::Inside,
    );

    let text = match zone {
        HitZone::XAxis => "Scroll: zoom X · Double-click: reset X".to_owned(),
        HitZone::YAxis => "Scroll: zoom Y · Double-click: reset Y".to_owned(),
        HitZone::Plot => {
            use properties::discovery::{CanvasStepTarget, canvas_step_target};
            let two_dimensional = app.doc.canvases[ci]
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
            let navigation = if two_dimensional {
                "Scroll/pinch: zoom X+Y"
            } else {
                "Scroll: zoom X · Pinch: zoom X+Y"
            };
            let display = match canvas_step_target(app, ci, object) {
                CanvasStepTarget::Unique { label, targets, .. } => {
                    format!("Alt+scroll: {label} ({} series)", targets.len())
                }
                CanvasStepTarget::Ambiguous { labels } => {
                    format!("Alt+scroll: choose layer ({})", labels.join(" / "))
                }
                CanvasStepTarget::None => "Alt+scroll: Y intensity".to_owned(),
            };
            let aspect = if app.doc.canvases[ci]
                .object(object)
                .and_then(|object| object.plot())
                .is_some_and(|plot| plot.figure().lock_aspect)
            {
                " · aspect locked"
            } else {
                ""
            };
            format!("{navigation} · {display} · Alt+drag: box zoom{aspect}")
        }
        HitZone::None => return,
    };
    let anchor = target_rect.left_top() + egui::vec2(READOUT_INSET_PX, READOUT_INSET_PX);
    let galley = painter.layout(
        text,
        FontId::proportional(READOUT_FONT_PT),
        chrome.selection_stroke,
        (target_rect.width() - 2.0 * READOUT_INSET_PX).max(40.0),
    );
    let text_rect = Align2::LEFT_TOP.anchor_size(anchor, galley.size());
    painter.rect_filled(
        text_rect.expand(3.0),
        3.0,
        Color32::from_black_alpha(if dark_mode { 170 } else { 28 }),
    );
    painter.galley(text_rect.min, galley, chrome.selection_stroke);
}
