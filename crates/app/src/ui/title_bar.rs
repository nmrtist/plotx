//! VS Code style title bar for Windows and Linux: the window is undecorated
//! and this strip hosts the logo, the menu bar, a drag region, and the
//! minimize / maximize / close controls. macOS keeps the native title bar and
//! never renders this module.

use super::*;
use egui::{
    Align, FontId, Layout, PointerButton, Rect, StrokeKind, UiBuilder, ViewportCommand, pos2, vec2,
};

pub(super) const BAR_HEIGHT: f32 = 34.0;
const BUTTON_WIDTH: f32 = 46.0;
const LOGO_SIZE: f32 = 16.0;

pub(super) fn render(
    app: &mut PlotxApp,
    clipboard: &mut clipboard_table::ClipboardTablePaste,
    ui: &mut Ui,
) {
    let (bar_rect, _) =
        ui.allocate_exact_size(vec2(ui.available_width(), BAR_HEIGHT), Sense::hover());
    let maximized = ui.input(|i| i.viewport().maximized.unwrap_or(false));

    // The whole bar moves the window. This zone is registered FIRST so every
    // interactive child added afterwards (menu buttons, window controls) wins
    // the hit test over it; dragging works anywhere else, including the gaps
    // between menus, matching native title bars and VS Code.
    let drag = ui.interact(
        bar_rect,
        ui.id().with("title_bar_drag"),
        Sense::click_and_drag(),
    );
    if drag.double_clicked() {
        ui.ctx()
            .send_viewport_cmd(ViewportCommand::Maximized(!maximized));
    } else if drag.drag_started_by(PointerButton::Primary) {
        ui.ctx().send_viewport_cmd(ViewportCommand::StartDrag);
    }

    let controls_left = bar_rect.right() - 3.0 * BUTTON_WIDTH;
    // The menu strip stretches to its ui's full width (MenuBar sets a
    // full-width min size), so cap the child ui at the window controls.
    let mut bar = ui.new_child(
        UiBuilder::new()
            .max_rect(Rect::from_min_max(
                bar_rect.min,
                pos2(controls_left, bar_rect.bottom()),
            ))
            .layout(Layout::left_to_right(Align::Center)),
    );
    bar.add_space(10.0);
    let logo = logo_texture(bar.ctx());
    bar.add(egui::Image::new((logo.id(), Vec2::splat(LOGO_SIZE))));
    bar.add_space(4.0);
    let menus_right = menus::menu_bar(app, clipboard, &mut bar);

    window_controls(ui, bar_rect, controls_left, maximized);

    // Centered window title, drawn only when it fits between the menus and
    // the window controls.
    let galley = ui.painter().layout_no_wrap(
        "PlotX".to_owned(),
        FontId::proportional(13.0),
        ui.visuals().weak_text_color(),
    );
    let text_rect = Rect::from_center_size(bar_rect.center(), galley.size());
    let free_rect = Rect::from_min_max(
        pos2(menus_right, bar_rect.top()),
        pos2(controls_left, bar_rect.bottom()),
    );
    if free_rect.contains_rect(text_rect.expand2(vec2(12.0, 0.0))) {
        ui.painter()
            .galley(text_rect.min, galley, ui.visuals().weak_text_color());
    }
}

#[derive(Clone, Copy, PartialEq)]
enum WindowControl {
    Minimize,
    Maximize,
    Close,
}

fn window_controls(ui: &mut Ui, bar_rect: Rect, controls_left: f32, maximized: bool) {
    for (index, control) in [
        WindowControl::Minimize,
        WindowControl::Maximize,
        WindowControl::Close,
    ]
    .into_iter()
    .enumerate()
    {
        let rect = Rect::from_min_size(
            pos2(controls_left + index as f32 * BUTTON_WIDTH, bar_rect.top()),
            vec2(BUTTON_WIDTH, bar_rect.height()),
        );
        let response = ui.interact(
            rect,
            ui.id().with(("window_control", index)),
            Sense::click(),
        );
        let hovered = response.hovered();
        if hovered {
            // The close button hovers red per the Windows convention; the
            // others use the regular hover wash.
            let fill = if control == WindowControl::Close {
                Color32::from_rgb(196, 43, 28)
            } else {
                ui.visuals().widgets.hovered.weak_bg_fill
            };
            ui.painter().rect_filled(rect, 0.0, fill);
        }
        let color = if control == WindowControl::Close && hovered {
            Color32::WHITE
        } else {
            ui.visuals().widgets.inactive.fg_stroke.color
        };
        control_glyph(ui, rect, control, maximized, color);
        if response.clicked() {
            let command = match control {
                WindowControl::Minimize => ViewportCommand::Minimized(true),
                WindowControl::Maximize => ViewportCommand::Maximized(!maximized),
                // Close goes through the normal close-request path so the
                // unsaved-changes confirmation still intercepts it.
                WindowControl::Close => ViewportCommand::Close,
            };
            ui.ctx().send_viewport_cmd(command);
        }
    }
}

fn control_glyph(ui: &Ui, rect: Rect, control: WindowControl, maximized: bool, color: Color32) {
    let painter = ui.painter();
    let stroke = Stroke::new(1.0_f32, color);
    let center = rect.center();
    match control {
        WindowControl::Minimize => {
            painter.line_segment(
                [
                    pos2(center.x - 5.0, center.y),
                    pos2(center.x + 5.0, center.y),
                ],
                stroke,
            );
        }
        WindowControl::Maximize if !maximized => {
            painter.rect_stroke(
                Rect::from_center_size(center, Vec2::splat(10.0)),
                1.0,
                stroke,
                StrokeKind::Inside,
            );
        }
        WindowControl::Maximize => {
            // Restore: front square plus the top-right edges of the one behind.
            let front = Rect::from_center_size(center + vec2(-1.0, 1.0), Vec2::splat(8.0));
            painter.rect_stroke(front, 1.0, stroke, StrokeKind::Inside);
            let offset = 2.5;
            let top_left = pos2(front.min.x + offset, front.min.y - offset);
            let top_right = pos2(front.max.x + offset, front.min.y - offset);
            let bottom_right = pos2(front.max.x + offset, front.max.y - offset);
            painter.line_segment([top_left, top_right], stroke);
            painter.line_segment([top_right, bottom_right], stroke);
        }
        WindowControl::Close => {
            let half = 5.0;
            painter.line_segment(
                [
                    pos2(center.x - half, center.y - half),
                    pos2(center.x + half, center.y + half),
                ],
                stroke,
            );
            painter.line_segment(
                [
                    pos2(center.x - half, center.y + half),
                    pos2(center.x + half, center.y - half),
                ],
                stroke,
            );
        }
    }
}

fn logo_texture(ctx: &egui::Context) -> egui::TextureHandle {
    let id = egui::Id::new("title_bar_logo");
    if let Some(texture) = ctx.data(|d| d.get_temp::<egui::TextureHandle>(id)) {
        return texture;
    }
    let image = image::load_from_memory(include_bytes!("../../../../assets/icon-256.png"))
        .expect("embedded icon PNG is valid")
        .into_rgba8();
    let size = [image.width() as usize, image.height() as usize];
    let color_image = egui::ColorImage::from_rgba_unmultiplied(size, image.as_raw());
    let texture = ctx.load_texture("title-bar-logo", color_image, egui::TextureOptions::LINEAR);
    ctx.data_mut(|d| d.insert_temp(id, texture.clone()));
    texture
}

/// Undecorated windows keep no system resize border (winit strips
/// `WS_SIZEBOX` on Windows and compositors provide none on Linux), so thin
/// drag zones along the window edges start an OS resize instead.
pub(super) fn resize_zones(ctx: &egui::Context) {
    let (maximized, fullscreen) = ctx.input(|i| {
        let viewport = i.viewport();
        (
            viewport.maximized.unwrap_or(false),
            viewport.fullscreen.unwrap_or(false),
        )
    });
    if maximized || fullscreen {
        return;
    }
    use egui::ResizeDirection as Dir;
    const EDGE: f32 = 5.0;
    const CORNER: f32 = 12.0;
    let rect = ctx.content_rect();
    let (left, right, top, bottom) = (rect.left(), rect.right(), rect.top(), rect.bottom());
    let zones = [
        (
            Dir::NorthWest,
            Rect::from_min_size(rect.min, Vec2::splat(CORNER)),
        ),
        (
            Dir::NorthEast,
            Rect::from_min_size(pos2(right - CORNER, top), Vec2::splat(CORNER)),
        ),
        (
            Dir::SouthWest,
            Rect::from_min_size(pos2(left, bottom - CORNER), Vec2::splat(CORNER)),
        ),
        (
            Dir::SouthEast,
            Rect::from_min_size(pos2(right - CORNER, bottom - CORNER), Vec2::splat(CORNER)),
        ),
        (
            Dir::North,
            Rect::from_min_max(pos2(left + CORNER, top), pos2(right - CORNER, top + EDGE)),
        ),
        (
            Dir::South,
            Rect::from_min_max(
                pos2(left + CORNER, bottom - EDGE),
                pos2(right - CORNER, bottom),
            ),
        ),
        (
            Dir::West,
            Rect::from_min_max(pos2(left, top + CORNER), pos2(left + EDGE, bottom - CORNER)),
        ),
        (
            Dir::East,
            Rect::from_min_max(
                pos2(right - EDGE, top + CORNER),
                pos2(right, bottom - CORNER),
            ),
        ),
    ];
    egui::Area::new(egui::Id::new("window_resize_zones"))
        .order(egui::Order::Foreground)
        // Areas are movable by default; a movable area would register its own
        // drag over the zones and swallow the resize interactions.
        .movable(false)
        .fixed_pos(rect.min)
        .show(ctx, |ui| {
            for (index, (direction, zone)) in zones.into_iter().enumerate() {
                let response = ui.interact(zone, ui.id().with(index), Sense::drag());
                if response.hovered() || response.dragged() {
                    ctx.set_cursor_icon(resize_cursor(direction));
                }
                if response.drag_started_by(PointerButton::Primary) {
                    ctx.send_viewport_cmd(ViewportCommand::BeginResize(direction));
                }
            }
        });
}

fn resize_cursor(direction: egui::ResizeDirection) -> egui::CursorIcon {
    use egui::{CursorIcon, ResizeDirection as Dir};
    match direction {
        Dir::North => CursorIcon::ResizeNorth,
        Dir::South => CursorIcon::ResizeSouth,
        Dir::East => CursorIcon::ResizeEast,
        Dir::West => CursorIcon::ResizeWest,
        Dir::NorthEast => CursorIcon::ResizeNorthEast,
        Dir::NorthWest => CursorIcon::ResizeNorthWest,
        Dir::SouthEast => CursorIcon::ResizeSouthEast,
        Dir::SouthWest => CursorIcon::ResizeSouthWest,
    }
}
