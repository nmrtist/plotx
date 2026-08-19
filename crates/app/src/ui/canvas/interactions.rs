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
    if app.plot_rejects_legacy_selection(ci, object_id)
        && app.plot_interaction_descriptor(ci, object_id).is_none()
    {
        return;
    }
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

    if let Some(descriptor) = app.plot_interaction_descriptor(ci, object_id) {
        let object = app.doc.canvases[ci].object(object_id).unwrap();
        let figure = object.plot().unwrap().figure();
        let start = screen_to_x(a.x, plot, figure.x.min, figure.x.span(), figure.x.reversed);
        let end = screen_to_x(b.x, plot, figure.x.min, figure.x.span(), figure.x.reversed);
        if let Some(request) = descriptor.range(start, end) {
            app.dispatch_plot_interaction(request);
        }
        return;
    }

    if app.plot_rejects_legacy_selection(ci, object_id) {
        return;
    }
    let object = app.doc.canvases[ci].object(object_id).unwrap();
    let plot_object = object.plot().unwrap();
    let fig = plot_object.figure();
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
        dataset: app.doc.datasets[dataset].resource_id(),
        canvas: app.doc.canvases[ci].resource_id,
        object: object_id,
        x_range: x,
        y_range: y,
        field: None,
        unit: None,
        source_stream: None,
    });
    let unit = if app.doc.datasets[dataset].as_mass_spec().is_some() {
        "min"
    } else {
        "ppm"
    };
    app.session.status = format!("Selected {:.3}-{:.3} {unit}.", x.min, x.max);
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
    let fig = plot_object.figure();
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
    app.commit_object_viewport(ci, object_id, before, after);
    app.session.status = "Zoomed selection.".into();
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
    if let Some(descriptor) = app.plot_interaction_descriptor(ci, id)
        && let Some(inner) = plot_inner_rect(app, ci, id, rect)
        && plot_contains(inner, screen_pos)
        && let Some(figure) = app.doc.canvases[ci]
            .object(id)
            .and_then(|object| object.plot())
            .map(|plot| plot.figure())
        && let Some(request) = descriptor.cursor(screen_to_x(
            screen_pos.x,
            inner,
            figure.x.min,
            figure.x.span(),
            figure.x.reversed,
        ))
    {
        app.dispatch_plot_interaction(request);
    }
    if app.doc.canvases[ci].selected_object == Some(id) {
        return;
    }
    app.select_object(ci, id);
    if app.doc.canvases[ci]
        .object(id)
        .and_then(|o| o.plot())
        .is_some()
    {
        app.focus_object_datasets(ci, id);
    }
}

pub(crate) fn handle_object_interactions(
    app: &mut PlotxApp,
    ci: usize,
    rect: egui::Rect,
    ui: &Ui,
    resp: &egui::Response,
) {
    let (hover, primary_down, primary_pressed, primary_released, shift, command, alt, esc, focused) =
        ui.input(|i| {
            (
                i.pointer.hover_pos(),
                i.pointer.primary_down(),
                i.pointer.primary_pressed(),
                i.pointer.primary_released(),
                i.modifiers.shift,
                i.modifiers.command,
                i.modifiers.alt,
                i.key_pressed(egui::Key::Escape),
                i.focused,
            )
        });

    if (esc || !focused)
        && matches!(
            app.interaction(),
            Interaction::Object(_) | Interaction::Panel(_)
        )
    {
        app.cancel_interaction();
        return;
    }

    if primary_pressed {
        let Some(screen_pos) = hover else {
            return;
        };
        let page_pos =
            screen_to_page_unbounded(app.session.board, &app.doc.canvases[ci], rect, screen_pos);
        let editing_panel = app
            .session
            .ui
            .hierarchical_selection
            .editing_panel()
            .filter(|(canvas, _)| *canvas == app.doc.canvases[ci].resource_id)
            .map(|(_, panel)| panel);

        // Page scope owns the Panel frame. This is deliberately checked before
        // content hit-testing so a child can never steal the parent drag.
        if editing_panel.is_none()
            && !alt
            && let Some(panel_hit) = page_pos
                .and_then(|point| hit_panel(&app.doc.canvases[ci], point, app.session.board.zoom))
        {
            app.session.ui.selection_scope = plotx_core::state::SelectionScope::CanvasObjects;
            if shift || command {
                if let Err(reason) = app.toggle_panel_sibling(ci, panel_hit.panel) {
                    app.session.status = reason.to_owned();
                }
            } else {
                app.select_panel(ci, panel_hit.panel);
                if resp.double_clicked() {
                    app.enter_panel(ci, panel_hit.panel);
                } else if app.doc.canvases[ci]
                    .panel(panel_hit.panel)
                    .is_some_and(|panel| !panel.locked)
                {
                    begin_panel_drag(
                        app,
                        ci,
                        panel_hit.panel,
                        panel_hit.kind,
                        page_pos,
                        screen_pos,
                        false,
                    );
                }
            }
            return;
        }
        let hit = page_pos.and_then(|page_pos| {
            if editing_panel.is_some() || alt || command || resp.double_clicked() {
                let hits = hit_content_objects(
                    &app.doc.canvases[ci],
                    page_pos,
                    app.session.board.zoom,
                    editing_panel,
                );
                if !alt {
                    return hits.into_iter().next();
                }
                let current = app
                    .session
                    .ui
                    .hierarchical_selection
                    .lead()
                    .and_then(|path| path.content);
                let next = current
                    .and_then(|id| hits.iter().position(|hit| hit.object == id))
                    .map_or(0, |index| (index + 1) % hits.len().max(1));
                return hits.get(next).copied();
            }
            if !alt {
                return hit_object(&app.doc.canvases[ci], page_pos, app.session.board.zoom);
            }
            let hits = hit_objects(&app.doc.canvases[ci], page_pos, app.session.board.zoom);
            let current = app
                .session
                .ui
                .hierarchical_selection
                .lead()
                .and_then(|path| path.content);
            let next = current
                .and_then(|id| hits.iter().position(|hit| hit.object == id))
                .map_or(0, |index| (index + 1) % hits.len().max(1));
            hits.get(next).copied()
        });

        if let Some(hit) = hit {
            let id = hit.object;
            let parent = app.doc.canvases[ci].parent_panel(id);
            if alt || command || (resp.double_clicked() && parent.is_some()) {
                app.session.ui.selection_scope = plotx_core::state::SelectionScope::CanvasObjects;
                app.select_content(ci, id);
            } else if shift {
                app.session.ui.selection_scope = plotx_core::state::SelectionScope::CanvasObjects;
                let editing_parent = app.session.ui.hierarchical_selection.editing_panel()
                    == parent.map(|panel| (app.doc.canvases[ci].resource_id, panel));
                let result = if let Some(panel) = parent.filter(|_| !editing_parent) {
                    app.toggle_panel_sibling(ci, panel)
                } else {
                    app.toggle_content_sibling(ci, id)
                };
                if let Err(reason) = result {
                    app.session.status = reason.to_owned();
                }
            } else {
                let keep_group = app.session.ui.selection.objects().len() > 1
                    && app.session.ui.selection.contains(id);
                if !keep_group {
                    app.session.ui.selection_scope =
                        plotx_core::state::SelectionScope::CanvasObjects;
                    let editing_parent = app.session.ui.hierarchical_selection.editing_panel()
                        == parent.map(|panel| (app.doc.canvases[ci].resource_id, panel));
                    if let Some(panel) = parent.filter(|_| !editing_parent) {
                        app.select_panel(ci, panel);
                    } else {
                        app.select_object(ci, id);
                    }
                }
                if matches!(app.interaction(), Interaction::PanelLabel(_)) {
                    app.reset_interaction();
                }
                app.focus_object_datasets(ci, id);
                let editable = parent
                    .and_then(|panel| app.doc.canvases[ci].panel(panel))
                    .map_or_else(
                        || {
                            app.doc.canvases[ci]
                                .object(id)
                                .is_some_and(|object| !object.locked)
                        },
                        |panel| {
                            !panel.locked
                                && app.doc.canvases[ci]
                                    .object(id)
                                    .is_some_and(|object| !object.locked)
                        },
                    );
                if editable {
                    let space = parent
                        .filter(|panel| editing_panel == Some(*panel))
                        .map(ObjectDragSpace::Panel)
                        .unwrap_or(ObjectDragSpace::Page);
                    if parent.is_some() && matches!(space, ObjectDragSpace::Page) {
                        // A page-scope child is represented by its Panel. The
                        // Panel hit branch normally returned above; this guard
                        // keeps an out-of-bounds child from falling through to a
                        // misleading content drag.
                        return;
                    }
                    let Some(before) = (if matches!(space, ObjectDragSpace::Panel(_)) {
                        app.doc.canvases[ci].object(id).map(|object| object.frame)
                    } else {
                        app.doc.canvases[ci].layout_frame(id)
                    }) else {
                        return;
                    };
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
                                    .and_then(|_| {
                                        let frame = if matches!(space, ObjectDragSpace::Panel(_)) {
                                            app.doc.canvases[ci]
                                                .object(oid)
                                                .map(|object| object.frame)
                                        } else {
                                            app.doc.canvases[ci].layout_frame(oid)
                                        }?;
                                        Some((oid, frame))
                                    })
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
                        space,
                    }));
                }
            }
        } else if rect.contains(screen_pos)
            && !page_screen_rect(app.session.board, &app.doc.canvases[ci], rect)
                .contains(screen_pos)
        {
            app.session.ui.selection_scope = plotx_core::state::SelectionScope::Board;
            // An empty press on the board outside any page body clears the
            // selection; a press over the side bars/toolbar (global pointer, no
            // object hit) must not.
            clear_canvas_interaction_state(app, ci, CanvasInteractionClearScope::Selection);
            app.session.status = "Selection cleared.".to_owned();
        } else if let Some(p) = page_pos.filter(|_| {
            page_screen_rect(app.session.board, &app.doc.canvases[ci], rect).contains(screen_pos)
        }) {
            app.session.ui.selection_scope = plotx_core::state::SelectionScope::CanvasObjects;
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
    handle_panel_drag(app, ci, rect, hover, primary_down, primary_released, alt);
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
                if let Some(source) = tile_source_for_object(app, &drag)
                    && update_tile_drop(app, ci, rect, source, hover)
                {
                    app.session.ui.snap_guides.clear();
                } else {
                    let candidate = drag_frame(drag.before, drag.kind, dpx, dpy);
                    let (mut snapped, guides) = if let ObjectDragSpace::Panel(panel) = drag.space {
                        snap_panel_content_frame(app, ci, panel, &drag, candidate, ui)
                    } else {
                        snap_object_frame(app, ci, &drag, candidate, ui)
                    };
                    let preserve_aspect = app.doc.canvases[ci].parent_panel(drag.object).is_some()
                        || app.doc.canvases[ci]
                            .object(drag.object)
                            .and_then(|item| match &item.kind {
                                CanvasObjectKind::RasterImage(image) => Some(image.preserve_aspect),
                                _ => None,
                            })
                            .unwrap_or(false);
                    if preserve_aspect {
                        snapped = preserve_aspect_frame(drag.before, snapped, drag.kind);
                    }
                    let applied = [snapped.x - drag.before.x, snapped.y - drag.before.y];
                    if matches!(drag.space, ObjectDragSpace::Panel(_)) {
                        if let Some(object) = app.doc.canvases[ci].object_mut(drag.object) {
                            object.frame = snapped;
                        }
                    } else {
                        app.doc.canvases[ci].set_layout_frame(drag.object, snapped);
                    }
                    for &(oid, before) in &drag.others {
                        let frame = ObjectFrame::new(
                            before.x + applied[0],
                            before.y + applied[1],
                            before.width,
                            before.height,
                        );
                        if matches!(drag.space, ObjectDragSpace::Panel(_)) {
                            if let Some(object) = app.doc.canvases[ci].object_mut(oid) {
                                object.frame = frame;
                            }
                        } else {
                            app.doc.canvases[ci].set_layout_frame(oid, frame);
                        }
                    }
                    update_content_drop_target(app, ci, &drag, pointer_page);
                    app.session.ui.snap_guides = guides;
                }
            }
        }
        if primary_released || !primary_down {
            app.session.ui.snap_guides.clear();
            if let Interaction::Object(drag) = app.take_interaction() {
                if let Some(preview) = app.session.ui.tile_drop.take() {
                    if let Some(source) = tile_source_for_object(app, &drag) {
                        commit_tile_drop(app, source, preview, alt);
                    } else {
                        finish_object_drag(app, ci, drag);
                    }
                } else if active {
                    let target = app.session.ui.panel_drop_target.take();
                    if target.is_some() || matches!(drag.space, ObjectDragSpace::Panel(_)) {
                        finish_content_drag(app, ci, drag, target);
                    } else {
                        finish_object_drag(app, ci, drag);
                    }
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
    if ui.button("Simplify inner axes").clicked() {
        app.simplify_inner_axes();
        ui.close();
    }
    ui.menu_button("Spacing basis", |ui| {
        for (label, mode) in [
            ("Frame", layout::SpacingMode::Frame),
            ("Visual", layout::SpacingMode::Visual),
        ] {
            let checked = app.doc.canvases[ci].layout.spacing_mode == mode;
            if ui.selectable_label(checked, label).clicked() {
                app.set_spacing_mode(mode);
                ui.close();
            }
        }
    });
    ui.menu_button("Minimum spacing", |ui| {
        for preset in layout::GutterPreset::ALL {
            let checked =
                (app.doc.canvases[ci].layout.gutter_mm - preset.millimetres()).abs() < 0.001;
            if ui
                .selectable_label(
                    checked,
                    format!("{} ({} mm)", preset.label(), preset.millimetres()),
                )
                .clicked()
            {
                app.set_gutter_preset(preset);
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
    let mut snap = app.settings.general.snap_enabled;
    if ui.checkbox(&mut snap, "Snap objects & frames").clicked() {
        app.set_snap_enabled(snap);
    }
    // Channel 4: whatever the selection draws, its settings are one click from
    // here. Navigation only — the entries jump to the panel section that owns
    // the controls, and are derived from the catalog rather than listed again.
    crate::ui::properties::discovery::context_menu(app, ui);
    ui.separator();
    if ui.button("Canvas settings…").clicked() {
        app.session.ui.canvas_settings = Some(ci);
        ui.close();
    }
}

pub(crate) fn finish_object_drag(app: &mut PlotxApp, ci: usize, drag: ObjectDrag) {
    if drag.others.is_empty() {
        if let Some(after) = app.doc.canvases[ci].layout_frame(drag.object) {
            app.execute_action(Action::move_resize_object(
                ci,
                drag.object,
                drag.before,
                after,
            ));
        }
        return;
    }
    let mut before = vec![(drag.object, drag.before)];
    before.extend(drag.others.iter().copied());
    let after: Vec<(ObjectId, ObjectFrame)> = before
        .iter()
        .filter_map(|&(id, _)| {
            app.doc.canvases[ci]
                .layout_frame(id)
                .map(|frame| (id, frame))
        })
        .collect();
    app.execute_action(Action::set_object_frames(ci, before, after));
}
