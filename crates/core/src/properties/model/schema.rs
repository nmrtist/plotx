//! Static schemas and values carried through property edits.

use super::{PropertyError, PropertyId};
use plotx_figure::Color;
use std::borrow::Cow;

/// One selectable choice of an enumerated property, together with the field
/// capabilities that make it selectable at all. The gate is declared here rather
/// than in a `match` on a data domain, so a new provider gains or loses a choice
/// purely by exposing or withholding a capability.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EnumVariant {
    pub id: &'static str,
    pub canonical_label: &'static str,
    /// Every capability the target's field must expose.
    pub required_capabilities: &'static [&'static str],
    /// Capabilities that make this choice meaningless even when the required
    /// ones are present — a fraction of the value range says nothing useful on a
    /// field with both signs.
    pub forbidden_capabilities: &'static [&'static str],
}

impl EnumVariant {
    pub const fn new(id: &'static str, canonical_label: &'static str) -> Self {
        Self {
            id,
            canonical_label,
            required_capabilities: &[],
            forbidden_capabilities: &[],
        }
    }

    pub const fn requiring(mut self, capabilities: &'static [&'static str]) -> Self {
        self.required_capabilities = capabilities;
        self
    }

    pub const fn forbidding(mut self, capabilities: &'static [&'static str]) -> Self {
        self.forbidden_capabilities = capabilities;
        self
    }
}

/// The numeric range of a float property.
///
/// A bound can be *open*: a level ratio must be strictly greater than one, or
/// the ladder it describes stops rising. Openness is part of the rule and
/// therefore belongs to the schema, not to whichever control or writer happens
/// to re-state it. The alternative — a control that stops at a rounded literal
/// while the writer tests the real bound — is two copies of one rule, and the
/// copies drift.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FloatBounds {
    pub min: f64,
    pub max: f64,
    /// Whether `min` itself is admitted.
    pub exclusive_min: bool,
    /// One otherwise in-range value that the domain excludes.
    pub excluded: Option<f64>,
    /// Reject every value whose magnitude is at or below this threshold.
    pub excluded_magnitude: Option<f64>,
}

impl FloatBounds {
    /// `min ..= max`.
    pub const fn inclusive(min: f64, max: f64) -> Self {
        Self {
            min,
            max,
            exclusive_min: false,
            excluded: None,
            excluded_magnitude: None,
        }
    }

    /// Strictly above `min`, up to and including `max`.
    pub const fn above(min: f64, max: f64) -> Self {
        Self {
            min,
            max,
            exclusive_min: true,
            excluded: None,
            excluded_magnitude: None,
        }
    }

    /// `min ..= max`, except for one value with no valid domain meaning.
    pub const fn excluding(min: f64, max: f64, excluded: f64) -> Self {
        Self {
            min,
            max,
            exclusive_min: false,
            excluded: Some(excluded),
            excluded_magnitude: None,
        }
    }

    /// `min ..= max`, excluding values too close to zero for the kernel to use.
    pub const fn excluding_magnitude(min: f64, max: f64, threshold: f64) -> Self {
        Self {
            min,
            max,
            exclusive_min: false,
            excluded: None,
            excluded_magnitude: Some(threshold),
        }
    }

    pub fn admits(self, value: f64) -> bool {
        value.is_finite()
            && value <= self.max
            && self.excluded != Some(value)
            && self
                .excluded_magnitude
                .is_none_or(|threshold| value.abs() > threshold)
            && if self.exclusive_min {
                value > self.min
            } else {
                value >= self.min
            }
    }

    /// The smallest value the bound admits.
    ///
    /// A control whose range is inclusive has to start here rather than at
    /// `min`, so it cannot offer a value the write path will reject. Deriving it
    /// keeps the offset out of the hands of whoever writes the next control.
    pub fn lowest(self) -> f64 {
        if self.exclusive_min {
            self.min.next_up()
        } else {
            self.min
        }
    }

    /// The rule in words, for an error a user reads.
    pub fn describe(self) -> String {
        let low = if self.exclusive_min {
            format!("greater than {}", self.min)
        } else {
            format!("at least {}", self.min)
        };
        let range = format!("{low} and at most {}", self.max);
        match (self.excluded_magnitude, self.excluded) {
            (Some(threshold), _) => {
                format!("{range}, with magnitude greater than {threshold}")
            }
            (None, Some(excluded)) => format!("{range}, and not {excluded}"),
            (None, None) => range,
        }
    }

    /// Validate a value against the bound, naming the property in the failure.
    ///
    /// The failure states the value that was rejected as well as the rule.
    /// "Level ratio must be greater than 1 and at most 10" leaves a caller that
    /// sent 10.0000001, or sent a string that parsed to something else
    /// entirely, unable to tell which end it fell off — and a headless caller
    /// has no control to look at. Naming both closes that.
    pub fn check(
        self,
        property: PropertyId,
        subject: &str,
        value: f64,
    ) -> Result<f64, PropertyError> {
        if self.admits(value) {
            return Ok(value);
        }
        Err(PropertyError::InvalidValue {
            property,
            message: format!(
                "{subject} {value} is out of range: it must be {}",
                self.describe()
            ),
        })
    }
}

/// A reversible transformation between a stored domain value and the number a
/// user edits.
///
/// The display-space unit is part of the transformation and must always be
/// obtained through [`Self::unit`]. Keeping both facts in this one value makes
/// a control that displays one numeric space while independently labelling
/// another unrepresentable.
///
/// The unit each variant carries is the unit of the *domain* quantity: a
/// logarithmic control still measures λ, it just edits its exponent. The
/// `log₁₀` the user reads therefore belongs to [`Self::caption`], which derives
/// it, rather than to each definition's string — a per-site prefix is a copy of
/// the transformation that can be forgotten, and was: the contour base level
/// edited an exponent under a bare "intensity" caption.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FloatDisplay {
    Linear(&'static str),
    Log10(&'static str),
    Degrees,
}

impl FloatDisplay {
    /// The unit of the stored domain value.
    pub const fn unit(self) -> &'static str {
        match self {
            Self::Linear(unit) | Self::Log10(unit) => unit,
            Self::Degrees => "°",
        }
    }

    /// What a control writes beside the number it shows. This is the only
    /// caption a human should read, because it is the only one derived from the
    /// same value that projects the number.
    pub fn caption(self) -> Cow<'static, str> {
        match self {
            Self::Log10("") => Cow::Borrowed("log₁₀"),
            Self::Log10(unit) => Cow::Owned(format!("log₁₀ {unit}")),
            other => Cow::Borrowed(other.unit()),
        }
    }

    pub fn to_display(self, value: f64) -> f64 {
        match self {
            Self::Linear(_) => value,
            Self::Log10(_) => value.log10(),
            Self::Degrees => value.to_degrees(),
        }
    }

    pub fn to_domain(self, value: f64) -> f64 {
        match self {
            Self::Linear(_) => value,
            Self::Log10(_) => 10.0_f64.powf(value),
            Self::Degrees => value.to_radians(),
        }
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Linear(_) => "linear",
            Self::Log10(_) => "log10",
            Self::Degrees => "degrees",
        }
    }
}

/// The static value schema. Bounds that depend on the target's current state are
/// reported by [`ResolvedProperty::schema`] instead, keeping the definition
/// context-free.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ValueSchema {
    Bool,
    Text,
    Int {
        min: i64,
        max: i64,
    },
    /// An integer whose established direct-manipulation scale is independent
    /// of the distance between admitted values.
    IntWithDrag {
        min: i64,
        max: i64,
        drag_step: f64,
    },
    SteppedInt {
        min: i64,
        max: i64,
        /// Distance between values admitted from `min`.
        step: i64,
        /// How far one notch of a direct-manipulation drag moves the value.
        drag_step: f64,
    },
    Float {
        bounds: FloatBounds,
        display: FloatDisplay,
        /// How far one notch of a direct-manipulation drag moves the value.
        ///
        /// A control that has to invent this can only derive it from the range,
        /// and a range is a statement about what is *admissible*, not about what
        /// is *usual*: line broadening is legal out to ±10 kHz and typically set
        /// between 0.3 and 5 Hz, so a range-derived notch moves it by a hundred
        /// hertz a pixel. The quantity's own scale is knowledge the definition
        /// has and the control does not, so the definition states it. `None`
        /// leaves the control to fall back on the range.
        drag_step: Option<f64>,
    },
    Enum {
        variants: &'static [EnumVariant],
    },
    Color,
}

impl ValueSchema {
    /// The declared numeric range, when this is a float schema. Both the control
    /// and the write path ask for it here rather than restating it.
    pub const fn float_bounds(&self) -> Option<FloatBounds> {
        match self {
            Self::Float { bounds, .. } => Some(*bounds),
            _ => None,
        }
    }
}

/// A property value in transit between the catalog and a control. It is never
/// stored: the authoritative value stays in the typed domain model.
#[derive(Clone, Debug, PartialEq)]
pub enum PropertyValue {
    Bool(bool),
    Text(String),
    Int(i64),
    Float(f64),
    /// One of the owning schema's static variant ids.
    Enum(&'static str),
    Color(Color),
}

impl PropertyValue {
    pub const fn kind(&self) -> &'static str {
        match self {
            Self::Bool(_) => "bool",
            Self::Text(_) => "text",
            Self::Int(_) => "int",
            Self::Float(_) => "float",
            Self::Enum(_) => "enum",
            Self::Color(_) => "color",
        }
    }

    pub const fn as_bool(&self) -> Option<bool> {
        match self {
            Self::Bool(value) => Some(*value),
            _ => None,
        }
    }

    pub fn as_text(&self) -> Option<&str> {
        match self {
            Self::Text(value) => Some(value),
            _ => None,
        }
    }

    pub const fn as_int(&self) -> Option<i64> {
        match self {
            Self::Int(value) => Some(*value),
            _ => None,
        }
    }

    pub const fn as_float(&self) -> Option<f64> {
        match self {
            Self::Float(value) => Some(*value),
            _ => None,
        }
    }

    pub const fn as_enum(&self) -> Option<&'static str> {
        match self {
            Self::Enum(value) => Some(*value),
            _ => None,
        }
    }

    pub const fn as_color(&self) -> Option<Color> {
        match self {
            Self::Color(value) => Some(*value),
            _ => None,
        }
    }
}

/// How the default of a property is obtained. Defaults are *derived*, never
/// stored next to the current value.
#[derive(Clone, Debug, PartialEq)]
pub enum DefaultPolicy {
    /// Re-run the same factory that materializes a new encoding, in the
    /// target's current context, and read this property out of the result.
    EncodingFactory,
    /// Re-run the typed processing recipe factory appropriate to the addressed
    /// step. Unlike an encoding factory, this is owned by a dataset pipeline
    /// and can vary between a 1D and a 2D default recipe.
    ProcessingFactory,
    /// The default is whatever the target's derived artifact currently shows, so it
    /// varies with the target and is recomputed on every read.
    Derived,
    /// A literal that does not depend on the target.
    Fixed(PropertyValue),
    /// Values with no meaningful reset target, normally read-only provenance.
    None,
}

/// How many copies of one setting a single target holds.
///
/// Most settings have exactly one copy per target, so a single-target read can
/// only ever be uniform. A few describe a shape the target mirrors — a contour
/// ladder keeps a positive and a negative half that share base, count and
/// ratio — and those have one copy per half, which is why even a single-target
/// read is an aggregate.
///
/// The distinction is declared here rather than inferred by a control, so a
/// frontend can say *which* sources disagree without knowing what the target's
/// domain model looks like.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ValueCopies {
    /// Exactly one copy per target.
    PerTarget,
    /// One copy per mirrored half of a symmetric pair the target holds.
    PerMirroredHalf,
}

/// One step of a direct-manipulation gesture along a property's own scale.
///
/// The gesture names a direction, never a value: what one step *is* belongs to
/// the property, so a canvas key and a panel control cannot disagree about it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PropertyStep {
    Raise,
    Lower,
}

impl PropertyStep {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Raise => "raise",
            Self::Lower => "lower",
        }
    }
}

/// One typed edit operation shared by all catalog entry points.
///
/// The service owns selection-wide planning; a provider receives one target and
/// this operation, applies it to its typed working copy, or explains why that
/// target cannot accept it. Keeping the operation here prevents set/reset/step
/// from growing three structurally identical planners as new providers arrive.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum EditOp<'a> {
    Set(&'a PropertyValue),
    Reset,
    Step(PropertyStep),
}
