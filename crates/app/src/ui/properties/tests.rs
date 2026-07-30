use super::*;
use plotx_core::properties::{PropertyAccess, ValueSchema, catalog, step_enabled};

/// Rows a single panel section renders without being expanded. Beyond this the
/// section is no longer a panel but a settings dump, and the fix is to re-tier
/// properties rather than to raise the number.
const MAX_ESSENTIAL_PER_SECTION: usize = 6;

#[derive(Debug)]
struct EssentialVisibilityProfile {
    discriminator: &'static str,
    visible: Vec<PropertyId>,
}

fn essential_visibility_profiles(section: &str) -> Vec<EssentialVisibilityProfile> {
    let declared: Vec<PropertyId> = essential_in(section).iter().map(|entry| entry.id).collect();
    if section != panel::CHART_SECTION {
        if section == panel::BASELINE_SECTION {
            return [
                baseline::OFFSET,
                baseline::POLYNOMIAL,
                baseline::ASYMMETRIC_LEAST_SQUARES,
            ]
            .into_iter()
            .map(|method| EssentialVisibilityProfile {
                discriminator: method,
                visible: declared
                    .iter()
                    .copied()
                    .filter(|property| baseline::property_applies_to_method(*property, method))
                    .collect(),
            })
            .collect();
        }
        return vec![EssentialVisibilityProfile {
            discriminator: "all declared Essential rows applicable",
            visible: declared,
        }];
    }

    let chart_type = definition(object::CHART_TYPE_ID).expect("Chart Type is registered");
    let ValueSchema::Enum { variants } = &chart_type.value_schema else {
        panic!("Chart Type must remain an enum");
    };
    variants
        .iter()
        .map(|variant| EssentialVisibilityProfile {
            discriminator: variant.id,
            visible: declared
                .iter()
                .copied()
                .filter(|property| object::chart_property_applies_to_type(*property, variant.id))
                .collect(),
        })
        .collect()
}

fn maximum_visible_essential(section: &str) -> EssentialVisibilityProfile {
    essential_visibility_profiles(section)
        .into_iter()
        .max_by_key(|profile| profile.visible.len())
        .unwrap_or(EssentialVisibilityProfile {
            discriminator: "empty section",
            visible: Vec::new(),
        })
}

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

/// §8.7: the panel budget. Exceeding it means the section has stopped being a
/// panel, and the remedy is to move rows to `Advanced`, not to raise the limit.
#[test]
fn no_panel_section_exceeds_its_essential_budget() {
    // Every route, not a hand-picked one: a section whose panel is missing here
    // has no build-time budget at all, which is how the processing rows grew
    // theirs unchecked.
    for panel in [
        PanelRoute::SecondarySidebar,
        PanelRoute::Processing,
        PanelRoute::CanvasSettings,
        PanelRoute::Preferences,
    ] {
        for section in panel.sections() {
            let profile = maximum_visible_essential(section);
            assert!(
                profile.visible.len() <= MAX_ESSENTIAL_PER_SECTION,
                "section '{section}' of the {} renders {} Essential properties \
                 together for '{}', \
                 over the budget of {MAX_ESSENTIAL_PER_SECTION}. Re-tier some of \
                 {:?} to Advanced or Expert instead of raising the budget.",
                panel.title(),
                profile.visible.len(),
                profile.discriminator,
                profile
                    .visible
                    .iter()
                    .map(|property| property.as_str())
                    .collect::<Vec<_>>()
            );
        }
    }
}

#[test]
fn migrated_object_sections_keep_their_existing_density() {
    for (section, declared, visible, combination) in [
        (
            panel::STACK_SECTION,
            5,
            5,
            "an offset line stack, including series visibility",
        ),
        (
            panel::CHART_SECTION,
            7,
            4,
            "table_surface: type, colormap, azimuth, elevation",
        ),
        (panel::TEXT_SECTION, 5, 5, "a text object"),
        (panel::SHAPE_SECTION, 5, 5, "a filled shape object"),
        (panel::PANEL_SECTION, 2, 2, "a plot with a panel label"),
        (panel::OBJECT_SECTION, 1, 1, "any selected object"),
    ] {
        assert_eq!(essential_in(section).len(), declared, "{section}");
        let profile = maximum_visible_essential(section);
        assert_eq!(
            profile.visible.len(),
            visible,
            "section '{section}' must retain its calibrated density from {combination}"
        );
        if section == panel::CHART_SECTION {
            assert_eq!(profile.discriminator, "table_surface");
        }
    }
}

#[test]
fn migrated_processing_controls_keep_their_pre_migration_visibility() {
    for property in [
        baseline::POLYNOMIAL_ORDER,
        baseline::SMOOTHNESS,
        baseline::ASYMMETRY,
        baseline::ITERATIONS,
        smooth::POLYNOMIAL_ORDER,
        bin::METHOD,
        phase::PIVOT,
    ] {
        assert_eq!(
            definition(property)
                .expect("the processing property is registered")
                .tier,
            Tier::Essential,
            "{property} must remain directly visible"
        );
    }
    let baseline_profile = maximum_visible_essential(panel::BASELINE_SECTION);
    assert_eq!(
        baseline_profile.visible.len(),
        4,
        "AsLS shows method, smoothness, asymmetry, and iterations together"
    );
    assert_eq!(
        baseline_profile.discriminator,
        baseline::ASYMMETRIC_LEAST_SQUARES
    );
}

#[test]
fn migrated_canvas_and_typography_controls_keep_their_visibility_and_section_density() {
    for (section, expected, combination) in [
        (
            panel::CANVAS_MARGINS_SECTION,
            5,
            "a canvas with all four margins and its gutter",
        ),
        (
            panel::CANVAS_GRID_SECTION,
            4,
            "a canvas with rows, columns, grid visibility, and spacing mode",
        ),
        (
            panel::CANVAS_SIZE_SECTION,
            3,
            "a canvas with width, height, and automatic height",
        ),
        (
            panel::CANVAS_CAPTION_SECTION,
            2,
            "a canvas with caption and panel-label controls",
        ),
        (
            panel::TYPOGRAPHY_SECTION,
            3,
            "a document with all typography controls",
        ),
    ] {
        assert_eq!(
            maximum_visible_essential(section).visible.len(),
            expected,
            "section '{section}' must reflect the controls visible for {combination}"
        );
    }
}

#[test]
fn migrated_preferences_keep_their_real_section_density() {
    for (section, expected, combination) in [
        (
            SettingsCategory::General.section_id(),
            4,
            "all catalog-backed General preferences",
        ),
        (
            SettingsCategory::Appearance.section_id(),
            3,
            "theme, GPU, and the accent override row",
        ),
        (
            SettingsCategory::Processing.section_id(),
            1,
            "the scale-content processing preference",
        ),
        (
            SettingsCategory::Export.section_id(),
            4,
            "all catalog-backed Export preferences",
        ),
        (
            panel::PREFERENCES_UPDATES_SECTION,
            2,
            "automatic checks and update channel",
        ),
        (
            SettingsCategory::Recent.section_id(),
            0,
            "no catalog-backed Essential rows",
        ),
    ] {
        assert_eq!(
            maximum_visible_essential(section).visible.len(),
            expected,
            "section '{section}' must retain the controls visible for {combination}"
        );
    }
}

#[test]
fn equal_scale_is_directly_visible_and_title_visibility_is_advanced() {
    let essential: Vec<PropertyId> = essential_in(panel::AXIS_SECTION)
        .iter()
        .map(|entry| entry.id)
        .collect();
    assert_eq!(
        essential,
        [
            axis::EQUAL_F1_F2_SCALE,
            axis::X_LABEL,
            axis::Y_LABEL,
            axis::X_SHOW_TICK_LABELS,
            axis::Y_SHOW_TICK_LABELS,
        ]
    );
    assert_eq!(
        presentation(axis::X_SHOW_LABEL)
            .expect("x-title visibility presentation")
            .tier(),
        Some(Tier::Advanced)
    );
    assert_eq!(
        presentation(axis::Y_SHOW_LABEL)
            .expect("y-title visibility presentation")
            .tier(),
        Some(Tier::Advanced)
    );
}

#[test]
fn only_physical_canvas_lengths_follow_the_users_canvas_unit() {
    let marked: Vec<PropertyId> = PRESENTATIONS
        .iter()
        .filter(|entry| entry.uses_canvas_length_unit)
        .map(|entry| entry.id)
        .collect();
    assert_eq!(
        marked,
        [
            canvas::MARGIN_TOP_MM,
            canvas::MARGIN_RIGHT_MM,
            canvas::MARGIN_BOTTOM_MM,
            canvas::MARGIN_LEFT_MM,
            canvas::GUTTER_MM,
            canvas::WIDTH_MM,
            canvas::HEIGHT_MM,
        ]
    );
}

/// The two contour controls that directly decide what dense data looks like
/// stay visible; the ladder shape and colours remain Advanced.
#[test]
fn contour_level_and_line_width_are_essential() {
    let essential: Vec<&str> = essential_in(panel::CONTOUR_SECTION)
        .iter()
        .map(|entry| entry.id.as_str())
        .collect();
    assert_eq!(
        essential,
        [
            contour::BASE_MAGNITUDE.as_str(),
            contour::LINE_WIDTH.as_str()
        ]
    );
}

/// The row checkbox is compact chrome, not a second presentation channel. Its
/// renderer selects from this same Essential set before drawing inline, so the
/// budget and the visible row cannot drift.
#[test]
fn the_inline_step_toggle_is_the_processing_step_sections_essential_set() {
    let essential: Vec<PropertyId> = essential_in(panel::PROCESSING_STEP_SECTION)
        .iter()
        .map(|entry| entry.id)
        .collect();
    assert_eq!(essential, [step_enabled::ENABLED]);
}
