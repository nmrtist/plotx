//! Property addressing, applicability, resolution, and commit results.

use super::{
    DefaultPolicy, EnumVariant, FloatBounds, FloatDisplay, PropertyAccess, PropertyId,
    PropertyValue, ScopeKind, Tier, ValueCopies, ValueSchema,
};
use crate::automation::{ComponentRef, TargetRef};
use std::fmt;

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
#[derive(Clone, Debug)]
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
    Disabled(&'static str),
    ReadOnly,
}

/// The schema narrowed to one concrete target: the enum choices its field's
/// capabilities permit, and the bounds and unit that its current state implies.
#[derive(Clone, Debug, PartialEq)]
pub enum ResolvedSchema {
    Bool,
    Text,
    Int {
        min: i64,
        max: i64,
        unit: &'static str,
    },
    IntWithDrag {
        min: i64,
        max: i64,
        drag_step: f64,
        unit: &'static str,
    },
    SteppedInt {
        min: i64,
        max: i64,
        step: i64,
        drag_step: f64,
        unit: &'static str,
    },
    Float {
        bounds: FloatBounds,
        display: FloatDisplay,
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
    /// Provider-owned storage can distinguish an explicit override from a
    /// derived value that happens to be equal. `None` uses value comparison.
    pub modified: Option<bool>,
    pub availability: Availability,
    pub schema: ResolvedSchema,
}

impl ResolvedProperty {
    /// Whether the row should expose its reset affordance. Most providers infer
    /// this from value comparison; override-backed providers can report storage
    /// presence explicitly when an override equals its current derived value.
    pub fn is_modified(&self) -> bool {
        if let Some(modified) = self.modified {
            return modified;
        }
        match &self.value {
            AggregateValue::Uniform(value) => self
                .default_value
                .as_ref()
                .is_some_and(|default| default != value),
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

/// Why one target of a selection-wide read or write did nothing.
///
/// The reason is typed because callers branch on it. "This target already holds
/// the value you asked for" is a success a caller should carry on from; "this
/// property does not apply to that target" means it addressed the wrong thing.
/// Leaving the two indistinguishable behind free text forces a caller to match
/// on prose that exists to be read by a person and is free to be reworded.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SkipReason {
    /// The target already holds the requested value, so nothing was written.
    AlreadyAtValue,
    /// The property does not apply to this target.
    NotApplicable,
    /// The address no longer names anything in the document.
    TargetMissing,
}

impl SkipReason {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::AlreadyAtValue => "already_at_value",
            Self::NotApplicable => "not_applicable",
            Self::TargetMissing => "target_missing",
        }
    }

    /// The reason a failed read or edit amounts to.
    pub const fn of(error: &PropertyError) -> Self {
        match error {
            PropertyError::UnknownTarget(_) => Self::TargetMissing,
            _ => Self::NotApplicable,
        }
    }
}

/// One target a selection-wide operation passed over, carrying the reason in
/// both the form a caller branches on and the form a person reads.
#[derive(Clone, Debug, PartialEq)]
pub struct PropertySkip {
    pub target: TargetRef,
    pub reason: SkipReason,
    pub message: String,
}

impl PropertySkip {
    pub fn new(target: TargetRef, reason: SkipReason, message: String) -> Self {
        Self {
            target,
            reason,
            message,
        }
    }

    /// The skip a failed read or edit amounts to, keeping the error's own words.
    pub fn from_error(target: TargetRef, error: &PropertyError) -> Self {
        Self::new(target, SkipReason::of(error), error.to_string())
    }
}

/// One property read across a selection. Targets the property does not apply to
/// are reported with a reason rather than silently dropped.
#[derive(Clone, Debug, PartialEq)]
pub struct ResolvedPropertySet {
    pub applicable_targets: Vec<PropertyAddress>,
    pub skipped_targets: Vec<PropertySkip>,
    pub value: AggregateValue<PropertyValue>,
}

/// A validated, not-yet-executed write. Providers have already selected the
/// typed storage payload; planning guarantees that at most one arm is present.
#[derive(Clone)]
pub struct PropertyCommit {
    pub(crate) document_action: Option<crate::actions::Action>,
    pub(crate) canvas_direct: Vec<crate::properties::transaction::CanvasDirectEdit>,
    pub(crate) app_preferences: Option<crate::settings::Settings>,
    pub applied: Vec<PropertyAddress>,
    pub skipped: Vec<PropertySkip>,
}

impl PropertyCommit {
    pub(crate) fn has_document_action(&self) -> bool {
        self.document_action.is_some() || !self.canvas_direct.is_empty()
    }
}

impl fmt::Debug for PropertyCommit {
    /// `Action` is deliberately not `Debug` — it carries whole document
    /// snapshots — so a commit reports what it would do, not the payload.
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PropertyCommit")
            .field("document_action", &self.document_action.is_some())
            .field("canvas_direct", &self.canvas_direct.len())
            .field("app_preferences", &self.app_preferences.is_some())
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
    #[error("one property commit cannot cross multiple storages: {storages}")]
    MixedStorage { storages: String },
}
