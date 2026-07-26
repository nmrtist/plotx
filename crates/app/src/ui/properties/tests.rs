use super::*;
use plotx_core::properties::{PropertyAccess, catalog};

/// Rows a single panel section renders without being expanded. Beyond this the
/// section is no longer a panel but a settings dump, and the fix is to re-tier
/// properties rather than to raise the number.
const MAX_ESSENTIAL_PER_SECTION: usize = 6;

/// §8.1: every user-visible property must have a presentation. A definition
/// without one is addressable by automation yet invisible and unsearchable in
/// the interface — exactly the asymmetry the catalog exists to remove.
#[test]
fn every_user_visible_property_has_a_presentation() {
    let missing: Vec<&str> = catalog()
        .iter()
        .filter(|definition| definition.access != PropertyAccess::ReadOnly)
        .filter(|definition| presentation(definition.id).is_none())
        .map(|definition| definition.id.as_str())
        .collect();
    assert!(
        missing.is_empty(),
        "these properties have no presentation entry: {missing:?}"
    );
}

/// §8.1: the reverse direction. The frontend may add localized search terms but
/// may never invent an entry with no semantic definition behind it.
#[test]
fn every_presentation_points_at_a_valid_definition() {
    assert!(
        search::orphan_presentations().is_empty(),
        "these presentations name no definition: {:?}",
        search::orphan_presentations()
    );
    for entry in PRESENTATIONS {
        let definition = entry.definition().expect("checked above");
        assert_eq!(definition.id, entry.id);
        assert_eq!(
            entry.tier(),
            Some(definition.tier),
            "{} must read its tier from the definition",
            entry.id
        );
    }
}

/// §8.1: stable ids are unique on the presentation side too, so navigation and
/// search can never land on two different rows for one id.
#[test]
fn presentation_ids_are_unique() {
    let mut ids: Vec<&str> = PRESENTATIONS
        .iter()
        .map(|entry| entry.id.as_str())
        .collect();
    let total = ids.len();
    ids.sort_unstable();
    ids.dedup();
    assert_eq!(ids.len(), total, "presentation ids must be unique");
}

/// §8.1: a home route must be resolvable — its panel has to declare the section
/// it names, otherwise a search hit opens nothing.
#[test]
fn every_home_route_resolves_to_a_declared_section() {
    for entry in PRESENTATIONS {
        let route = entry.home_route;
        assert!(
            route.panel.sections().contains(&route.section),
            "{} routes to '{}', which {} does not render",
            entry.id,
            route.section,
            route.panel.title()
        );
    }
}

/// §8.1: aliases must actually reach the unified search index. A canonical
/// alias nobody indexes is a comment, not a search term.
#[test]
fn aliases_are_indexed_by_the_unified_search() {
    let hits = property_hits();
    assert_eq!(hits.len(), PRESENTATIONS.len());
    for definition in catalog() {
        let Some(entry) = presentation(definition.id) else {
            continue;
        };
        let hit = hits
            .iter()
            .find(|hit| hit.id == definition.id)
            .expect("every presented property is indexed");
        for alias in definition.canonical_aliases {
            assert!(
                hit.terms.contains(&alias.to_lowercase()),
                "canonical alias '{alias}' of {} is not indexed",
                definition.id
            );
        }
        for alias in entry.localized_aliases {
            assert!(
                hit.terms.contains(&alias.get().to_lowercase()),
                "localized alias '{}' of {} is not indexed",
                alias.get(),
                definition.id
            );
        }
        assert!(
            hit.terms
                .contains(&entry.localized_label.get().to_lowercase())
        );
        // The dotted id is searchable term by term, so "contour count" finds
        // `series.contour.count` without the user knowing the id.
        for token in definition.id.tokens() {
            assert!(hit.terms.contains(&token.to_lowercase()));
        }
    }
}

/// §8.6: the panel budget. Exceeding it means the section has stopped being a
/// panel, and the remedy is to move rows to `Advanced`, not to raise the limit.
#[test]
fn no_panel_section_exceeds_its_essential_budget() {
    // Every route, not a hand-picked one: a section whose panel is missing here
    // has no build-time budget at all, which is how the processing rows grew
    // theirs unchecked.
    for panel in [PanelRoute::SecondarySidebar, PanelRoute::Processing] {
        for section in panel.sections() {
            let essential = essential_in(section);
            assert!(
                essential.len() <= MAX_ESSENTIAL_PER_SECTION,
                "section '{section}' of the {} renders {} Essential properties, \
                 over the budget of {MAX_ESSENTIAL_PER_SECTION}. Re-tier some of \
                 {:?} to Advanced or Expert instead of raising the budget.",
                panel.title(),
                essential.len(),
                essential
                    .iter()
                    .map(|entry| entry.id.as_str())
                    .collect::<Vec<_>>()
            );
        }
    }
}

/// §12: only the lowest level is Essential on a contour; the ladder shape and
/// the negative-half colour are for users who went looking for them.
#[test]
fn only_the_lowest_contour_level_is_essential() {
    let essential: Vec<&str> = essential_in(panel::CONTOUR_SECTION)
        .iter()
        .map(|entry| entry.id.as_str())
        .collect();
    assert_eq!(essential, [contour::BASE_MAGNITUDE.as_str()]);
}
