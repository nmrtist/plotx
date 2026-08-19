//! Canvas settings window and layout editing controls.

use super::*;

/// Stable layer of the Canvas settings window, shared with the size chip so a
/// chip click can raise an already-open window above the chip's own layer.
pub(in crate::ui) fn canvas_settings_layer() -> egui::LayerId {
    egui::LayerId::new(egui::Order::Middle, egui::Id::new("canvas_settings_window"))
}

pub(in crate::ui) fn canvas_settings_window(app: &mut PlotxApp, ctx: &egui::Context) {
    let Some(ci) = app.session.ui.canvas_settings else {
        return;
    };
    if ci >= app.doc.canvases.len() {
        app.session.ui.canvas_settings = None;
        return;
    }
    let mut open = true;
    let title = format!("Canvas settings — {}", app.doc.canvases[ci].name);
    let target = app.canvas_target(app.doc.canvases[ci].resource_id);
    egui::Window::new(title)
        .id(canvas_settings_layer().id)
        .collapsible(false)
        .resizable(false)
        .open(&mut open)
        .show(ctx, |ui| {
            super::canvas_size::size_section(app, ci, ui);
            crate::ui::properties::panel::canvas_size_section(app, &target, ui);
            crate::ui::properties::panel::canvas_margins_section(app, &target, ui);
            ui.weak("Visual spacing is a minimum request; axis furniture may make it larger.");
            ui.horizontal(|ui| {
                ui.label("Presets");
                for preset in plotx_core::layout::GutterPreset::ALL {
                    let selected = (app.doc.canvases[ci].layout.gutter_mm - preset.millimetres())
                        .abs()
                        < 0.001;
                    if ui.selectable_label(selected, preset.label()).clicked() {
                        app.set_gutter_preset(preset);
                    }
                }
            });

            crate::ui::properties::panel::canvas_grid_section(app, &target, ui);
            ui.horizontal(|ui| {
                let (rows, cols) = {
                    let l = app.doc.canvases[ci].layout;
                    (l.rows, l.cols)
                };
                let simplify_id = egui::Id::new(("apply_grid_simplify", ci));
                let mut simplify = ui
                    .data_mut(|data| data.get_temp::<bool>(simplify_id))
                    .unwrap_or(false);
                if ui.checkbox(&mut simplify, "Simplify inner axes").changed() {
                    ui.data_mut(|data| data.insert_temp(simplify_id, simplify));
                }
                if ui
                    .button("Apply grid")
                    .on_hover_text("Reposition all plots into these cells")
                    .clicked()
                {
                    app.arrange_active_canvas_grid_with_simplify(rows, cols, simplify);
                }
            });

            crate::ui::properties::panel::canvas_caption_section(app, &target, ui);
            ui.weak(
                "The scientific summary is generated automatically. This optional page note is shown after it on the board only.",
            );

            let resp = ui.add(
                egui::TextEdit::multiline(&mut app.doc.canvases[ci].caption)
                    .desired_width(340.0)
                    .desired_rows(3)
                    .hint_text("Optional page note…"),
            );
            if resp.gained_focus() {
                app.session.ui.caption_edit_before = Some((
                    ci,
                    app.doc.canvases[ci].caption.clone(),
                    app.doc.canvases[ci].caption_visible,
                ));
            }
            if resp.changed() {
                app.mark_document_dirty();
            }
            if resp.lost_focus() {
                commit_caption_edit(app, ci);
            }

            ui.add_space(12.0);
            ui.separator();
            panels_section(app, ci, ui);
        });
    if !open {
        commit_caption_edit(app, ci);
        commit_note_edit(app);
        app.session.ui.canvas_settings = None;
    }
}

/// User notes are listed after the automatic scientific summary.
fn panels_section(app: &mut PlotxApp, ci: usize, ui: &mut Ui) {
    ui.label(crate::typography::headline("Panels"));
    ui.add_space(6.0);

    ui.weak("Letters identify multi-panel plots; optional notes follow the scientific summary.");
    ui.add_space(4.0);

    let order = app.doc.canvases[ci].plot_reading_order();
    if order.is_empty() {
        ui.weak("No plots on this page yet.");
        return;
    }
    for (i, id) in order.into_iter().enumerate() {
        let letter = app.doc.canvases[ci].panel_label_style.format(i);
        let Some(_panel) = app.doc.canvases[ci].panel_meta_for_content(id) else {
            continue;
        };
        ui.horizontal(|ui| {
            ui.label(crate::typography::headline(&letter));
            crate::ui::properties::panel::panel_inline_section(app, ci, id, ui);
        });
    }
}

/// Commit an in-progress per-panel note edit as one undoable step. A no-op when
/// nothing changed (or the panel/page is gone).
fn commit_note_edit(app: &mut PlotxApp) {
    let Some((ci, id, before)) = app.session.ui.note_edit_before.take() else {
        return;
    };
    let Some(after) = app
        .doc
        .canvases
        .get(ci)
        .and_then(|c| c.panel_meta_for_content(id))
    else {
        return;
    };
    app.execute_action(Action::set_panel_meta(ci, id, before, after));
}

/// Commit an in-progress caption text edit for `ci` as one undoable step. A no-op
/// when nothing changed during the edit session (or it targeted another canvas).
fn commit_caption_edit(app: &mut PlotxApp, ci: usize) {
    let Some((canvas, before_text, before_visible)) = app.session.ui.caption_edit_before.take()
    else {
        return;
    };
    if canvas != ci || ci >= app.doc.canvases.len() {
        return;
    }
    let after = (
        app.doc.canvases[ci].caption.clone(),
        app.doc.canvases[ci].caption_visible,
    );
    app.execute_action(Action::set_canvas_caption(
        ci,
        (before_text, before_visible),
        after,
    ));
}
