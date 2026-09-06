use egui::{Color32, Rect, Response, Sense, Stroke, StrokeKind, Ui, vec2};

pub(super) const BUTTON_WIDTH: f32 = 30.0;

#[derive(Clone, Copy)]
pub(super) enum Edge {
    Left,
    Right,
    Top,
    Bottom,
}

pub(super) fn toggle(ui: &mut Ui, edge: Edge, visible: bool, label: &str) -> Response {
    let (rect, response) = button(ui, label);
    let color = if visible {
        ui.style().interact(&response).text_color()
    } else {
        ui.visuals().weak_text_color()
    };
    paint_glyph(ui, rect, edge, visible, color);
    response.on_hover_text(label)
}

pub(super) fn close(ui: &mut Ui, label: &str) -> Response {
    ui.add_sized(
        vec2(BUTTON_WIDTH, ui.spacing().interact_size.y),
        egui::Button::new(egui_phosphor::regular::X).frame_when_inactive(false),
    )
    .on_hover_text(label)
}

fn button(ui: &mut Ui, label: &str) -> (Rect, Response) {
    let (rect, response) = ui.allocate_exact_size(
        vec2(BUTTON_WIDTH, ui.spacing().interact_size.y),
        Sense::click(),
    );
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), label)
    });
    let visuals = ui.style().interact(&response);
    if response.hovered() || response.is_pointer_button_down_on() || response.has_focus() {
        ui.painter()
            .rect_filled(rect, visuals.corner_radius, visuals.weak_bg_fill);
    }
    (rect, response)
}

fn paint_glyph(ui: &Ui, rect: Rect, edge: Edge, filled: bool, color: Color32) {
    let painter = ui.painter();
    let outer = Rect::from_center_size(rect.center(), vec2(16.0, 12.0));
    painter.rect_stroke(outer, 3.0, Stroke::new(1.2_f32, color), StrokeKind::Inside);
    let inner = outer.shrink(2.0);
    let band = match edge {
        Edge::Left => Rect::from_min_size(inner.min, vec2(5.0, inner.height())),
        Edge::Right => Rect::from_min_size(
            inner.right_top() - vec2(5.0, 0.0),
            vec2(5.0, inner.height()),
        ),
        Edge::Top => Rect::from_min_size(inner.min, vec2(inner.width(), 3.0)),
        Edge::Bottom => Rect::from_min_size(
            inner.left_bottom() - vec2(0.0, 3.0),
            vec2(inner.width(), 3.0),
        ),
    };
    if filled {
        painter.rect_filled(band, 1.5, color);
    } else {
        painter.rect_stroke(band, 1.5, Stroke::new(1.0_f32, color), StrokeKind::Inside);
    }
}

pub(super) fn collapse(ui: &mut Ui, collapsed: bool, name: &str) -> Response {
    let action = if collapsed { "Expand" } else { "Collapse" };
    toggle(ui, Edge::Bottom, !collapsed, &format!("{action} {name}"))
}
