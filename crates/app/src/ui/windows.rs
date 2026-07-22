use super::*;

pub(super) fn save_project_window(app: &mut PlotxApp, ctx: &egui::Context) {
    if !app.session.ui.save_project_options {
        return;
    }

    let mut save = false;
    let mut save_as = false;
    let mut cancel = false;
    let modal = super::modal(ctx, "save_project_modal", ModalKind::Dialog).show(ctx, |ui| {
        ui.set_width(390.0);
        ui.heading("Save project");
        ui.separator();
        if app.session.status.starts_with("Save failed:") {
            ui.colored_label(ui.visuals().error_fg_color, &app.session.status);
            if ui.link("Open diagnostic details").clicked() {
                app.session.ui.diagnostics_open = true;
            }
            ui.add_space(8.0);
        }
        ui.checkbox(
            &mut app.doc.save_include_view_snapshots,
            "Include rendered canvas snapshots",
        )
        .on_hover_text(
            "Stores materialized view data for faster and more stable reopening. \
                 This can make .plotx files much larger.",
        );
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            if ui.button("Save").clicked() {
                save = true;
            }
            if ui.button("Save As…").clicked() {
                save_as = true;
            }
            if ui.button("Cancel").clicked() {
                cancel = true;
            }
        });
    });

    if save {
        if let Some(path) = app.doc.project_path.clone() {
            app.session.ui.save_project_options =
                !app.save_project_to(&path, app.doc.save_include_view_snapshots);
        } else {
            crate::ui::file_dialogs::save_project_as(app, app.doc.save_include_view_snapshots);
            app.session.ui.save_project_options = app.doc.dirty;
        }
    } else if save_as {
        crate::ui::file_dialogs::save_project_as(app, app.doc.save_include_view_snapshots);
        app.session.ui.save_project_options = app.doc.dirty;
    } else if cancel || modal.should_close() {
        app.session.ui.save_project_options = false;
    }
}

/// Intercept a window-close request when the project has unsaved changes: veto the
/// close and raise the confirm dialog. Once the user confirms (Save or Discard),
/// `allow_close` lets the re-issued request through.
pub(super) fn handle_close_request(app: &mut PlotxApp, ctx: &egui::Context) {
    if !ctx.input(|i| i.viewport().close_requested()) {
        return;
    }
    if app.doc.dirty && !app.session.allow_close {
        ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
        app.session.ui.quit_confirm = true;
    }
}

/// Save / Discard / Cancel dialog shown when a close was intercepted on a dirty
/// project. Save routes through the normal save flow (opening Save As… if the
/// project has no path yet) and only closes once the save actually succeeds.
pub(super) fn quit_confirm_window(app: &mut PlotxApp, ctx: &egui::Context) {
    if !app.session.ui.quit_confirm {
        return;
    }
    let mut save = false;
    let mut discard = false;
    let mut cancel = false;
    let modal = super::modal(ctx, "quit_confirm_modal", ModalKind::Dialog).show(ctx, |ui| {
        ui.set_width(420.0);
        ui.heading("Unsaved changes");
        ui.separator();
        ui.label("This project has unsaved changes. Save before closing?");
        if app.session.status.starts_with("Save failed:") {
            ui.add_space(8.0);
            egui::Frame::new()
                .fill(ui.visuals().error_fg_color.linear_multiply(0.12))
                .corner_radius(6)
                .inner_margin(8)
                .show(ui, |ui| {
                    ui.colored_label(ui.visuals().error_fg_color, &app.session.status);
                    if ui.link("Open diagnostic details").clicked() {
                        app.session.ui.diagnostics_open = true;
                    }
                });
        }
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            if ui.button("Save").clicked() {
                save = true;
            }
            if ui.button("Discard").clicked() {
                discard = true;
            }
            if ui.button("Cancel").clicked() {
                cancel = true;
            }
        });
    });

    if save {
        let saved = if let Some(path) = app.doc.project_path.clone() {
            app.save_project_to(&path, app.doc.save_include_view_snapshots)
        } else {
            crate::ui::file_dialogs::save_project_as(app, app.doc.save_include_view_snapshots);
            !app.doc.dirty
        };
        if saved {
            app.session.ui.quit_confirm = false;
            app.session.allow_close = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        } else {
            crate::cancel_relaunch();
        }
    } else if discard {
        app.session.ui.quit_confirm = false;
        app.session.allow_close = true;
        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
    } else if cancel || modal.should_close() {
        app.session.ui.quit_confirm = false;
        crate::cancel_relaunch();
    }
}
/// Stable layer of the Canvas settings window, shared with the size chip so a
/// chip click can raise an already-open window above the chip's own layer.
pub(super) fn canvas_settings_layer() -> egui::LayerId {
    egui::LayerId::new(egui::Order::Middle, egui::Id::new("canvas_settings_window"))
}

pub(super) fn canvas_settings_window(app: &mut PlotxApp, ctx: &egui::Context) {
    let Some(ci) = app.session.ui.canvas_settings else {
        return;
    };
    if ci >= app.doc.canvases.len() {
        app.session.ui.canvas_settings = None;
        return;
    }
    let mut open = true;
    let title = format!("Canvas settings — {}", app.doc.canvases[ci].name);
    egui::Window::new(title)
        .id(canvas_settings_layer().id)
        .collapsible(false)
        .resizable(false)
        .open(&mut open)
        .show(ctx, |ui| {
            super::canvas_size::size_section(app, ci, ui);

            ui.add_space(12.0);
            ui.separator();
            ui.strong("Layout");
            ui.add_space(6.0);

            let unit = app.session.ui.canvas_size_unit;
            ui.horizontal(|ui| {
                ui.label("Margins");
                margin_drag(app, ci, ui, unit, 0, "T");
                margin_drag(app, ci, ui, unit, 3, "L");
                margin_drag(app, ci, ui, unit, 2, "B");
                margin_drag(app, ci, ui, unit, 1, "R");
                ui.label(unit.label());
            });

            ui.horizontal(|ui| {
                ui.label("Minimum spacing");
                gutter_drag(app, ci, ui, unit);
                ui.label(unit.label());
            });
            ui.weak("Visual spacing is a minimum request; axis furniture may make it larger.");

            ui.horizontal(|ui| {
                ui.label("Spacing basis");
                for (label, mode) in [
                    ("Frame", plotx_core::layout::SpacingMode::Frame),
                    ("Visual", plotx_core::layout::SpacingMode::Visual),
                ] {
                    let selected = app.doc.canvases[ci].layout.spacing_mode == mode;
                    if ui.selectable_label(selected, label).clicked() {
                        app.set_spacing_mode(mode);
                    }
                }
            });
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

            ui.horizontal(|ui| {
                ui.label("Grid");
                grid_count_drag(app, ci, ui, true);
                ui.label("rows ×");
                grid_count_drag(app, ci, ui, false);
                ui.label("cols");
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

            let mut show_grid = app.doc.canvases[ci].layout.show_grid;
            if ui.checkbox(&mut show_grid, "Show layout grid").changed() {
                app.set_show_grid(ci, show_grid);
            }

            ui.add_space(12.0);
            ui.separator();
            ui.strong("Caption");
            ui.add_space(6.0);
            ui.weak("Shown below the page on the board only — not exported or presented.");

            let mut visible = app.doc.canvases[ci].caption_visible;
            if ui
                .checkbox(&mut visible, "Show caption below page")
                .changed()
            {
                let before = (app.doc.canvases[ci].caption.clone(), !visible);
                app.execute_action(Action::set_canvas_caption(
                    ci,
                    before,
                    (app.doc.canvases[ci].caption.clone(), visible),
                ));
            }

            let resp = ui.add(
                egui::TextEdit::multiline(&mut app.doc.canvases[ci].caption)
                    .desired_width(340.0)
                    .desired_rows(3)
                    .hint_text("e.g. Figure 1. Concentration vs. time…"),
            );
            if resp.gained_focus() {
                app.session.ui.caption_edit_before = Some((
                    ci,
                    app.doc.canvases[ci].caption.clone(),
                    app.doc.canvases[ci].caption_visible,
                ));
            }
            if resp.changed() {
                app.doc.dirty = true;
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

/// Notes are auto-listed below the page on the board.
fn panels_section(app: &mut PlotxApp, ci: usize, ui: &mut Ui) {
    ui.strong("Panels");
    ui.add_space(6.0);

    let style = app.doc.canvases[ci].panel_label_style;
    ui.horizontal(|ui| {
        ui.label("Letter style");
        egui::ComboBox::from_id_salt(("panel_label_style", ci))
            .selected_text(style.label())
            .show_ui(ui, |ui| {
                for option in PanelLabelStyle::ALL {
                    if ui
                        .selectable_label(style == option, option.label())
                        .clicked()
                    {
                        if option != style {
                            app.execute_action(Action::SetPanelLabelStyle {
                                canvas: ci,
                                before: style,
                                after: option,
                            });
                        }
                        ui.close();
                    }
                }
            });
    });

    ui.add_space(6.0);
    ui.weak("Letters are top-left in each plot; notes list below the page (board only).");
    ui.add_space(4.0);

    let order = app.doc.canvases[ci].plot_reading_order();
    if order.is_empty() {
        ui.weak("No plots on this page yet.");
        return;
    }
    for (i, id) in order.into_iter().enumerate() {
        let letter = app.doc.canvases[ci].panel_label_style.format(i);
        let Some(title) = app.doc.canvases[ci]
            .object(id)
            .and_then(|o| o.plot())
            .map(|p| p.panel.clone())
        else {
            continue;
        };
        ui.horizontal(|ui| {
            let mut visible = title.visible;
            if ui
                .checkbox(&mut visible, "")
                .on_hover_text("Show this panel's letter")
                .changed()
            {
                let mut after = title.clone();
                after.visible = visible;
                app.execute_action(Action::set_panel_meta(ci, id, title.clone(), after));
            }
            ui.strong(&letter);
            let Some(plot) = app.doc.canvases[ci]
                .object_mut(id)
                .and_then(|o| o.plot_mut())
            else {
                return;
            };
            let resp = ui.add(
                egui::TextEdit::singleline(&mut plot.panel.user_note)
                    .desired_width(260.0)
                    .hint_text("Panel note…"),
            );
            if resp.gained_focus() {
                commit_note_edit(app);
                app.session.ui.note_edit_before = Some((ci, id, title.clone()));
            }
            if resp.changed() {
                app.doc.dirty = true;
            }
            if resp.lost_focus() {
                commit_note_edit(app);
            }
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
        .and_then(|c| c.object(id))
        .and_then(|o| o.plot())
        .map(|p| p.panel.clone())
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

/// Commits a page-layout edit as one undoable step, coalescing a slider drag
/// into a single history entry (mirrors `handle_canvas_dimension_response`).
/// The caller has already applied the live change to `canvas.layout`.
pub(super) fn handle_layout_response(
    app: &mut PlotxApp,
    ci: usize,
    resp: &Response,
    before_fallback: PageLayout,
) {
    if resp.drag_started() {
        app.session.ui.page_layout_edit = Some(PendingPageLayoutEdit {
            canvas: ci,
            before: before_fallback,
        });
    }
    if resp.drag_stopped() {
        let before = app
            .session
            .ui
            .page_layout_edit
            .take()
            .filter(|edit| edit.canvas == ci)
            .map(|edit| edit.before)
            .unwrap_or(before_fallback);
        let after = app.doc.canvases[ci].layout;
        app.commit_page_layout(ci, before, after);
    } else if resp.changed() && !resp.dragged() {
        let after = app.doc.canvases[ci].layout;
        app.commit_page_layout(ci, before_fallback, after);
    }
}

pub(super) fn margin_drag(
    app: &mut PlotxApp,
    ci: usize,
    ui: &mut Ui,
    unit: CanvasSizeUnit,
    idx: usize,
    label: &str,
) {
    ui.label(label);
    let before = app.doc.canvases[ci].layout;
    let mut value = unit.from_mm(before.margin_mm[idx]);
    let resp = ui.add(
        egui::DragValue::new(&mut value)
            .speed(unit.drag_speed())
            .range(unit.from_mm(0.0)..=unit.from_mm(100.0))
            .max_decimals(unit.decimals()),
    );
    if resp.changed() {
        app.doc.canvases[ci].layout.margin_mm[idx] = unit.to_mm(value).clamp(0.0, 100.0);
        app.doc.dirty = true;
    }
    handle_layout_response(app, ci, &resp, before);
}

pub(super) fn gutter_drag(app: &mut PlotxApp, ci: usize, ui: &mut Ui, unit: CanvasSizeUnit) {
    let before = app.doc.canvases[ci].layout;
    let mut value = unit.from_mm(before.gutter_mm);
    let resp = ui.add(
        egui::DragValue::new(&mut value)
            .speed(unit.drag_speed())
            .range(unit.from_mm(0.0)..=unit.from_mm(100.0))
            .max_decimals(unit.decimals()),
    );
    if resp.changed() {
        app.doc.canvases[ci].layout.gutter_mm = unit.to_mm(value).clamp(0.0, 100.0);
        app.doc.dirty = true;
    }
    handle_layout_response(app, ci, &resp, before);
}

pub(super) fn grid_count_drag(app: &mut PlotxApp, ci: usize, ui: &mut Ui, rows: bool) {
    let before = app.doc.canvases[ci].layout;
    let mut value = if rows { before.rows } else { before.cols };
    let resp = ui.add(egui::DragValue::new(&mut value).speed(0.1).range(1..=12));
    if resp.changed() {
        let value = value.clamp(1, 12);
        if rows {
            app.doc.canvases[ci].layout.rows = value;
        } else {
            app.doc.canvases[ci].layout.cols = value;
        }
        app.doc.dirty = true;
    }
    handle_layout_response(app, ci, &resp, before);
}

pub(super) fn panel_note_edit_window(app: &mut PlotxApp, ctx: &egui::Context) {
    let Some(edit) = app.session.ui.panel_note_edit.as_ref() else {
        return;
    };
    let ci = edit.canvas;
    let object_id = edit.object;
    if ci >= app.doc.canvases.len()
        || app.doc.canvases[ci]
            .object(object_id)
            .and_then(|object| object.plot())
            .is_none()
    {
        app.session.ui.panel_note_edit = None;
        app.session.ui.selection = Selection::None;
        return;
    }

    let mut open = true;
    let mut save = false;
    let mut delete = false;
    let mut cancel = false;
    egui::Window::new("Edit panel note")
        .collapsible(false)
        .resizable(false)
        .open(&mut open)
        .show(ctx, |ui| {
            let Some(edit) = app.session.ui.panel_note_edit.as_mut() else {
                return;
            };
            let resp = ui.add(
                egui::TextEdit::multiline(&mut edit.buffer)
                    .desired_width(320.0)
                    .desired_rows(3),
            );
            if edit.focus {
                resp.request_focus();
                edit.focus = false;
            }
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ui.button("Save").clicked() {
                    save = true;
                }
                if ui.button("Clear").clicked() {
                    delete = true;
                }
                if ui.button("Cancel").clicked() {
                    cancel = true;
                }
            });
        });

    if save {
        let buffer = app
            .session
            .ui
            .panel_note_edit
            .as_ref()
            .map(|edit| edit.buffer.trim().to_owned())
            .unwrap_or_default();
        if let Some(before) = app.doc.canvases[ci]
            .object(object_id)
            .and_then(|object| object.plot())
            .map(|plot| plot.panel.clone())
        {
            let mut after = before.clone();
            after.user_note = buffer;
            app.execute_action(Action::set_panel_meta(ci, object_id, before, after));
            app.select_panel_label(ci, object_id);
            app.session.status = "Panel note updated.".to_owned();
        }
        app.session.ui.panel_note_edit = None;
    } else if delete {
        if let Some(before) = app.doc.canvases[ci]
            .object(object_id)
            .and_then(|object| object.plot())
            .map(|plot| plot.panel.clone())
        {
            let mut after = before.clone();
            after.user_note.clear();
            app.execute_action(Action::set_panel_meta(ci, object_id, before, after));
            app.session.status = "Panel note cleared.".to_owned();
        }
        app.session.ui.panel_note_edit = None;
        app.select_object(ci, object_id);
    } else if cancel || !open {
        app.session.ui.panel_note_edit = None;
    }
}

pub(super) fn text_edit_window(app: &mut PlotxApp, ctx: &egui::Context) {
    let Some(edit) = app.session.ui.text_edit.as_ref() else {
        return;
    };
    let ci = edit.canvas;
    let object_id = edit.object;
    if ci >= app.doc.canvases.len()
        || app.doc.canvases[ci]
            .object(object_id)
            .and_then(|object| object.text())
            .is_none()
    {
        app.session.ui.text_edit = None;
        return;
    }

    let mut open = true;
    let mut save = false;
    let mut delete = false;
    let mut cancel = false;
    egui::Window::new("Edit text")
        .collapsible(false)
        .resizable(false)
        .open(&mut open)
        .show(ctx, |ui| {
            let Some(edit) = app.session.ui.text_edit.as_mut() else {
                return;
            };
            let resp = ui.add(
                egui::TextEdit::multiline(&mut edit.buffer)
                    .desired_width(320.0)
                    .desired_rows(3),
            );
            if edit.focus {
                resp.request_focus();
                edit.focus = false;
            }
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ui.button("Save").clicked() {
                    save = true;
                }
                if ui.button("Delete").clicked() {
                    delete = true;
                }
                if ui.button("Cancel").clicked() {
                    cancel = true;
                }
            });
        });

    if save {
        let buffer = app
            .session
            .ui
            .text_edit
            .as_ref()
            .map(|edit| edit.buffer.trim().to_owned())
            .unwrap_or_default();
        if let Some(before) = app.doc.canvases[ci]
            .object(object_id)
            .and_then(|object| object.text())
            .cloned()
        {
            let mut after = before.clone();
            if !buffer.is_empty() {
                after.text = buffer;
            }
            app.execute_action(Action::set_object_text(ci, object_id, before, after));
            app.select_object(ci, object_id);
            app.session.status = "Text updated.".to_owned();
        }
        app.session.ui.text_edit = None;
    } else if delete {
        if let Some(action) = Action::delete_object(app, ci, object_id) {
            app.execute_action(action);
        }
        app.session.ui.text_edit = None;
        app.session.status = "Object deleted.".to_owned();
    } else if cancel || !open {
        app.session.ui.text_edit = None;
    }
}
