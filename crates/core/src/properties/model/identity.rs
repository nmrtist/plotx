//! Property identity, ownership scope, access, and panel tier.

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
