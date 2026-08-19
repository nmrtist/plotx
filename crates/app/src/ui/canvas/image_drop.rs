use super::*;

#[derive(Clone, Copy, Debug)]
pub(crate) struct ImagePanelDropTarget {
    pub canvas: plotx_core::state::CanvasId,
    pub panel: plotx_core::state::PanelId,
    pub position: [f32; 2],
    pub screen_rect: egui::Rect,
}

pub(crate) fn image_drop_target(
    app: &PlotxApp,
    screen: egui::Rect,
    pointer: egui::Pos2,
) -> Option<ImagePanelDropTarget> {
    let selected = app.session.ui.hierarchical_selection.lead()?;
    let panel_id = selected.panel?;
    let ci = app.doc.canvas_index(selected.canvas)?;
    if app.session.active_canvas != Some(ci) {
        return None;
    }
    let canvas = &app.doc.canvases[ci];
    let panel = canvas.panel(panel_id)?;
    if !panel.visible || panel.locked {
        return None;
    }
    let transform = BoardTransform::from_board(app.session.board, screen);
    let page_rect = transform.page_screen_rect(canvas);
    let screen_rect = egui::Rect::from_min_size(
        page_rect.min
            + egui::vec2(
                panel.frame.x * transform.zoom,
                panel.frame.y * transform.zoom,
            ),
        egui::vec2(
            panel.frame.width * transform.zoom,
            panel.frame.height * transform.zoom,
        ),
    );
    if !screen_rect.contains(pointer) {
        return None;
    }
    let page = transform.screen_to_page(canvas, pointer);
    Some(ImagePanelDropTarget {
        canvas: canvas.resource_id,
        panel: panel_id,
        position: [page.x, page.y],
        screen_rect,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use plotx_core::state::{CanvasDocument, ObjectFrame};

    fn app_with_panel() -> (PlotxApp, plotx_core::state::PanelId) {
        let mut app = PlotxApp::default();
        let mut canvas = CanvasDocument::new("Page".to_owned(), [100.0, 100.0]);
        let panel =
            canvas.create_panel("Panel".to_owned(), ObjectFrame::new(10.0, 10.0, 80.0, 60.0));
        app.doc.canvases.push(canvas);
        app.session.active_canvas = Some(0);
        app.session.board.world_center = [200.0, 200.0];
        app.session.viewport_mode = ViewportMode::Manual;
        (app, panel)
    }

    #[test]
    fn page_hit_without_an_explicit_panel_selection_creates_no_panel_target() {
        let (app, _) = app_with_panel();
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(400.0, 400.0));
        assert!(image_drop_target(&app, screen, egui::pos2(20.0, 20.0)).is_none());
    }

    #[test]
    fn selected_unlocked_panel_is_a_stable_direct_drop_target() {
        let (mut app, panel) = app_with_panel();
        app.select_panel(0, panel);
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(400.0, 400.0));
        let target = image_drop_target(&app, screen, egui::pos2(20.0, 20.0)).unwrap();
        assert_eq!(target.canvas, app.doc.canvases[0].resource_id);
        assert_eq!(target.panel, panel);

        app.doc.canvases[0].panel_mut(panel).unwrap().locked = true;
        assert!(image_drop_target(&app, screen, egui::pos2(20.0, 20.0)).is_none());
    }
}
