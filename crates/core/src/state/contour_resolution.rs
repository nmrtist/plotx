use super::*;

/// Resolve one half. Pure: it reads only its arguments and the caller's
/// `estimate` lookup, and reports both an unmet estimate and an unreachable
/// threshold by appending to caller-owned buffers rather than touching session
/// state, which the caller owns and knows how to word.
pub(super) fn resolve_half(
    source: VersionedFieldRef,
    level: &ContourLevelSpec,
    summary: FieldSummary,
    negative: bool,
    estimate: &mut impl FnMut(&EstimateKey) -> Option<EstimateResult>,
    pending: &mut Vec<EstimateKey>,
    unreachable: &mut Vec<UnreachableContourThreshold>,
) -> Option<Vec<FiniteF64>> {
    let min = summary.min.get();
    let max = summary.max.get();
    let peak = if negative {
        -min.min(0.0)
    } else {
        max.max(0.0)
    };
    if peak <= 0.0 {
        return Some(Vec::new());
    }
    let base = resolve_contour_base(source, level, summary, negative, estimate, pending)?;

    // The ladder — including which policies may be rewritten when their base is
    // unusable — is shared with the analysis-map path and speaks only in
    // positive magnitudes; this half applies its own sign afterwards. Deciding
    // there and reporting here keeps one policy: a half is blank for exactly the
    // reason the ladder says it is.
    let ladder = crate::contour_ladder::contour_level_ladder(base, peak, level);
    if let Some(threshold) = ladder.threshold_above_peak
        && let Some(threshold) = FiniteF64::new(threshold)
        && let Some(peak) = FiniteF64::new(peak)
    {
        unreachable.push(UnreachableContourThreshold {
            negative,
            threshold,
            peak,
        });
    }
    Some(
        ladder
            .levels
            .into_iter()
            .map(|value| if negative { -value } else { value })
            .filter_map(FiniteF64::new)
            .collect(),
    )
}

/// Resolve the anchor independently of whether this sign currently crosses it.
/// A phase preview freezes this magnitude, but still clips the ladder against
/// the new real plane so emerging lobes and new high levels remain visible.
pub(crate) fn resolve_contour_base(
    source: VersionedFieldRef,
    level: &ContourLevelSpec,
    summary: FieldSummary,
    negative: bool,
    estimate: &mut impl FnMut(&EstimateKey) -> Option<EstimateResult>,
    pending: &mut Vec<EstimateKey>,
) -> Option<f64> {
    let min = summary.min.get();
    let max = summary.max.get();
    let peak = if negative {
        -min.min(0.0)
    } else {
        max.max(0.0)
    };
    Some(match &level.base {
        ContourBasePolicy::Absolute(value) => value.get(),
        ContourBasePolicy::FractionOfRange(fraction) => {
            // A base policy never yields a signed magnitude (§4.3): this half
            // owns the sign and applies it below. Working across the raw
            // `min..max` span instead would hand a signed base to a field that
            // has both signs — for a spectrum running -P..P the positive half's
            // "four percent" came out at -0.92·P. Measuring from this half's own
            // floor (the sample closest to zero on its side) up to its peak
            // keeps the result an unsigned magnitude for every field, and is
            // identical to the span form on the single-signed fields the
            // `Bounded` capability admits.
            let floor = if negative {
                (-max).max(0.0)
            } else {
                min.max(0.0)
            };
            floor + fraction.get() * (peak - floor)
        }
        ContourBasePolicy::NoiseFloor {
            multiplier,
            peak_fraction,
            estimator,
        } => {
            let key = EstimateKey {
                source,
                kind: EstimateKind::Noise,
                estimator: estimator.clone(),
            };
            let Some(EstimateResult::Scale(result)) = estimate(&key) else {
                pending.push(key);
                return None;
            };
            // The floor is measured against the *field's* peak, not this half's.
            // Sampling artefacts are driven by the strongest feature whatever
            // its sign, and a per-half floor would also split the two halves
            // onto different ladders, which the geometry budget relies on them
            // not doing.
            multiplier.get() * resolved_noise_scale(result.scale, *peak_fraction, summary).0
        }
        ContourBasePolicy::BackgroundScale {
            multiplier,
            estimator,
        } => {
            let key = EstimateKey {
                source,
                kind: EstimateKind::Background,
                estimator: estimator.clone(),
            };
            let Some(EstimateResult::LocationScale(result)) = estimate(&key) else {
                pending.push(key);
                return None;
            };
            // Background fields carry a location as well as a spread. The
            // contour policy expresses the physical level `location + k*scale`;
            // a contour half later supplies its sign.
            (result.location.get() + multiplier.get() * result.scale.get()).abs()
        }
    })
}
