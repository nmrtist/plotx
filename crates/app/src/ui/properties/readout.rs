//! Wording for the live contour readout (§4.3).
//!
//! The control edits a number whose meaning depends on the anchor, so the
//! interface states the whole sentence: `5 × σ = 1.2e4`, not `5`. The resolved
//! half comes from an asynchronous estimate, so this also has to say what an
//! unmeasured or degenerate anchor looks like — honestly, and without ever
//! asking for the measurement.

use plotx_core::properties::{ContourAnchor, ContourBaseReadout, PropertyReadout, PropertyValue};
use plotx_core::state::{
    CONTOUR_BASE_ABSOLUTE, CONTOUR_BASE_BACKGROUND_SCALE, CONTOUR_BASE_FRACTION_OF_RANGE,
    CONTOUR_BASE_NOISE_FLOOR,
};

/// The multiple in the terms the user set it in, with no resolved level: `5 × σ`,
/// `background + 5 × spread`, `4% of range`, or a bare level.
///
/// A floored noise anchor has two terms and names the one actually in force. It
/// has to: `5 × σ` over a level the estimate did not produce would describe a
/// picture the data never made, and the whole point of showing the anchor rather
/// than the bare number is that the sentence is true.
pub(crate) fn anchor_expression(readout: &ContourBaseReadout) -> String {
    let magnitude = number(readout.magnitude);
    match readout.kind {
        CONTOUR_BASE_NOISE_FLOOR if readout.anchor == ContourAnchor::Floored => {
            format!("{magnitude} × {}", peak_floor(readout))
        }
        CONTOUR_BASE_NOISE_FLOOR => format!("{magnitude} × σ"),
        CONTOUR_BASE_BACKGROUND_SCALE => format!("background + {magnitude} × spread"),
        CONTOUR_BASE_FRACTION_OF_RANGE => format!("{magnitude} of range"),
        // An absolute level, and anything a future policy adds: the number is
        // already in the field's own units.
        _ => magnitude,
    }
}

/// The anchor's floor, named as the quantity it is: a fraction of the field's
/// own peak. Falls back to the generic words when no fraction is known, so a
/// missing number never turns into a missing explanation.
fn peak_floor(readout: &ContourBaseReadout) -> String {
    match readout.peak_fraction {
        Some(fraction) => format!("{}% of peak", number(fraction * 100.0)),
        None => "the noise floor".to_owned(),
    }
}

/// The full sentence for a compact space — the plot corner, or the row next to
/// the control.
///
/// An absolute base is already a level, so restating it as `1.2e4 = 1.2e4`
/// would be noise; every other anchor resolves to something the number alone
/// does not say.
pub(crate) fn summary(readout: &ContourBaseReadout) -> String {
    let expression = anchor_expression(readout);
    match readout.anchor {
        // The estimator measured no spread at all. Reporting `5 × σ = 0` would
        // describe a blank plot, and the plot is not blank: the ladder falls
        // back to one derived from the field's own peak. Say that instead.
        ContourAnchor::Degenerate => format!("{expression} — no spread measured"),
        ContourAnchor::Measuring => format!("{expression} — measuring…"),
        // The expression already names the floor; this says why it, and not the
        // estimate, is what the level came from. Without the clause the reader
        // has no way to tell a floor that is merely configured from one that is
        // currently deciding the picture.
        ContourAnchor::Floored => match readout.lowest_level {
            Some(level) => format!("{expression} = {} — σ is below this floor", number(level)),
            None => format!("{expression} — σ is below this floor"),
        },
        ContourAnchor::Direct | ContourAnchor::Measured => match readout.lowest_level {
            Some(level) if readout.kind != CONTOUR_BASE_ABSOLUTE => {
                format!("{expression} = {}", number(level))
            }
            Some(level) => number(level),
            None => expression,
        },
    }
}

/// The corner label for one plot, over every series the gesture would move.
///
/// `None` when the plot has no such series. When it has several that agree the
/// label is their shared sentence; when they disagree it says so rather than
/// promoting one series' threshold to the plot's, which would put a number on
/// screen that the keys are about to move away from on some other series.
pub(crate) fn aggregate_summary(readouts: &[ContourBaseReadout]) -> Option<String> {
    let first = readouts.first()?;
    if readouts.iter().all(|readout| readout == first) {
        return Some(summary(first));
    }
    Some(format!(
        "{} contour series — no single lowest level",
        readouts.len()
    ))
}

/// The canvas readout is selected by a property id, not by an encoding type.
/// Contours retain their scientific sentence; ordinary providers get their
/// resolved scalar rendered through the same aggregation rule, so adding a
/// steppable encoding cannot accidentally revive a contour-only lookup path.
pub(crate) fn aggregate_property_summary(readouts: &[PropertyReadout]) -> Option<String> {
    let first = readouts.first()?;
    match first {
        PropertyReadout::ContourBase(_) => {
            let contours: Vec<ContourBaseReadout> = readouts
                .iter()
                .filter_map(|readout| match readout {
                    PropertyReadout::ContourBase(readout) => Some(*readout),
                    PropertyReadout::Value(_) => None,
                })
                .collect();
            if contours.len() != readouts.len() {
                return Some(format!(
                    "{} series — no single property readout",
                    readouts.len()
                ));
            }
            aggregate_summary(&contours)
        }
        PropertyReadout::Value(value) => {
            if readouts.iter().all(|readout| readout == first) {
                Some(value_summary(*value))
            } else {
                Some(format!(
                    "{} series — no single property value",
                    readouts.len()
                ))
            }
        }
    }
}

fn value_summary(value: PropertyValue) -> String {
    match value {
        PropertyValue::Bool(value) => value.to_string(),
        PropertyValue::Int(value) => value.to_string(),
        PropertyValue::Float(value) => number(value),
        PropertyValue::Enum(value) => value.to_owned(),
        PropertyValue::Color(color) => format!("#{:02x}{:02x}{:02x}", color.r, color.g, color.b),
    }
}

/// Just the resolved half, for a row that already shows the number the control
/// edits. `None` when the number is the level and there is nothing to add.
pub(crate) fn resolution_suffix(readout: &ContourBaseReadout) -> Option<String> {
    match readout.anchor {
        ContourAnchor::Degenerate => Some("no spread measured".to_owned()),
        ContourAnchor::Measuring => Some("measuring…".to_owned()),
        // The row's own unit reads "× noise floor", which is true of both terms.
        // Naming the floor here is what makes the row say which one is in force.
        ContourAnchor::Floored => Some(match readout.lowest_level {
            Some(level) => format!("= {} ({})", number(level), peak_floor(readout)),
            None => format!("= {}", peak_floor(readout)),
        }),
        ContourAnchor::Direct | ContourAnchor::Measured => readout
            .lowest_level
            .filter(|_| readout.kind != CONTOUR_BASE_ABSOLUTE)
            .map(|level| format!("= {}", number(level))),
    }
}

/// The longer form, for a tooltip that has room to explain rather than label.
pub(crate) fn explanation(readout: &ContourBaseReadout) -> String {
    let level = readout
        .lowest_level
        .map(|level| format!("The lowest level drawn is {}. ", number(level)))
        .unwrap_or_default();
    if readout.anchor == ContourAnchor::Floored {
        return format!(
            "{level}This field's estimated noise σ is below the anchor's floor of {floor}, \
             so the multiple is measured against the floor instead. A field with this much \
             dynamic range carries the sampling artefacts of its own strongest feature well \
             above its thermal noise, and a level under the floor traces those rather than \
             peaks. Choose the absolute anchor to set a level below it anyway.",
            floor = peak_floor(readout),
        );
    }
    let anchor = match readout.anchor {
        ContourAnchor::Direct => "This level is set directly, so it needs no measurement.",
        // Only an anchor that *has* a floor may mention one; a background anchor
        // has none, and inventing one here would describe a rule it does not
        // follow.
        ContourAnchor::Measured if readout.peak_fraction.is_some() => {
            return format!(
                "{level}The multiple is measured against this field's own estimated noise σ, \
                 so it follows the data. σ is at or above the anchor's floor of {floor}, so \
                 the floor is not in force.",
                floor = peak_floor(readout),
            );
        }
        ContourAnchor::Measured => {
            "The multiple is measured against this field's own estimated scale, \
             so it follows the data."
        }
        ContourAnchor::Floored => unreachable!("returned above"),
        ContourAnchor::Degenerate => {
            "This field has no measurable spread — a flat or perfectly regular \
             grid — so the multiple anchors nothing and the levels fall back to \
             a ladder derived from the field's peak."
        }
        ContourAnchor::Measuring => {
            "The scale this multiple is measured against is still being \
             estimated in the background; the level appears when it arrives."
        }
    };
    format!("{level}{anchor}")
}

/// Print a level or a multiple the way a user reads it: plain decimals across
/// the range typed by hand, scientific notation once the digits stop being
/// legible.
fn number(value: f64) -> String {
    if value != 0.0 && (value.abs() >= 1.0e5 || value.abs() < 1.0e-3) {
        format!("{value:.3e}")
    } else {
        let rounded = (value * 1.0e4).round() / 1.0e4;
        format!("{rounded}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn readout(
        kind: &'static str,
        anchor: ContourAnchor,
        level: Option<f64>,
    ) -> ContourBaseReadout {
        ContourBaseReadout {
            kind,
            magnitude: 5.0,
            lowest_level: level,
            peak_fraction: (kind == CONTOUR_BASE_NOISE_FLOOR).then_some(1.0e-4),
            anchor,
        }
    }

    /// §4.3: the interface states the semantics the estimate gives the number.
    #[test]
    fn a_measured_multiple_reads_as_the_level_it_resolves_to() {
        assert_eq!(
            summary(&readout(
                CONTOUR_BASE_NOISE_FLOOR,
                ContourAnchor::Measured,
                Some(12_000.0)
            )),
            "5 × σ = 12000"
        );
        // Past the point where digits stop being countable, the level switches
        // to scientific notation rather than becoming an unreadable run.
        assert_eq!(
            summary(&readout(
                CONTOUR_BASE_NOISE_FLOOR,
                ContourAnchor::Measured,
                Some(1.234e8)
            )),
            "5 × σ = 1.234e8"
        );
        assert_eq!(
            summary(&readout(
                CONTOUR_BASE_BACKGROUND_SCALE,
                ContourAnchor::Measured,
                Some(3.5)
            )),
            "background + 5 × spread = 3.5"
        );
    }

    /// A degenerate estimate must never be presented as `5 × σ = 0`: that reads
    /// as a blank plot, and the plot is drawing a fallback ladder.
    #[test]
    fn a_degenerate_estimate_is_not_reported_as_a_zero_level() {
        let text = summary(&readout(
            CONTOUR_BASE_NOISE_FLOOR,
            ContourAnchor::Degenerate,
            Some(0.0),
        ));
        assert_eq!(text, "5 × σ — no spread measured");
        assert!(!text.contains("= 0"));
        assert!(
            explanation(&readout(
                CONTOUR_BASE_NOISE_FLOOR,
                ContourAnchor::Degenerate,
                None
            ))
            .contains("no measurable spread")
        );
    }

    /// A pending estimate shows the multiple and admits the rest is unknown.
    #[test]
    fn a_pending_estimate_shows_the_multiple_and_says_it_is_measuring() {
        assert_eq!(
            summary(&readout(
                CONTOUR_BASE_NOISE_FLOOR,
                ContourAnchor::Measuring,
                None
            )),
            "5 × σ — measuring…"
        );
    }

    /// The corner label speaks for every series the keys would move.
    ///
    /// It used to read the plot's *first* series, which is neither necessarily
    /// the one the gesture edits — a contour stacked over a heatmap is drawn
    /// second — nor necessarily the only one. With several contours it printed
    /// one of their thresholds while `+` moved them all.
    #[test]
    fn a_corner_label_over_several_series_states_a_level_only_when_they_agree() {
        let one = readout(
            CONTOUR_BASE_NOISE_FLOOR,
            ContourAnchor::Measured,
            Some(12_000.0),
        );
        assert_eq!(aggregate_summary(&[]), None, "no series, no label");
        assert_eq!(aggregate_summary(&[one]), Some(summary(&one)));
        assert_eq!(
            aggregate_summary(&[one, one]),
            Some(summary(&one)),
            "series that agree read as the value they agree on"
        );

        let other = ContourBaseReadout {
            magnitude: 9.0,
            lowest_level: Some(21_600.0),
            ..one
        };
        let mixed = aggregate_summary(&[one, other]).expect("two series still get a label");
        assert!(
            !mixed.contains("12000") && !mixed.contains("21600"),
            "no single series' level may stand for the plot: {mixed}"
        );
        assert!(mixed.contains('2'), "it says how many disagree: {mixed}");
    }

    /// The sentence that made the floor worth spelling into the policy.
    ///
    /// When the peak floor is what the level came from, saying `5 × σ` would
    /// name a quantity the plot was not drawn from — the exact substitution this
    /// readout exists to prevent. Every surface has to carry it: the compact
    /// corner label, the row beside the control, and the tooltip.
    #[test]
    fn a_floored_anchor_names_the_floor_and_never_the_estimate() {
        let floored = readout(
            CONTOUR_BASE_NOISE_FLOOR,
            ContourAnchor::Floored,
            Some(1.6521e5),
        );

        assert_eq!(
            summary(&floored),
            "5 × 0.01% of peak = 1.652e5 — σ is below this floor"
        );
        assert_eq!(anchor_expression(&floored), "5 × 0.01% of peak");
        assert_eq!(
            resolution_suffix(&floored).as_deref(),
            Some("= 1.652e5 (0.01% of peak)")
        );
        for text in [
            summary(&floored),
            anchor_expression(&floored),
            resolution_suffix(&floored).unwrap_or_default(),
        ] {
            assert!(
                !text.contains("× σ"),
                "a floored level must never be presented as a multiple of σ: {text}"
            );
        }

        let explained = explanation(&floored);
        assert!(explained.contains("below the anchor's floor of 0.01% of peak"));
        assert!(
            explained.contains("absolute anchor"),
            "the tooltip ends on the way past the floor: {explained}"
        );
    }

    /// The other half of the same property: an anchor whose estimate clears the
    /// floor reads exactly as it did before the floor existed, and says the
    /// floor is not in force.
    #[test]
    fn a_measured_anchor_is_unchanged_and_says_the_floor_is_not_in_force() {
        let measured = readout(
            CONTOUR_BASE_NOISE_FLOOR,
            ContourAnchor::Measured,
            Some(12_000.0),
        );
        assert_eq!(summary(&measured), "5 × σ = 12000");
        assert_eq!(resolution_suffix(&measured).as_deref(), Some("= 12000"));
        assert!(explanation(&measured).contains("at or above the anchor's floor"));

        // A background anchor has no floor, so nothing may claim one for it.
        let background = readout(
            CONTOUR_BASE_BACKGROUND_SCALE,
            ContourAnchor::Measured,
            Some(3.5),
        );
        assert!(!explanation(&background).contains("floor"));
    }

    /// An absolute base is already the level; restating it twice is noise.
    #[test]
    fn an_absolute_base_is_stated_once() {
        assert_eq!(
            summary(&readout(
                CONTOUR_BASE_ABSOLUTE,
                ContourAnchor::Direct,
                Some(1_200.0)
            )),
            "1200"
        );
    }
}
