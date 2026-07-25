//! The one contour level-ladder policy, shared by every contour path.
//!
//! `plotx_render::contour::geometric_levels` is a pure ladder generator: given a
//! usable base it emits levels and truncates at the peak. Deciding what to do
//! when the base is *not* usable is policy rather than rendering, so it lives
//! here instead of in `plotx-render` — and in exactly one place, so the legacy
//! ILT/DOSY analysis-map path and the versioned field resolver cannot drift.

use plotx_figure::{ContourBasePolicy, ContourLevelSpec};

/// One contour half's ladder, together with the reason it may be empty.
pub(crate) struct ContourLadder {
    pub levels: Vec<f64>,
    /// The user's own [`ContourBasePolicy::Absolute`] threshold, recorded only
    /// when this half's peak never reaches it so nothing could be drawn. Callers
    /// that can talk to the user must say so; a blank half with no explanation
    /// is the failure this carries the numbers to prevent.
    pub threshold_above_peak: Option<f64>,
}

/// Resolve one contour half's level ladder.
///
/// `base` and `peak` are unsigned magnitudes belonging to a single half: a base
/// policy never produces a signed absolute value, and the caller applies its
/// half's sign to the returned magnitudes.
///
/// An unusable base is handled according to *where the base came from*, because
/// the two cases mean opposite things:
///
/// - A base a policy *derived* — most often a zero scale estimate on a flat or
///   ideal synthetic grid — is not something the user typed, and would otherwise
///   leave a permanently blank plot with no indication of why. It falls back to
///   a base derived from the spec the user actually selected, never to a hidden
///   peak fraction, so `count` and `ratio` still control the output. A fallback
///   that is itself unusable (a non-finite peak, or a ratio ladder that
///   overflows) draws nothing.
/// - [`ContourBasePolicy::Absolute`] *is* the user's explicit input, the
///   strongest term of the value-resolution order. Rewriting it would silently
///   draw a ladder at levels the user never asked for, so it is obeyed
///   literally: a threshold the field never reaches yields no levels, and
///   `threshold_above_peak` carries the numbers needed to explain that.
pub(crate) fn contour_level_ladder(
    base: f64,
    peak: f64,
    level: &ContourLevelSpec,
) -> ContourLadder {
    let usable = |value: f64| value.is_finite() && value > 0.0 && value < peak;
    let base = if usable(base) {
        base
    } else if matches!(level.base, ContourBasePolicy::Absolute(_)) {
        // `Absolute` wraps a `PositiveFiniteF64`, and both callers reject a
        // non-positive peak before reaching here, so the only way an explicit
        // threshold is unusable is that it sits at or above the peak. The
        // comparison is still written out rather than assumed, so a future
        // caller with a non-finite peak reports nothing instead of a bad number.
        return ContourLadder {
            levels: Vec::new(),
            threshold_above_peak: (base >= peak).then_some(base),
        };
    } else if level.count == 1 {
        // One level always means one visible, interior contour: a lone level at
        // or beyond the peak has no crossing at all.
        peak / 2.0
    } else {
        peak / level
            .ratio
            .get()
            .powi(i32::from(level.count.saturating_sub(1)))
    };
    if !usable(base) {
        return ContourLadder {
            levels: Vec::new(),
            threshold_above_peak: None,
        };
    }
    ContourLadder {
        levels: plotx_render::contour::geometric_levels(
            base,
            peak,
            usize::from(level.count),
            level.ratio.get(),
        ),
        threshold_above_peak: None,
    }
}
