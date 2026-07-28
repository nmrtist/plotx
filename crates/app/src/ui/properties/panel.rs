//! The property panel: catalog-driven controls for the current selection.
//!
//! Every row here is generated from a [`PropertyDefinition`] plus its
//! presentation entry, so a property gains a control, a search entry, a
//! "modified" marker and a reset by being registered once. Only `Essential`
//! rows are rendered by default; everything else is folded away, which is what
//! keeps the panel from growing a row per feature.

#[path = "control.rs"]
mod control;
#[path = "sections.rs"]
mod sections;

use super::{PRESENTATIONS, PropertyPresentation};
use egui::{DragValue, Ui};
use egui_phosphor::regular as icon;
use plotx_core::automation::TargetRef;
use plotx_core::properties::{
    AggregateValue, EncodingKind, PropertyDefinition, PropertyId, PropertyReadout, PropertyValue,
    ResolvedProperty, ResolvedPropertySet, ResolvedSchema, ValueCopies, ValueSchema, contour,
    phase, zero_fill,
};
use plotx_core::state::{ObjectId, PlotxApp, PropertyFocus, PropertyTextEditState};

/// The home-route section id of the contour rows. The route table and the
/// collapsing header below must agree on it, so both read this constant.
pub(crate) const CONTOUR_SECTION: &str = "object.contour";
/// The home section for scalar heatmap colour-range rows.
pub(crate) const HEATMAP_SECTION: &str = "object.heatmap";
/// The home section for line-encoding rows on selected plot objects.
pub(crate) const LINE_SECTION: &str = "object.line";
pub(crate) const AXIS_SECTION: &str = "object.axes";
pub(crate) const STACK_SECTION: &str = "object.stack";
pub(crate) const CHART_SECTION: &str = "object.chart";
pub(crate) const TEXT_SECTION: &str = "object.text";
pub(crate) const SHAPE_SECTION: &str = "object.shape";
pub(crate) const PANEL_SECTION: &str = "object.panel";
pub(crate) const OBJECT_SECTION: &str = "object.general";
/// The document root's figure typography rows.
pub(crate) const TYPOGRAPHY_SECTION: &str = "document.figure_typography";
pub(crate) const CANVAS_MARGINS_SECTION: &str = "canvas.margins";
pub(crate) const CANVAS_GRID_SECTION: &str = "canvas.grid";
pub(crate) const CANVAS_SIZE_SECTION: &str = "canvas.size";
pub(crate) const CANVAS_CAPTION_SECTION: &str = "canvas.caption";
/// The processing editor's per-step apodization rows.
pub(crate) const APODIZATION_SECTION: &str = "dataset.apodization";
pub(crate) const ZERO_FILL_SECTION: &str = "dataset.zero_fill";
pub(crate) const PHASE_SECTION: &str = "dataset.phase";
pub(crate) const BASELINE_SECTION: &str = "dataset.baseline";
pub(crate) const REFERENCE_SECTION: &str = "dataset.reference";
pub(crate) const SMOOTH_SECTION: &str = "dataset.smooth";
pub(crate) const NORMALIZE_SECTION: &str = "dataset.normalize";
pub(crate) const BIN_SECTION: &str = "dataset.bin";
pub(crate) const PROCESSING_STEP_SECTION: &str = "dataset.processing_step";
pub(crate) const PROCESSING_ADVANCED_SECTION: &str = "dataset.processing_advanced";
/// Updates remain in the General Preferences rail page, but have their own
/// density budget because they are a distinct settings sub-struct.
pub(crate) const PREFERENCES_UPDATES_SECTION: &str = "preferences.updates";

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

/// What a section counts itself in, in both grammatical numbers.
///
/// English does not pluralize by appending an `s` — "2 contour seriess" is what
/// that produces — so both forms are declared rather than derived.
#[derive(Clone, Copy)]
struct SectionNoun {
    singular: &'static str,
    plural: &'static str,
}

impl SectionNoun {
    const fn new(singular: &'static str, plural: &'static str) -> Self {
        Self { singular, plural }
    }

    fn of(self, count: usize) -> &'static str {
        if count == 1 {
            self.singular
        } else {
            self.plural
        }
    }

    fn counted(self, count: usize) -> String {
        format!("{count} {}", self.of(count))
    }
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
    readout: Option<PropertyReadout>,
}

impl Row {
    /// The current value, or `None` when the sources behind the row disagree —
    /// several selected series, or the two halves of one contour ladder.
    fn value(&self) -> Option<&PropertyValue> {
        self.set.value.uniform()
    }

    /// The value a control edits *from* when there is none to show. It is the
    /// factory default, never one source's value: the numeric and choice
    /// controls hide it outright, and the colour swatch — which cannot render
    /// blank — then shows a colour that belongs to nobody in the selection
    /// rather than passing one target's off as the answer.
    fn editing_value(&self) -> Option<&PropertyValue> {
        self.value().or(self.representative.default_value.as_ref())
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
    ResetEncoding(EncodingKind),
}

/// What a continuous control did to the gesture it belongs to this frame.
///
/// A drag writes every frame; only the release ends the gesture. The control
/// reports the transition and the section acts on it once the immutable borrows
/// are done, so every catalog row coalesces the same way regardless of which
/// typed store it happens to write.
#[derive(Clone, Copy, PartialEq)]
enum GestureEdge {
    Started,
    Stopped,
}

pub(crate) use self::sections::{
    apodization_section, axis_section, baseline_section, bin_section, canvas_caption_section,
    canvas_grid_section, canvas_margins_section, canvas_size_section, chart_section,
    contour_section, general_object_section, heatmap_section, line_section, normalize_section,
    panel_inline_section, panel_section, phase_section, preferences_section,
    processing_advanced_section, processing_step_section, reference_section, shape_section,
    smooth_section, stack_section, text_section, typography_section, zero_fill_section,
};

#[cfg(test)]
fn resolve_rows(app: &PlotxApp, targets: &[TargetRef]) -> Vec<Row> {
    resolve_rows_for(app, targets, CONTOUR_SECTION)
}

fn resolve_rows_for(app: &PlotxApp, targets: &[TargetRef], section: &str) -> Vec<Row> {
    let mut rows = Vec::new();
    for presentation in PRESENTATIONS {
        let Some(definition) = presentation.definition() else {
            continue;
        };
        if presentation.home_route.section != section {
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
        let readout = if presentation.id == contour::BASE_MAGNITUDE && set.value.uniform().is_some()
        {
            match app.property_readout(first) {
                Ok(readout @ PropertyReadout::ContourBase(_)) => Some(readout),
                Ok(
                    PropertyReadout::Value(_)
                    | PropertyReadout::ZeroFillTarget(_)
                    | PropertyReadout::PhasePivotPpm { .. },
                )
                | Err(_) => None,
            }
        } else if [zero_fill::MODE, phase::PIVOT].contains(&presentation.id)
            && set.value.uniform().is_some()
        {
            app.property_readout(first).ok()
        } else {
            None
        };
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

/// The union of the targets this section's rows apply to, in selection order.
fn applicable_targets(rows: &[Row]) -> Vec<TargetRef> {
    let mut targets: Vec<TargetRef> = Vec::new();
    for address in rows
        .iter()
        .flat_map(|row| row.set.applicable_targets.iter())
    {
        if !targets.contains(&address.target) {
            targets.push(address.target.clone());
        }
    }
    targets
}

fn apply(app: &mut PlotxApp, targets: &[TargetRef], pending: Pending, status_noun: SectionNoun) {
    let planned = match pending {
        Pending::Write(property, value) => app.plan_property_write(property, targets, &value),
        Pending::Reset(property) => app.plan_property_reset(property, targets),
        // Scoped to the encoding this section is about: a plot that stacks a
        // contour over a heatmap must not have the heatmap rebuilt by a button
        // that names the contour.
        Pending::ResetEncoding(encoding) => app.plan_encoding_reset(encoding, targets),
    };
    match planned {
        Ok(commit) => {
            let skipped = commit.skipped.clone();
            let applied = commit.applied.len();
            // A skipped target is reported, never silently dropped: the user
            // asked for the whole selection and must learn what it did not do.
            app.session.status = if skipped.is_empty() {
                format!("Updated {}.", status_noun.counted(applied))
            } else {
                format!(
                    "Updated {}; skipped {}: {}",
                    status_noun.counted(applied),
                    skipped.len(),
                    skipped[0].message
                )
            };
            // Persistence failures are reported by the commit after this
            // optimistic status is installed, so they remain visible instead
            // of being overwritten by "Updated".
            app.commit_property(commit);
        }
        Err(error) => {
            app.session.status = format!("Could not change {}: {error}", status_noun.plural);
        }
    }
}

#[cfg(test)]
#[path = "panel_tests.rs"]
mod tests;
