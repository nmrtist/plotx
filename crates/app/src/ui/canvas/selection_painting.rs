use super::*;

pub(crate) fn paint_selection_drag(
    app: &PlotxApp,
    ci: usize,
    object_id: ObjectId,
    plot: PlotRect,
    painter: &egui::Painter,
    chrome: ChromeStyle,
) {
    let drag = match &app.session.ui.interaction {
        Interaction::Selection(d) => *d,
        _ => return,
    };
    if drag.canvas != ci || drag.object != object_id {
        return;
    }
    let a = clamp_to_plot(plot, pos(drag.start));
    let b = clamp_to_plot(plot, pos(drag.current));
    let r = EguiRect::from_min_max(
        Pos2::new(a.x.min(b.x), plot.top),
        Pos2::new(a.x.max(b.x), plot.bottom()),
    )
    .intersect(plot_rect(plot));
    if r.width() < 1.0 {
        return;
    }
    painter.rect_filled(r, 0.0, chrome.selection_fill);
    painter.rect_stroke(
        r,
        0.0,
        Stroke::new(1.0_f32, chrome.selection_stroke),
        StrokeKind::Inside,
    );
}

pub(crate) fn paint_document(app: &PlotxApp, ci: usize, rect: egui::Rect, painter: &egui::Painter) {
    super::image_painting::paint_document(app, ci, rect, painter);
}

pub(crate) fn paint_layout_overlay(
    app: &PlotxApp,
    ci: usize,
    rect: egui::Rect,
    painter: &egui::Painter,
    chrome: ChromeStyle,
) {
    let canvas = &app.doc.canvases[ci];
    let bt = BoardTransform::from_board(app.session.board, rect);
    let page = bt.page_screen_rect(canvas);
    let zoom = bt.zoom;

    let layout_tool = app.session.tool.is_layout_tool();
    if layout_tool {
        let [top, right, bottom, left] = canvas.layout.margin_mm;
        let mm = plotx_core::state::MM_TO_PT * zoom;
        let stroke = Stroke::new(1.0_f32, chrome.margin_guide);
        let dashed = |points: [Pos2; 2]| {
            for segment in egui::Shape::dashed_line(&points, stroke, 5.0, 4.0) {
                painter.add(segment);
            }
        };
        if top > 0.0 {
            let y = page.top() + top * mm;
            dashed([Pos2::new(page.left(), y), Pos2::new(page.right(), y)]);
        }
        if right > 0.0 {
            let x = page.right() - right * mm;
            dashed([Pos2::new(x, page.top()), Pos2::new(x, page.bottom())]);
        }
        if bottom > 0.0 {
            let y = page.bottom() - bottom * mm;
            dashed([Pos2::new(page.left(), y), Pos2::new(page.right(), y)]);
        }
        if left > 0.0 {
            let x = page.left() + left * mm;
            dashed([Pos2::new(x, page.top()), Pos2::new(x, page.bottom())]);
        }
    }

    if canvas.layout.show_grid && layout_tool {
        let stroke = Stroke::new(1.0_f32, chrome.layout_grid);
        for cell in layout::grid_frames(canvas.size_pt(), &canvas.layout) {
            let r = EguiRect::from_min_size(
                Pos2::new(page.left() + cell.x * zoom, page.top() + cell.y * zoom),
                Vec2::new(cell.width * zoom, cell.height * zoom),
            );
            painter.rect_stroke(r, 0.0, stroke, StrokeKind::Inside);
        }
    }

    let stroke = Stroke::new(1.0_f32, chrome.snap_guide);
    for guide in &app.session.ui.snap_guides {
        if guide.vertical {
            let x = page.left() + guide.pos * zoom;
            painter.line_segment(
                [Pos2::new(x, page.top()), Pos2::new(x, page.bottom())],
                stroke,
            );
        } else {
            let y = page.top() + guide.pos * zoom;
            painter.line_segment(
                [Pos2::new(page.left(), y), Pos2::new(page.right(), y)],
                stroke,
            );
        }
    }
}

pub(crate) fn paint_author_drag(
    app: &PlotxApp,
    ci: usize,
    rect: egui::Rect,
    painter: &egui::Painter,
    chrome: ChromeStyle,
) {
    let drag = match &app.session.ui.interaction {
        Interaction::Author(d) => *d,
        _ => return,
    };
    if drag.canvas != ci {
        return;
    }
    let bt = BoardTransform::from_board(app.session.board, rect);
    let page = bt.page_screen_rect(&app.doc.canvases[ci]);
    let zoom = bt.zoom;
    let to_screen = |p: [f32; 2]| Pos2::new(page.left() + p[0] * zoom, page.top() + p[1] * zoom);
    let r = EguiRect::from_two_pos(to_screen(drag.start), to_screen(drag.current));
    painter.rect_filled(r, 0.0, chrome.selection_fill);
    painter.rect_stroke(
        r,
        0.0,
        Stroke::new(1.0_f32, chrome.selection_stroke),
        StrokeKind::Inside,
    );
}

pub(crate) fn paint_panel_label_selection(
    app: &PlotxApp,
    ci: usize,
    rect: egui::Rect,
    painter: &egui::Painter,
    chrome: ChromeStyle,
) {
    let Some((canvas, object_id)) = app.panel_label_selection() else {
        return;
    };
    if canvas != ci {
        return;
    }
    let Some(r) =
        panel_label_screen_rect(app.session.board, &app.doc.canvases[ci], object_id, rect)
    else {
        return;
    };
    painter.rect_stroke(
        r,
        0.0,
        Stroke::new(1.0_f32, chrome.selection_active),
        StrokeKind::Inside,
    );
}

pub(crate) fn paint_object_selection(
    app: &PlotxApp,
    ci: usize,
    rect: egui::Rect,
    _page: egui::Rect,
    painter: &egui::Painter,
    chrome: ChromeStyle,
) {
    super::panel_selection::paint(app, ci, rect, painter, chrome);
    let selection = &app.session.ui.selection;
    let mut ids = selection.objects().to_vec();
    if let Some(primary) = selection.object().filter(|id| !ids.contains(id)) {
        ids.push(primary);
    }
    let handles = ids.len() == 1 && app.session.tool.is_layout_tool();
    let editing_panel = app
        .session
        .ui
        .hierarchical_selection
        .editing_panel()
        .filter(|(canvas, _)| *canvas == app.doc.canvases[ci].resource_id);
    for id in ids {
        let frame = editing_panel
            .and_then(|(_, panel)| {
                (app.doc.canvases[ci].parent_panel(id) == Some(panel)).then(|| {
                    content_screen_rect(app.session.board, &app.doc.canvases[ci], id, rect)
                })
            })
            .flatten()
            .or_else(|| object_screen_rect(app.session.board, &app.doc.canvases[ci], id, rect));
        let Some(frame) = frame else { continue };
        let r = plot_rect(frame);
        let stroke = if data_edit_target(app, ci) == Some(id) {
            Stroke::new(2.0_f32, chrome.selection_active)
        } else {
            Stroke::new(1.5_f32, chrome.selection_stroke)
        };
        painter.rect_stroke(r, 0.0, stroke, StrokeKind::Inside);
        if handles {
            for p in [
                r.left_top(),
                r.right_top(),
                r.left_bottom(),
                r.right_bottom(),
            ] {
                painter.rect_filled(
                    egui::Rect::from_center_size(p, egui::vec2(HANDLE_SIZE_PX, HANDLE_SIZE_PX)),
                    0.0,
                    chrome.selection_stroke,
                );
            }
        }
    }
}

pub(crate) fn paint_marquee(
    app: &PlotxApp,
    ci: usize,
    rect: egui::Rect,
    painter: &egui::Painter,
    chrome: ChromeStyle,
) {
    let marq = match &app.session.ui.interaction {
        Interaction::Marquee(d) => *d,
        _ => return,
    };
    if marq.canvas != ci {
        return;
    }
    let bt = BoardTransform::from_board(app.session.board, rect);
    let page = bt.page_screen_rect(&app.doc.canvases[ci]);
    let zoom = bt.zoom;
    let to_screen = |p: [f32; 2]| Pos2::new(page.left() + p[0] * zoom, page.top() + p[1] * zoom);
    let r = EguiRect::from_two_pos(to_screen(marq.start), to_screen(marq.current));
    painter.rect_filled(r, 0.0, chrome.selection_fill);
    painter.rect_stroke(
        r,
        0.0,
        Stroke::new(1.0_f32, chrome.selection_stroke),
        StrokeKind::Inside,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn browse_zoom_box_interaction_has_a_preview_rect() {
        let mut app = PlotxApp::new();
        app.session.tool = Tool::BrowseZoom;
        app.session.ui.interaction = Interaction::Zoom(ZoomDrag {
            canvas: 2,
            object: ObjectId::new(7),
            start: [10.0, 20.0],
            current: [40.0, 60.0],
            axis: ZoomAxis::Box,
        });

        let rect = active_box_zoom_rect(
            &app,
            2,
            ObjectId::new(7),
            PlotRect::new(0.0, 0.0, 100.0, 100.0),
        );

        assert_eq!(
            rect,
            Some(EguiRect::from_min_max(
                Pos2::new(10.0, 20.0),
                Pos2::new(40.0, 60.0)
            ))
        );
    }
}
