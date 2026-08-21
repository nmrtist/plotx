use super::*;

pub(crate) fn handle_and_paint_craft_result(
    app: &mut PlotxApp,
    ci: usize,
    object_id: ObjectId,
    plot: PlotRect,
    painter: &egui::Painter,
    ui: &Ui,
) {
    let Some(plotx_core::state::CanvasAnalysisBinding::Craft { dataset, run }) =
        app.doc.canvases[ci].analysis_binding
    else {
        return;
    };
    let overview = app.doc.canvases[ci]
        .x_viewport_links
        .first()
        .and_then(|group| group.members.first())
        .copied();
    if overview != Some(object_id) {
        return;
    }
    let Some((nmr, stored)) = app
        .doc
        .dataset_by_id(dataset)
        .and_then(Dataset::as_nmr)
        .and_then(|nmr| Some((nmr, nmr.craft_run(run)?)))
    else {
        return;
    };
    if stored.is_stale_for(&nmr.data, nmr.craft_reference()) {
        painter.text(
            Pos2::new(plot.left + 6.0, plot.top + 6.0),
            egui::Align2::LEFT_TOP,
            "Stale CRAFT run",
            egui::FontId::proportional(10.0),
            ui.visuals().warn_fg_color,
        );
    }
    let Some(figure) = app.doc.canvases[ci]
        .object(object_id)
        .and_then(|object| object.plot())
        .map(|plot| plot.figure())
    else {
        return;
    };
    if let Some(selected) = app.session.ui.craft_selected_component
        && let Some(component) = stored
            .components
            .iter()
            .find(|component| component.id == selected)
    {
        let x = x_to_screen(
            component.chemical_shift_ppm,
            plot,
            figure.x.min,
            figure.x.span(),
            figure.x.reversed,
        );
        painter.text(
            Pos2::new(x, plot.top + 6.0),
            egui::Align2::CENTER_TOP,
            format!("{:.5} ppm", component.chemical_shift_ppm),
            egui::FontId::proportional(10.0),
            ui.visuals().selection.stroke.color,
        );
    }
    let (clicked, pointer) =
        ui.input(|input| (input.pointer.primary_clicked(), input.pointer.hover_pos()));
    if !clicked {
        return;
    }
    let Some(pointer) = pointer.filter(|pointer| plot_contains(plot, *pointer)) else {
        return;
    };
    let selected = stored
        .components
        .iter()
        .map(|component| {
            let x = x_to_screen(
                component.chemical_shift_ppm,
                plot,
                figure.x.min,
                figure.x.span(),
                figure.x.reversed,
            );
            (component, (x - pointer.x).abs())
        })
        .filter(|(_, distance)| *distance <= 8.0)
        .min_by(|left, right| left.1.total_cmp(&right.1))
        .map(|(component, _)| (component.id, component.region));
    if let Some((component, region)) = selected {
        app.session.ui.craft_task_dataset = Some(dataset);
        app.session.ui.craft_selected_run = Some(run);
        app.session.ui.craft_selected_component = Some(component);
        app.session.ui.craft_component_region = Some(region);
        app.session.ui.craft_result_tab = plotx_core::state::CraftResultTab::Components;
        app.session.ui.craft_task_page = plotx_core::state::CraftTaskPage::Results;
        app.session
            .ui
            .open_task_tab(plotx_core::state::TaskDockTab::Craft);
    }
}
