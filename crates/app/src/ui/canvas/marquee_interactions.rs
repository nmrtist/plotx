use super::*;

const MARQUEE_CLICK_PT: f32 = 3.0;

pub(crate) fn finish_marquee(app: &mut PlotxApp, ci: usize, marq: MarqueeDrag) {
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
    let canvas = &app.doc.canvases[ci];
    let canvas_id = canvas.resource_id;
    let intersects = |frame: ObjectFrame| {
        max_x >= frame.x
            && min_x <= frame.x + frame.width
            && max_y >= frame.y
            && min_y <= frame.y + frame.height
    };
    let editing_panel = app
        .session
        .ui
        .hierarchical_selection
        .editing_panel()
        .filter(|(id, _)| *id == canvas_id)
        .map(|(_, panel)| panel);
    let paths: Vec<_> = if let Some(panel_id) = editing_panel {
        canvas
            .panel(panel_id)
            .into_iter()
            .flat_map(|panel| panel.item_order.iter())
            .filter_map(|id| {
                canvas
                    .object(*id)
                    .filter(|item| item.visible)
                    .and_then(|_| canvas.content_page_frame(*id))
                    .filter(|frame| intersects(*frame))
                    .map(|_| {
                        plotx_core::state::SelectionPath::content(canvas_id, Some(panel_id), *id)
                    })
            })
            .collect()
    } else {
        let panels = canvas
            .panels
            .iter()
            .filter(|panel| panel.visible && intersects(panel.frame))
            .map(|panel| plotx_core::state::SelectionPath::panel(canvas_id, panel.id));
        let loose = canvas
            .objects
            .iter()
            .filter(|item| {
                item.visible && canvas.parent_panel(item.id).is_none() && intersects(item.frame)
            })
            .map(|item| plotx_core::state::SelectionPath::content(canvas_id, None, item.id));
        panels.chain(loose).collect()
    };
    if let Err(reason) = app.set_hierarchical_paths(ci, &paths, marq.additive) {
        app.session.status = reason.to_owned();
        return;
    }
    app.session.status = format!(
        "Selected {} item(s).",
        app.session.ui.hierarchical_selection.paths().len()
    );
}
