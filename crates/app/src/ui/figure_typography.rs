//! Document-level Figure Typography window: the tick / axis-title / figure-title
//! point sizes stamped onto every plot. Edits apply live (each rebuild restamps
//! the document value) and one slider gesture coalesces into one undo step, the
//! same contract as the canvas-size fields.

use super::*;
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
                    size_row(app, ui, "Tick labels", |t| &mut t.tick_pt);
                    ui.end_row();
                    size_row(app, ui, "Axis titles", |t| &mut t.label_pt);
                    ui.end_row();
                    size_row(app, ui, "Figure title", |t| &mut t.title_pt);
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
fn size_row(
    app: &mut PlotxApp,
    ui: &mut Ui,
    label: &str,
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
            .range(4.0..=24.0)
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
