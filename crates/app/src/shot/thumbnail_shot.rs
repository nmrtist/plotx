use plotx_core::actions::Action;
use plotx_core::state::{CanvasId, PlotxApp, PrimaryView};

pub(super) fn setup(app: &mut PlotxApp) {
    app.session.view = PrimaryView::Canvas;
    app.session.ui.close_task_cards();
    let mut portrait = app.doc.canvases[0].clone();
    portrait.resource_id = CanvasId::new();
    portrait.name = "Portrait comparison with a long canvas name".to_owned();
    portrait.size_mm = [110.0, 180.0];
    portrait.board_pos[0] += 600.0;
    app.execute_action(Action::insert_canvas(
        app.doc.canvases.len(),
        portrait,
        app.session.active_canvas,
    ));
    app.activate_canvas(0);
}
