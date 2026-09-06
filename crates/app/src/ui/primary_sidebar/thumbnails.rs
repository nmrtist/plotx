use egui::{Align2, Color32, Rect, Response, Sense, Stroke, StrokeKind, Ui, vec2};
use plotx_core::state::PlotxApp;

pub(super) fn canvas_row(
    app: &PlotxApp,
    ci: usize,
    ui: &mut Ui,
    selected: bool,
    name: &str,
) -> Response {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 64.0), Sense::hover());
    let response = ui.interact(
        rect,
        ui.id()
            .with(("canvas_row", app.doc.canvases[ci].resource_id)),
        Sense::click(),
    );
    response.widget_info(|| {
        egui::WidgetInfo::selected(
            egui::WidgetType::SelectableLabel,
            ui.is_enabled(),
            selected,
            name,
        )
    });
    if !ui.is_rect_visible(rect) {
        return response;
    }
    let visuals = ui.style().interact_selectable(&response, selected);
    let painter = ui.painter_at(rect);
    if selected || response.hovered() || response.has_focus() {
        painter.rect_filled(rect, 4.0, visuals.weak_bg_fill);
        painter.rect_stroke(rect, 4.0, visuals.bg_stroke, StrokeKind::Inside);
    }
    let preview = Rect::from_min_size(rect.min + vec2(6.0, 6.0), vec2(72.0, 52.0));
    painter.rect_filled(preview, 2.0, ui.visuals().faint_bg_color);
    let document = crate::ui::canvas::image_painting::canvas_document(app, ci);
    if document.width.is_finite()
        && document.height.is_finite()
        && document.width > 0.0
        && document.height > 0.0
    {
        let available = preview.shrink(3.0);
        let zoom = (available.width() / document.width).min(available.height() / document.height);
        let size = vec2(document.width, document.height) * zoom;
        let page = Rect::from_center_size(available.center(), size);
        // A thumbnail has its own viewport, independent of board navigation.
        plotx_render::screen::paint_document_for_editor_with_detail(
            &painter.with_clip_rect(preview.intersect(ui.clip_rect())),
            plotx_render::Rect::new(page.left(), page.top(), page.width(), page.height()),
            &document,
            plotx_render::DocumentViewport {
                zoom,
                pan: [0.0, 0.0],
            },
            plotx_render::screen::ScreenRenderDetail::Interactive,
        );
        painter.rect_stroke(
            page,
            0.0,
            Stroke::new(0.5_f32, Color32::from_gray(150)),
            StrokeKind::Inside,
        );
    }
    let text_rect = Rect::from_min_max(
        egui::pos2(preview.right() + 8.0, rect.top()),
        rect.max - vec2(6.0, 0.0),
    );
    let galley = egui::WidgetText::from(name).into_galley(
        ui,
        Some(egui::TextWrapMode::Truncate),
        text_rect.width().max(0.0),
        egui::TextStyle::Button,
    );
    let label = Align2::LEFT_CENTER.align_size_within_rect(galley.size(), text_rect);
    painter.galley(label.min, galley, visuals.text_color());
    response.on_hover_text(name)
}
