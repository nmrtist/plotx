//! The language-neutral half of the property catalog.
//!
//! A [`PropertyDefinition`] describes *what a property is* — its identity,
//! schema, ownership scope, applicability and default policy. It deliberately
//! carries no target, no current value and no stored "is default" flag: the
//! current value and the default are derived from the document on demand, so
//! there is never a second copy of a value that already lives in a typed domain
//! model. There is, for the same reason, no generic value store here; writes are
//! compiled into the existing typed `Action`s.

use crate::automation::{ComponentRef, TargetRef};
use plotx_figure::Color;
use std::fmt;

/// Stable, language-neutral identity of one catalog entry. Definitions are
/// static, so the identity is a `&'static str`: it cannot be minted at runtime
/// and cannot drift between releases without a visible source change.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PropertyId(pub &'static str);

impl PropertyId {
    pub const fn as_str(self) -> &'static str {
        self.0
    }

    /// The dotted segments of the id, used as search tokens so a headless
    /// caller can find `series.contour.count` by typing "contour count".
    pub fn tokens(self) -> impl Iterator<Item = &'static str> {
        self.0.split('.')
    }
}

impl fmt::Display for PropertyId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.0)
    }
}

/// Who owns the value and how many instances of it exist. Ownership decides the
/// scope, never "does editing it trigger a recomputation": a contour threshold
/// is owned by one series in one plot even though changing it rebuilds geometry.
///
/// There is no `Session` scope on purpose — the current slice, panel expansion
/// and board zoom are navigation state and stay out of the catalog (§8.4).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScopeKind {
    App,
    Document,
    Canvas,
    Dataset,
    Object,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PropertyAccess {
    ReadOnly,
    ReadWrite,
}

/// Panel budget tier. This is the single definition of a property's tier;
/// the presentation layer reads it rather than storing its own copy, so the two
/// cannot drift apart.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Tier {
    Essential,
    Advanced,
    Expert,
}

impl Tier {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Essential => "essential",
            Self::Advanced => "advanced",
            Self::Expert => "expert",
        }
    }
}

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
}

impl FloatBounds {
    /// `min ..= max`.
    pub const fn inclusive(min: f64, max: f64) -> Self {
        Self {
            min,
            max,
            exclusive_min: false,
        }
    }

    /// Strictly above `min`, up to and including `max`.
    pub const fn above(min: f64, max: f64) -> Self {
        Self {
            min,
            max,
            exclusive_min: true,
        }
    }

    pub fn admits(self, value: f64) -> bool {
        value.is_finite()
            && value <= self.max
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
        format!("{low} and at most {}", self.max)
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

/// The static value schema. Bounds that depend on the target's current state are
/// reported by [`ResolvedProperty::schema`] instead, keeping the definition
/// context-free.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ValueSchema {
    Bool,
    Int { min: i64, max: i64 },
    Float { bounds: FloatBounds, log: bool },
    Enum { variants: &'static [EnumVariant] },
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
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PropertyValue {
    Bool(bool),
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
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum DefaultPolicy {
    /// Re-run the same factory that materializes a new encoding, in the
    /// target's current context, and read this property out of the result.
    EncodingFactory,
    /// A literal that does not depend on the target.
    Fixed(PropertyValue),
    /// Read-only values have no default to reset to.
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

/// Which owner-local component the address must name. Field and column
/// properties are addressed through their own child `ResourceRef` with no
/// component at all, so they are absent here (§3.1).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ComponentKind {
    None,
    Series,
    ProcessingStep,
}

impl ComponentKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Series => "series",
            Self::ProcessingStep => "processing_step",
        }
    }

    pub fn of(component: Option<&ComponentRef>) -> Self {
        match component {
            None => Self::None,
            Some(ComponentRef::Series(_)) => Self::Series,
            Some(ComponentRef::ProcessingStep(_)) => Self::ProcessingStep,
        }
    }
}

/// Which concrete visual encoding a property belongs to. This is a fact about
/// the rendering model, not about a data domain: any field that can be drawn as
/// a contour exposes the same contour properties, whatever it measures.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EncodingKind {
    Line,
    Contour,
    Heatmap,
    Image,
}

impl EncodingKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Line => "line",
            Self::Contour => "contour",
            Self::Heatmap => "heatmap",
            Self::Image => "image",
        }
    }

    pub const fn of(encoding: &plotx_figure::SeriesEncoding) -> Self {
        match encoding {
            plotx_figure::SeriesEncoding::Line(_) => Self::Line,
            plotx_figure::SeriesEncoding::Contour(_) => Self::Contour,
            plotx_figure::SeriesEncoding::Heatmap(_) => Self::Heatmap,
            plotx_figure::SeriesEncoding::Image(_) => Self::Image,
        }
    }
}

/// When a property applies to a target. Everything here is expressed as a
/// component shape plus rendering capabilities; no branch of the catalog may
/// name a data domain.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Applicability {
    pub component: ComponentKind,
    pub encoding: Option<EncodingKind>,
    pub required_capabilities: &'static [&'static str],
}

impl Applicability {
    pub const fn component(component: ComponentKind) -> Self {
        Self {
            component,
            encoding: None,
            required_capabilities: &[],
        }
    }

    pub const fn encoding(component: ComponentKind, encoding: EncodingKind) -> Self {
        Self {
            component,
            encoding: Some(encoding),
            required_capabilities: &[],
        }
    }

    pub const fn requiring(mut self, capabilities: &'static [&'static str]) -> Self {
        self.required_capabilities = capabilities;
        self
    }
}

/// The static, language-neutral description of one property.
#[derive(Clone, Copy, Debug)]
pub struct PropertyDefinition {
    pub id: PropertyId,
    pub scope_kind: ScopeKind,
    pub value_schema: ValueSchema,
    pub access: PropertyAccess,
    pub applicability: Applicability,
    pub default_policy: DefaultPolicy,
    pub tier: Tier,
    /// How many copies of this setting one target holds, so a control can word
    /// a disagreement precisely instead of listing every way one could arise.
    pub copies: ValueCopies,
    pub canonical_label: &'static str,
    /// Stable English search terms. The presentation layer adds localized ones;
    /// it may never introduce an entry that has no definition here.
    pub canonical_aliases: &'static [&'static str],
}

/// Where a property lives: a target (resource plus at most one owner-local
/// component) and the definition being addressed.
#[derive(Clone, Debug, PartialEq)]
pub struct PropertyAddress {
    pub target: TargetRef,
    pub definition: PropertyId,
}

impl PropertyAddress {
    pub fn new(target: TargetRef, definition: PropertyId) -> Self {
        Self { target, definition }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Availability {
    Editable,
    ReadOnly,
}

/// The schema narrowed to one concrete target: the enum choices its field's
/// capabilities permit, and the bounds and unit that its current state implies.
#[derive(Clone, Debug, PartialEq)]
pub enum ResolvedSchema {
    Bool,
    Int {
        min: i64,
        max: i64,
    },
    Float {
        bounds: FloatBounds,
        log: bool,
        /// A short unit or multiplier caption ("× σ", "fraction"), empty when
        /// the number speaks for itself.
        unit: &'static str,
    },
    Enum {
        variants: Vec<&'static EnumVariant>,
    },
    Color,
}

/// A property read against one target. The value and its default are derived on
/// the spot; neither is stored anywhere in the catalog.
#[derive(Clone, Debug, PartialEq)]
pub struct ResolvedProperty {
    pub address: PropertyAddress,
    /// One target can already hold the setting more than once — a contour
    /// ladder keeps a positive and a negative half that share base, count and
    /// ratio — so even a single-target read is an aggregate. `Mixed` here means
    /// the target's own copies disagree, and the read refuses to pass one of
    /// them off as the whole setting.
    pub value: AggregateValue<PropertyValue>,
    /// `None` for read-only properties, which have nothing to reset to.
    pub default_value: Option<PropertyValue>,
    pub availability: Availability,
    pub schema: ResolvedSchema,
}

impl ResolvedProperty {
    /// Whether the target currently differs from what the default policy would
    /// produce for it right now. A target whose own copies disagree cannot be
    /// what the factory produced, which always writes one value to every copy.
    pub fn is_modified(&self) -> bool {
        match &self.value {
            AggregateValue::Uniform(value) => {
                self.default_value.is_some_and(|default| default != *value)
            }
            AggregateValue::Mixed => true,
            AggregateValue::Unavailable => false,
        }
    }
}

/// The read side of an aggregate: several sources of one setting, folded into
/// the single answer a control can show.
///
/// The sources are not necessarily targets. Both the copies one target holds of
/// a shared setting and the targets of a multi-selection aggregate through this
/// same type, so "the two halves of this ladder disagree" and "these two series
/// disagree" are one fact with one representation rather than two parallel ones.
#[derive(Clone, Debug, PartialEq)]
pub enum AggregateValue<T> {
    Uniform(T),
    Mixed,
    Unavailable,
}

impl<T> AggregateValue<T> {
    pub const fn uniform(&self) -> Option<&T> {
        match self {
            Self::Uniform(value) => Some(value),
            Self::Mixed | Self::Unavailable => None,
        }
    }
}

impl<T: PartialEq> AggregateValue<T> {
    /// Fold one more source in.
    ///
    /// `Unavailable` is the empty read and therefore the identity, so folding
    /// over no sources at all yields it. Once any source is `Mixed`, or two
    /// sources carry different values, the result stays `Mixed`: disagreement
    /// inside a source and disagreement between sources compose instead of one
    /// hiding the other.
    #[must_use]
    pub fn merge(self, other: Self) -> Self {
        match (self, other) {
            (Self::Unavailable, other) => other,
            (current, Self::Unavailable) => current,
            (Self::Uniform(current), Self::Uniform(next)) if current == next => {
                Self::Uniform(current)
            }
            _ => Self::Mixed,
        }
    }
}

/// One property read across a selection. Targets the property does not apply to
/// are reported with a reason rather than silently dropped.
#[derive(Clone, Debug, PartialEq)]
pub struct ResolvedPropertySet {
    pub applicable_targets: Vec<PropertyAddress>,
    pub skipped_targets: Vec<(TargetRef, String)>,
    pub value: AggregateValue<PropertyValue>,
}

/// A validated, not-yet-executed write. Every applicable target is already
/// folded into one atomic action, so a commit either lands everywhere or
/// nowhere.
#[derive(Clone)]
pub struct PropertyCommit {
    pub action: crate::actions::Action,
    pub applied: Vec<PropertyAddress>,
    pub skipped: Vec<(TargetRef, String)>,
}

impl fmt::Debug for PropertyCommit {
    /// `Action` is deliberately not `Debug` — it carries whole document
    /// snapshots — so a commit reports what it would do, not the payload.
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PropertyCommit")
            .field("applied", &self.applied)
            .field("skipped", &self.skipped)
            .finish_non_exhaustive()
    }
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum PropertyError {
    #[error("unknown property '{0}'")]
    UnknownProperty(String),
    #[error("property {property} addresses a {expected} component, not {actual}")]
    ComponentKind {
        property: PropertyId,
        expected: &'static str,
        actual: &'static str,
    },
    #[error("no such target: {0}")]
    UnknownTarget(String),
    #[error("{0}")]
    NotApplicable(String),
    #[error("property {0} is read-only")]
    ReadOnly(PropertyId),
    #[error("invalid value for {property}: {message}")]
    InvalidValue {
        property: PropertyId,
        message: String,
    },
}
