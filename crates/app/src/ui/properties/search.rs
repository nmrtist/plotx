//! The property half of the unified search index.
//!
//! Registering a definition and its presentation is what makes a property
//! findable; nothing here is written per property. The matched terms are the
//! union the design calls for: the id's own tokens, the canonical label and
//! aliases (stable English, so a headless or scripted caller finds the same
//! entry), and the active locale's label and aliases.

use super::{PropertyPresentation, presentation};
use plotx_core::properties::{PropertyDefinition, PropertyId, catalog};

/// One searchable property entry.
#[derive(Clone, Debug)]
pub(crate) struct PropertyHit {
    pub id: PropertyId,
    /// What the palette row shows.
    pub label: String,
    /// Where activating it navigates to.
    pub home: &'static str,
    /// Lower-cased terms a query is matched against.
    pub terms: Vec<String>,
}

/// Every user-visible property, in catalog order.
pub(crate) fn property_hits() -> Vec<PropertyHit> {
    let pairs: Vec<(&PropertyDefinition, &PropertyPresentation)> = catalog()
        .iter()
        .filter_map(|definition| Some((*definition, presentation(definition.id)?)))
        .collect();
    hits_from(&pairs)
}

/// Build the index from an explicit table.
///
/// Nothing here is written per property: a definition and its presentation are
/// enough to produce a searchable entry, which is what makes registration alone
/// sufficient. Taking the table as an argument is what lets a test prove that
/// with an entry that exists nowhere else.
pub(crate) fn hits_from(
    pairs: &[(&PropertyDefinition, &PropertyPresentation)],
) -> Vec<PropertyHit> {
    pairs
        .iter()
        .map(|(definition, presentation)| {
            let mut terms: Vec<String> = Vec::new();
            terms.push(definition.id.as_str().to_lowercase());
            terms.extend(definition.id.tokens().map(str::to_lowercase));
            terms.push(definition.canonical_label.to_lowercase());
            terms.extend(
                definition
                    .canonical_aliases
                    .iter()
                    .map(|alias| alias.to_lowercase()),
            );
            terms.push(presentation.localized_label.get().to_lowercase());
            terms.extend(
                presentation
                    .localized_aliases
                    .iter()
                    .map(|alias| alias.get().to_lowercase()),
            );
            terms.sort_unstable();
            terms.dedup();
            PropertyHit {
                id: definition.id,
                label: presentation.localized_label.get().to_owned(),
                home: presentation.home_route.panel.title(),
                terms,
            }
        })
        .collect()
}

/// Presentations with no definition behind them. The index above simply skips
/// such an entry, so a mistake degrades rather than panics at runtime; this
/// exists so the consistency test can fail the build instead.
#[cfg(test)]
pub(crate) fn orphan_presentations() -> Vec<PropertyId> {
    super::PRESENTATIONS
        .iter()
        .filter(|presentation| presentation.definition().is_none())
        .map(|presentation| presentation.id)
        .collect()
}
