use super::*;

pub(crate) fn canvas_breadcrumb(app: &mut PlotxApp, ci: usize, ui: &mut Ui) {
    let canvas_name = app.doc.canvases[ci].name.clone();
    let lead = app.session.ui.hierarchical_selection.lead();
    let panel = lead.and_then(|path| path.panel).and_then(|id| {
        app.doc.canvases[ci]
            .panel(id)
            .map(|panel| (id, panel.name.clone()))
    });
    let content = lead.and_then(|path| path.content).and_then(|id| {
        app.doc.canvases[ci]
            .object(id)
            .map(|object| (id, object.name.clone()))
    });
    let mut clicked = None;
    ui.horizontal(|ui| {
        ui.add_space(2.0);
        if ui.small_button(canvas_name).clicked() {
            clicked = Some(BreadcrumbTarget::Canvas);
        }
        if let Some((id, name)) = panel {
            ui.weak("›");
            if ui.small_button(name).clicked() {
                clicked = Some(BreadcrumbTarget::Panel(id));
            }
        }
        if let Some((id, name)) = content {
            ui.weak("›");
            if ui.small_button(name).clicked() {
                clicked = Some(BreadcrumbTarget::Content(id));
            }
        } else if lead.is_none() {
            ui.weak("›");
            ui.small(app.session.tool.label());
        }
    });
    ui.add_space(2.0);
    match clicked {
        Some(BreadcrumbTarget::Canvas) => app.exit_panel_scope(),
        Some(BreadcrumbTarget::Panel(panel)) => app.select_panel(ci, panel),
        Some(BreadcrumbTarget::Content(content)) => app.select_content(ci, content),
        None => {}
    }
}

enum BreadcrumbTarget {
    Canvas,
    Panel(PanelId),
    Content(ObjectId),
}
