use super::*;
use plotx_core::properties::contour;

fn indices(items: &[PaletteItem], query: &str) -> Vec<PaletteAction> {
    filter(items, query)
        .into_iter()
        .map(|index| items[index].action)
        .collect()
}

#[test]
fn filter_matches_all_terms() {
    let app = PlotxApp::new_with_settings(plotx_core::settings::Settings::default());
    let items = search_set(&app);
    assert!(
        indices(&items, "toggle snapping")
            .contains(&PaletteAction::Command(commands::CommandId::ToggleSnap))
    );
}

/// The gap this stage closes: before, the search set was verbs only, so a
/// setting the user could see on screen could not be found by name.
#[test]
fn a_setting_is_findable_by_its_label() {
    let app = PlotxApp::new_with_settings(plotx_core::settings::Settings::default());
    let items = search_set(&app);
    assert!(
        indices(&items, "lowest level").contains(&PaletteAction::Property(contour::BASE_MAGNITUDE)),
        "the essential contour control must be searchable by its label"
    );
}

/// Every indexed term reaches the filter, not just the label: the canonical
/// alias, the localized alias, and the id read as separate words.
#[test]
fn canonical_aliases_and_id_tokens_are_searchable() {
    let app = PlotxApp::new_with_settings(plotx_core::settings::Settings::default());
    let items = search_set(&app);
    for query in [
        "contour threshold",
        "sigma",
        "contour count",
        "series.contour.ratio",
    ] {
        assert!(
            indices(&items, query)
                .iter()
                .any(|action| matches!(action, PaletteAction::Property(_))),
            "'{query}' must reach a property"
        );
    }
    assert!(
        indices(&items, "contour count").contains(&PaletteAction::Property(contour::COUNT)),
        "id tokens are matched word by word"
    );
}

/// Resources join the same set, so the search finds the thing as well as the
/// verb and the setting.
#[test]
fn resources_are_part_of_the_search_set() {
    let mut app = PlotxApp::new_with_settings(plotx_core::settings::Settings::default());
    app.doc
        .canvases
        .push(plotx_core::state::CanvasDocument::new(
            "Figure 3".to_owned(),
            [120.0, 80.0],
        ));
    let items = search_set(&app);
    assert!(indices(&items, "figure 3").contains(&PaletteAction::Canvas(0)));
}

/// Activating a property hit routes to its declared home and starts the
/// reveal — expand, scroll, highlight — rather than executing anything.
#[test]
fn activating_a_property_requests_its_home_route() {
    let mut app = PlotxApp::new_with_settings(plotx_core::settings::Settings::default());
    app.session.secondary_sidebar_visible = false;
    reveal_property(&mut app, contour::BASE_MAGNITUDE, 10.0);
    assert!(app.session.secondary_sidebar_visible);
    let focus = app.session.ui.property_focus.expect("focus is requested");
    assert_eq!(focus.property, contour::BASE_MAGNITUDE);
    assert!(focus.pending);
    assert!((focus.highlight_until - 10.8).abs() < 1e-9);
}

fn property_item(items: &[PaletteItem], property: PropertyId) -> &PaletteItem {
    items
        .iter()
        .find(|item| item.action == PaletteAction::Property(property))
        .expect("every presented property is in the search set")
}

/// §8.5 channel 1 gates on applicability, because activating a hit only asks the
/// panel to reveal a row. A hit that applies to nothing had nowhere to scroll
/// to: the focus was requested and then hung there, having moved nothing.
///
/// The entry stays in the list, disabled with a reason, rather than
/// disappearing — the crate's hide-vs-disable rule, and the reason a user
/// searching by name still finds the setting they were looking for.
#[test]
fn a_setting_that_applies_to_nothing_is_disabled_with_a_reason() {
    let app = PlotxApp::new_with_settings(plotx_core::settings::Settings::default());
    let items = search_set(&app);
    let hit = property_item(&items, contour::BASE_MAGNITUDE);
    assert!(
        !hit.enabled,
        "an empty document has no series to change the lowest level of"
    );
    let reason = hit
        .disabled_reason
        .as_deref()
        .expect("a gated entry explains itself");
    assert!(
        reason.starts_with("Select"),
        "the reason names the fix: {reason}"
    );
    // Still findable: the gate is on activation, not on discovery.
    assert!(
        indices(&items, "lowest level").contains(&PaletteAction::Property(contour::BASE_MAGNITUDE))
    );
}

/// The gate is the catalog's own applicability answer, so a target that exists
/// but does not carry the setting is refused with the catalog's own reason
/// rather than a second rule written in the palette.
#[test]
fn a_setting_the_selected_series_cannot_carry_is_disabled_with_the_catalogs_reason() {
    let (mut app, ids) = properties::fixture::contour_page(1);
    properties::fixture::draw_as_heatmap(&mut app, ids[0]);
    let items = search_set(&app);
    let hit = property_item(&items, contour::BASE_MAGNITUDE);
    assert!(!hit.enabled);
    let reason = hit
        .disabled_reason
        .as_deref()
        .expect("a gated entry explains itself");
    assert!(
        reason.contains("heatmap"),
        "the reason states what the series actually draws: {reason}"
    );
}

#[test]
fn a_setting_the_selection_can_receive_stays_active() {
    let (app, _) = properties::fixture::contour_page(1);
    let items = search_set(&app);
    let hit = property_item(&items, contour::BASE_MAGNITUDE);
    assert!(hit.enabled, "a selected contour series can receive it");
    assert!(hit.disabled_reason.is_none());
}

/// Navigating to a page is not only a change of what is drawn.
///
/// `ObjectId`s are allocated per page and start again at one, so a selection
/// left over from the page being left resolves here to an unrelated object —
/// and the inspector, the property panel and every tool would then act on it.
/// The palette therefore navigates through the same path every other page
/// switch uses instead of assigning `active_canvas` on its own.
#[test]
fn activating_a_page_does_not_carry_a_stale_selection_onto_it() {
    let (mut app, ids) = properties::fixture::contour_page(2);
    let (second, elsewhere) = properties::fixture::add_page(&mut app, "Figure 2", 0, 1);
    assert_eq!(
        elsewhere[0], ids[0],
        "per-page allocation makes the ids collide, which is the whole problem"
    );
    app.select_object(0, ids[1]);

    let ctx = egui::Context::default();
    let mut clipboard = clipboard_table::ClipboardTablePaste::default();
    activate(
        PaletteAction::Canvas(second),
        &mut app,
        &mut clipboard,
        &ctx,
    );

    assert_eq!(app.session.active_canvas, Some(second));
    assert_eq!(
        app.session.ui.selection.objects(),
        &elsewhere[..],
        "the selection is re-derived from the page entered, never inherited"
    );
    assert!(!app.session.ui.selection.objects().contains(&ids[1]));
}

/// Landing on an object points the data focus at what that object draws, so the
/// two halves of the session do not end up describing different things.
#[test]
fn activating_an_object_brings_the_data_focus_with_it() {
    let (mut app, _) = properties::fixture::contour_page(1);
    let second_dataset = properties::fixture::add_dataset(&mut app);
    let (page, objects) = properties::fixture::add_page(&mut app, "Figure 2", second_dataset, 1);
    app.focus_single(0);

    let ctx = egui::Context::default();
    let mut clipboard = clipboard_table::ClipboardTablePaste::default();
    activate(
        PaletteAction::Object(page, objects[0]),
        &mut app,
        &mut clipboard,
        &ctx,
    );

    assert_eq!(app.session.active_canvas, Some(page));
    assert_eq!(app.session.ui.selection.objects(), &objects[..]);
    assert_eq!(
        app.active_dataset(),
        Some(second_dataset),
        "the data focus follows the object the search landed on"
    );
}

#[test]
fn empty_state_is_constructible() {
    let state = plotx_core::state::CommandPaletteState::default();
    assert!(state.query.is_empty());
}

/// The palette disables a setting it cannot reveal, and the reason has to name
/// the state that blocks it. A locked plot used to disappear from the selection
/// entirely, so the palette reported "Select a plot whose series draws
/// contours…" about a contour plot the user had selected.
#[test]
fn a_locked_selection_disables_a_setting_with_the_reason_that_actually_applies() {
    use crate::ui::properties::fixture;

    let (mut app, ids) = fixture::contour_page(1);
    let targets = properties::discovery::targets_for_property(&app, contour::BASE_MAGNITUDE);
    assert!(property_unavailable_reason(&app, contour::BASE_MAGNITUDE, &targets).is_none());

    if let Some(object) = app.doc.canvases[0].object_mut(ids[0]) {
        object.locked = true;
    }
    let targets = properties::discovery::targets_for_property(&app, contour::BASE_MAGNITUDE);
    let reason = property_unavailable_reason(&app, contour::BASE_MAGNITUDE, &targets)
        .expect("a locked plot cannot receive the write");
    assert_eq!(reason, properties::discovery::LOCKED_REASON);
    assert!(
        !reason.contains("draws contours"),
        "the plot does draw contours; saying otherwise sends the user to fix the wrong thing"
    );
}

/// Revealing a setting that lives on a processing step has to open that step,
/// and has to do it by moving the panel's own expansion state.
///
/// The previous arrangement derived the expansion from the property focus while
/// rendering, so the focus's ~800 ms highlight timer collapsed the row again
/// with no user action behind it — the crate's layout-stability rule forbids
/// exactly that — and, because a focus names a property rather than a step, it
/// opened every step that could carry the setting at once.
#[test]
fn revealing_a_step_setting_opens_that_step_and_leaves_it_open() {
    use plotx_core::properties::apodization;

    let mut app = PlotxApp::new_with_settings(plotx_core::settings::Settings::default());
    app.doc
        .datasets
        .push(crate::ui::properties::fixture::time_domain_2d());
    app.set_active_dataset(Some(0));

    let expected = properties::discovery::targets_for_property(&app, apodization::KIND)
        .into_iter()
        .find_map(|target| match target.component {
            Some(plotx_core::automation::ComponentRef::ProcessingStep(step)) => Some(step),
            _ => None,
        })
        .expect("the time-domain factory recipe has an apodization step");

    reveal_property(&mut app, apodization::KIND, 10.0);
    assert_eq!(
        app.session.ui.proc_expanded_step.map(|(_, step)| step),
        Some(expected),
        "the reveal opens the step that carries the setting"
    );

    // The highlight expires on a timer. The expansion may not follow it.
    app.session.ui.property_focus = None;
    assert_eq!(
        app.session.ui.proc_expanded_step.map(|(_, step)| step),
        Some(expected),
        "a timer must never collapse a row the user asked to see"
    );
}
