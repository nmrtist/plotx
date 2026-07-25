//! §8.5's central claim: **register once, and every applicable channel follows.**
//!
//! The four channels are not four registries. A property is declared exactly
//! once — a `PropertyDefinition` in the core catalog plus one
//! [`PropertyPresentation`] here — and search, the Ribbon group, the context
//! menu and the canvas gesture are all *derived* from that pair. These tests
//! pin that down by registering a property that exists nowhere else and
//! checking every channel picks it up with no further edit.
//!
//! The derivations are written against slices for exactly this reason: a test
//! can hand them a table with one extra entry, which is what "no second
//! registration" has to mean if it is to mean anything.

use crate::ui::commands::{self, CommandId};
use crate::ui::properties::{
    CONTOUR_HOME, GROUPS, LocalizedText, PRESENTATIONS, PropertyPresentation, discovery, panel,
    presentation, search,
};
use plotx_core::properties::{
    Applicability, ComponentKind, DefaultPolicy, EncodingKind, PropertyAccess, PropertyDefinition,
    PropertyId, PropertyStep, ScopeKind, Tier, ValueCopies, ValueSchema,
};
use plotx_core::state::PlotxApp;

/// A property registered nowhere but here. It shares the contour section's
/// home, which is the ordinary case: a new setting joins a group that already
/// exists.
const NEWCOMER: PropertyDefinition = PropertyDefinition {
    id: PropertyId("series.contour.newcomer"),
    scope_kind: ScopeKind::Object,
    value_schema: ValueSchema::Bool,
    access: PropertyAccess::ReadWrite,
    applicability: Applicability::encoding(ComponentKind::Series, EncodingKind::Contour),
    default_policy: DefaultPolicy::None,
    tier: Tier::Advanced,
    copies: ValueCopies::PerTarget,
    canonical_label: "Newcomer setting",
    canonical_aliases: &["freshly registered"],
};

const NEWCOMER_PRESENTATION: PropertyPresentation = PropertyPresentation {
    id: NEWCOMER.id,
    localized_label: LocalizedText("Newcomer"),
    localized_aliases: &[LocalizedText("brand new")],
    home_route: CONTOUR_HOME,
    canvas_step: true,
};

fn with_newcomer() -> Vec<PropertyPresentation> {
    let mut table = vec![NEWCOMER_PRESENTATION];
    table.extend_from_slice(PRESENTATIONS);
    table
}

/// Channel 1: a single registration is enough to be searchable, by its id
/// tokens, its canonical terms and its localized ones.
#[test]
fn one_registration_is_searchable() {
    let hits = search::hits_from(&[(&NEWCOMER, &NEWCOMER_PRESENTATION)]);
    let hit = hits.first().expect("a registered property is indexed");
    assert_eq!(hit.id, NEWCOMER.id);
    for term in [
        "newcomer",
        "series",
        "contour",
        "freshly registered",
        "brand new",
    ] {
        assert!(
            hit.terms.contains(&term.to_owned()),
            "'{term}' is not a search term of {}",
            NEWCOMER.id
        );
    }
}

/// Channel 2 and 4: both address groups, and group membership is read off the
/// home route. A property that joins an existing group therefore appears in
/// that group's Ribbon button and context-menu entry without touching `GROUPS`.
#[test]
fn one_registration_joins_its_group_without_a_second_entry() {
    let table = with_newcomer();
    let members = discovery::members_of(panel::CONTOUR_SECTION, &table);
    assert!(
        members.iter().any(|entry| entry.id == NEWCOMER.id),
        "the newcomer must be a member of the group whose home it declared"
    );
    assert_eq!(
        members.len(),
        PRESENTATIONS.len() + 1,
        "membership is derived, so it grows with the table and nothing else"
    );
    // The group table itself is untouched: the newcomer contributed no entry.
    assert_eq!(GROUPS.len(), 1);
}

/// Channel 3: the gesture picks up whichever property declared itself
/// steppable. The declaration lives on the same entry as everything else.
#[test]
fn one_registration_claims_the_canvas_gesture() {
    let table = with_newcomer();
    assert_eq!(
        discovery::steppable_in(&table).map(|entry| entry.id),
        Some(NEWCOMER.id),
        "a property that declares the gesture drives it with no further wiring"
    );
}

/// Every declared group has a Ribbon button, a menu entry and a palette hit —
/// derived from `GROUPS`, so declaring a group is the whole registration.
#[test]
fn every_declared_group_has_a_ribbon_entry() {
    let app = PlotxApp::new_with_settings(plotx_core::settings::Settings::default());
    let catalog = commands::catalog(&app);
    for group in GROUPS {
        let descriptor = catalog
            .iter()
            .find(|command| command.id == CommandId::PropertyGroup(group.section))
            .unwrap_or_else(|| panic!("group '{}' has no command", group.section));
        let placement = commands::describe(&app, descriptor.id)
            .ribbon
            .unwrap_or_else(|| panic!("group '{}' has no Ribbon placement", group.section));
        assert_eq!(placement.tab, group.ribbon.tab);
        assert_eq!(placement.group, group.ribbon.group);
        assert!(descriptor.label.contains(group.label.get()));
    }
}

/// The Ribbon and the context menu are entry maps. Every group must therefore
/// name a property to land on, and that property must have a resolvable home —
/// a button that opens nothing is worse than no button.
#[test]
fn every_group_lands_on_a_property_with_a_home() {
    for group in GROUPS {
        let property = discovery::entry_property(group.section, PRESENTATIONS)
            .unwrap_or_else(|| panic!("group '{}' has no member to land on", group.section));
        let entry = presentation(property).expect("the landing property is presented");
        assert_eq!(entry.home_route.section, group.section);
        assert!(entry.home_route.panel.sections().contains(&group.section));
    }
}

/// A group with no section behind it could never be navigated to, and a
/// section with no group would silently lose channels 2 and 4. Both directions
/// are checked so the omission fails the build instead of the interface.
#[test]
fn groups_and_home_sections_correspond() {
    for group in GROUPS {
        assert!(
            !discovery::members_of(group.section, PRESENTATIONS).is_empty(),
            "group '{}' has no members",
            group.section
        );
    }
    for entry in PRESENTATIONS {
        assert!(
            discovery::group(entry.home_route.section).is_some(),
            "{} lives in section '{}', which declares no group, so it is \
             unreachable from the Ribbon and the context menu",
            entry.id,
            entry.home_route.section
        );
    }
}

/// The gesture drives one setting at a time. Two steppable properties would
/// make `+` mean different things depending on table order, which is precisely
/// the kind of hidden ambiguity a derived channel must not introduce.
#[test]
fn at_most_one_property_claims_the_canvas_gesture() {
    let claiming: Vec<&str> = PRESENTATIONS
        .iter()
        .filter(|entry| entry.canvas_step)
        .map(|entry| entry.id.as_str())
        .collect();
    assert!(
        claiming.len() <= 1,
        "these properties all claim the `+`/`-` gesture: {claiming:?}"
    );
}

/// The gesture is registered as a command, so it is searchable, appears in
/// menus, and is gated with a reason like every other action.
#[test]
fn the_gesture_is_a_catalog_command_with_a_reason_when_it_cannot_run() {
    let app = PlotxApp::new_with_settings(plotx_core::settings::Settings::default());
    for step in [PropertyStep::Raise, PropertyStep::Lower] {
        let descriptor = commands::describe(&app, CommandId::StepProperty(step));
        assert!(!descriptor.enabled, "an empty document has nothing to step");
        let reason = descriptor
            .disabled_reason
            .expect("a gated command explains");
        assert!(
            reason.starts_with("Select"),
            "reason must name the fix: {reason}"
        );
        assert!(!descriptor.label.is_empty());
    }
}
