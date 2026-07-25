//! The property panel: catalog-driven controls for the current selection.
//!
//! Every row here is generated from a [`PropertyDefinition`] plus its
//! presentation entry, so a property gains a control, a search entry, a
//! "modified" marker and a reset by being registered once. Only `Essential`
//! rows are rendered by default; everything else is folded away, which is what
//! keeps the panel from growing a row per feature.

use super::{PRESENTATIONS, PropertyPresentation};
use egui::{DragValue, Ui};
use egui_phosphor::regular as icon;
use plotx_core::automation::TargetRef;
use plotx_core::properties::{
    AggregateValue, ContourBaseReadout, EncodingKind, PropertyDefinition, PropertyId,
    PropertyValue, ResolvedProperty, ResolvedPropertySet, ResolvedSchema, ValueCopies, ValueSchema,
    contour,
};
use plotx_core::state::{ObjectId, PlotxApp, PropertyFocus};

/// The home-route section id of the contour rows. The route table and the
/// collapsing header below must agree on it, so both read this constant.
pub(crate) const CONTOUR_SECTION: &str = "object.contour";

/// What a control shows in place of a number or a choice when there is none:
/// the sources behind the row do not agree, so no value may be presented as the
/// current one.
const NO_SINGLE_VALUE: &str = "—";

/// Why a row has no single value, and what setting one now will do.
///
/// Which sources disagree is derived from two facts the row already knows: how
/// many targets it read, and whether the definition says one target holds one
/// copy of the setting or one per mirrored half. Stating both possibilities at
/// once was true but blunt — a single selected series with an asymmetric ladder
/// and two series that merely differ are different problems with different
/// fixes, and the row can tell them apart.
fn no_single_value_hint(targets: usize, copies: ValueCopies) -> String {
    let halves = copies == ValueCopies::PerMirroredHalf;
    let sources = match (targets, halves) {
        (0..=1, true) => "the positive and negative halves of this ladder hold different values",
        (0..=1, false) => "this series holds more than one value for it",
        (_, true) => {
            "the selected series — and the two halves of each ladder — do not all hold the \
             same value"
        }
        (_, false) => "the selected series do not all hold the same value",
    };
    format!("No single value: {sources}. Setting it now applies to all of them.")
}

/// One catalog row, already resolved against the selection.
struct Row {
    presentation: &'static PropertyPresentation,
    definition: &'static PropertyDefinition,
    set: ResolvedPropertySet,
    representative: ResolvedProperty,
    /// What the number in this row currently *means* (§4.3). Present only on
    /// the anchored-level row, only when the row has a single value to explain,
    /// and only from what the derived caches already hold — reading it never
    /// starts a measurement.
    readout: Option<ContourBaseReadout>,
}

impl Row {
    /// The current value, or `None` when the sources behind the row disagree —
    /// several selected series, or the two halves of one contour ladder.
    fn value(&self) -> Option<PropertyValue> {
        self.set.value.uniform().copied()
    }

    /// The value a control edits *from* when there is none to show. It is the
    /// factory default, never one source's value: the numeric and choice
    /// controls hide it outright, and the colour swatch — which cannot render
    /// blank — then shows a colour that belongs to nobody in the selection
    /// rather than passing one target's off as the answer.
    fn editing_value(&self) -> Option<PropertyValue> {
        self.value().or(self.representative.default_value)
    }

    fn mixed(&self) -> bool {
        matches!(self.set.value, AggregateValue::Mixed)
    }

    fn modified(&self) -> bool {
        self.representative.is_modified() || self.mixed()
    }
}

/// A control edit waiting to be committed once the immutable borrows are done.
enum Pending {
    Write(PropertyId, PropertyValue),
    Reset(PropertyId),
    ResetEncoding,
}

/// Render the contour section for the current selection. Returns `false` when
/// nothing in the selection draws a contour, in which case the caller draws no
/// heading either.
pub(crate) fn contour_section(
    app: &mut PlotxApp,
    canvas: usize,
    objects: &[ObjectId],
    ui: &mut Ui,
) -> bool {
    let targets: Vec<TargetRef> = objects
        .iter()
        .flat_map(|&object| app.series_targets(canvas, object))
        .collect();
    if targets.is_empty() {
        return false;
    }
    let rows = resolve_rows(app, &targets);
    if rows.is_empty() {
        return false;
    }

    let now = ui.input(|input| input.time);
    let focus = app.session.ui.property_focus;
    let focused_here =
        focus.is_some_and(|focus| rows.iter().any(|row| row.presentation.id == focus.property));

    ui.separator();
    ui.horizontal(|ui| {
        ui.strong("Contour");
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.weak(match targets.len() {
                1 => "1 series".to_owned(),
                count => format!("{count} series"),
            });
        });
    });

    // The rows rendered without expanding anything are exactly the list the
    // budget check counts, so the check cannot pass while the panel shows more.
    let essential: Vec<PropertyId> = super::essential_in(CONTOUR_SECTION)
        .into_iter()
        .map(|entry| entry.id)
        .collect();
    let mut pending: Option<Pending> = None;
    for row in rows
        .iter()
        .filter(|row| essential.contains(&row.presentation.id))
    {
        property_row(row, focus, now, &mut pending, ui);
    }

    let advanced: Vec<&Row> = rows
        .iter()
        .filter(|row| !essential.contains(&row.presentation.id))
        .collect();
    if !advanced.is_empty() {
        let id = ui.make_persistent_id(("property_section", CONTOUR_SECTION));
        let mut state =
            egui::collapsing_header::CollapsingState::load_with_default_open(ui.ctx(), id, false);
        if focused_here
            && focus.is_some_and(|focus| {
                advanced
                    .iter()
                    .any(|row| row.presentation.id == focus.property)
            })
        {
            state.set_open(true);
            state.store(ui.ctx());
        }
        egui::CollapsingHeader::new("Advanced")
            .id_salt(("property_section", CONTOUR_SECTION))
            .show(ui, |ui| {
                for row in advanced {
                    property_row(row, focus, now, &mut pending, ui);
                }
            });
    }

    if ui
        .small_button("Reset contour")
        .on_hover_text("Rebuild this series' encoding from its defaults")
        .clicked()
    {
        pending = Some(Pending::ResetEncoding);
    }

    // The reveal is one-shot: once the section has been drawn with the row in
    // it, only the fading highlight remains.
    if let Some(focus) = app.session.ui.property_focus.as_mut()
        && focused_here
    {
        focus.pending = false;
    }
    if focus.is_some_and(|focus| now >= focus.highlight_until) {
        app.session.ui.property_focus = None;
    }

    if let Some(pending) = pending {
        apply(app, &targets, pending);
    }
    true
}

fn resolve_rows(app: &PlotxApp, targets: &[TargetRef]) -> Vec<Row> {
    let mut rows = Vec::new();
    for presentation in PRESENTATIONS {
        let Some(definition) = presentation.definition() else {
            continue;
        };
        if definition.applicability.encoding != Some(plotx_core::properties::EncodingKind::Contour)
        {
            continue;
        }
        let set = app.resolve_property_set(presentation.id, targets);
        let Some(first) = set.applicable_targets.first() else {
            continue;
        };
        let Ok(representative) = app.resolve_property(first) else {
            continue;
        };
        // A readout is a statement about *the* current value, and it is read
        // from one target. When the sources disagree there is no such value, so
        // the row is given none: resolving one target's level and captioning it
        // as the row's would pass one series' threshold off as the selection's,
        // which is the same misrepresentation the control itself refuses when it
        // blanks its number.
        let readout = (presentation.id == contour::BASE_MAGNITUDE && set.value.uniform().is_some())
            .then(|| app.contour_base_readout(&first.target))
            .flatten();
        rows.push(Row {
            presentation,
            definition,
            set,
            representative,
            readout,
        });
    }
    rows
}

fn property_row(
    row: &Row,
    focus: Option<PropertyFocus>,
    now: f64,
    pending: &mut Option<Pending>,
    ui: &mut Ui,
) {
    let highlighted = focus
        .is_some_and(|focus| focus.property == row.presentation.id && now < focus.highlight_until);
    let response = ui
        .scope(|ui| {
            ui.horizontal(|ui| {
                ui.label(row.presentation.localized_label.get())
                    .on_hover_text(row.definition.canonical_label);
                control(row, pending, ui);
                if row.modified() {
                    modified_marker(row, pending, ui);
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

/// The "modified" affordance: a marker whose tooltip names the default, and a
/// one-click reset back to it.
fn modified_marker(row: &Row, pending: &mut Option<Pending>, ui: &mut Ui) {
    let default = row
        .representative
        .default_value
        .map(|value| describe(row, value))
        .unwrap_or_else(|| "no default".to_owned());
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

/// Draw the control for one row.
///
/// When the row has no single value the widget still edits — one gesture must
/// be enough to make the whole selection agree — but it displays nothing that
/// could be read as the current setting: an em dash instead of a number or a
/// choice, and no checkbox state at all.
fn control(row: &Row, pending: &mut Option<Pending>, ui: &mut Ui) {
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
                let mut current = current;
                if ui.checkbox(&mut current, "").changed() {
                    *pending = Some(Pending::Write(
                        row.presentation.id,
                        PropertyValue::Bool(current),
                    ));
                }
            }
        }
        (ResolvedSchema::Int { min, max }, PropertyValue::Int(current)) => {
            let mut current = current;
            let drag = DragValue::new(&mut current).speed(0.25).range(*min..=*max);
            if ui.add(hide_value(drag, mixed)).changed() {
                *pending = Some(Pending::Write(
                    row.presentation.id,
                    PropertyValue::Int(current),
                ));
            }
        }
        (ResolvedSchema::Float { bounds, log, unit }, PropertyValue::Float(current)) => {
            let mut next = current;
            // A logarithmic quantity spans many decades, so the drag step has
            // to follow the value rather than the (unbounded) range.
            let speed = if *log {
                (current.abs() * 0.02).max(f64::MIN_POSITIVE)
            } else {
                ((bounds.max - bounds.min) / 200.0).max(1.0e-3)
            };
            // An egui range is inclusive, so an open bound is entered here by
            // asking the schema for the smallest value it admits rather than by
            // nudging the literal — the nudge would be a second copy of the rule.
            let drag = DragValue::new(&mut next)
                .speed(speed)
                .range(bounds.lowest()..=bounds.max);
            if ui.add(hide_value(drag, mixed)).changed() {
                *pending = Some(Pending::Write(
                    row.presentation.id,
                    PropertyValue::Float(next),
                ));
            }
            if !unit.is_empty() {
                ui.weak(*unit);
            }
            // §4.3: the multiple alone does not say whether a cross peak
            // survives; the resolved level does.
            if let Some(readout) = &row.readout
                && let Some(suffix) = super::readout::resolution_suffix(readout)
            {
                ui.weak(suffix)
                    .on_hover_text(super::readout::explanation(readout));
            }
        }
        (ResolvedSchema::Enum { variants }, PropertyValue::Enum(current)) => {
            // Nothing is current when the sources disagree, so no variant is
            // marked selected and the box names none of them.
            let current = (!mixed).then_some(current);
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

/// Blank a drag control's readout while leaving it draggable and typable. The
/// number it still carries only decides where a drag starts; it is never shown.
fn hide_value(drag: DragValue<'_>, hidden: bool) -> DragValue<'_> {
    if hidden {
        drag.custom_formatter(|_, _| NO_SINGLE_VALUE.to_owned())
    } else {
        drag
    }
}

fn describe(row: &Row, value: PropertyValue) -> String {
    match value {
        PropertyValue::Bool(value) => if value { "on" } else { "off" }.to_owned(),
        PropertyValue::Int(value) => value.to_string(),
        PropertyValue::Float(value) => format!("{value:.4}"),
        // Read the label from the full static variant list, not the ones this
        // field permits: a default may name a choice the user cannot switch
        // back to by hand, and it still has to be nameable in the tooltip.
        PropertyValue::Enum(value) => match row.definition.value_schema {
            ValueSchema::Enum { variants } => variants
                .iter()
                .find(|variant| variant.id == value)
                .map(|variant| variant.canonical_label.to_owned())
                .unwrap_or_else(|| value.to_owned()),
            _ => value.to_owned(),
        },
        PropertyValue::Color(color) => format!("#{:02x}{:02x}{:02x}", color.r, color.g, color.b),
    }
}

fn apply(app: &mut PlotxApp, targets: &[TargetRef], pending: Pending) {
    let planned = match pending {
        Pending::Write(property, value) => app.plan_property_write(property, targets, &value),
        Pending::Reset(property) => app.plan_property_reset(property, targets),
        // Scoped to the encoding this section is about: a plot that stacks a
        // contour over a heatmap must not have the heatmap rebuilt by a button
        // that names the contour.
        Pending::ResetEncoding => app.plan_encoding_reset(EncodingKind::Contour, targets),
    };
    match planned {
        Ok(commit) => {
            let skipped = commit.skipped.clone();
            let applied = app.commit_property(commit);
            // A skipped target is reported, never silently dropped: the user
            // asked for the whole selection and must learn what it did not do.
            app.session.status = if skipped.is_empty() {
                format!("Updated {applied} contour series.")
            } else {
                format!(
                    "Updated {applied} contour series; skipped {}: {}",
                    skipped.len(),
                    skipped[0].1
                )
            };
        }
        Err(error) => {
            app.session.status = format!("Could not change the contour: {error}");
        }
    }
}

#[cfg(test)]
#[path = "panel_tests.rs"]
mod tests;
