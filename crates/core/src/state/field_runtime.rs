//! Runtime field snapshots, versioned derived-data keys, and contour resolution.
//!
//! These types deliberately live below the document model. Field versions and
//! caches are session state: project persistence retains provenance and encoding
//! choices, never a stale runtime token or derived geometry.

use super::{DatasetId, FieldCapabilities, FieldId, scalar_grid_capabilities};
use crate::automation::{CAP_FIELD_COLORED_RASTER_2D, CAP_FIELD_CURVE_1D, CapabilityId};
use plotx_figure::{
    ContourBasePolicy, ContourLevelSpec, ContourSpec, EstimatorSelection, PositiveFiniteF64,
    UnitInterval,
};
use serde::{Deserialize, Deserializer, Serialize, Serializer, de};
use std::cmp::Ordering;
use std::collections::BTreeMap;
use std::fmt;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

/// A finite floating-point value whose equality and hash use canonical IEEE
/// bits. `-0.0` is normalized to `0.0`, and NaN/infinity cannot enter a cache
/// key or field summary.
#[derive(Clone, Copy, Default)]
pub struct FiniteF64(u64);

impl FiniteF64 {
    pub fn new(value: f64) -> Option<Self> {
        value
            .is_finite()
            .then(|| Self(if value == 0.0 { 0 } else { value.to_bits() }))
    }

    pub const fn get(self) -> f64 {
        f64::from_bits(self.0)
    }
}

impl fmt::Debug for FiniteF64 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_tuple("FiniteF64")
            .field(&self.get())
            .finish()
    }
}

impl PartialEq for FiniteF64 {
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0
    }
}

impl Eq for FiniteF64 {}

impl PartialOrd for FiniteF64 {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for FiniteF64 {
    fn cmp(&self, other: &Self) -> Ordering {
        self.get().total_cmp(&other.get())
    }
}

impl Hash for FiniteF64 {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.0.hash(state);
    }
}

impl Serialize for FiniteF64 {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_f64(self.get())
    }
}

impl<'de> Deserialize<'de> for FiniteF64 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = f64::deserialize(deserializer)?;
        Self::new(value).ok_or_else(|| de::Error::custom("expected a finite floating-point value"))
    }
}

/// A reference to a field child resource. It is a data source, never a plot
/// component: contour properties remain addressed by the owning `SeriesId`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FieldRef {
    pub resource: DatasetId,
    pub field: FieldId,
}

/// Runtime-only revision of immutable field data. It is deliberately separate
/// from persisted field identity and is not part of the project format.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FieldVersion(pub u64);

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct VersionedFieldRef {
    pub field: FieldRef,
    pub version: FieldVersion,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FieldAlgorithmProvenance {
    pub algorithm: String,
    pub version: u32,
}

/// Persistable provenance deliberately excludes the session-only field version.
#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct FieldProvenance {
    pub source_fingerprint: Option<String>,
    pub algorithm: Option<FieldAlgorithmProvenance>,
    pub metadata: BTreeMap<String, String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct FieldSummary {
    pub min: FiniteF64,
    pub max: FiniteF64,
}

/// A complete, owned snapshot suitable for a worker. Scalar summaries follow
/// scalar snapshots; colored rasters intentionally leave this as `None`.
#[derive(Clone, Debug)]
pub struct FieldSnapshot {
    pub source: VersionedFieldRef,
    pub payload: FieldPayload,
    pub summary: Option<FieldSummary>,
    pub provenance: FieldProvenance,
}

impl FieldSnapshot {
    /// `cached` is a summary the runtime already holds for exactly this
    /// `(field, version)`; supplying it skips the full min/max scan. The scalar
    /// invariant is enforced here rather than by callers: only a scalar grid
    /// ever carries a summary, and a well-formed one always does.
    pub fn new(
        source: VersionedFieldRef,
        payload: FieldPayload,
        provenance: FieldProvenance,
        cached: Option<FieldSummary>,
    ) -> Self {
        let summary = match payload.scalar_grid() {
            Some(_) => cached.or_else(|| payload.summary()),
            None => None,
        };
        Self {
            source,
            payload,
            summary,
            provenance,
        }
    }
}

/// Field payloads stay arity- and representation-specific. In particular, a
/// colored raster has no scalar statistics and cannot reach contour resolution.
#[derive(Clone, Debug)]
pub enum FieldPayload {
    ScalarGrid2D(ScalarGrid2D),
    Curve1D(Curve1D),
    ColoredRaster2D(ColoredRaster2D),
}

impl FieldPayload {
    pub fn scalar_grid(&self) -> Option<&ScalarGrid2D> {
        match self {
            Self::ScalarGrid2D(grid) => Some(grid),
            Self::Curve1D(_) | Self::ColoredRaster2D(_) => None,
        }
    }

    pub fn summary(&self) -> Option<FieldSummary> {
        self.scalar_grid().and_then(ScalarGrid2D::summary)
    }

    pub fn representation(&self) -> FieldRepresentation {
        match self {
            Self::ScalarGrid2D(grid) => grid.representation(),
            Self::Curve1D(_) => FieldRepresentation::Curve1D,
            Self::ColoredRaster2D(_) => FieldRepresentation::ColoredRaster2D,
        }
    }

    /// Capabilities implied by the concrete payload representation. Providers
    /// may add semantic capabilities (signed, noise scale, units), but must not
    /// claim a regular scalar grid for an explicitly sampled one.
    ///
    /// This delegates so that a materialized payload and the cheap
    /// [`FieldRepresentation`] query a provider answers on the UI thread can
    /// never disagree: there is one derivation, not two.
    pub fn intrinsic_capabilities(&self) -> FieldCapabilities {
        self.representation().intrinsic_capabilities()
    }
}

/// Everything capability derivation needs to know about a field, and nothing
/// that requires materializing its values. Providers answer this on the UI
/// thread on every descriptor lookup, so it must stay O(rows + cols) at worst.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FieldRepresentation {
    ScalarGrid2D {
        rows: usize,
        cols: usize,
        /// Length of the row-major buffer the provider would produce. Kept
        /// separate from `rows * cols` so a malformed import is rejected here
        /// rather than by indexing inside marching squares.
        values: usize,
        x_linear: bool,
        y_linear: bool,
    },
    Curve1D,
    ColoredRaster2D,
}

impl FieldRepresentation {
    /// The single derivation of representation-implied capabilities.
    pub fn intrinsic_capabilities(self) -> FieldCapabilities {
        match self {
            Self::ScalarGrid2D { .. } => scalar_grid_capabilities(self.is_regular(), &[]),
            Self::Curve1D => FieldCapabilities::new([CapabilityId::new(CAP_FIELD_CURVE_1D)]),
            Self::ColoredRaster2D => {
                FieldCapabilities::new([CapabilityId::new(CAP_FIELD_COLORED_RASTER_2D)])
            }
        }
    }

    /// Marching squares accepts only a linearly sampled grid whose declared
    /// shape matches its buffer. `AxisSampling::Explicit` is not regular.
    pub fn is_regular(self) -> bool {
        match self {
            Self::ScalarGrid2D {
                rows,
                cols,
                values,
                x_linear,
                y_linear,
            } => {
                rows >= 2
                    && cols >= 2
                    && rows
                        .checked_mul(cols)
                        .is_some_and(|expected| expected == values)
                    && x_linear
                    && y_linear
            }
            Self::Curve1D | Self::ColoredRaster2D => false,
        }
    }
}

#[derive(Clone, Debug)]
pub struct ScalarGrid2D {
    pub values: Arc<[f32]>,
    pub rows: usize,
    pub cols: usize,
    pub x: AxisSampling,
    pub y: AxisSampling,
}

impl ScalarGrid2D {
    /// A worker may only index a grid after this check. Provider adapters build
    /// exact row-major buffers, but malformed imported dimensions must become a
    /// recoverable field error rather than an indexing panic in marching squares.
    pub fn has_valid_shape(&self) -> bool {
        self.rows
            .checked_mul(self.cols)
            .is_some_and(|expected| expected == self.values.len())
    }

    pub fn representation(&self) -> FieldRepresentation {
        FieldRepresentation::ScalarGrid2D {
            rows: self.rows,
            cols: self.cols,
            values: self.values.len(),
            x_linear: matches!(self.x, AxisSampling::Linear { .. }),
            y_linear: matches!(self.y, AxisSampling::Linear { .. }),
        }
    }

    pub fn is_regular(&self) -> bool {
        self.representation().is_regular()
    }

    pub fn linear_bounds(&self) -> Option<[f64; 4]> {
        let (
            AxisSampling::Linear { start: x0, end: x1 },
            AxisSampling::Linear { start: y0, end: y1 },
        ) = (&self.x, &self.y)
        else {
            return None;
        };
        [*x0, *x1, *y0, *y1]
            .iter()
            .all(|value| value.is_finite())
            .then_some([*x0, *x1, *y0, *y1])
    }

    pub fn summary(&self) -> Option<FieldSummary> {
        if !self.has_valid_shape() {
            return None;
        }
        let mut values = self
            .values
            .iter()
            .copied()
            .filter(|value| value.is_finite());
        let first = f64::from(values.next()?);
        let (min, max) = values.fold((first, first), |(min, max), value| {
            let value = f64::from(value);
            (min.min(value), max.max(value))
        });
        Some(FieldSummary {
            min: FiniteF64::new(min)?,
            max: FiniteF64::new(max)?,
        })
    }
}

#[derive(Clone, Debug)]
pub enum AxisSampling {
    Linear { start: f64, end: f64 },
    Explicit(Arc<[f64]>),
}

#[derive(Clone, Debug)]
pub struct Curve1D {
    pub x: Arc<[f64]>,
    pub values: Arc<[f32]>,
}

#[derive(Clone, Debug)]
pub struct ColoredRaster2D {
    pub pixels: Arc<[u8]>,
    pub rows: usize,
    pub cols: usize,
    pub format: RasterFormat,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RasterFormat {
    Rgb8,
    Rgba8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum EstimateKind {
    Noise,
    Background,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct EstimateKey {
    pub source: VersionedFieldRef,
    pub kind: EstimateKind,
    pub estimator: EstimatorSelection,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EstimateProvenance {
    pub estimator: String,
    pub version: u32,
}

/// A scale an estimator successfully produced.
///
/// A flat, constant or ideal synthetic field genuinely has no spread. That is a
/// valid, cacheable *result* — the estimator ran, and its provenance is real —
/// which is why it is spelled here rather than by widening
/// [`PositiveFiniteF64`]: that invariant guards renderer and cache keys and must
/// keep rejecting zero. An estimator that could not run at all never reaches
/// this type; it stays an error and reaches the user.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum EstimatedScale {
    Positive(PositiveFiniteF64),
    /// The estimator ran and measured no spread whatsoever.
    Degenerate,
}

impl EstimatedScale {
    /// Classify a raw estimator output. A finite zero is a degenerate result;
    /// anything non-finite or negative means the estimator itself misbehaved,
    /// so it must be reported rather than cached.
    pub fn new(value: f64) -> Option<Self> {
        if let Some(positive) = PositiveFiniteF64::new(value) {
            return Some(Self::Positive(positive));
        }
        (value.is_finite() && value == 0.0).then_some(Self::Degenerate)
    }

    /// The measured magnitude; a degenerate estimate measures exactly zero.
    pub const fn get(self) -> f64 {
        match self {
            Self::Positive(scale) => scale.get(),
            Self::Degenerate => 0.0,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ScaleEstimate {
    pub scale: EstimatedScale,
    pub provenance: EstimateProvenance,
}

#[derive(Clone, Debug, PartialEq)]
pub struct LocationScaleEstimate {
    pub location: FiniteF64,
    pub scale: EstimatedScale,
    pub provenance: EstimateProvenance,
}

#[derive(Clone, Debug, PartialEq)]
pub enum EstimateResult {
    Scale(ScaleEstimate),
    LocationScale(LocationScaleEstimate),
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ResolvedContourLevels {
    pub positive: Arc<[FiniteF64]>,
    pub negative: Arc<[FiniteF64]>,
}

impl ResolvedContourLevels {
    pub fn empty() -> Self {
        Self {
            positive: Arc::from([]),
            negative: Arc::from([]),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ContourGeometryCacheKey {
    pub source: VersionedFieldRef,
    pub levels: ResolvedContourLevels,
}

pub type ContourSegment = [[f64; 2]; 2];

#[derive(Clone, Debug)]
pub struct ContourGeometry {
    pub positive: Arc<[ContourSegment]>,
    pub negative: Arc<[ContourSegment]>,
    pub positive_levels: u16,
    pub negative_levels: u16,
    /// The levels the renderer's segment budget left undrawn, or `None` when
    /// every resolved level was drawn. It travels with the geometry rather than
    /// beside it because the two are one answer: the segments say what was
    /// drawn, and this says what the same build refused to draw and why the
    /// plot is therefore not the ladder the panel shows.
    pub omitted: Option<OmittedContourLevels>,
}

/// Levels a contour build dropped because drawing them would have exceeded
/// [`plotx_render::contour::MAX_CONTOUR_SEGMENTS`].
///
/// Whole levels are dropped, outermost kept first, precisely so the omission
/// stays explainable: a plot that draws its top *n* levels is a plot with a
/// higher lowest level, which is a picture a user can reason about and fix. A
/// contour cut off part-way along its own path is not.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OmittedContourLevels {
    pub positive: u16,
    pub negative: u16,
    /// Magnitude of the outermost level that did not fit. Every level at or
    /// below it was dropped, in both halves.
    pub highest_omitted: FiniteF64,
    /// Magnitude of the lowest level actually drawn, or `None` when the budget
    /// could not fit even one level.
    pub lowest_drawn: Option<FiniteF64>,
}

impl ContourGeometry {
    pub fn empty() -> Self {
        Self {
            positive: Arc::from([]),
            negative: Arc::from([]),
            positive_levels: 0,
            negative_levels: 0,
            omitted: None,
        }
    }
}

/// An explicit contour threshold the field never reaches, so its half draws
/// nothing at all.
///
/// A mistyped magnitude — one extra zero — otherwise produces a blank plot with
/// no way to tell it apart from a field that simply has no signal. Resolution is
/// a pure function of its inputs and has no business writing status text, so it
/// reports the two numbers and lets the application layer word them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UnreachableContourThreshold {
    /// Which half fell out. Both values below are unsigned magnitudes, so for
    /// the negative half `peak` is the magnitude of the most negative sample.
    pub negative: bool,
    /// The threshold the user set for this half.
    pub threshold: FiniteF64,
    /// This half's peak magnitude, always strictly below `threshold`.
    pub peak: FiniteF64,
}

/// Main-thread contour resolution outcome. Workers only ever receive the
/// `Ready` absolute levels, never a spec, summary, or estimate.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ContourResolution {
    Ready {
        levels: ResolvedContourLevels,
        /// Explicit thresholds no level could be drawn from. Empty on every
        /// ordinary resolution; a non-empty list means a half is deliberately
        /// blank and the user must be told why.
        unreachable: Vec<UnreachableContourThreshold>,
    },
    Pending(Vec<EstimateKey>),
    Unavailable,
}

/// Which term of a floored noise anchor supplied the scale actually in force.
///
/// A readout that named only the multiplier would be true of both cases and
/// informative about neither: `5 × σ` describes a level five thermal-noise units
/// up, and the same words over a floored anchor describe a level that has
/// nothing to do with the estimate. The resolver therefore reports which term
/// won alongside the value, and the interface says so.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NoiseScaleTerm {
    /// The estimator's own measurement is at or above the policy's floor.
    Estimated,
    /// The estimate is below the floor, so the multiple is measured against a
    /// fraction of the field's peak magnitude instead.
    PeakFloor,
}

/// The peak magnitude of a summary: the largest absolute value the field holds,
/// whatever its sign.
pub fn summary_peak_magnitude(summary: FieldSummary) -> f64 {
    summary.max.get().abs().max(summary.min.get().abs())
}

/// The noise scale a floored anchor resolves against, and the term that supplied
/// it. Ties go to the estimate: with no floor configured, or with a floor the
/// estimate already clears, this is exactly the estimator's answer.
///
/// A [`EstimatedScale::Degenerate`] result is deliberately *not* floored. The
/// floor stands for the sampling artefacts a real measurement carries around its
/// own strongest feature; a field that measures no spread whatsoever — a plane,
/// an ideal synthetic — has no such structure to be protected from, and
/// flooring it would trade a ladder that spans the field for fourteen rungs
/// crowded against zero. "Nothing was measurable" is a different answer from "a
/// very small scale was measured", which is why the estimate type spells the two
/// apart, and the degenerate ladder policy in `contour_ladder` stays in force.
pub fn resolved_noise_scale(
    scale: EstimatedScale,
    peak_fraction: UnitInterval,
    summary: FieldSummary,
) -> (f64, NoiseScaleTerm) {
    let EstimatedScale::Positive(estimated) = scale else {
        return (scale.get(), NoiseScaleTerm::Estimated);
    };
    let floor = peak_fraction.get() * summary_peak_magnitude(summary);
    if floor.is_finite() && floor > estimated.get() {
        (floor, NoiseScaleTerm::PeakFloor)
    } else {
        (estimated.get(), NoiseScaleTerm::Estimated)
    }
}

pub fn resolve_contour_levels(
    source: VersionedFieldRef,
    spec: &ContourSpec,
    summary: FieldSummary,
    mut estimate: impl FnMut(&EstimateKey) -> Option<EstimateResult>,
) -> ContourResolution {
    let mut pending = Vec::new();
    let mut unreachable = Vec::new();
    let positive = resolve_half(
        source,
        &spec.positive,
        summary,
        false,
        &mut estimate,
        &mut pending,
        &mut unreachable,
    );
    let negative = spec.negative.as_ref().map(|level| {
        resolve_half(
            source,
            level,
            summary,
            true,
            &mut estimate,
            &mut pending,
            &mut unreachable,
        )
    });
    if !pending.is_empty() {
        let mut unique = Vec::new();
        for key in pending {
            if !unique.contains(&key) {
                unique.push(key);
            }
        }
        return ContourResolution::Pending(unique);
    }
    let (Some(positive), Some(negative)) = (positive, negative.unwrap_or(Some(Vec::new()))) else {
        return ContourResolution::Unavailable;
    };
    ContourResolution::Ready {
        levels: ResolvedContourLevels {
            positive: Arc::from(positive),
            negative: Arc::from(negative),
        },
        unreachable,
    }
}

/// Resolve one half. Pure: it reads only its arguments and the caller's
/// `estimate` lookup, and reports both an unmet estimate and an unreachable
/// threshold by appending to caller-owned buffers rather than touching session
/// state, which the caller owns and knows how to word.
fn resolve_half(
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
    let base = match &level.base {
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
    };
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

#[cfg(test)]
#[path = "field_runtime_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "field_runtime_degenerate_tests.rs"]
mod degenerate_tests;

#[cfg(test)]
#[path = "field_runtime_threshold_tests.rs"]
mod threshold_tests;

#[cfg(test)]
#[path = "field_runtime_fraction_tests.rs"]
mod fraction_tests;

#[cfg(test)]
#[path = "field_runtime_floor_tests.rs"]
mod floor_tests;

#[cfg(test)]
#[path = "field_progress_tests.rs"]
mod progress_tests;
