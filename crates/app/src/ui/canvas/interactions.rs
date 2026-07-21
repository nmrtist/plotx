use super::*;

pub(crate) fn handle_view_interactions(
    app: &mut PlotxApp,
    ci: usize,
    object_id: ObjectId,
    _rect: egui::Rect,
    plot: PlotRect,
    ui: &Ui,
    _resp: &egui::Response,
) {
    // Scroll-zoom, pinch and double-click reset are handled ambiently for the
    // panel under the cursor (see `handle_navigation`), regardless of tool. This
    // verb owns only the rubber-band box-zoom drag.
    let esc = ui.input(|i| i.key_pressed(egui::Key::Escape));
    if esc
        && matches!(
            app.interaction(),
            Interaction::Zoom(_) | Interaction::Selection(_)
        )
    {
        app.reset_interaction();
    }

    handle_zoom_drag(app, ci, object_id, plot, ui);
}

pub(crate) fn handle_selection_drag(
    app: &mut PlotxApp,
    ci: usize,
    object_id: ObjectId,
    dataset: usize,
    plot: PlotRect,
    ui: &Ui,
) {
    let (hover, primary_down, primary_pressed, primary_released, esc) = ui.input(|i| {
        (
            i.pointer.hover_pos(),
            i.pointer.primary_down(),
            i.pointer.primary_pressed(),
            i.pointer.primary_released(),
            i.key_pressed(egui::Key::Escape),
        )
    });

    if esc {
        app.clear_analysis_selection();
        return;
    }

    if let Interaction::Selection(drag) = &mut app.session.ui.interaction {
        if drag.canvas != ci || drag.object != object_id {
            return;
        }
        if let Some(p) = hover {
            drag.current = [p.x, p.y];
        }
        if (primary_released || !primary_down)
            && let Interaction::Selection(drag) = app.take_interaction()
        {
            finish_selection_drag(app, ci, object_id, dataset, plot, drag);
        }
        return;
    }

    if primary_pressed
        && let Some(p) = hover
        && plot_contains(plot, p)
    {
        freeze_board_for_gesture(app);
        app.begin_interaction(Interaction::Selection(SelectionDrag {
            canvas: ci,
            object: object_id,
            dataset,
            start: [p.x, p.y],
            current: [p.x, p.y],
        }));
    }
}

pub(crate) fn finish_selection_drag(
    app: &mut PlotxApp,
    ci: usize,
    object_id: ObjectId,
    dataset: usize,
    plot: PlotRect,
    drag: SelectionDrag,
) {
    if drag.dataset != dataset {
        return;
    }
    let a = clamp_to_plot(plot, pos(drag.start));
    let b = clamp_to_plot(plot, pos(drag.current));
    if (a.x - b.x).abs() < SELECT_MIN_PX {
        return;
    }

    let object = app.doc.canvases[ci].object(object_id).unwrap();
    let plot_object = object.plot().unwrap();
    let fig = &plot_object.figure;
    let x = AxisRange::new(
        screen_to_x(a.x, plot, fig.x.min, fig.x.span(), fig.x.reversed),
        screen_to_x(b.x, plot, fig.x.min, fig.x.span(), fig.x.reversed),
    );
    let y = ((a.y - b.y).abs() >= SELECT_MIN_PX).then(|| {
        AxisRange::new(
            screen_to_y(a.y, plot, fig.y.min, fig.y.span(), fig.y.reversed),
            screen_to_y(b.y, plot, fig.y.min, fig.y.span(), fig.y.reversed),
        )
    });

    app.session.ui.analysis_selection = Some(AnalysisSelection {
        dataset,
        canvas: ci,
        object: object_id,
        x_range: x,
        y_range: y,
    });
    app.session.status = format!("Selected {:.3}-{:.3} ppm.", x.min, x.max);
}

pub(crate) fn handle_zoom_drag(
    app: &mut PlotxApp,
    ci: usize,
    object_id: ObjectId,
    plot: PlotRect,
    ui: &Ui,
) {
    let (hover, primary_down, primary_pressed, primary_released) = ui.input(|i| {
        (
            i.pointer.hover_pos(),
            i.pointer.primary_down(),
            i.pointer.primary_pressed(),
            i.pointer.primary_released(),
        )
    });

    // Axis-strip zooms are owned start-to-finish by the ambient navigation layer;
    // this in-body handler drives only the box zoom.
    if let Interaction::Zoom(drag) = &mut app.session.ui.interaction {
        if drag.canvas != ci || drag.object != object_id || drag.axis != ZoomAxis::Box {
            return;
        }
        if let Some(p) = hover {
            drag.current = [p.x, p.y];
        }
        if (primary_released || !primary_down)
            && let Interaction::Zoom(drag) = app.take_interaction()
        {
            finish_zoom_drag(app, ci, object_id, plot, drag);
        }
        return;
    }

    if primary_pressed
        && let Some(p) = hover
        && plot_contains(plot, p)
    {
        freeze_board_for_gesture(app);
        app.begin_interaction(Interaction::Zoom(ZoomDrag {
            canvas: ci,
            object: object_id,
            start: [p.x, p.y],
            current: [p.x, p.y],
            axis: ZoomAxis::Box,
        }));
    }
}

pub(crate) fn finish_zoom_drag(
    app: &mut PlotxApp,
    ci: usize,
    object_id: ObjectId,
    plot: PlotRect,
    drag: ZoomDrag,
) {
    let a = clamp_to_plot(plot, pos(drag.start));
    let b = clamp_to_plot(plot, pos(drag.current));
    let width = (a.x - b.x).abs();
    let height = (a.y - b.y).abs();
    if width < SELECT_MIN_PX && height < SELECT_MIN_PX {
        return;
    }

    let object = app.doc.canvases[ci].object(object_id).unwrap();
    let plot_object = object.plot().unwrap();
    let fig = &plot_object.figure;
    let before = plot_object.viewport.clone();
    let x = if width >= SELECT_MIN_PX {
        Some(AxisRange::new(
            screen_to_x(a.x, plot, fig.x.min, fig.x.span(), fig.x.reversed),
            screen_to_x(b.x, plot, fig.x.min, fig.x.span(), fig.x.reversed),
        ))
    } else {
        None
    };
    let y = if height >= SELECT_MIN_PX {
        Some(AxisRange::new(
            screen_to_y(a.y, plot, fig.y.min, fig.y.span(), fig.y.reversed),
            screen_to_y(b.y, plot, fig.y.min, fig.y.span(), fig.y.reversed),
        ))
    } else {
        None
    };

    let mut after = before.clone();
    after.select(fig, x, y);
    app.execute_action(Action::set_object_viewport(ci, object_id, before, after));
    app.session.status = "Zoomed selection.".into();
}

pub(crate) fn ensure_board_view(app: &mut PlotxApp, rect: egui::Rect) {
    // A gesture owns the viewport: never re-fit under an active drag, even if a
    // fit shortcut re-armed `auto_fit` mid-gesture (see `freeze_board_for_gesture`).
    if app.session.ui.gesture_active() {
        return;
    }
    if !app.session.board.auto_fit {
        return;
    }
    if let Some(bbox) = all_frames_bbox(app) {
        app.session.board = board_fit_bbox_with_chrome(bbox, rect);
    }
}

/// Single-click select only — the actual data drag is owned by the data block's
/// per-tool handler.
pub(crate) fn handle_data_tool_target(
    app: &mut PlotxApp,
    ci: usize,
    rect: egui::Rect,
    ui: &Ui,
    _resp: &egui::Response,
) {
    let (hover, primary_pressed) =
        ui.input(|i| (i.pointer.hover_pos(), i.pointer.primary_pressed()));
    if !primary_pressed {
        return;
    }
    let Some(screen_pos) = hover.filter(|p| rect.contains(*p)) else {
        return;
    };
    let Some(hit) =
        screen_to_page_unbounded(app.session.board, &app.doc.canvases[ci], rect, screen_pos)
            .and_then(|p| hit_object(&app.doc.canvases[ci], p, app.session.board.zoom))
    else {
        return;
    };
    let id = hit.object;
    if app.doc.canvases[ci].selected_object == Some(id) {
        return;
    }
    app.select_object(ci, id);
    if app.doc.canvases[ci]
        .object(id)
        .and_then(|o| o.plot())
        .is_some()
    {
        select_object_datasets(app, ci, id);
    }
}

pub(crate) fn handle_object_interactions(
    app: &mut PlotxApp,
    ci: usize,
    rect: egui::Rect,
    ui: &Ui,
    _resp: &egui::Response,
) {
    let (hover, primary_down, primary_pressed, primary_released, shift) = ui.input(|i| {
        (
            i.pointer.hover_pos(),
            i.pointer.primary_down(),
            i.pointer.primary_pressed(),
            i.pointer.primary_released(),
            i.modifiers.shift,
        )
    });

    if primary_pressed {
        let Some(screen_pos) = hover else {
            return;
        };
        let page_pos =
            screen_to_page_unbounded(app.session.board, &app.doc.canvases[ci], rect, screen_pos);
        let hit = page_pos.and_then(|page_pos| {
            hit_object(&app.doc.canvases[ci], page_pos, app.session.board.zoom)
        });

        if let Some(hit) = hit {
            let id = hit.object;
            if shift {
                app.toggle_object_selection(ci, id);
            } else {
                let keep_group = app.session.ui.selection.objects().len() > 1
                    && app.session.ui.selection.contains(id);
                if !keep_group {
                    app.select_object(ci, id);
                }
                if matches!(app.interaction(), Interaction::PanelLabel(_)) {
                    app.reset_interaction();
                }
                select_object_datasets(app, ci, id);
                if let Some(object) = app.doc.canvases[ci].object(id).filter(|o| !o.locked) {
                    let before = object.frame;
                    let start = page_pos.map(|p| [p.x, p.y]).unwrap_or([before.x, before.y]);
                    let others = if matches!(hit.kind, ObjectDragKind::Move) {
                        app.session
                            .ui
                            .selection
                            .objects()
                            .iter()
                            .copied()
                            .filter(|&oid| oid != id)
                            .filter_map(|oid| {
                                app.doc.canvases[ci]
                                    .object(oid)
                                    .filter(|o| !o.locked)
                                    .map(|o| (oid, o.frame))
                            })
                            .collect()
                    } else {
                        Vec::new()
                    };
                    freeze_board_for_gesture(app);
                    app.begin_interaction(Interaction::Object(ObjectDrag {
                        canvas: ci,
                        object: id,
                        kind: hit.kind,
                        before,
                        start_pointer: start,
                        start_pointer_screen: [screen_pos.x, screen_pos.y],
                        others,
                        active: matches!(hit.kind, ObjectDragKind::Resize(_)),
                    }));
                }
            }
        } else if rect.contains(screen_pos)
            && !page_screen_rect(app.session.board, &app.doc.canvases[ci], rect)
                .contains(screen_pos)
        {
            // An empty press on the board outside any page body clears the
            // selection; a press over the side bars/toolbar (global pointer, no
            // object hit) must not.
            clear_canvas_interaction_state(app, ci, CanvasInteractionClearScope::Selection);
            app.session.status = "Selection cleared.".to_owned();
        } else if let Some(p) = page_pos.filter(|_| {
            page_screen_rect(app.session.board, &app.doc.canvases[ci], rect).contains(screen_pos)
        }) {
            // Marquee is scoped to the frame it begins in: only start when the
            // press lands inside this page's body, never on empty board.
            freeze_board_for_gesture(app);
            app.begin_interaction(Interaction::Marquee(MarqueeDrag {
                canvas: ci,
                start: [p.x, p.y],
                current: [p.x, p.y],
                additive: shift,
            }));
        }
    }

    let object_drag = match &app.session.ui.interaction {
        Interaction::Object(d) if d.canvas == ci => Some(d.clone()),
        _ => None,
    };
    if let Some(drag) = object_drag {
        let mut active = drag.active;
        if primary_down
            && let Some(screen_now) = hover
            && let Some(pointer_page) =
                screen_to_page_unbounded(app.session.board, &app.doc.canvases[ci], rect, screen_now)
        {
            let dpx = pointer_page.x - drag.start_pointer[0];
            let dpy = pointer_page.y - drag.start_pointer[1];
            // Dead-zone is screen-space pointer travel, not page displacement: the
            // frozen viewport makes these equal today, but measuring intent in
            // input space keeps a click from becoming a drag even if the transform
            // ever shifts under the cursor.
            let dsx = screen_now.x - drag.start_pointer_screen[0];
            let dsy = screen_now.y - drag.start_pointer_screen[1];
            active |= dsx.hypot(dsy) > DRAG_START_PX;
            if let Interaction::Object(d) = &mut app.session.ui.interaction {
                d.active = active;
            }
            if active {
                if update_tile_drop(app, ci, rect, &drag, hover) {
                    app.session.ui.snap_guides.clear();
                } else {
                    let candidate = drag_frame(drag.before, drag.kind, dpx, dpy);
                    let (snapped, guides) = snap_object_frame(app, ci, &drag, candidate, ui);
                    let applied = [snapped.x - drag.before.x, snapped.y - drag.before.y];
                    if let Some(object) = app.doc.canvases[ci].object_mut(drag.object) {
                        object.frame = snapped;
                    }
                    for &(oid, before) in &drag.others {
                        if let Some(o) = app.doc.canvases[ci].object_mut(oid) {
                            o.frame = ObjectFrame::new(
                                before.x + applied[0],
                                before.y + applied[1],
                                before.width,
                                before.height,
                            );
                        }
                    }
                    app.session.ui.snap_guides = guides;
                }
            }
        }
        if primary_released || !primary_down {
            app.session.ui.snap_guides.clear();
            if let Interaction::Object(drag) = app.take_interaction() {
                if let Some(preview) = app.session.ui.tile_drop.take() {
                    commit_tile_drop(app, ci, drag, preview);
                } else if active {
                    finish_object_drag(app, ci, drag);
                }
            }
        }
    }

    if matches!(&app.session.ui.interaction, Interaction::Marquee(m) if m.canvas == ci) {
        if primary_down
            && let Some(p) = hover.and_then(|p| {
                screen_to_page_unbounded(app.session.board, &app.doc.canvases[ci], rect, p)
            })
            && let Interaction::Marquee(m) = &mut app.session.ui.interaction
        {
            m.current = [p.x, p.y];
        }
        if (primary_released || !primary_down)
            && let Interaction::Marquee(marq) = app.take_interaction()
        {
            finish_marquee(app, ci, marq);
        }
    }
}

const MARQUEE_CLICK_PT: f32 = 3.0;

fn finish_marquee(app: &mut PlotxApp, ci: usize, marq: MarqueeDrag) {
    let dx = (marq.current[0] - marq.start[0]).abs();
    let dy = (marq.current[1] - marq.start[1]).abs();
    if dx < MARQUEE_CLICK_PT && dy < MARQUEE_CLICK_PT {
        if !marq.additive {
            clear_canvas_interaction_state(app, ci, CanvasInteractionClearScope::Selection);
            app.session.status = "Selection cleared.".to_owned();
        }
        return;
    }
    let min_x = marq.start[0].min(marq.current[0]);
    let max_x = marq.start[0].max(marq.current[0]);
    let min_y = marq.start[1].min(marq.current[1]);
    let max_y = marq.start[1].max(marq.current[1]);
    let hits: Vec<ObjectId> = app.doc.canvases[ci]
        .objects
        .iter()
        .filter(|o| o.visible)
        .filter(|o| {
            let f = o.frame;
            max_x >= f.x && min_x <= f.x + f.width && max_y >= f.y && min_y <= f.y + f.height
        })
        .map(|o| o.id)
        .collect();
    app.set_page_selection(ci, &hits, marq.additive);
    app.session.status = format!(
        "Selected {} object(s).",
        app.session.ui.selection.objects().len()
    );
}

fn select_object_datasets(app: &mut PlotxApp, ci: usize, id: ObjectId) {
    let Some(object) = app.doc.canvases[ci].object(id) else {
        return;
    };
    let active = object.dataset();
    let datasets = object.dataset_indices();
    if !datasets.is_empty() {
        app.focus_datasets(&datasets, active);
    } else {
        app.set_active_dataset(active);
    }
}

pub(crate) fn arrange_context_menu(app: &mut PlotxApp, ci: usize, ui: &mut Ui) {
    if ui.button("Copy figure").clicked() {
        let ctx = ui.ctx().clone();
        crate::ui::clipboard_figure::copy_canvas_figure(app, &ctx, ci);
        ui.close();
    }
    frame_zoom_menu(app, ui);
    ui.menu_button("Arrange into grid", |ui| {
        for &(label, rows, cols) in layout::GRID_PRESETS {
            if ui.button(label).clicked() {
                app.arrange_active_canvas_grid(rows, cols);
                ui.close();
            }
        }
    });
    if !app.session.ui.selection.objects().is_empty() {
        ui.menu_button("Order", |ui| {
            for (label, op) in [
                ("Bring to Front", plotx_core::actions::ZOrder::Front),
                ("Bring Forward", plotx_core::actions::ZOrder::Forward),
                ("Send Backward", plotx_core::actions::ZOrder::Backward),
                ("Send to Back", plotx_core::actions::ZOrder::Back),
            ] {
                if ui.button(label).clicked() {
                    app.z_order_selected(op);
                    ui.close();
                }
            }
        });
        let ids: Vec<ObjectId> = app.session.ui.selection.objects().to_vec();
        let others = crate::ui::menus::other_canvas_destinations(app, ci);
        let mut picked = None;
        crate::ui::menus::transfer_to_canvas_menu(
            ui,
            &others,
            "Move selection to canvas",
            "Copy selection to canvas",
            &mut picked,
        );
        if let Some((to, is_move)) = picked {
            app.transfer_objects_to_canvas(ci, &ids, to, is_move);
        }
    }
    ui.separator();
    let mut show_grid = app.doc.canvases[ci].layout.show_grid;
    if ui.checkbox(&mut show_grid, "Show layout grid").clicked() {
        app.set_show_grid(ci, show_grid);
    }
    let mut snap = app.session.ui.snap_enabled;
    if ui.checkbox(&mut snap, "Snap to grid & objects").clicked() {
        app.set_snap_enabled(snap);
    }
    ui.separator();
    if ui.button("Canvas settings…").clicked() {
        app.session.ui.canvas_settings = Some(ci);
        ui.close();
    }
}

pub(crate) fn finish_object_drag(app: &mut PlotxApp, ci: usize, drag: ObjectDrag) {
    if drag.others.is_empty() {
        if let Some(object) = app.doc.canvases[ci].object(drag.object) {
            app.execute_action(Action::move_resize_object(
                ci,
                drag.object,
                drag.before,
                object.frame,
            ));
        }
        return;
    }
    let mut before = vec![(drag.object, drag.before)];
    before.extend(drag.others.iter().copied());
    let after: Vec<(ObjectId, ObjectFrame)> = before
        .iter()
        .filter_map(|&(id, _)| app.doc.canvases[ci].object(id).map(|o| (id, o.frame)))
        .collect();
    app.execute_action(Action::set_object_frames(ci, before, after));
}
