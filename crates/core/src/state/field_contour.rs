use super::PeakMagnitude;
use plotx_figure::{ContourBasePolicy, EstimatorSelection, PositiveFiniteF64, UnitInterval};

/// Stable ids of the contour base policies, shared by the default factory and
/// the property catalog so a base chosen either way is the same value.
pub const CONTOUR_BASE_ABSOLUTE: &str = "absolute";
pub const CONTOUR_BASE_NOISE_FLOOR: &str = "noise_floor";
pub const CONTOUR_BASE_BACKGROUND_SCALE: &str = "background_scale";
pub const CONTOUR_BASE_FRACTION_OF_RANGE: &str = "fraction_of_range";

/// The conventional lowest level of a peak-anchored ladder, and the fraction the
/// bounded policy starts from.
const CONTOUR_BASE_FRACTION: f64 = 0.04;
/// The conventional distance from the noise or background floor.
const CONTOUR_BASE_MULTIPLIER: f64 = 5.0;
/// The smallest noise scale a σ-anchored base accepts, as a fraction of the
/// field's peak magnitude.
///
/// This is a calibration, not a convention, and it is the one number in this
/// file that should be re-measured when new evidence arrives.
///
/// A noise estimator measures thermal noise. A 2D plane with large dynamic
/// range also carries the sampling artefacts of its own strongest feature —
/// indirect-dimension (t₁) noise ridges and residual solvent ridges — whose
/// amplitude scales with that feature rather than with the thermal floor, and
/// which are conventionally quoted at 10⁻³ to 10⁻⁴ of the parent peak. A level
/// below that traces artefacts, not signal.
///
/// Measured on a 2048 × 8192 ¹H–¹H NOESY (peak 3.304e8, robust σ 1.669e3, so
/// 197,900:1 dynamic range) by counting the grid crossings of a single level
/// swept geometrically: 2.81e6 crossings at 0.001 % of peak, 8.99e5 at 0.004 %,
/// then 7.56e4 at 0.008 % and a smooth halving per octave above that. The knee
/// at ≈ 0.008 % of peak — 16 σ — is where contours stop following the artefact
/// floor, and it agrees with the conventional t₁-noise magnitude. The floor is
/// set at that knee, and the ladder's own 5× multiplier then places the lowest
/// level five artefact-floor units above it, exactly as 5σ places it five
/// thermal-noise units above thermal noise.
///
/// Re-calibrate if the noise estimator changes what it measures, if the
/// renderer's segment budget changes, or if fields are seen whose artefact floor
/// sits elsewhere. The floor binds only above a dynamic range of 1/this value;
/// below it the estimated scale wins and nothing about resolution changes.
const CONTOUR_NOISE_FLOOR_PEAK_FRACTION: f64 = 1.0e-4;

pub fn contour_base_kind(policy: &ContourBasePolicy) -> &'static str {
    match policy {
        ContourBasePolicy::Absolute(_) => CONTOUR_BASE_ABSOLUTE,
        ContourBasePolicy::NoiseFloor { .. } => CONTOUR_BASE_NOISE_FLOOR,
        ContourBasePolicy::BackgroundScale { .. } => CONTOUR_BASE_BACKGROUND_SCALE,
        ContourBasePolicy::FractionOfRange(_) => CONTOUR_BASE_FRACTION_OF_RANGE,
    }
}

/// The canonical parameters of one base policy.
///
/// Whether a policy *may* be chosen is a capability question answered by the
/// caller; this only says what it looks like when it is. Returns `None` for an
/// unknown id rather than substituting a policy the caller did not ask for.
pub fn contour_base_policy(kind: &str, peak: PeakMagnitude<'_>) -> Option<ContourBasePolicy> {
    let policy = match kind {
        CONTOUR_BASE_ABSOLUTE => ContourBasePolicy::Absolute(absolute_base(peak)),
        CONTOUR_BASE_NOISE_FLOOR => ContourBasePolicy::NoiseFloor {
            multiplier: PositiveFiniteF64::new(CONTOUR_BASE_MULTIPLIER)
                .expect("literal multiplier is valid"),
            peak_fraction: UnitInterval::new(CONTOUR_NOISE_FLOOR_PEAK_FRACTION)
                .expect("literal fraction is valid"),
            estimator: EstimatorSelection::Frozen {
                estimator: plotx_analysis::robust::ROBUST_DIFFERENCE_MAD_ID.to_owned(),
                version: plotx_analysis::robust::ROBUST_DIFFERENCE_MAD_VERSION,
            },
        },
        CONTOUR_BASE_BACKGROUND_SCALE => ContourBasePolicy::BackgroundScale {
            multiplier: PositiveFiniteF64::new(CONTOUR_BASE_MULTIPLIER)
                .expect("literal multiplier is valid"),
            estimator: EstimatorSelection::Frozen {
                estimator: plotx_analysis::robust::DEPLANED_LOCATION_SCALE_ID.to_owned(),
                version: plotx_analysis::robust::DEPLANED_LOCATION_SCALE_VERSION,
            },
        },
        CONTOUR_BASE_FRACTION_OF_RANGE => ContourBasePolicy::FractionOfRange(
            UnitInterval::new(CONTOUR_BASE_FRACTION).expect("literal fraction is valid"),
        ),
        _ => return None,
    };
    Some(policy)
}

/// An absolute base anchored to the field's own peak.
///
/// A fixed literal cannot serve here: a base of one intensity unit draws nothing
/// at all on any field whose peak is below one, and does so silently, with no
/// control in the panel that explains the blank plot. The peak is the only
/// scale-free anchor available when no capability offers a better one; the
/// literal remains solely as the last resort when even that is unknown.
fn absolute_base(peak: PeakMagnitude<'_>) -> PositiveFiniteF64 {
    peak()
        .map(|peak| peak * CONTOUR_BASE_FRACTION)
        .and_then(PositiveFiniteF64::new)
        .unwrap_or_else(|| PositiveFiniteF64::new(1.0).expect("literal base is valid"))
}
