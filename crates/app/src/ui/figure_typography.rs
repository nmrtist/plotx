//! Document-level Figure Typography window: the tick / axis-title / figure-title
//! point sizes stamped onto every plot. Edits apply live (each rebuild restamps
//! the document value) and one slider gesture coalesces into one undo step, the
//! same contract as the canvas-size fields.

use super::*;
use plotx_core::properties::FloatBounds;
use plotx_figure::FigureTypography;

pub(super) fn figure_typography_window(app: &mut PlotxApp, ctx: &egui::Context) {
    if !app.session.ui.figure_typography_open {
        return;
    }
    let mut open = true;
    egui::Window::new("Figure typography")
        .collapsible(false)
        .resizable(false)
        .open(&mut open)
        .show(ctx, |ui| {
            ui.label(
                egui::RichText::new(
                    "Point sizes of every plot's axis text in this document. Sizes are \
                     absolute (journal convention): resizing a panel never changes them.",
                )
                .small()
                .color(ui.visuals().weak_text_color()),
            );
            ui.add_space(6.0);
            egui::Grid::new("figure_typography_grid")
                .num_columns(2)
                .spacing([12.0, 6.0])
                .show(ui, |ui| {
                    // The tick size is a catalog property, so its range comes
                    // from the definition the catalog control and the write path
                    // are both built from. A literal here would be a second copy
                    // of the rule, and it was: this window clamped to 24 pt while
                    // the catalog admitted 72, so any interaction here silently
                    // pulled a 40 pt figure back down.
                    size_row(app, ui, "Tick labels", tick_bounds(), |t| &mut t.tick_pt);
                    ui.end_row();
                    // The other two are not catalog properties yet and keep the
                    // range this window has always applied to them.
                    size_row(app, ui, "Axis titles", UNREGISTERED_BOUNDS, |t| {
                        &mut t.label_pt
                    });
                    ui.end_row();
                    size_row(app, ui, "Figure title", UNREGISTERED_BOUNDS, |t| {
                        &mut t.title_pt
                    });
                    ui.end_row();
                });
            ui.add_space(8.0);
            if ui.button("Reset to defaults").clicked() {
                let before = app.doc.style_library.figure_typography;
                app.execute_action(Action::set_figure_typography(
                    before,
                    FigureTypography::default(),
                ));
            }
        });
    if !open {
        app.session.ui.figure_typography_open = false;
        app.session.ui.figure_typography_before = None;
    }
}

/// One labelled pt-size drag. Live-applies while dragging and commits a single
/// undoable action per gesture (or per typed edit), mirroring
/// `handle_canvas_dimension_response`.
/// The range this window applies to the two sizes that have no catalog entry.
const UNREGISTERED_BOUNDS: FloatBounds = FloatBounds::inclusive(4.0, 24.0);

/// The declared range of the tick-label size, read from its definition.
fn tick_bounds() -> FloatBounds {
    plotx_core::properties::definition(plotx_core::properties::typography::TICK_PT)
        .and_then(|definition| definition.value_schema.float_bounds())
        .unwrap_or(UNREGISTERED_BOUNDS)
}

fn size_row(
    app: &mut PlotxApp,
    ui: &mut Ui,
    label: &str,
    bounds: FloatBounds,
    field: impl Fn(&mut FigureTypography) -> &mut f32,
) {
    ui.label(label);
    let frame_before = app.doc.style_library.figure_typography;
    let mut value = {
        let mut current = frame_before;
        *field(&mut current)
    };
    let resp = ui.add(
        egui::DragValue::new(&mut value)
            .speed(0.25)
            .range(bounds.lowest()..=bounds.max)
            .max_decimals(1)
            .suffix(" pt"),
    );
    if resp.drag_started() {
        app.session.ui.figure_typography_before = Some(frame_before);
    }
    if resp.changed() {
        let mut after = frame_before;
        *field(&mut after) = value;
        app.set_figure_typography_value(after);
        app.doc.dirty = true;
    }
    if resp.drag_stopped() {
        let before = app
            .session
            .ui
            .figure_typography_before
            .take()
            .unwrap_or(frame_before);
        let after = app.doc.style_library.figure_typography;
        app.execute_action(Action::set_figure_typography(before, after));
    } else if resp.changed() && !resp.dragged() {
        let after = app.doc.style_library.figure_typography;
        app.execute_action(Action::set_figure_typography(frame_before, after));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One definition of the range. The window used to stop at 24 pt while the
    /// catalog admitted 72, so a size set through the catalog was silently
    /// clamped the next time this window was touched.
    #[test]
    fn the_tick_row_takes_its_range_from_the_catalog() {
        let declared =
            plotx_core::properties::definition(plotx_core::properties::typography::TICK_PT)
                .and_then(|definition| definition.value_schema.float_bounds())
                .expect("the tick-label size is a registered float property");
        let row = tick_bounds();
        assert_eq!(row.max, declared.max);
        assert_eq!(row.lowest(), declared.lowest());
        assert!(
            row.admits(40.0),
            "a size the catalog accepts must survive a visit to this window"
        );
    }
}
