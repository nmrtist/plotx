use egui::{Id, Response, Ui};
use plotx_core::state::{BoardFrameId, ObjectId, PlotxApp};

#[derive(Clone, Copy, Default)]
pub(super) struct SelectModifiers {
    pub shift: bool,
    pub command: bool,
}

pub(super) fn select_modifiers(ui: &Ui) -> SelectModifiers {
    ui.input(|input| SelectModifiers {
        shift: input.modifiers.shift,
        command: input.modifiers.command || input.modifiers.ctrl,
    })
}

const LIST_FOCUS_KEY: &str = "plotx.primary_sidebar.list_focus";

pub(super) fn claim_list_keyboard_focus(ui: &Ui, response: &Response) {
    response.request_focus();
    ui.ctx().data_mut(|data| {
        data.insert_temp(Id::new(LIST_FOCUS_KEY), response.id);
    });
}

pub(super) fn select_canvas_range(app: &mut PlotxApp, clicked: usize, additive: bool) {
    let anchor = app
        .session
        .ui
        .selection_anchors
        .canvas
        .and_then(|id| app.doc.canvas_index(id))
        .unwrap_or(clicked);
    let (start, end) = if anchor <= clicked {
        (anchor, clicked)
    } else {
        (clicked, anchor)
    };
    let range = app.doc.canvases[start..=end]
        .iter()
        .map(|canvas| BoardFrameId::Page(canvas.resource_id));
    if !additive {
        app.session.ui.frame_selection.clear();
    }
    for id in range {
        if !app.session.ui.frame_selection.contains(&id) {
            app.session.ui.frame_selection.push(id);
        }
    }
    app.activate_canvas(clicked);
    app.session.ui.selection_anchors.canvas_lead = Some(app.doc.canvases[clicked].resource_id);
    plotx_core::state::sync_frame_selection_to_data(app);
}

pub(super) fn select_dataset_range(
    app: &mut PlotxApp,
    visible: &[usize],
    clicked: usize,
    modifiers: SelectModifiers,
) {
    let clicked_id = app.doc.datasets[clicked].resource_id();
    if modifiers.shift {
        let anchor_id = app
            .session
            .ui
            .selection_anchors
            .dataset
            .unwrap_or(clicked_id);
        let anchor = visible
            .iter()
            .position(|index| app.doc.datasets[*index].resource_id() == anchor_id)
            .unwrap_or_else(|| {
                visible
                    .iter()
                    .position(|index| *index == clicked)
                    .unwrap_or(0)
            });
        let lead = visible
            .iter()
            .position(|index| *index == clicked)
            .unwrap_or(anchor);
        let (start, end) = if anchor <= lead {
            (anchor, lead)
        } else {
            (lead, anchor)
        };
        let mut selected = if modifiers.command {
            app.session.ui.data_selection.clone()
        } else {
            Vec::new()
        };
        for &index in &visible[start..=end] {
            if !selected.contains(&index) {
                selected.push(index);
            }
        }
        app.focus_datasets(&selected, Some(clicked));
    } else {
        app.toggle_selection(clicked, modifiers.command);
        app.session.ui.selection_anchors.dataset = Some(clicked_id);
    }
    app.session.ui.selection_anchors.dataset_lead = Some(clicked_id);
}

pub(super) fn select_dataset(
    app: &mut PlotxApp,
    ui: &Ui,
    visible: &[usize],
    dataset: usize,
    modifiers: SelectModifiers,
) {
    let table_frame = app.doc.datasets[dataset].as_table().and_then(|table| {
        if table.board_sheet_visible() {
            Some(plotx_core::state::FrameRef::Sheet(dataset))
        } else {
            plotx_core::state::page_frame_showing_dataset(app, dataset)
        }
    });
    app.session.ui.selection_scope = plotx_core::state::SelectionScope::DataList;
    select_dataset_range(app, visible, dataset, modifiers);
    app.session.ui.data_browser_selected_node = Some(format!("dataset:{dataset}"));
    if let Some(frame) = table_frame {
        if modifiers.command && !modifiers.shift {
            plotx_core::state::toggle_frame_selection(app, frame);
        } else if !modifiers.shift {
            if let Some(id) = plotx_core::state::board_frame_id(app, frame) {
                app.session.ui.frame_selection = vec![id];
            }
            crate::ui::canvas::request_board_fit(app, ui.ctx(), frame);
        }
    }
}

pub(super) fn select_layer_range(
    app: &mut PlotxApp,
    canvas: usize,
    clicked: ObjectId,
    modifiers: SelectModifiers,
) {
    let order = app.doc.canvases[canvas]
        .objects
        .iter()
        .rev()
        .map(|object| object.id)
        .collect::<Vec<_>>();
    let clicked_index = order.iter().position(|id| *id == clicked).unwrap_or(0);
    if modifiers.shift {
        let anchor = app
            .session
            .ui
            .selection_anchors
            .layer
            .and_then(|id| order.iter().position(|candidate| *candidate == id))
            .unwrap_or(clicked_index);
        let (start, end) = if anchor <= clicked_index {
            (anchor, clicked_index)
        } else {
            (clicked_index, anchor)
        };
        app.set_page_selection(canvas, &order[start..=end], modifiers.command);
    } else if modifiers.command {
        app.toggle_object_selection(canvas, clicked);
        app.session.ui.selection_anchors.layer = Some(clicked);
    } else {
        app.select_object(canvas, clicked);
        app.session.ui.selection_anchors.layer = Some(clicked);
    }
    app.session.ui.selection_anchors.layer_lead = Some(clicked);
    app.focus_object_datasets(canvas, clicked);
}

pub(crate) fn handle_keyboard_selection(app: &mut PlotxApp, ctx: &egui::Context) {
    use plotx_core::state::SelectionScope;
    if ctx.egui_wants_keyboard_input() {
        return;
    }
    let focused = ctx.memory(|memory| memory.focused());
    let list_focus = ctx.data(|data| data.get_temp::<Id>(Id::new(LIST_FOCUS_KEY)));
    if focused.is_none() || focused != list_focus {
        return;
    }
    let mut input = ctx.input(|input| {
        let edge = if input.key_pressed(egui::Key::Home) {
            Some(false)
        } else if input.key_pressed(egui::Key::End) {
            Some(true)
        } else {
            None
        };
        let delta = if input.key_pressed(egui::Key::ArrowUp) {
            -1
        } else if input.key_pressed(egui::Key::ArrowDown) {
            1
        } else {
            0
        };
        (
            edge,
            delta,
            input.modifiers.shift,
            input.key_pressed(egui::Key::Space),
        )
    });
    if input.3 {
        let canvas_rect =
            ctx.data(|data| data.get_temp::<egui::Rect>(Id::new("plotx.canvas.navigation_rect")));
        let pointer_over_canvas = ctx
            .pointer_hover_pos()
            .zip(canvas_rect)
            .is_some_and(|(pointer, rect)| rect.contains(pointer));
        if pointer_over_canvas {
            input.3 = false;
        }
    }
    if input.0.is_none() && input.1 == 0 && !input.3 {
        return;
    }
    match app.session.ui.selection_scope {
        SelectionScope::CanvasList if !app.doc.canvases.is_empty() => {
            let current = app
                .session
                .ui
                .selection_anchors
                .canvas_lead
                .and_then(|id| app.doc.canvas_index(id))
                .or(app.session.active_canvas)
                .unwrap_or(0);
            let target = keyboard_target(current, app.doc.canvases.len(), input.0, input.1);
            if input.3 {
                plotx_core::state::toggle_frame_selection_synced(
                    app,
                    plotx_core::state::FrameRef::Page(current),
                );
            } else if input.2 {
                select_canvas_range(app, target, false);
            } else {
                let id = app.doc.canvases[target].resource_id;
                app.activate_canvas(target);
                app.session.ui.frame_selection = vec![BoardFrameId::Page(id)];
                app.session.ui.selection_anchors.canvas = Some(id);
                app.session.ui.selection_anchors.canvas_lead = Some(id);
            }
        }
        SelectionScope::DataList if !app.doc.datasets.is_empty() => {
            let query = app.session.ui.data_browser_filter.clone();
            let filtering = !query.trim().is_empty();
            let visible = super::data_browser::DataTree::build(app)
                .filtered(app, &query)
                .visible_datasets(app, filtering);
            if visible.is_empty() {
                return;
            }
            let current = app
                .session
                .ui
                .selection_anchors
                .dataset_lead
                .and_then(|id| {
                    visible
                        .iter()
                        .position(|index| app.doc.datasets[*index].resource_id() == id)
                })
                .or_else(|| {
                    app.active_dataset()
                        .and_then(|active| visible.iter().position(|index| *index == active))
                })
                .unwrap_or(0);
            let target = keyboard_target(current, visible.len(), input.0, input.1);
            let modifiers = SelectModifiers {
                shift: input.2,
                command: input.3,
            };
            select_dataset_range(app, &visible, visible[target], modifiers);
        }
        SelectionScope::Layers => {
            let Some(canvas) = app.session.active_canvas else {
                return;
            };
            let order = app.doc.canvases[canvas]
                .objects
                .iter()
                .rev()
                .map(|o| o.id)
                .collect::<Vec<_>>();
            if order.is_empty() {
                return;
            }
            let current = app
                .session
                .ui
                .selection_anchors
                .layer_lead
                .and_then(|id| order.iter().position(|candidate| *candidate == id))
                .unwrap_or(0);
            let target = keyboard_target(current, order.len(), input.0, input.1);
            select_layer_range(
                app,
                canvas,
                order[target],
                SelectModifiers {
                    shift: input.2,
                    command: input.3,
                },
            );
        }
        _ => return,
    }
    ctx.request_repaint();
}

fn keyboard_target(current: usize, len: usize, edge: Option<bool>, delta: isize) -> usize {
    if let Some(end) = edge {
        if end { len - 1 } else { 0 }
    } else {
        current.saturating_add_signed(delta).min(len - 1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use plotx_core::state::CanvasDocument;

    #[test]
    fn additive_canvas_range_keeps_outside_frames_and_updates_the_lead() {
        let mut app = PlotxApp::new();
        for index in 0..4 {
            app.doc
                .canvases
                .push(CanvasDocument::new(format!("page {index}"), [100.0, 80.0]));
        }
        let ids = app
            .doc
            .canvases
            .iter()
            .map(|canvas| canvas.resource_id)
            .collect::<Vec<_>>();
        app.session.ui.selection_anchors.canvas = Some(ids[1]);
        app.session.ui.frame_selection =
            vec![BoardFrameId::Page(ids[0]), BoardFrameId::Page(ids[3])];

        select_canvas_range(&mut app, 2, true);

        assert_eq!(app.session.ui.frame_selection.len(), 4);
        assert_eq!(app.session.ui.selection_anchors.canvas_lead, Some(ids[2]));
    }

    #[test]
    fn keyboard_navigation_advances_from_the_moving_lead() {
        let first = keyboard_target(0, 4, None, 1);
        let second = keyboard_target(first, 4, None, 1);
        assert_eq!((first, second), (1, 2));
    }
}
