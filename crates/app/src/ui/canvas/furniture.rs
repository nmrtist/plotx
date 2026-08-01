use super::*;

enum FurnitureHit {
    Legend {
        object: ObjectId,
        rect: PlotRect,
    },
    ColorScale {
        object: ObjectId,
    },
    RegionLabel {
        object: ObjectId,
        dataset: plotx_core::state::DatasetId,
        region: RegionId,
        rect: PlotRect,
    },
}

impl FurnitureHit {
    fn object(&self) -> ObjectId {
        match self {
            Self::Legend { object, .. }
            | Self::ColorScale { object, .. }
            | Self::RegionLabel { object, .. } => *object,
        }
    }
}

pub(crate) fn handle_furniture_interactions(
    app: &mut PlotxApp,
    ci: usize,
    canvas_rect: egui::Rect,
    ui: &Ui,
) -> bool {
    if !matches!(app.session.tool, Tool::Select | Tool::Regions) {
        return false;
    }
    let (pointer, down, pressed, released, double_clicked, escape) = ui.input(|input| {
        (
            input.pointer.hover_pos(),
            input.pointer.primary_down(),
            input.pointer.primary_pressed(),
            input.pointer.primary_released(),
            input
                .pointer
                .button_double_clicked(egui::PointerButton::Primary),
            input.key_pressed(egui::Key::Escape),
        )
    });

    if matches!(app.interaction(), Interaction::Furniture(_)) {
        if escape {
            app.cancel_interaction();
            app.session.status = "Restored the previous label position.".to_owned();
            return true;
        }
        if down && let Some(pointer) = pointer {
            update_furniture_drag(app, ci, canvas_rect, pointer);
        }
        if released || !down {
            finish_furniture_drag(app);
        }
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
        return true;
    }

    let Some(pointer) = pointer else {
        return false;
    };
    let Some(hit) = furniture_hit(app, ci, canvas_rect, pointer) else {
        return false;
    };
    ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
    let object = hit.object();
    if double_clicked {
        app.select_object(ci, object);
        if matches!(
            &hit,
            FurnitureHit::Legend { .. } | FurnitureHit::ColorScale { .. }
        ) {
            app.session.ui.requested_inspector_section =
                Some(crate::ui::properties::panel::GUIDE_SECTION.to_owned());
        }
        reset_furniture_position(app, ci, hit);
        return true;
    }
    if !pressed {
        return true;
    }

    app.select_object(ci, object);
    if matches!(
        &hit,
        FurnitureHit::Legend { .. } | FurnitureHit::ColorScale { .. }
    ) {
        app.session.ui.requested_inspector_section =
            Some(crate::ui::properties::panel::GUIDE_SECTION.to_owned());
    }
    if matches!(&hit, FurnitureHit::ColorScale { .. }) {
        return true;
    }
    freeze_board_for_gesture(app);
    let target = match hit {
        FurnitureHit::Legend { rect, .. } => {
            let before = app.doc.canvases[ci]
                .object(object)
                .and_then(|object| object.plot())
                .map(|plot| plot.axis_overrides.clone())
                .unwrap_or_default();
            FurnitureTarget::Legend {
                before,
                grab_offset: [pointer.x - rect.left, pointer.y - rect.top],
            }
        }
        FurnitureHit::ColorScale { .. } => return true,
        FurnitureHit::RegionLabel {
            dataset,
            region,
            rect,
            ..
        } => {
            let before = app
                .doc
                .dataset_index(dataset)
                .and_then(|index| app.doc.datasets[index].region_analysis())
                .map(|state| state.regions.clone())
                .unwrap_or_default();
            FurnitureTarget::RegionLabel {
                dataset,
                region,
                before,
                grab_offset: [
                    pointer.x - (rect.left + rect.width * 0.5),
                    pointer.y - (rect.top + rect.height * 0.5),
                ],
            }
        }
    };
    app.begin_interaction(Interaction::Furniture(FurnitureDrag {
        canvas: ci,
        object,
        target,
    }));
    true
}

fn furniture_hit(
    app: &PlotxApp,
    ci: usize,
    canvas_rect: egui::Rect,
    pointer: Pos2,
) -> Option<FurnitureHit> {
    let canvas = app.doc.canvases.get(ci)?;
    for object in canvas
        .objects
        .iter()
        .rev()
        .filter(|object| object.visible && !object.locked)
    {
        let Some(plot_object) = object.plot() else {
            continue;
        };
        let Some((plot, scale)) = plot_geometry(app, ci, object.id, canvas_rect) else {
            continue;
        };
        let figure = plot_object.figure();
        if let Some(rect) = plotx_render::legend_rect(figure, plot, scale)
            && rect_contains(rect, pointer, 2.0)
        {
            return Some(FurnitureHit::Legend {
                object: object.id,
                rect,
            });
        }
        if let Some(rect) = plotx_render::color_scale_rect(figure, plot, scale)
            && rect_contains(rect, pointer, 3.0)
        {
            return Some(FurnitureHit::ColorScale { object: object.id });
        }
        let Some(dataset) = object.dataset() else {
            continue;
        };
        for annotation in figure.range_annotations.iter().rev() {
            let x0 = x_to_screen(
                annotation.x0,
                plot,
                figure.x.min,
                figure.x.span(),
                figure.x.reversed,
            );
            let x1 = x_to_screen(
                annotation.x1,
                plot,
                figure.x.min,
                figure.x.span(),
                figure.x.reversed,
            );
            let Some(layout) = plotx_render::range_label_layout(
                plot,
                x0.min(x1),
                x0.max(x1),
                figure.typography.tick_pt * scale,
                &annotation.label,
                annotation.label_position,
            ) else {
                continue;
            };
            let rect = layout.rect(figure.typography.tick_pt * scale);
            if rect_contains(rect, pointer, 3.0) {
                return Some(FurnitureHit::RegionLabel {
                    object: object.id,
                    dataset,
                    region: RegionId::new(annotation.source_id),
                    rect,
                });
            }
        }
    }
    None
}

pub(crate) fn furniture_hovered(
    app: &PlotxApp,
    ci: usize,
    canvas_rect: egui::Rect,
    pointer: Pos2,
) -> bool {
    matches!(app.session.tool, Tool::Select | Tool::Regions)
        && furniture_hit(app, ci, canvas_rect, pointer).is_some()
}

fn plot_geometry(
    app: &PlotxApp,
    ci: usize,
    object: ObjectId,
    canvas_rect: egui::Rect,
) -> Option<(PlotRect, f32)> {
    let outer = object_screen_rect(
        app.session.board,
        app.doc.canvases.get(ci)?,
        object,
        canvas_rect,
    )?;
    let figure = app.doc.canvases[ci].object(object)?.plot()?.figure();
    let scale = app.session.board.zoom;
    let layout = plotx_render::axis_layout(figure, outer.width / scale, outer.height / scale);
    let projector = plotx_render::Projector::new(figure, outer, &layout.margins.scaled(scale));
    Some((projector.plot, scale))
}

fn update_furniture_drag(app: &mut PlotxApp, ci: usize, canvas_rect: egui::Rect, pointer: Pos2) {
    let Interaction::Furniture(drag) = app.interaction() else {
        return;
    };
    if drag.canvas != ci {
        return;
    }
    let object = drag.object;
    let target = drag.target.clone();
    let Some((plot, scale)) = plot_geometry(app, ci, object, canvas_rect) else {
        return;
    };
    match target {
        FurnitureTarget::Legend { grab_offset, .. } => {
            let Some(plot_object) = app.doc.canvases[ci]
                .object(object)
                .and_then(|object| object.plot())
            else {
                return;
            };
            let Some(position) = plotx_render::legend_position_for_origin(
                plot_object.figure(),
                plot,
                scale,
                [pointer.x - grab_offset[0], pointer.y - grab_offset[1]],
            ) else {
                return;
            };
            let mut overrides = plot_object.axis_overrides.clone();
            overrides.guide_placement = Some(plotx_figure::GuidePlacement::Inside);
            overrides.legend_position = Some(position);
            app.set_axis_overrides_value(ci, object, &overrides);
        }
        FurnitureTarget::RegionLabel {
            dataset,
            region,
            grab_offset,
            ..
        } => {
            let Some(index) = app.doc.dataset_index(dataset) else {
                return;
            };
            let center = [pointer.x - grab_offset[0], pointer.y - grab_offset[1]];
            let position = [
                ((center[0] - plot.left) / plot.width).clamp(0.0, 1.0),
                ((center[1] - plot.top) / plot.height).clamp(0.0, 1.0),
            ];
            if let Some(item) = app.doc.datasets[index]
                .region_analysis_mut()
                .and_then(|state| state.regions.iter_mut().find(|item| item.id == region))
            {
                item.label_position = Some(position);
                app.rebuild_canvases_for(index);
            }
        }
    }
}

fn finish_furniture_drag(app: &mut PlotxApp) {
    let Interaction::Furniture(drag) = app.take_interaction() else {
        return;
    };
    match drag.target {
        FurnitureTarget::Legend { before, .. } => {
            let Some(after) = app.doc.canvases[drag.canvas]
                .object(drag.object)
                .and_then(|object| object.plot())
                .map(|plot| plot.axis_overrides.clone())
            else {
                return;
            };
            app.execute_action(Action::set_axis_overrides(
                drag.canvas,
                drag.object,
                before,
                after,
            ));
            app.session.status =
                "Moved legend. Double-click it to restore automatic placement.".into();
        }
        FurnitureTarget::RegionLabel {
            dataset, before, ..
        } => {
            let Some(index) = app.doc.dataset_index(dataset) else {
                return;
            };
            let after = app.doc.datasets[index]
                .region_analysis()
                .map(|state| state.regions.clone())
                .unwrap_or_default();
            app.execute_action(Action::set_regions(dataset, before, after));
            app.session.status =
                "Moved region label. Double-click it to restore automatic placement.".into();
        }
    }
}

fn reset_furniture_position(app: &mut PlotxApp, ci: usize, hit: FurnitureHit) {
    match hit {
        FurnitureHit::Legend { object, .. } => {
            let Some(before) = app.doc.canvases[ci]
                .object(object)
                .and_then(|object| object.plot())
                .map(|plot| plot.axis_overrides.clone())
            else {
                return;
            };
            let mut after = before.clone();
            after.legend_position = None;
            after.guide_placement = None;
            app.execute_action(Action::set_axis_overrides(ci, object, before, after));
            app.session.status = "Restored automatic legend placement.".into();
        }
        FurnitureHit::ColorScale { object, .. } => {
            let Some(before) = app.doc.canvases[ci]
                .object(object)
                .and_then(|object| object.plot())
                .map(|plot| plot.axis_overrides.clone())
            else {
                return;
            };
            let mut after = before.clone();
            after.guide_placement = None;
            app.execute_action(Action::set_axis_overrides(ci, object, before, after));
            app.session.status = "Restored automatic colour-scale placement.".into();
        }
        FurnitureHit::RegionLabel {
            dataset, region, ..
        } => {
            let Some(index) = app.doc.dataset_index(dataset) else {
                return;
            };
            let before = app.doc.datasets[index]
                .region_analysis()
                .map(|state| state.regions.clone())
                .unwrap_or_default();
            let mut after = before.clone();
            if let Some(item) = after.iter_mut().find(|item| item.id == region) {
                item.label_position = None;
            }
            app.execute_action(Action::set_regions(dataset, before, after));
            app.session.status = "Restored automatic region-label placement.".into();
        }
    }
}

fn rect_contains(rect: PlotRect, pointer: Pos2, padding: f32) -> bool {
    pointer.x >= rect.left - padding
        && pointer.x <= rect.right() + padding
        && pointer.y >= rect.top - padding
        && pointer.y <= rect.bottom() + padding
}
