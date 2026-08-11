use super::*;

pub(crate) fn handle_panel_drag(
    app: &mut PlotxApp,
    ci: usize,
    rect: egui::Rect,
    hover: Option<Pos2>,
    primary_down: bool,
    primary_released: bool,
    alt: bool,
) {
    let Some(drag) = (match &app.session.ui.interaction {
        Interaction::Panel(d) if d.canvas == ci => Some(d.clone()),
        _ => None,
    }) else {
        return;
    };
    let mut active = drag.active;
    if primary_down
        && let Some(screen_now) = hover
        && let Some(pointer_page) =
            screen_to_page_unbounded(app.session.board, &app.doc.canvases[ci], rect, screen_now)
    {
        let dpx = pointer_page.x - drag.start_pointer[0];
        let dpy = pointer_page.y - drag.start_pointer[1];
        let dsx = screen_now.x - drag.start_pointer_screen[0];
        let dsy = screen_now.y - drag.start_pointer_screen[1];
        active |= dsx.hypot(dsy) > DRAG_START_PX;
        if let Interaction::Panel(current) = &mut app.session.ui.interaction {
            current.active = active;
        }
        if active {
            let frame = drag_frame(drag.before, drag.kind, dpx, dpy);
            if let Some(page) = app.doc.canvases.get_mut(ci) {
                let scale = [
                    frame.width / drag.before.width,
                    frame.height / drag.before.height,
                ];
                if matches!(drag.kind, ObjectDragKind::Resize(_)) {
                    for &(id, before) in &drag.children {
                        if let Some(object) = page.object_mut(id) {
                            object.frame = ObjectFrame::new(
                                before.x * scale[0],
                                before.y * scale[1],
                                before.width * scale[0],
                                before.height * scale[1],
                            );
                        }
                    }
                }
                if let Some(panel) = page.panel_mut(drag.panel) {
                    panel.frame = frame;
                }
                if matches!(drag.kind, ObjectDragKind::Move) {
                    let delta = [frame.x - drag.before.x, frame.y - drag.before.y];
                    for &(id, before) in &drag.others {
                        if let Some(panel) = page.panel_mut(id) {
                            panel.frame = ObjectFrame::new(
                                before.x + delta[0],
                                before.y + delta[1],
                                before.width,
                                before.height,
                            );
                        }
                    }
                }
            }
            if let Some(source) = tile_source_for_panel(app, &drag)
                && update_tile_drop(app, ci, rect, source, hover)
            {
                app.session.ui.snap_guides.clear();
            }
        }
    }
    if (primary_released || !primary_down)
        && let Interaction::Panel(drag) = app.take_interaction()
        && active
    {
        if let Some(preview) = app.session.ui.tile_drop.take()
            && let Some(source) = tile_source_for_panel(app, &drag)
        {
            commit_tile_drop(app, source, preview, alt);
        } else {
            finish_panel_drag(app, ci, drag);
        }
    }
}

pub(crate) fn begin_panel_drag(
    app: &mut PlotxApp,
    ci: usize,
    panel_id: PanelId,
    kind: ObjectDragKind,
    page_pos: Option<Pos2>,
    screen_pos: Pos2,
    preserve_selection: bool,
) {
    let Some(panel) = app.doc.canvases[ci].panel(panel_id).cloned() else {
        return;
    };
    let children = panel
        .item_order
        .iter()
        .filter_map(|id| {
            app.doc.canvases[ci]
                .object(*id)
                .map(|object| (*id, object.frame))
        })
        .collect();
    let others = if preserve_selection && matches!(kind, ObjectDragKind::Move) {
        app.session
            .ui
            .hierarchical_selection
            .paths()
            .iter()
            .filter_map(|path| {
                let id = path.panel.filter(|id| *id != panel_id)?;
                app.doc.canvases[ci]
                    .panel(id)
                    .filter(|panel| !panel.locked)
                    .map(|panel| (id, panel.frame))
            })
            .collect()
    } else {
        Vec::new()
    };
    let start = page_pos
        .map(|p| [p.x, p.y])
        .unwrap_or([panel.frame.x, panel.frame.y]);
    freeze_board_for_gesture(app);
    app.begin_interaction(Interaction::Panel(PanelDrag {
        canvas: ci,
        panel: panel_id,
        kind,
        before: panel.frame,
        others,
        children,
        start_pointer: start,
        start_pointer_screen: [screen_pos.x, screen_pos.y],
        active: matches!(kind, ObjectDragKind::Resize(_)),
    }));
}

fn update_panel_drop_target(app: &mut PlotxApp, ci: usize, drag: &ObjectDrag, point: Pos2) {
    let source = app.doc.canvases[ci].parent_panel(drag.object);
    let target = app.doc.canvases[ci]
        .panels
        .iter()
        .rev()
        .find(|panel| {
            Some(panel.id) != source
                && panel.visible
                && !panel.locked
                && point.x >= panel.frame.x
                && point.x <= panel.frame.x + panel.frame.width
                && point.y >= panel.frame.y
                && point.y <= panel.frame.y + panel.frame.height
        })
        .map(|panel| panel.id);
    app.session.ui.panel_drop_target = target;
}

pub(crate) fn update_content_drop_target(
    app: &mut PlotxApp,
    ci: usize,
    drag: &ObjectDrag,
    point: Pos2,
) {
    update_panel_drop_target(app, ci, drag, point);
}

fn finish_panel_drag(app: &mut PlotxApp, ci: usize, drag: PanelDrag) {
    let Some(page) = app.doc.canvases.get(ci) else {
        return;
    };
    let layout = page.panel(drag.panel).map(|panel| {
        (
            panel.layout,
            panel.layout_gap,
            panel.layout_padding,
            panel.layout_alignment,
        )
    });
    let after = match layout
        .filter(|(layout, ..)| *layout != plotx_core::state::PanelLayout::Free)
        .map(|(layout, gap, padding, alignment)| {
            app.set_panel_layout_action(ci, drag.panel, layout, gap, padding, alignment)
        }) {
        Some(Ok(Action::ReplacePanelState { after, .. })) => after,
        Some(Ok(_)) => PanelState::of(&app.doc.canvases[ci]),
        Some(Err(error)) => {
            app.session.status = format!("Could not apply Panel layout: {error}");
            PanelState::of(&app.doc.canvases[ci])
        }
        None => PanelState::of(&app.doc.canvases[ci]),
    };
    let mut before = after.clone();
    if let Some(panel) = before
        .panels
        .iter_mut()
        .find(|panel| panel.id == drag.panel)
    {
        panel.frame = drag.before;
    }
    for (id, frame) in drag.others {
        if let Some(panel) = before.panels.iter_mut().find(|panel| panel.id == id) {
            panel.frame = frame;
        }
    }
    for (id, frame) in drag.children {
        if let Some(object) = before.objects.iter_mut().find(|object| object.id == id) {
            object.frame = frame;
        }
    }
    app.execute_action(Action::ReplacePanelState {
        canvas: ci,
        before,
        after,
    });
    app.rebuild_canvas(ci);
}

pub(crate) fn finish_content_drag(
    app: &mut PlotxApp,
    ci: usize,
    drag: ObjectDrag,
    target: Option<PanelId>,
) {
    let mut contents = vec![drag.object];
    contents.extend(drag.others.iter().map(|(id, _)| *id));
    contents.sort_unstable();
    contents.dedup();

    let Some(page) = app.doc.canvases.get(ci) else {
        return;
    };
    let mut before = PanelState::of(page);
    for (id, frame) in
        std::iter::once((drag.object, drag.before)).chain(drag.others.iter().copied())
    {
        if let Some(object) = before.objects.iter_mut().find(|object| object.id == id) {
            object.frame = frame;
        }
    }

    let after = match target {
        Some(panel) => {
            match app.move_contents_to_panel_action(ci, &contents, Some(panel), usize::MAX) {
                Ok(Action::ReplacePanelState { after, .. }) => after,
                Ok(_) => PanelState::of(&app.doc.canvases[ci]),
                Err(error) => {
                    app.session.status = format!("Could not move content to Panel: {error}");
                    PanelState::of(&app.doc.canvases[ci])
                }
            }
        }
        None => PanelState::of(&app.doc.canvases[ci]),
    };
    app.execute_action(Action::ReplacePanelState {
        canvas: ci,
        before,
        after,
    });
    app.rebuild_canvas(ci);
}
