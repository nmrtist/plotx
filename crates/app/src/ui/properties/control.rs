//! Property row controls, value descriptions, and gesture edges.

use super::*;
use plotx_core::properties::{contour, line};
use plotx_core::state::CanvasSizeUnit;
use std::borrow::Cow;

const LINE_WIDTH_PRESETS: [(&str, f64); 3] = [("Fine", 0.5), ("Medium", 0.75), ("Bold", 1.25)];

pub(super) struct RowEdits<'a> {
    pub pending: &'a mut Option<Pending>,
    pub gesture: &'a mut Option<(PropertyId, GestureEdge)>,
    pub text_edits: &'a mut Vec<PropertyTextEditState>,
}

pub(super) fn property_row(
    row: &Row,
    focus: Option<PropertyFocus>,
    now: f64,
    edits: &mut RowEdits<'_>,
    targets: &[TargetRef],
    length_unit: CanvasSizeUnit,
    ui: &mut Ui,
) {
    let highlighted = focus
        .is_some_and(|focus| focus.property == row.presentation.id && now < focus.highlight_until);
    let response = ui
        .scope(|ui| {
            ui.horizontal(|ui| {
                ui.label(row.presentation.localized_label.get())
                    .on_hover_text(property_hint(row));
                match row.representative.availability {
                    plotx_core::properties::Availability::Editable => {
                        control(
                            row,
                            edits.pending,
                            edits.gesture,
                            targets,
                            edits.text_edits,
                            length_unit,
                            ui,
                        );
                        if row.modified() {
                            modified_marker(row, edits.pending, ui);
                        }
                    }
                    plotx_core::properties::Availability::Disabled(reason) => {
                        ui.add_enabled_ui(false, |ui| {
                            control(
                                row,
                                edits.pending,
                                edits.gesture,
                                targets,
                                edits.text_edits,
                                length_unit,
                                ui,
                            );
                            if row.modified() {
                                modified_marker(row, edits.pending, ui);
                            }
                        })
                        .response
                        .on_disabled_hover_text(reason);
                    }
                    plotx_core::properties::Availability::ReadOnly => {
                        ui.weak(describe(
                            row,
                            row.value().unwrap_or_else(|| {
                                row.editing_value()
                                    .expect("a resolved read-only row has a value")
                            }),
                        ));
                    }
                }
            });
        })
        .response;
    if highlighted {
        ui.painter().rect_stroke(
            response.rect.expand(2.0),
            4.0,
            egui::Stroke::new(1.5_f32, ui.visuals().selection.bg_fill),
            egui::StrokeKind::Outside,
        );
        ui.ctx().request_repaint();
    }
    if focus.is_some_and(|focus| focus.property == row.presentation.id && focus.pending) {
        response.scroll_to_me(None);
    }
}

/// Compact form of the same row for a list item that already supplies the
/// property's label through its surrounding context.
pub(super) fn property_row_inline(
    row: &Row,
    focus: Option<PropertyFocus>,
    now: f64,
    edits: &mut RowEdits<'_>,
    targets: &[TargetRef],
    length_unit: CanvasSizeUnit,
    ui: &mut Ui,
) {
    let highlighted = focus
        .is_some_and(|focus| focus.property == row.presentation.id && now < focus.highlight_until);
    let response = ui
        .horizontal(|ui| {
            control(
                row,
                edits.pending,
                edits.gesture,
                targets,
                edits.text_edits,
                length_unit,
                ui,
            );
            if row.modified() {
                modified_marker(row, edits.pending, ui);
            }
        })
        .response
        .on_hover_text(row.definition.canonical_label);
    if highlighted {
        ui.painter().rect_stroke(
            response.rect.expand(2.0),
            4.0,
            egui::Stroke::new(1.5_f32, ui.visuals().selection.bg_fill),
            egui::StrokeKind::Outside,
        );
        ui.ctx().request_repaint();
    }
}

/// The "modified" affordance: a marker whose tooltip names the default, and a
/// one-click reset back to it.
fn modified_marker(row: &Row, pending: &mut Option<Pending>, ui: &mut Ui) {
    let default = row
        .representative
        .default_value
        .as_ref()
        .map(|value| describe(row, value))
        .unwrap_or(Cow::Borrowed("no default"));
    let hint = if row.mixed() {
        format!(
            "{} Default: {default}",
            no_single_value_hint(row.set.applicable_targets.len(), row.definition.copies)
        )
    } else {
        format!("Changed from the default: {default}")
    };
    ui.label(icon::DOT_OUTLINE).on_hover_text(&hint);
    if ui
        .small_button(icon::ARROW_COUNTER_CLOCKWISE)
        .on_hover_text(format!("Reset to {default}"))
        .clicked()
    {
        *pending = Some(Pending::Reset(row.presentation.id));
    }
}

fn property_hint(row: &Row) -> &'static str {
    match row.presentation.id {
        line::STROKE_WIDTH => "Width of spectrum and line-series strokes, in points.",
        contour::LINE_WIDTH => {
            "Width of contour strokes, in points. Lowest level and Levels control which features are drawn."
        }
        _ => row.definition.canonical_label,
    }
}

/// Draw the control for one row.
///
/// When the row has no single value the widget still edits — one gesture must
/// be enough to make the whole selection agree — but it displays nothing that
/// could be read as the current setting: an em dash instead of a number or a
/// choice, and no checkbox state at all.
fn control(
    row: &Row,
    pending: &mut Option<Pending>,
    gesture: &mut Option<(PropertyId, GestureEdge)>,
    targets: &[TargetRef],
    text_edits: &mut Vec<PropertyTextEditState>,
    length_unit: CanvasSizeUnit,
    ui: &mut Ui,
) {
    let mixed = row.mixed();
    let Some(value) = row.editing_value() else {
        ui.weak("unavailable");
        return;
    };
    match (&row.representative.schema, value) {
        (ResolvedSchema::Bool, PropertyValue::Bool(current)) => {
            if mixed {
                // A checkbox has no third state, and an unticked box would be a
                // claim about the selection. Two unselected choices are not.
                for (label, next) in [("On", true), ("Off", false)] {
                    if ui.selectable_label(false, label).clicked() {
                        *pending = Some(Pending::Write(
                            row.presentation.id,
                            PropertyValue::Bool(next),
                        ));
                    }
                }
            } else {
                let mut current = *current;
                if ui.checkbox(&mut current, "").changed() {
                    *pending = Some(Pending::Write(
                        row.presentation.id,
                        PropertyValue::Bool(current),
                    ));
                }
            }
        }
        (ResolvedSchema::Text, PropertyValue::Text(current)) => {
            text_control(row, current, mixed, targets, text_edits, pending, ui);
        }
        (ResolvedSchema::Int { min, max, unit }, PropertyValue::Int(current)) => {
            let mut current = *current;
            let drag = DragValue::new(&mut current).speed(0.25).range(*min..=*max);
            let response = ui.add(hide_value(drag, mixed));
            note_gesture(row, &response, gesture);
            if response.changed() {
                *pending = Some(Pending::Write(
                    row.presentation.id,
                    PropertyValue::Int(current),
                ));
            }
            draw_unit(ui, unit);
        }
        (
            ResolvedSchema::IntWithDrag {
                min,
                max,
                drag_step,
                unit,
            },
            PropertyValue::Int(current),
        ) => {
            let mut current = *current;
            let drag = DragValue::new(&mut current)
                .speed(*drag_step)
                .range(*min..=*max);
            let response = ui.add(hide_value(drag, mixed));
            note_gesture(row, &response, gesture);
            if response.changed() {
                *pending = Some(Pending::Write(
                    row.presentation.id,
                    PropertyValue::Int(current),
                ));
            }
            draw_unit(ui, unit);
        }
        (
            ResolvedSchema::SteppedInt {
                min,
                max,
                step,
                drag_step,
                unit,
            },
            PropertyValue::Int(current),
        ) => {
            let mut current = *current;
            let drag = DragValue::new(&mut current)
                .speed(*drag_step)
                .range(*min..=*max);
            let response = ui.add(hide_value(drag, mixed));
            note_gesture(row, &response, gesture);
            if response.changed() {
                current = snapped_stepped_int(current, *min, *max, *step);
                *pending = Some(Pending::Write(
                    row.presentation.id,
                    PropertyValue::Int(current),
                ));
            }
            draw_unit(ui, unit);
        }
        (ResolvedSchema::Float { bounds, display }, PropertyValue::Float(current)) => {
            float_control(
                row,
                FloatControlInput {
                    bounds: *bounds,
                    display: *display,
                    current: *current,
                    mixed,
                    length_unit,
                },
                pending,
                gesture,
                ui,
            );
        }
        (ResolvedSchema::Enum { variants }, PropertyValue::Enum(current)) => {
            // Nothing is current when the sources disagree, so no variant is
            // marked selected and the box names none of them.
            let current = (!mixed).then_some(*current);
            let selected = current
                .map(|current| {
                    variants
                        .iter()
                        .find(|variant| variant.id == current)
                        .map(|variant| variant.canonical_label)
                        .unwrap_or(current)
                })
                .unwrap_or(NO_SINGLE_VALUE);
            egui::ComboBox::from_id_salt(row.presentation.id.as_str())
                .selected_text(selected)
                .show_ui(ui, |ui| {
                    for variant in variants {
                        if ui
                            .selectable_label(current == Some(variant.id), variant.canonical_label)
                            .clicked()
                        {
                            *pending = Some(Pending::Write(
                                row.presentation.id,
                                PropertyValue::Enum(variant.id),
                            ));
                        }
                    }
                });
            if let Some(PropertyReadout::ZeroFillTarget(readout)) = row.readout {
                ui.weak(format!("{} {} points", icon::ARROW_RIGHT, readout.points));
            }
        }
        (ResolvedSchema::Color, PropertyValue::Color(current)) => {
            let mut rgb = [current.r, current.g, current.b];
            if ui.color_edit_button_srgb(&mut rgb).changed() {
                *pending = Some(Pending::Write(
                    row.presentation.id,
                    PropertyValue::Color(plotx_figure::Color::rgb(rgb[0], rgb[1], rgb[2])),
                ));
            }
        }
        // A control and a value of different shapes means the schema and the
        // domain model disagree; say so rather than drawing a wrong widget.
        _ => {
            ui.weak("unavailable");
        }
    }
    if mixed {
        ui.weak("mixed").on_hover_text(no_single_value_hint(
            row.set.applicable_targets.len(),
            row.definition.copies,
        ));
    }
}

fn text_control(
    row: &Row,
    current: &str,
    mixed: bool,
    targets: &[TargetRef],
    text_edits: &mut Vec<PropertyTextEditState>,
    pending: &mut Option<Pending>,
    ui: &mut Ui,
) {
    text_edits
        .retain(|edit| edit.property != row.presentation.id || edit.targets.as_slice() == targets);
    let index = text_edits
        .iter()
        .position(|edit| edit.property == row.presentation.id && edit.targets.as_slice() == targets)
        .unwrap_or_else(|| {
            text_edits.push(PropertyTextEditState {
                property: row.presentation.id,
                targets: targets.to_vec(),
                text: if mixed {
                    String::new()
                } else {
                    current.to_owned()
                },
                editing: false,
            });
            text_edits.len() - 1
        });
    let edit = &mut text_edits[index];
    if !edit.editing {
        let shown = if mixed { "" } else { current };
        if edit.text != shown {
            edit.text.clear();
            edit.text.push_str(shown);
        }
    }
    let hint = if mixed {
        NO_SINGLE_VALUE
    } else {
        row.representative
            .default_value
            .as_ref()
            .and_then(PropertyValue::as_text)
            .unwrap_or("")
    };
    let response = ui.add(
        egui::TextEdit::singleline(&mut edit.text)
            .hint_text(hint)
            .desired_width(132.0),
    );
    if response.gained_focus() {
        edit.editing = true;
    }
    let enter = response.has_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter));
    if enter {
        ui.memory_mut(|memory| memory.surrender_focus(response.id));
    }
    if should_submit_text_edit(
        &mut edit.editing,
        response.changed(),
        response.lost_focus(),
        enter,
    ) {
        *pending = Some(Pending::Write(
            row.presentation.id,
            PropertyValue::Text(edit.text.clone()),
        ));
    }
}

fn should_submit_text_edit(
    editing: &mut bool,
    _changed: bool,
    lost_focus: bool,
    enter: bool,
) -> bool {
    let submit = *editing && (lost_focus || enter);
    if submit {
        *editing = false;
    }
    submit
}

/// The drag notch this row's definition declares, if it declares one.
fn declared_drag_step(row: &Row) -> Option<f64> {
    match row.definition.value_schema {
        ValueSchema::Float { drag_step, .. } => drag_step,
        _ => None,
    }
}

fn float_control(
    row: &Row,
    input: FloatControlInput,
    pending: &mut Option<Pending>,
    gesture: &mut Option<(PropertyId, GestureEdge)>,
    ui: &mut Ui,
) {
    let FloatControlInput {
        bounds,
        display,
        current,
        mixed,
        length_unit,
    } = input;
    let projection = FloatControlProjection::new(
        row.presentation.uses_canvas_length_unit,
        bounds,
        display,
        current,
        length_unit,
        declared_drag_step(row),
    );
    let mut displayed = projection.displayed;
    // Every declared step is in display space. This keeps a degree control at
    // half a degree and a logarithmic control at a tenth of a decade while the
    // values sent to the property service remain radians and λ respectively.
    let mut drag = DragValue::new(&mut displayed)
        .speed(projection.speed)
        .range(projection.range.clone());
    if let Some(decimals) = projection.decimals {
        drag = drag.max_decimals(decimals);
    }
    let response = ui.add(hide_value(drag, mixed));
    note_gesture(row, &response, gesture);
    if response.changed() {
        let proposed = projection.to_domain(displayed);
        let next = admitted_float_from_control(
            bounds,
            current,
            proposed,
            display,
            projection.domain_step(),
        );
        *pending = Some(Pending::Write(
            row.presentation.id,
            PropertyValue::Float(next),
        ));
    }
    draw_unit(ui, &projection.caption);
    line_width_presets(row, current, mixed, pending, ui);
    if let Some(PropertyReadout::ContourBase(readout)) = &row.readout
        && let Some(suffix) = super::super::readout::resolution_suffix(readout)
    {
        ui.weak(suffix)
            .on_hover_text(super::super::readout::explanation(readout));
    }
    if let Some(PropertyReadout::PhasePivotPpm { ppm }) = row.readout {
        ui.weak(format!(
            "{} {ppm:.3} ppm",
            egui_phosphor::regular::ARROW_RIGHT
        ));
    }
}

fn line_width_presets(
    row: &Row,
    current: f64,
    mixed: bool,
    pending: &mut Option<Pending>,
    ui: &mut Ui,
) {
    if !matches!(
        row.presentation.id,
        line::STROKE_WIDTH | contour::LINE_WIDTH
    ) {
        return;
    }
    ui.menu_button("Presets", |ui| {
        for (name, value) in LINE_WIDTH_PRESETS {
            // Every preset is exactly representable, and widths round-trip
            // through `PositiveFiniteF32`, so the stored value either is the
            // preset or is a notch away from it. Equality is the honest test.
            let selected = !mixed && current == value;
            if ui
                .selectable_label(selected, format!("{name} — {value:.2} pt"))
                .clicked()
            {
                if !selected {
                    *pending = Some(Pending::Write(
                        row.presentation.id,
                        PropertyValue::Float(value),
                    ));
                }
                ui.close();
            }
        }
    })
    .response
    .on_hover_text("Choose a common data-line width");
}

struct FloatControlInput {
    bounds: plotx_core::properties::FloatBounds,
    display: plotx_core::properties::FloatDisplay,
    current: f64,
    mixed: bool,
    length_unit: CanvasSizeUnit,
}

struct FloatControlProjection {
    displayed: f64,
    range: std::ops::RangeInclusive<f64>,
    speed: f64,
    decimals: Option<usize>,
    caption: std::borrow::Cow<'static, str>,
    length_unit: Option<CanvasSizeUnit>,
    display: plotx_core::properties::FloatDisplay,
}

impl FloatControlProjection {
    fn new(
        uses_canvas_length_unit: bool,
        bounds: plotx_core::properties::FloatBounds,
        display: plotx_core::properties::FloatDisplay,
        current: f64,
        length_unit: CanvasSizeUnit,
        declared_step: Option<f64>,
    ) -> Self {
        if uses_canvas_length_unit {
            debug_assert_eq!(display, plotx_core::properties::FloatDisplay::Linear("mm"));
            return Self {
                displayed: length_from_mm(length_unit, current),
                range: length_from_mm(length_unit, bounds.lowest())
                    ..=length_from_mm(length_unit, bounds.max),
                speed: length_unit.drag_speed(),
                decimals: Some(length_unit.decimals()),
                caption: std::borrow::Cow::Borrowed(length_unit.label()),
                length_unit: Some(length_unit),
                display,
            };
        }
        let speed = declared_step.unwrap_or_else(|| match display {
            plotx_core::properties::FloatDisplay::Log10(_) => 0.1,
            _ => ((display.to_display(bounds.max) - display.to_display(bounds.lowest())) / 200.0)
                .abs()
                .max(1.0e-3),
        });
        Self {
            displayed: display.to_display(current),
            range: display.to_display(bounds.lowest())..=display.to_display(bounds.max),
            speed,
            decimals: None,
            caption: display.caption(),
            length_unit: None,
            display,
        }
    }

    fn to_domain(&self, displayed: f64) -> f64 {
        self.length_unit
            .map(|unit| length_to_mm(unit, displayed))
            .unwrap_or_else(|| self.display.to_domain(displayed))
    }

    fn domain_step(&self) -> f64 {
        self.length_unit
            .map(|unit| length_to_mm(unit, self.speed))
            .unwrap_or(self.speed)
    }
}

fn length_from_mm(unit: CanvasSizeUnit, value_mm: f64) -> f64 {
    f64::from(unit.from_mm(value_mm as f32))
}

fn length_to_mm(unit: CanvasSizeUnit, value: f64) -> f64 {
    f64::from(unit.to_mm(value as f32))
}

fn draw_unit(ui: &mut Ui, unit: &str) {
    if !unit.is_empty() {
        ui.weak(unit);
    }
}

fn snapped_stepped_int(value: i64, min: i64, max: i64, step: i64) -> i64 {
    debug_assert!(step > 0);
    let offset = value.saturating_sub(min);
    let lower = min.saturating_add(offset.div_euclid(step).saturating_mul(step));
    let upper = lower.saturating_add(step);
    let snapped = if value.saturating_sub(lower) < upper.saturating_sub(value) {
        lower
    } else {
        upper
    };
    snapped.clamp(min, max - (max - min).rem_euclid(step))
}

fn admitted_float_from_control(
    bounds: plotx_core::properties::FloatBounds,
    current: f64,
    proposed: f64,
    display: plotx_core::properties::FloatDisplay,
    display_step: f64,
) -> f64 {
    if bounds.admits(proposed) {
        return proposed;
    }
    if let Some(threshold) = bounds.excluded_magnitude
        && proposed.abs() <= threshold
    {
        let domain_step = (display.to_domain(display.to_display(current) + display_step) - current)
            .abs()
            .max(threshold.next_up());
        let sign = if proposed < current { -1.0 } else { 1.0 };
        let candidate = sign * domain_step;
        if bounds.admits(candidate) {
            return candidate;
        }
    }
    current
}

/// Report a continuous control's drag edges to the section that owns the
/// gesture. Only the edges: what happens in between is an ordinary write.
fn note_gesture(
    row: &Row,
    response: &egui::Response,
    gesture: &mut Option<(PropertyId, GestureEdge)>,
) {
    if response.drag_started() {
        *gesture = Some((row.presentation.id, GestureEdge::Started));
    } else if response.drag_stopped() {
        *gesture = Some((row.presentation.id, GestureEdge::Stopped));
    }
}

/// Blank a drag control's readout while leaving it draggable and typable. The
/// number it still carries only decides where a drag starts; it is never shown.
fn hide_value(drag: DragValue<'_>, hidden: bool) -> DragValue<'_> {
    if hidden {
        drag.custom_formatter(|_, _| NO_SINGLE_VALUE.to_owned())
    } else {
        drag
    }
}

fn describe<'a>(row: &Row, value: &'a PropertyValue) -> Cow<'a, str> {
    match value {
        PropertyValue::Bool(value) => Cow::Borrowed(if *value { "on" } else { "off" }),
        PropertyValue::Text(value) => Cow::Borrowed(value),
        PropertyValue::Int(value) => Cow::Owned(value.to_string()),
        PropertyValue::Float(value) => Cow::Owned(format!("{value:.4}")),
        // Read the label from the full static variant list, not the ones this
        // field permits: a default may name a choice the user cannot switch
        // back to by hand, and it still has to be nameable in the tooltip.
        PropertyValue::Enum(value) => match row.definition.value_schema {
            ValueSchema::Enum { variants } => variants
                .iter()
                .find(|variant| variant.id == *value)
                .map(|variant| Cow::Borrowed(variant.canonical_label))
                .unwrap_or_else(|| Cow::Borrowed(value)),
            _ => Cow::Borrowed(value),
        },
        PropertyValue::Color(color) => {
            Cow::Owned(format!("#{:02x}{:02x}{:02x}", color.r, color.g, color.b))
        }
    }
}

#[cfg(test)]
#[path = "control_tests.rs"]
mod tests;
