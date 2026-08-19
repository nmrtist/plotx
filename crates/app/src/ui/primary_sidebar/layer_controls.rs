use egui::{Response, Ui};
use egui_phosphor::regular as icon;
use plotx_core::state::CanvasObjectKind;

const TOGGLE_WIDTH: f32 = 20.0;

pub(super) fn visibility_button(ui: &mut Ui, visible: &mut bool) -> Response {
    let (glyph, tooltip) = if *visible {
        (icon::EYE, "Hide")
    } else {
        (icon::EYE_SLASH, "Show")
    };
    let mut response = ui.add_sized(
        [TOGGLE_WIDTH, ui.spacing().interact_size.y],
        egui::Button::new(glyph)
            .small()
            .frame(false)
            .selected(!*visible),
    );
    if response.clicked() {
        *visible = !*visible;
        response.mark_changed();
    }
    response.on_hover_text(tooltip)
}

pub(super) fn lock_button(ui: &mut Ui, locked: &mut bool) -> Response {
    let (glyph, tooltip) = if *locked {
        (icon::LOCK, "Unlock")
    } else {
        (icon::LOCK_OPEN, "Lock")
    };
    let mut response = ui.add_sized(
        [TOGGLE_WIDTH, ui.spacing().interact_size.y],
        egui::Button::new(glyph)
            .small()
            .frame(false)
            .selected(*locked),
    );
    if response.clicked() {
        *locked = !*locked;
        response.mark_changed();
    }
    response.on_hover_text(tooltip)
}

pub(super) fn row<Left, Right>(
    ui: &mut Ui,
    left: impl FnOnce(&mut Ui) -> Left,
    right: impl FnOnce(&mut Ui) -> Right,
) -> (Left, Right) {
    egui::containers::Sides::new()
        .shrink_left()
        .truncate()
        .show(ui, left, right)
}

pub(super) fn truncated_selectable(
    ui: &mut Ui,
    selected: bool,
    text: impl Into<egui::WidgetText>,
) -> Response {
    ui.add_sized(
        [ui.available_width().max(0.0), ui.spacing().interact_size.y],
        egui::Button::selectable(selected, text).truncate(),
    )
}

pub(super) fn kind_glyph(kind: &CanvasObjectKind) -> &'static str {
    match kind {
        CanvasObjectKind::Plot(_) => icon::CHART_LINE,
        CanvasObjectKind::Text(_) => "T",
        CanvasObjectKind::Shape(_) => icon::SHAPES,
        CanvasObjectKind::RasterImage(_) => icon::FILE,
    }
}

pub(super) fn kind_label(kind: &CanvasObjectKind) -> &'static str {
    match kind {
        CanvasObjectKind::Plot(_) => "Plot",
        CanvasObjectKind::Text(_) => "Text",
        CanvasObjectKind::Shape(_) => "Shape",
        CanvasObjectKind::RasterImage(_) => "Image",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    #[test]
    fn long_names_cannot_overlap_the_trailing_layer_controls() {
        let ctx = egui::Context::default();
        let observed = Cell::new(None);
        let _ = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(150.0, 80.0),
                )),
                ..Default::default()
            },
            |ui| {
                ui.set_width(150.0);
                let (name, lock) = row(
                    ui,
                    |ui| {
                        let mut visible = true;
                        visibility_button(ui, &mut visible);
                        ui.weak(icon::CHART_LINE);
                        truncated_selectable(
                            ui,
                            false,
                            "A canvas layer name that is much wider than the sidebar",
                        )
                    },
                    |ui| {
                        let mut locked = false;
                        lock_button(ui, &mut locked)
                    },
                );
                observed.set(Some((name.rect, lock.rect)));
            },
        );

        let (name, lock) = observed.take().expect("layer row responses");
        assert!(name.right() <= lock.left());
        assert_eq!(lock.width(), TOGGLE_WIDTH);
        assert!(lock.right() <= 150.0);
    }
}
