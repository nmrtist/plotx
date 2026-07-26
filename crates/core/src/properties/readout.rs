//! What a contour's lowest level *currently means*, for display next to the
//! control and in the corner of the plot.
//!
//! A multiple is not a threshold. "5" means five noise σ, five units of
//! intensity, or five percent of a range depending on the anchor, and only the
//! first of those tells the user whether a cross peak will survive. §4.3
//! therefore asks the interface to show the resolved semantics — `5 × σ =
//! 1.2e4` — rather than the bare number the control edits.
//!
//! The resolved half of that sentence comes from an estimate, and estimates are
//! asynchronous, content-addressed and computed on demand (§3.4, §6). This
//! module is consequently **read-only in the strongest sense**: it takes
//! `&self`, reads only what the derived caches already hold, and has no path to
//! minting a field version, queueing a job, or materializing a payload. A miss
//! is reported as a miss. Computing an estimate so that a label could be
//! populated would make drawing the interface schedule scientific work, which
//! is exactly the coupling the field runtime exists to prevent.

use super::contour;
use super::target::series_context_unchecked;
use super::{PropertyError, PropertyValue, ResolvedProperty};
use crate::automation::TargetRef;
use crate::state::{
    ContourResolution, EstimateKey, EstimateKind, EstimateResult, EstimatedScale, FieldRef,
    FieldSummary, NoiseScaleTerm, PlotxApp, VersionedFieldRef, contour_base_kind,
    resolve_contour_levels, resolved_noise_scale,
};
use plotx_figure::{ContourBasePolicy, SeriesEncoding, UnitInterval};

/// A display-oriented value belonging to one addressed property.
///
/// Most settings need only their resolved scalar. Providers with a value whose
/// meaning depends on cached scientific state can return a richer variant
/// without teaching the service which encoding owns it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PropertyReadout {
    Value(PropertyValue),
    ContourBase(ContourBaseReadout),
}

/// Turn an ordinary resolved value into a readout.
///
/// A display readout cannot invent one value for a property whose own copies
/// disagree. Callers get an explicit error and can leave the label absent
/// rather than rendering an arbitrary target as the selection's value.
pub(crate) fn uniform_readout(
    resolved: ResolvedProperty,
) -> Result<PropertyReadout, PropertyError> {
    let property = resolved.address.definition;
    let value = resolved.value.uniform().copied().ok_or_else(|| {
        PropertyError::NotApplicable(format!(
            "{} has no single value to show in a readout",
            property.as_str()
        ))
    })?;
    Ok(PropertyReadout::Value(value))
}

/// Whether the number a user set can currently be turned into a level, and why
/// not when it cannot.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ContourAnchor {
    /// The magnitude needs no measurement: it is already a level, or a fraction
    /// of a range the field summary knows.
    Direct,
    /// The estimate this multiple is measured against is cached, and it is at
    /// or above the anchor's floor, so the estimate is what the number means.
    Measured,
    /// The cached estimate is *below* the anchor's peak floor, so the multiple
    /// is measured against the floor instead.
    ///
    /// This is a different sentence from [`Self::Measured`] rather than a detail
    /// of it. The level on screen no longer follows the estimator, and an
    /// interface that still read `5 × σ` here would be naming a quantity the
    /// plot is not drawn from — the exact substitution this readout exists to
    /// make impossible.
    Floored,
    /// The estimator ran and measured no spread at all. The multiple anchors
    /// nothing — five times zero is zero — so the ladder falls back to one
    /// derived from the field's own peak, and saying "5σ = 0" would be a
    /// misleading way to describe a plot that is visibly drawing contours.
    Degenerate,
    /// Nothing is cached yet. The multiple is known and the level is not;
    /// inventing one here would mean measuring on the interface thread.
    Measuring,
}

/// The lowest contour level of one series, in the terms the user set it in and
/// in the field's own units.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ContourBaseReadout {
    /// The anchor kind, one of the `CONTOUR_BASE_*` ids, so a caller can word
    /// the multiple without matching on a policy type it should not know.
    pub kind: &'static str,
    /// The number the control edits: a multiplier, a fraction, or a level.
    pub magnitude: f64,
    /// The lowest level actually drawn right now, when it is known. This is the
    /// resolved ladder's first rung, not `base` — a ladder truncated at the
    /// peak, or rebuilt from the peak after a degenerate estimate, draws
    /// something other than the number the anchor implies, and the readout
    /// reports what is on screen.
    pub lowest_level: Option<f64>,
    /// The fraction of the field's peak magnitude this anchor will not resolve
    /// below, for anchors that carry such a floor; `None` for the rest. It is
    /// reported whether or not the floor is currently the term in force, so a
    /// caller can name the floor in both sentences.
    pub peak_fraction: Option<f64>,
    pub anchor: ContourAnchor,
}

/// Read what one contour series' lowest level currently means.
///
/// This is the contour provider's specialization of the generic property
/// readout. It remains cache-only: a target whose estimate has not arrived
/// returns [`ContourAnchor::Measuring`] rather than starting work.
pub(crate) fn contour_base_readout(
    app: &PlotxApp,
    target: &TargetRef,
) -> Option<ContourBaseReadout> {
    let context = series_context_unchecked(app, target).ok()?;
    let SeriesEncoding::Contour(spec) = context.encoding else {
        return None;
    };
    let source = VersionedFieldRef {
        field: FieldRef {
            resource: context.dataset.resource_id(),
            field: context.field,
        },
        // A field with no runtime version has never been drawn, so nothing
        // about it is cached. Minting one here would be this module
        // allocating session state on behalf of a label.
        version: app.session.compute.current_field_version(FieldRef {
            resource: context.dataset.resource_id(),
            field: context.field,
        })?,
    };
    let summary = app.session.compute.peek_field_summary(source);
    let anchor = match &spec.positive.base {
        ContourBasePolicy::Absolute(_) | ContourBasePolicy::FractionOfRange(_) => {
            ContourAnchor::Direct
        }
        ContourBasePolicy::NoiseFloor {
            peak_fraction,
            estimator,
            ..
        } => floored_anchor_of(
            app,
            &EstimateKey {
                source,
                kind: EstimateKind::Noise,
                estimator: estimator.clone(),
            },
            *peak_fraction,
            summary,
        ),
        ContourBasePolicy::BackgroundScale { estimator, .. } => anchor_of(
            app,
            &EstimateKey {
                source,
                kind: EstimateKind::Background,
                estimator: estimator.clone(),
            },
        ),
    };
    // Resolution is pure arithmetic over a cached summary and cached
    // estimates; the payload is never touched. A miss simply yields
    // `Pending`, which is reported rather than acted on.
    let lowest_level = summary.and_then(|summary| {
        match resolve_contour_levels(source, spec, summary, |key| {
            app.session.compute.peek_estimate(key).cloned()
        }) {
            ContourResolution::Ready { levels, .. } => {
                levels.positive.first().map(|level| level.get())
            }
            ContourResolution::Pending(_) | ContourResolution::Unavailable => None,
        }
    });
    Some(ContourBaseReadout {
        kind: contour_base_kind(&spec.positive.base),
        magnitude: contour::base_magnitude(&spec.positive.base),
        lowest_level,
        peak_fraction: contour::base_peak_fraction(&spec.positive.base),
        anchor,
    })
}

/// Classify a floored noise anchor: whether the estimate or the floor is the
/// scale in force right now.
///
/// Without a cached summary the floor cannot be compared against anything, and
/// answering `Measured` would assert the estimate is in force when it may not
/// be. A field with no cached summary has never been drawn, so reporting the
/// measurement as outstanding is both true and the only thing this read-only
/// module may do about it.
fn floored_anchor_of(
    app: &PlotxApp,
    key: &EstimateKey,
    peak_fraction: UnitInterval,
    summary: Option<FieldSummary>,
) -> ContourAnchor {
    let (Some(EstimateResult::Scale(estimate)), Some(summary)) =
        (app.session.compute.peek_estimate(key), summary)
    else {
        return ContourAnchor::Measuring;
    };
    match resolved_noise_scale(estimate.scale, peak_fraction, summary) {
        // Neither term measured anything: a flat field with no peak either.
        (scale, _) if scale <= 0.0 => ContourAnchor::Degenerate,
        (_, NoiseScaleTerm::PeakFloor) => ContourAnchor::Floored,
        (_, NoiseScaleTerm::Estimated) => ContourAnchor::Measured,
    }
}

fn anchor_of(app: &PlotxApp, key: &EstimateKey) -> ContourAnchor {
    let scale = match app.session.compute.peek_estimate(key) {
        None => return ContourAnchor::Measuring,
        Some(EstimateResult::Scale(estimate)) => estimate.scale,
        Some(EstimateResult::LocationScale(estimate)) => estimate.scale,
    };
    match scale {
        EstimatedScale::Positive(_) => ContourAnchor::Measured,
        EstimatedScale::Degenerate => ContourAnchor::Degenerate,
    }
}

#[cfg(test)]
#[path = "readout_tests.rs"]
mod tests;
