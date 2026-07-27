//! What the panel says when its sources do not agree, and which sources it has.
//!
//! Both are questions about representation rather than about editing. A row
//! whose targets disagree may show no value at all — not a number, and not a
//! caption derived from one of them — and a panel that reads a narrower
//! selection than the channels which navigate to it cannot show a disagreement
//! at all, because it never sees two targets.

use super::*;
use crate::ui::properties::{discovery, fixture};

/// Resolve the section's rows the way the panel does: over the plot selection
/// discovery resolves, not over a hand-picked target list.
fn rows_for_selection(app: &PlotxApp) -> Vec<Row> {
    let targets: Vec<TargetRef> = discovery::selection_objects(app)
        .into_iter()
        .flat_map(|object| app.series_targets(0, object))
        .collect();
    resolve_rows(app, &targets)
}

fn lowest_level_row(rows: &[Row]) -> &Row {
    rows.iter()
        .find(|row| row.presentation.id == contour::BASE_MAGNITUDE)
        .expect("the lowest level is a row of the contour section")
}

/// The debt phase 5a recorded: one hint that stated both possibilities at
/// once was true but blunt. With the definition declaring how many copies a
/// target holds, the row can name the sources that actually disagree — and
/// it does so without knowing what a `ContourSpec` is.
#[test]
fn the_hint_names_the_sources_that_actually_disagree() {
    let one_ladder = no_single_value_hint(1, ValueCopies::PerMirroredHalf);
    assert!(
        one_ladder.contains("positive and negative halves"),
        "a single series with an asymmetric ladder is a halves problem: {one_ladder}"
    );
    assert!(!one_ladder.contains("selected series"));

    let many_plain = no_single_value_hint(3, ValueCopies::PerTarget);
    assert!(
        many_plain.contains("selected series"),
        "several targets of a one-copy setting is a selection problem: {many_plain}"
    );
    assert!(!many_plain.contains("halves"));

    // Only when both are possible may the hint state both.
    let many_ladders = no_single_value_hint(3, ValueCopies::PerMirroredHalf);
    assert!(many_ladders.contains("selected series"));
    assert!(many_ladders.contains("halves"));
}

/// A single selected plot is the ordinary case, and it is uniform.
#[test]
fn one_selected_plot_reads_as_a_single_value() {
    let (app, _) = fixture::contour_page(1);
    let row = rows_for_selection(&app).len();
    assert!(row > 0, "a selected contour plot renders catalog rows");
    let rows = rows_for_selection(&app);
    assert!(!lowest_level_row(&rows).mixed());
}

/// The cross-target `Mixed` aggregate has to be *reachable from the interface*.
///
/// Everything below the panel could already report it, but the section was
/// rendered inside a single-selection guard, so no interface path ever handed
/// it two targets. A read model that cannot be exercised is not a feature; this
/// selects two contour plots that disagree and checks the row the panel would
/// draw says so.
#[test]
fn two_selected_plots_that_disagree_reach_the_panel_as_mixed() {
    let (mut app, ids) = fixture::contour_page(2);
    fixture::set_lowest_level(&mut app, ids[1], 9.0);
    assert_eq!(
        discovery::selection_objects(&app).len(),
        2,
        "both plots are in the resolved selection"
    );

    let rows = rows_for_selection(&app);
    let row = lowest_level_row(&rows);
    assert!(
        row.mixed(),
        "two series holding different lowest levels have no single value"
    );
    assert_eq!(row.set.applicable_targets.len(), 2);
    assert!(row.value().is_none(), "a mixed row shows no number");
    assert!(row.modified(), "a mixed row is not the factory default");
}

/// Selecting nothing falls back to the page's active plot, which is what the
/// Ribbon button and the context menu already gate on. The panel has to agree,
/// or those channels enable a jump to a section that draws nothing.
#[test]
fn an_empty_selection_still_resolves_the_pages_active_plot() {
    let (mut app, _) = fixture::contour_page(1);
    app.set_selection(plotx_core::state::Selection::None);
    assert_eq!(discovery::selection_objects(&app).len(), 1);
    assert!(
        !rows_for_selection(&app).is_empty(),
        "the section renders wherever the discovery channels say it applies"
    );
}

/// §8.3 and §4.3 together: a row with no single value may not caption itself
/// with one target's resolved level.
///
/// The control already blanks its number when the sources disagree, but the
/// readout beside it was resolved from the first applicable target and printed
/// regardless — `5 × σ = 1.2e4` next to an em dash, presenting one series'
/// threshold as the selection's. The row is given no readout at all rather than
/// being trusted to hide one, so there is no level text for a control to print.
#[test]
fn a_mixed_row_carries_no_resolved_level() {
    let (mut app, ids) = fixture::contour_page(2);
    fixture::set_lowest_level(&mut app, ids[1], 9.0);

    let rows = rows_for_selection(&app);
    let row = lowest_level_row(&rows);
    assert!(row.mixed());
    assert!(
        row.readout.is_none(),
        "a mixed row has no single lowest level to resolve, so it states none: {:?}",
        row.readout
    );
}

/// The complement, so the suppression cannot be satisfied by never producing a
/// readout at all: one plot, agreeing with itself, still gets its anchor
/// sentence.
#[test]
fn a_uniform_row_still_carries_its_resolved_level() {
    let (app, _) = fixture::contour_page(1);
    let rows = rows_for_selection(&app);
    let row = lowest_level_row(&rows);
    assert!(!row.mixed());
    let readout = row
        .readout
        .as_ref()
        .expect("a single agreeing series states what its multiple means");
    let plotx_core::properties::PropertyReadout::ContourBase(readout) = readout else {
        panic!("the lowest-level row carries a contour readout");
    };
    assert_eq!(readout.magnitude, 5.0);
}

/// The heading is a count of what the section acts on, and it has to be a
/// sentence. Appending an `s` produced "2 contour seriess", and counting the
/// whole selection produced a two where only one plot draws a contour — both
/// are the heading claiming something the rows do not do.
#[test]
fn the_heading_counts_what_the_section_supplies_and_says_it_in_english() {
    let (mut app, ids) = fixture::contour_page(2);
    fixture::draw_as_heatmap(&mut app, ids[1]);
    let targets: Vec<TargetRef> = ids
        .iter()
        .flat_map(|&id| app.series_targets(0, id))
        .collect();
    assert_eq!(targets.len(), 2, "both plots are in the selection");

    let rows = resolve_rows(&app, &targets);
    let applicable = applicable_targets(&rows);
    assert_eq!(
        applicable.len(),
        1,
        "only one of the two selected series draws a contour"
    );

    let series = SectionNoun::new("contour series", "contour series");
    assert_eq!(series.counted(applicable.len()), "1 contour series");
    assert_eq!(series.counted(2), "2 contour series");
    let document = SectionNoun::new("document", "documents");
    assert_eq!(document.counted(1), "1 document");
    assert_eq!(document.counted(2), "2 documents");
}
