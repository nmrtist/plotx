//! The property catalog: one registration point for every addressable,
//! persistent setting, the counterpart of the existing command catalog.
//!
//! The catalog owns descriptions, addressing, reading, validation and the
//! compilation of an edit into a typed storage commit. It does not own values:
//! there is no `HashMap<PropertyId, PropertyValue>` anywhere in this module,
//! because every value already has exactly one home in a typed domain model.
//! Presentation — localized labels and panel routing — belongs to the
//! application crate and is keyed by the same [`PropertyId`].

pub mod apodization;
pub mod contour;
pub mod export_dpi;
pub mod ilt;
pub mod line;
mod model;
mod provider;
mod readout;
mod service;
mod target;
mod transaction;
pub mod typography;

pub use model::*;
pub use readout::{ContourAnchor, ContourBaseReadout, PropertyReadout};

pub(crate) use provider::{PropertyProvider, PropertyProviderGroup};
pub(crate) use transaction::PropertyTransaction;

use crate::state::FieldCapabilities;
use std::sync::LazyLock;

/// The single aggregation point. Each area exports its own slice of
/// definitions; adding one here is what makes it addressable, searchable and
/// resettable everywhere at once.
pub(crate) static GROUPS: &[PropertyProviderGroup] = &[
    PropertyProviderGroup {
        provider: &apodization::PROVIDER,
    },
    PropertyProviderGroup {
        provider: &contour::PROVIDER,
    },
    PropertyProviderGroup {
        provider: &export_dpi::PROVIDER,
    },
    PropertyProviderGroup {
        provider: &ilt::PROVIDER,
    },
    PropertyProviderGroup {
        provider: &line::PROVIDER,
    },
    PropertyProviderGroup {
        provider: &typography::PROVIDER,
    },
];

static CATALOG: LazyLock<Vec<&'static PropertyDefinition>> = LazyLock::new(|| {
    GROUPS
        .iter()
        .flat_map(|group| group.provider.definitions())
        .collect()
});

pub fn catalog() -> &'static [&'static PropertyDefinition] {
    &CATALOG
}

/// Whether a dataset holds any component this catalog can address.
///
/// The automation resource gate asks this instead of testing what kind of data
/// the dataset holds, so admission to the `properties.*` tools stays a
/// capability question (§1 principle 3).
pub fn has_addressable_components(dataset: &crate::state::Dataset) -> bool {
    target::dataset_has_property_components(dataset)
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

pub(crate) fn provider_for(id: PropertyId) -> Option<&'static dyn PropertyProvider> {
    GROUPS
        .iter()
        .find(|group| {
            group
                .provider
                .definitions()
                .iter()
                .any(|definition| definition.id == id)
        })
        .map(|group| group.provider)
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

/// The choice ids of a variant set, for an error a caller can act on.
///
/// Every rejection of an enumerated value names the alternatives, whether the
/// value failed to name a choice at all (the wire boundary) or named one this
/// field's capabilities withhold (the planner). One formatting keeps those two
/// messages from drifting into two different vocabularies for one list.
pub fn variant_list(variants: &[&'static EnumVariant]) -> String {
    if variants.is_empty() {
        return "no choices at all".to_owned();
    }
    variants
        .iter()
        .map(|variant| format!("'{}'", variant.id))
        .collect::<Vec<_>>()
        .join(", ")
}

// Visible crate-wide under `cfg(test)` only: the automation adapter's
// differential tests drive the very same page fixture through the JSON entry
// point, and building a second copy of it there would let the two entry points
// be compared against two different documents.
#[cfg(test)]
#[path = "tests.rs"]
pub(crate) mod tests;

#[cfg(test)]
#[path = "ladder_tests.rs"]
mod ladder_tests;

#[cfg(test)]
#[path = "step_tests.rs"]
mod step_tests;

#[cfg(test)]
#[path = "scope_tests.rs"]
mod scope_tests;

#[cfg(test)]
#[path = "apodization_tests.rs"]
mod apodization_tests;

#[cfg(test)]
#[path = "export_dpi_tests.rs"]
mod export_dpi_tests;

#[cfg(test)]
#[path = "ilt_tests.rs"]
pub(crate) mod ilt_tests;
