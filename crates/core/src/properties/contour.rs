//! Contour properties on a plot object's series.
//!
//! These are the catalog's first entries and its driving case: the levels a 2D
//! scalar field is drawn at. Their address is deliberately
//! `target.resource = plot object`, `component = Series(SeriesId)` — the field a
//! series reads from is a *data source*, and where geometry happens to be cached
//! is an implementation accident. Neither determines who owns the setting.
//!
//! The ladder is symmetric: base, count and ratio describe one ladder that the
//! negative half mirrors, and each half applies its own sign during resolution.
//! Writes therefore update every half that exists. A [`ContourSpec`] can still
//! hold two halves that differ, so reads of a shared rung report a value only
//! when both halves agree and report [`AggregateValue::Mixed`] when they do not:
//! passing the positive half off as the whole setting is what would let an
//! asymmetric ladder be overwritten without the user ever seeing it.

use super::model::*;
use super::provider::PropertyProvider;
use super::target::{
    not_applicable_encoding, resolved_schema as standard_resolved_schema, series_context,
};
use super::{PropertyAddress, PropertyTransaction, definition, permitted_variants, variant_list};
use crate::automation::{
    CAP_FIELD_BOUNDED, CAP_FIELD_LOCATION_SCALE, CAP_FIELD_NOISE_SCALE,
    CAP_FIELD_SCALAR_GRID_2D_REGULAR, CAP_FIELD_SIGNED,
};
use crate::state::{
    CONTOUR_BASE_ABSOLUTE, CONTOUR_BASE_BACKGROUND_SCALE, CONTOUR_BASE_FRACTION_OF_RANGE,
    CONTOUR_BASE_NOISE_FLOOR, PeakMagnitude, PlotxApp, contour_base_kind, contour_base_policy,
    default_contour_spec, field_peak_magnitude,
};
use plotx_figure::{
    ColorSource, ContourBasePolicy, ContourLevelSpec, ContourSpec, PositiveFiniteF32,
    PositiveFiniteF64, SeriesEncoding, UnitInterval,
};

pub const BASE_POLICY: PropertyId = PropertyId("series.contour.base.policy");
pub const BASE_MAGNITUDE: PropertyId = PropertyId("series.contour.base.magnitude");
pub const COUNT: PropertyId = PropertyId("series.contour.count");
pub const RATIO: PropertyId = PropertyId("series.contour.ratio");
pub const NEGATIVE_ENABLED: PropertyId = PropertyId("series.contour.negative.enabled");
pub const POSITIVE_COLOR: PropertyId = PropertyId("series.contour.positive_color");
pub const NEGATIVE_COLOR: PropertyId = PropertyId("series.contour.negative_color");
pub const LINE_WIDTH: PropertyId = PropertyId("series.contour.line_width");

/// The largest multiplier a σ- or background-anchored base accepts. Well above
/// any real ladder, low enough that a slipped decimal point is rejected instead
/// of silently blanking the plot.
const MAX_MULTIPLIER: f64 = 1.0e4;
/// Ratios beyond this compress a ladder into one visible level.
const MAX_RATIO: f64 = 10.0;

/// The level ratio's range. Open at one because a ladder whose rungs sit at the
/// same height is not a ladder; §4.3 states the invariant as `ratio > 1.0`.
const RATIO_BOUNDS: FloatBounds = FloatBounds::above(1.0, MAX_RATIO);
/// Below this a stroke is invisible on screen and hairline in print.
const LINE_WIDTH_BOUNDS: FloatBounds = FloatBounds::inclusive(0.05, 10.0);

/// The base-policy choices and the capability each one needs. `FractionOfRange`
/// requires a bounded field and is withheld from signed ones: "four percent of
/// the value range" is not a threshold a user can reason about when the range
/// straddles zero.
const BASE_POLICIES: &[EnumVariant] = &[
    EnumVariant::new(CONTOUR_BASE_ABSOLUTE, "Absolute level"),
    EnumVariant::new(CONTOUR_BASE_NOISE_FLOOR, "Multiple of the noise floor")
        .requiring(&[CAP_FIELD_NOISE_SCALE]),
    EnumVariant::new(
        CONTOUR_BASE_BACKGROUND_SCALE,
        "Background + multiple of spread",
    )
    .requiring(&[CAP_FIELD_LOCATION_SCALE]),
    EnumVariant::new(CONTOUR_BASE_FRACTION_OF_RANGE, "Fraction of value range")
        .requiring(&[CAP_FIELD_BOUNDED])
        .forbidding(&[CAP_FIELD_SIGNED]),
];

const CONTOUR: Applicability =
    Applicability::encoding(ComponentKind::Series, EncodingKind::Contour)
        .requiring(&[CAP_FIELD_SCALAR_GRID_2D_REGULAR]);

const SIGNED_CONTOUR: Applicability =
    Applicability::encoding(ComponentKind::Series, EncodingKind::Contour)
        .requiring(&[CAP_FIELD_SCALAR_GRID_2D_REGULAR, CAP_FIELD_SIGNED]);

pub(crate) const DEFINITIONS: &[PropertyDefinition] = &[
    PropertyDefinition {
        id: BASE_MAGNITUDE,
        scope_kind: ScopeKind::Object,
        value_schema: ValueSchema::Float {
            bounds: FloatBounds::above(0.0, f64::MAX),
            log: true,
            drag_step: None,
        },
        access: PropertyAccess::ReadWrite,
        applicability: CONTOUR,
        default_policy: DefaultPolicy::EncodingFactory,
        tier: Tier::Essential,
        copies: ValueCopies::PerMirroredHalf,
        canonical_label: "Lowest contour level",
        canonical_aliases: &[
            "contour threshold",
            "contour base",
            "lowest level",
            "cutoff",
            "noise multiple",
            "sigma",
        ],
    },
    PropertyDefinition {
        id: BASE_POLICY,
        scope_kind: ScopeKind::Object,
        value_schema: ValueSchema::Enum {
            variants: BASE_POLICIES,
        },
        access: PropertyAccess::ReadWrite,
        applicability: CONTOUR,
        default_policy: DefaultPolicy::EncodingFactory,
        tier: Tier::Advanced,
        copies: ValueCopies::PerMirroredHalf,
        canonical_label: "Level anchor",
        canonical_aliases: &["contour base policy", "threshold anchor", "level policy"],
    },
    PropertyDefinition {
        id: COUNT,
        scope_kind: ScopeKind::Object,
        value_schema: ValueSchema::Int {
            min: 1,
            max: ContourLevelSpec::MAX_COUNT as i64,
        },
        access: PropertyAccess::ReadWrite,
        applicability: CONTOUR,
        default_policy: DefaultPolicy::EncodingFactory,
        tier: Tier::Advanced,
        copies: ValueCopies::PerMirroredHalf,
        canonical_label: "Contour levels",
        canonical_aliases: &["contour count", "number of levels", "level count"],
    },
    PropertyDefinition {
        id: RATIO,
        scope_kind: ScopeKind::Object,
        // Open at one: a ratio of exactly one draws every level at the same
        // height. The bound is declared once, so the control cannot offer a
        // value the write path refuses.
        value_schema: ValueSchema::Float {
            bounds: RATIO_BOUNDS,
            log: false,
            drag_step: None,
        },
        access: PropertyAccess::ReadWrite,
        applicability: CONTOUR,
        default_policy: DefaultPolicy::EncodingFactory,
        tier: Tier::Advanced,
        copies: ValueCopies::PerMirroredHalf,
        canonical_label: "Level ratio",
        canonical_aliases: &["contour ratio", "level spacing", "geometric ratio"],
    },
    PropertyDefinition {
        id: NEGATIVE_ENABLED,
        scope_kind: ScopeKind::Object,
        value_schema: ValueSchema::Bool,
        access: PropertyAccess::ReadWrite,
        applicability: SIGNED_CONTOUR,
        default_policy: DefaultPolicy::EncodingFactory,
        tier: Tier::Advanced,
        copies: ValueCopies::PerTarget,
        canonical_label: "Draw negative contours",
        canonical_aliases: &["negative levels", "negative peaks", "show negative"],
    },
    PropertyDefinition {
        id: POSITIVE_COLOR,
        scope_kind: ScopeKind::Object,
        value_schema: ValueSchema::Color,
        access: PropertyAccess::ReadWrite,
        applicability: CONTOUR,
        default_policy: DefaultPolicy::EncodingFactory,
        tier: Tier::Advanced,
        copies: ValueCopies::PerTarget,
        canonical_label: "Positive contour colour",
        canonical_aliases: &["contour colour", "contour color", "positive colour"],
    },
    PropertyDefinition {
        id: NEGATIVE_COLOR,
        scope_kind: ScopeKind::Object,
        value_schema: ValueSchema::Color,
        access: PropertyAccess::ReadWrite,
        applicability: SIGNED_CONTOUR,
        default_policy: DefaultPolicy::EncodingFactory,
        tier: Tier::Advanced,
        copies: ValueCopies::PerTarget,
        canonical_label: "Negative contour colour",
        canonical_aliases: &["negative colour", "negative color"],
    },
    PropertyDefinition {
        id: LINE_WIDTH,
        scope_kind: ScopeKind::Object,
        value_schema: ValueSchema::Float {
            bounds: LINE_WIDTH_BOUNDS,
            log: false,
            drag_step: None,
        },
        access: PropertyAccess::ReadWrite,
        applicability: CONTOUR,
        default_policy: DefaultPolicy::EncodingFactory,
        tier: Tier::Advanced,
        copies: ValueCopies::PerTarget,
        canonical_label: "Contour line width",
        canonical_aliases: &["contour width", "line width", "stroke width"],
    },
];

pub(crate) struct ContourProvider;

pub(crate) static PROVIDER: ContourProvider = ContourProvider;

impl PropertyProvider for ContourProvider {
    fn definitions(&self) -> &'static [PropertyDefinition] {
        DEFINITIONS
    }

    fn read(
        &self,
        app: &PlotxApp,
        address: &PropertyAddress,
    ) -> Result<ResolvedProperty, PropertyError> {
        let definition = definition(address.definition).ok_or_else(|| {
            PropertyError::UnknownProperty(address.definition.as_str().to_owned())
        })?;
        let context = series_context(app, &address.target, definition)?;
        let SeriesEncoding::Contour(spec) = context.encoding else {
            return Err(not_applicable_encoding(definition, context.encoding));
        };
        let value = read(definition.id, spec)
            .ok_or_else(|| PropertyError::UnknownProperty(definition.id.as_str().to_owned()))?;
        let default_value =
            default_value(definition, &context, spec).and_then(|value| value.uniform().copied());
        let availability = match definition.access {
            PropertyAccess::ReadOnly => Availability::ReadOnly,
            PropertyAccess::ReadWrite => Availability::Editable,
        };
        Ok(ResolvedProperty {
            address: address.clone(),
            value,
            default_value,
            availability,
            schema: resolved_schema(definition.id, spec)
                .unwrap_or_else(|| standard_resolved_schema(definition, &context.capabilities)),
        })
    }

    fn readout(
        &self,
        app: &PlotxApp,
        address: &PropertyAddress,
    ) -> Result<super::PropertyReadout, PropertyError> {
        if address.definition != BASE_MAGNITUDE {
            return super::readout::uniform_readout(self.read(app, address)?);
        }
        // Validate the address without resolving the property's default. The
        // default factory may inspect a field payload; a readout must remain a
        // cache-only observation and therefore cannot call the full reader.
        let definition = definition(address.definition).ok_or_else(|| {
            PropertyError::UnknownProperty(address.definition.as_str().to_owned())
        })?;
        let context = series_context(app, &address.target, definition)?;
        if !matches!(context.encoding, SeriesEncoding::Contour(_)) {
            return Err(not_applicable_encoding(definition, context.encoding));
        }
        super::readout::contour_base_readout(app, &address.target)
            .map(super::PropertyReadout::ContourBase)
            .ok_or_else(|| {
                PropertyError::NotApplicable(
                    "the contour base readout needs a contour series".to_owned(),
                )
            })
    }

    fn edit(
        &self,
        app: &PlotxApp,
        transaction: &mut PropertyTransaction,
        address: &PropertyAddress,
        operation: EditOp,
    ) -> Result<(), PropertyError> {
        let definition = definition(address.definition).ok_or_else(|| {
            PropertyError::UnknownProperty(address.definition.as_str().to_owned())
        })?;
        let context = series_context(app, &address.target, definition)?;
        let SeriesEncoding::Contour(current) = context.encoding else {
            return Err(not_applicable_encoding(definition, context.encoding));
        };
        let value = match operation {
            EditOp::Set(value) => {
                let permitted = permitted_variants(&definition.value_schema, &context.capabilities);
                if let PropertyValue::Enum(choice) = value
                    && !permitted.iter().any(|variant| variant.id == choice)
                {
                    return Err(PropertyError::InvalidValue {
                        property: definition.id,
                        message: format!(
                            "'{choice}' needs a capability this field does not expose; this field allows {}",
                            variant_list(&permitted)
                        ),
                    });
                }
                value
            }
            EditOp::Reset => default_value(definition, &context, current)
                .and_then(|value| value.uniform().copied())
                .ok_or(PropertyError::InvalidValue {
                    property: definition.id,
                    message: "the default factory has no single value for this setting".to_owned(),
                })?,
            EditOp::Step(direction) => {
                let binding = transaction.data_binding(app, context.canvas, context.object)?;
                let series = binding
                    .series
                    .iter_mut()
                    .find(|series| series.id == context.series)
                    .ok_or_else(|| PropertyError::UnknownTarget(address.target.describe()))?;
                let SeriesEncoding::Contour(spec) = &mut series.encoding else {
                    return Err(PropertyError::NotApplicable(
                        "the series is no longer a contour".to_owned(),
                    ));
                };
                return step(definition.id, spec, direction);
            }
        };
        let binding = transaction.data_binding(app, context.canvas, context.object)?;
        let series = binding
            .series
            .iter_mut()
            .find(|series| series.id == context.series)
            .ok_or_else(|| PropertyError::UnknownTarget(address.target.describe()))?;
        let SeriesEncoding::Contour(spec) = &mut series.encoding else {
            return Err(PropertyError::NotApplicable(
                "the series is no longer a contour".to_owned(),
            ));
        };
        write(definition.id, spec, &value, &|| {
            field_peak_magnitude(context.dataset, context.field)
        })
    }
}

fn default_value(
    definition: &'static PropertyDefinition,
    context: &super::target::SeriesContext<'_>,
    current: &ContourSpec,
) -> Option<AggregateValue<PropertyValue>> {
    match definition.default_policy {
        DefaultPolicy::ProcessingFactory | DefaultPolicy::None => None,
        DefaultPolicy::Fixed(value) => Some(AggregateValue::Uniform(value)),
        DefaultPolicy::EncodingFactory => {
            let defaults = default_contour_spec(&context.capabilities, &|| {
                field_peak_magnitude(context.dataset, context.field)
            });
            default_for(definition.id, &defaults, current, &|| {
                field_peak_magnitude(context.dataset, context.field)
            })
        }
    }
}

/// Read one property out of a spec. `None` means the id is not a contour
/// property; applicability has already been decided by the caller.
///
/// The four rungs of the shared ladder aggregate over both halves. Everything
/// else — the colours, the line width, whether a negative half exists at all —
/// has exactly one copy and is always `Uniform`.
pub(super) fn read(id: PropertyId, spec: &ContourSpec) -> Option<AggregateValue<PropertyValue>> {
    let value = match id {
        // The base has two faces on one value. A half whose policy is a
        // different *kind* disagrees whatever its number says, and a magnitude
        // read under a different kind would mean something else entirely — "5"
        // is five σ or five intensity units depending on the anchor — so the
        // magnitude agrees only when the kind does too.
        BASE_POLICY => shared(spec, base_kind, |half| {
            PropertyValue::Enum(contour_base_kind(&half.base))
        }),
        BASE_MAGNITUDE => shared(
            spec,
            |half| (base_kind(half), base_magnitude(&half.base)),
            |half| PropertyValue::Float(base_magnitude(&half.base)),
        ),
        COUNT => shared(
            spec,
            |half| half.count,
            |half| PropertyValue::Int(i64::from(half.count)),
        ),
        RATIO => shared(
            spec,
            |half| half.ratio.get(),
            |half| PropertyValue::Float(half.ratio.get()),
        ),
        NEGATIVE_ENABLED => AggregateValue::Uniform(PropertyValue::Bool(spec.negative.is_some())),
        POSITIVE_COLOR => {
            AggregateValue::Uniform(PropertyValue::Color(spec.style.positive_color.resolve()))
        }
        NEGATIVE_COLOR => {
            AggregateValue::Uniform(PropertyValue::Color(spec.style.negative_color.resolve()))
        }
        LINE_WIDTH => {
            AggregateValue::Uniform(PropertyValue::Float(f64::from(spec.style.width.get())))
        }
        _ => return None,
    };
    Some(value)
}

/// Read one rung of the shared ladder across the halves that exist.
///
/// `witness` is what "the halves agree on this rung" means; `value` is what is
/// reported when they do. A spec with no negative half has one source, not a
/// disagreeing one: the user switched the negative contours off, which is a
/// setting of its own and not an asymmetric ladder.
fn shared<W: PartialEq>(
    spec: &ContourSpec,
    witness: impl Fn(&ContourLevelSpec) -> W,
    value: impl Fn(&ContourLevelSpec) -> PropertyValue,
) -> AggregateValue<PropertyValue> {
    match &spec.negative {
        Some(negative) if witness(negative) != witness(&spec.positive) => AggregateValue::Mixed,
        _ => AggregateValue::Uniform(value(&spec.positive)),
    }
}

fn base_kind(half: &ContourLevelSpec) -> &'static str {
    contour_base_kind(&half.base)
}

/// What the default policy resolves to for one property *in the context the
/// target is actually in*.
///
/// Most properties read straight out of the spec the encoding factory would
/// produce. The base magnitude cannot, because a magnitude has no meaning apart
/// from its anchor: "5" is five noise σ under one anchor and five times the
/// value range under another. Handing the factory's number — measured against
/// the anchor the factory chose — to a target the user has since re-anchored
/// produces a value the writer must reject, which is a reset that can never
/// succeed. §8.1 asks for the default to be *derived* from the current context,
/// and the current anchor is part of that context, so the magnitude's default is
/// the one a freshly built base of the target's own kind carries. That is the
/// same construction switching the anchor performs, so "switch to this anchor"
/// and "reset the level under this anchor" agree by construction rather than by
/// two lists of literals staying in step.
pub(super) fn default_for(
    property: PropertyId,
    defaults: &ContourSpec,
    current: &ContourSpec,
    peak: PeakMagnitude<'_>,
) -> Option<AggregateValue<PropertyValue>> {
    if property != BASE_MAGNITUDE {
        return read(property, defaults);
    }
    let base = contour_base_policy(contour_base_kind(&current.positive.base), peak)?;
    Some(AggregateValue::Uniform(PropertyValue::Float(
        base_magnitude(&base),
    )))
}

/// The bounds and caption that this target's *current* base policy implies.
/// The static schema stays context-free; this is what a control is built from.
pub(super) fn resolved_schema(id: PropertyId, spec: &ContourSpec) -> Option<ResolvedSchema> {
    if id != BASE_MAGNITUDE {
        return None;
    }
    let schema = match &spec.positive.base {
        ContourBasePolicy::Absolute(_) => ResolvedSchema::Float {
            bounds: FloatBounds::above(0.0, f64::MAX),
            log: true,
            unit: "intensity",
        },
        ContourBasePolicy::NoiseFloor { .. } => ResolvedSchema::Float {
            bounds: FloatBounds::above(0.0, MAX_MULTIPLIER),
            log: false,
            unit: "× noise floor",
        },
        ContourBasePolicy::BackgroundScale { .. } => ResolvedSchema::Float {
            bounds: FloatBounds::above(0.0, MAX_MULTIPLIER),
            log: false,
            unit: "× spread",
        },
        ContourBasePolicy::FractionOfRange(_) => ResolvedSchema::Float {
            bounds: FloatBounds::above(0.0, 1.0),
            log: false,
            unit: "of range",
        },
    };
    Some(schema)
}

/// The numeric bounds a definition declares, so the write path validates against
/// the very value the control was built from rather than a second copy of it.
fn declared_bounds(id: PropertyId) -> Result<FloatBounds, PropertyError> {
    DEFINITIONS
        .iter()
        .find(|definition| definition.id == id)
        .and_then(|definition| definition.value_schema.float_bounds())
        .ok_or_else(|| PropertyError::UnknownProperty(id.as_str().to_owned()))
}

/// Apply one property to a spec.
///
/// Whether the *choice* is permitted at all is a capability question the
/// planner has already answered from the definition's schema; this validates
/// the value's own range. Base, count and ratio describe the shared ladder and
/// are written to both halves; a half that does not exist is simply absent.
/// Every failure is returned, never swallowed, so a rejected value cannot land
/// partially.
pub(super) fn write(
    id: PropertyId,
    spec: &mut ContourSpec,
    value: &PropertyValue,
    peak: PeakMagnitude<'_>,
) -> Result<(), PropertyError> {
    match id {
        BASE_POLICY => {
            let kind = expect_enum(id, value)?;
            let policy =
                contour_base_policy(kind, peak).ok_or_else(|| PropertyError::InvalidValue {
                    property: id,
                    message: format!("'{kind}' is not a contour base policy"),
                })?;
            for half in halves(spec) {
                half.base = policy.clone();
            }
        }
        BASE_MAGNITUDE => {
            let magnitude = expect_float(id, value)?;
            // Validate against the current policy before touching either half,
            // so a rejected magnitude leaves the spec exactly as it was.
            let updated = with_magnitude(&spec.positive.base, magnitude).ok_or({
                PropertyError::InvalidValue {
                    property: id,
                    message: magnitude_message(&spec.positive.base, magnitude),
                }
            })?;
            for half in halves(spec) {
                half.base = updated.clone();
            }
        }
        COUNT => {
            let count = expect_int(id, value)?;
            let count = u16::try_from(count)
                .ok()
                .filter(|count| *count >= 1 && *count <= ContourLevelSpec::MAX_COUNT)
                .ok_or(PropertyError::InvalidValue {
                    property: id,
                    message: format!(
                        "level count {count} is out of range: it must be between 1 and {}",
                        ContourLevelSpec::MAX_COUNT
                    ),
                })?;
            for half in halves(spec) {
                half.count = count;
            }
        }
        RATIO => {
            let ratio = declared_bounds(id)?.check(id, "level ratio", expect_float(id, value)?)?;
            let ratio = PositiveFiniteF64::new(ratio).ok_or(PropertyError::InvalidValue {
                property: id,
                message: "level ratio must be a finite positive number".to_owned(),
            })?;
            for half in halves(spec) {
                half.ratio = ratio;
            }
        }
        NEGATIVE_ENABLED => {
            let enabled = expect_bool(id, value)?;
            if !enabled {
                spec.negative = None;
            } else if spec.negative.is_none() {
                // The negative half mirrors the ladder the positive half is
                // already drawing; only its sign differs.
                spec.negative = Some(spec.positive.clone());
            }
        }
        POSITIVE_COLOR => {
            spec.style.positive_color = ColorSource::Explicit(expect_color(id, value)?);
        }
        NEGATIVE_COLOR => {
            spec.style.negative_color = ColorSource::Explicit(expect_color(id, value)?);
        }
        LINE_WIDTH => {
            let width = declared_bounds(id)?.check(id, "line width", expect_float(id, value)?)?;
            spec.style.width =
                PositiveFiniteF32::new(width as f32).ok_or(PropertyError::InvalidValue {
                    property: id,
                    message: "line width must be a finite positive number".to_owned(),
                })?;
        }
        _ => {
            return Err(PropertyError::UnknownProperty(id.as_str().to_owned()));
        }
    }
    Ok(())
}

/// Move the lowest level one rung along the ladder this series actually draws.
///
/// The step is *geometric* — the ladder's own `ratio` — rather than additive.
/// Contour levels are `base·ratio^k`, so one press moves the floor by exactly
/// one drawn rung whatever decade the base sits in, and the gesture means the
/// same thing on a spectrum whose peak is 1e2 and one whose peak is 1e9. An
/// additive step would have to be retuned per dataset, which is the reason the
/// ladder is geometric to begin with. A σ- or fraction-anchored base is stepped
/// the same way: its multiple is a position on the same ladder.
///
/// The new value is then written through [`write`], so the gesture is validated
/// by exactly the rules a typed value from the panel is.
pub(super) fn step(
    id: PropertyId,
    spec: &mut ContourSpec,
    step: PropertyStep,
) -> Result<(), PropertyError> {
    if id != BASE_MAGNITUDE {
        return Err(PropertyError::InvalidValue {
            property: id,
            message: "this setting has no step gesture".to_owned(),
        });
    }
    let ratio = spec.positive.ratio.get();
    let current = base_magnitude(&spec.positive.base);
    let stepped = match step {
        PropertyStep::Raise => current * ratio,
        PropertyStep::Lower => current / ratio,
    };
    let next = ceiling(&spec.positive.base).map_or(stepped, |ceiling| stepped.min(ceiling));
    if next == current {
        return Err(PropertyError::InvalidValue {
            property: id,
            message: format!(
                "the lowest contour level is already at the highest value this anchor allows ({current})"
            ),
        });
    }
    write(id, spec, &PropertyValue::Float(next), crate::state::NO_PEAK)
}

/// The largest magnitude a base policy accepts, where it has one. An absolute
/// level is bounded by the field, not by the catalog, so it has none.
fn ceiling(base: &ContourBasePolicy) -> Option<f64> {
    match base {
        ContourBasePolicy::Absolute(_) => None,
        ContourBasePolicy::NoiseFloor { .. } | ContourBasePolicy::BackgroundScale { .. } => {
            Some(MAX_MULTIPLIER)
        }
        ContourBasePolicy::FractionOfRange(_) => Some(1.0),
    }
}

/// Every level ladder the spec currently carries, positive half first.
fn halves(spec: &mut ContourSpec) -> impl Iterator<Item = &mut ContourLevelSpec> {
    std::iter::once(&mut spec.positive).chain(spec.negative.iter_mut())
}

pub(super) fn base_magnitude(base: &ContourBasePolicy) -> f64 {
    match base {
        ContourBasePolicy::Absolute(value) => value.get(),
        ContourBasePolicy::NoiseFloor { multiplier, .. }
        | ContourBasePolicy::BackgroundScale { multiplier, .. } => multiplier.get(),
        ContourBasePolicy::FractionOfRange(fraction) => fraction.get(),
    }
}

/// The peak fraction a base policy will not resolve below, where it has one.
///
/// It is not the number the control edits, so it is deliberately not part of
/// [`base_magnitude`]; it is what a readout needs in order to name the floor it
/// is reporting.
pub(super) fn base_peak_fraction(base: &ContourBasePolicy) -> Option<f64> {
    match base {
        ContourBasePolicy::NoiseFloor { peak_fraction, .. } => Some(peak_fraction.get()),
        ContourBasePolicy::Absolute(_)
        | ContourBasePolicy::BackgroundScale { .. }
        | ContourBasePolicy::FractionOfRange(_) => None,
    }
}

fn with_magnitude(base: &ContourBasePolicy, magnitude: f64) -> Option<ContourBasePolicy> {
    let policy = match base {
        ContourBasePolicy::Absolute(_) => {
            ContourBasePolicy::Absolute(PositiveFiniteF64::new(magnitude)?)
        }
        // The floor travels with the policy: it is a persisted calibration, not
        // a constant resolution reaches for, so editing the multiple must carry
        // the field's own floor forward rather than re-deriving it from whatever
        // the current build happens to think it is.
        ContourBasePolicy::NoiseFloor {
            peak_fraction,
            estimator,
            ..
        } => ContourBasePolicy::NoiseFloor {
            multiplier: bounded_multiplier(magnitude)?,
            peak_fraction: *peak_fraction,
            estimator: estimator.clone(),
        },
        ContourBasePolicy::BackgroundScale { estimator, .. } => {
            ContourBasePolicy::BackgroundScale {
                multiplier: bounded_multiplier(magnitude)?,
                estimator: estimator.clone(),
            }
        }
        ContourBasePolicy::FractionOfRange(_) => ContourBasePolicy::FractionOfRange(
            UnitInterval::new(magnitude).filter(|fraction| fraction.get() > 0.0)?,
        ),
    };
    Some(policy)
}

fn bounded_multiplier(value: f64) -> Option<PositiveFiniteF64> {
    PositiveFiniteF64::new(value).filter(|value| value.get() <= MAX_MULTIPLIER)
}

/// Why a magnitude was refused, in terms of the anchor that refused it.
///
/// The rejected value is part of the message. The bound alone tells a user what
/// the rule is but not which value broke it, and the anchor's bound is not the
/// definition's static bound — a multiplier stops at `MAX_MULTIPLIER` while an
/// absolute level does not — so a caller cannot reconstruct it from the schema.
fn magnitude_message(base: &ContourBasePolicy, magnitude: f64) -> String {
    match base {
        ContourBasePolicy::Absolute(_) => {
            format!(
                "absolute level {magnitude} is out of range: it must be a finite value greater than zero"
            )
        }
        ContourBasePolicy::NoiseFloor { .. } | ContourBasePolicy::BackgroundScale { .. } => {
            format!(
                "multiplier {magnitude} is out of range: it must be greater than 0 and at most {MAX_MULTIPLIER}"
            )
        }
        ContourBasePolicy::FractionOfRange(_) => {
            format!("fraction {magnitude} is out of range: it must be greater than 0 and at most 1")
        }
    }
}

fn expect_bool(property: PropertyId, value: &PropertyValue) -> Result<bool, PropertyError> {
    value.as_bool().ok_or(PropertyError::InvalidValue {
        property,
        message: format!("expected a boolean, got {}", value.kind()),
    })
}

fn expect_int(property: PropertyId, value: &PropertyValue) -> Result<i64, PropertyError> {
    value.as_int().ok_or(PropertyError::InvalidValue {
        property,
        message: format!("expected an integer, got {}", value.kind()),
    })
}

fn expect_float(property: PropertyId, value: &PropertyValue) -> Result<f64, PropertyError> {
    value.as_float().ok_or(PropertyError::InvalidValue {
        property,
        message: format!("expected a number, got {}", value.kind()),
    })
}

fn expect_enum(property: PropertyId, value: &PropertyValue) -> Result<&'static str, PropertyError> {
    value.as_enum().ok_or(PropertyError::InvalidValue {
        property,
        message: format!("expected a choice, got {}", value.kind()),
    })
}

fn expect_color(
    property: PropertyId,
    value: &PropertyValue,
) -> Result<plotx_figure::Color, PropertyError> {
    value.as_color().ok_or(PropertyError::InvalidValue {
        property,
        message: format!("expected a colour, got {}", value.kind()),
    })
}
