//! The property catalog: one registration point for every addressable,
//! persistent setting, the counterpart of the existing command catalog.
//!
//! The catalog owns descriptions, addressing, reading, validation and the
//! compilation of an edit into a typed [`crate::actions::Action`]. It does not
//! own values: there is no `HashMap<PropertyId, PropertyValue>` anywhere in this
//! module, because every value already has exactly one home in a typed domain
//! model. Presentation — localized labels and panel routing — belongs to the
//! application crate and is keyed by the same [`PropertyId`].

pub mod contour;
mod model;
mod readout;
mod service;

pub use model::*;
pub use readout::{ContourAnchor, ContourBaseReadout};

use crate::state::FieldCapabilities;
use std::sync::LazyLock;

/// The single aggregation point. Each area exports its own slice of
/// definitions; adding one here is what makes it addressable, searchable and
/// resettable everywhere at once.
static CATALOG: LazyLock<Vec<&'static PropertyDefinition>> =
    LazyLock::new(|| contour::DEFINITIONS.iter().collect());

pub fn catalog() -> &'static [&'static PropertyDefinition] {
    &CATALOG
}

pub fn definition(id: PropertyId) -> Option<&'static PropertyDefinition> {
    catalog()
        .iter()
        .copied()
        .find(|definition| definition.id == id)
}

/// Look a definition up by its stable string id, the form an automation or
/// search caller carries.
pub fn definition_by_key(key: &str) -> Option<&'static PropertyDefinition> {
    catalog()
        .iter()
        .copied()
        .find(|definition| definition.id.as_str() == key)
}

/// The enum choices a field's capabilities actually permit. Capability decides
/// whether a choice can exist at all; the user's current value decides which one
/// is in force.
pub fn permitted_variants(
    schema: &ValueSchema,
    capabilities: &FieldCapabilities,
) -> Vec<&'static EnumVariant> {
    let ValueSchema::Enum { variants } = schema else {
        return Vec::new();
    };
    variants
        .iter()
        .filter(|variant| {
            variant
                .required_capabilities
                .iter()
                .all(|capability| capabilities.contains(capability))
                && !variant
                    .forbidden_capabilities
                    .iter()
                    .any(|capability| capabilities.contains(capability))
        })
        .collect()
}

#[cfg(test)]
#[path = "tests.rs"]
mod tests;

#[cfg(test)]
#[path = "ladder_tests.rs"]
mod ladder_tests;

#[cfg(test)]
#[path = "step_tests.rs"]
mod step_tests;

#[cfg(test)]
#[path = "scope_tests.rs"]
mod scope_tests;
