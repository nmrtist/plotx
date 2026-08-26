use egui::{Rect, Vec2};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum HorizontalAnchor {
    Left,
    Right,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum VerticalAnchor {
    Top,
    Bottom,
}

#[derive(Clone, Copy, Debug)]
pub(super) struct CardLayout {
    pub rect: Rect,
    pub bounds: Rect,
    pub horizontal: HorizontalAnchor,
    pub vertical: VerticalAnchor,
    pub chrome_height: f32,
    pub extra_width: f32,
    pub collapsed: bool,
}

/// Rebuild a card from its preferred size and the edges fixed by the user's
/// last gesture. Viewport fitting may temporarily reduce the rendered size,
/// but never mutates that preferred size.
pub(super) fn fit_layout(
    mut layout: CardLayout,
    bounds: Rect,
    desired_size: Vec2,
    collapsed: bool,
) -> CardLayout {
    let rect = match layout.horizontal {
        HorizontalAnchor::Left => {
            let left = (layout.rect.left() + bounds.left() - layout.bounds.left())
                .clamp(bounds.left(), bounds.right());
            let width = desired_size.x.min((bounds.right() - left).max(1.0));
            Rect::from_x_y_ranges(left..=left + width, layout.rect.y_range())
        }
        HorizontalAnchor::Right => {
            let right = (layout.rect.right() + bounds.right() - layout.bounds.right())
                .clamp(bounds.left(), bounds.right());
            let width = desired_size.x.min((right - bounds.left()).max(1.0));
            Rect::from_x_y_ranges(right - width..=right, layout.rect.y_range())
        }
    };
    layout.rect = match layout.vertical {
        VerticalAnchor::Top => {
            let top = (rect.top() + bounds.top() - layout.bounds.top())
                .clamp(bounds.top(), bounds.bottom());
            let height = desired_size.y.min((bounds.bottom() - top).max(1.0));
            Rect::from_x_y_ranges(rect.x_range(), top..=top + height)
        }
        VerticalAnchor::Bottom => {
            let bottom = (rect.bottom() + bounds.bottom() - layout.bounds.bottom())
                .clamp(bounds.top(), bounds.bottom());
            let height = desired_size.y.min((bottom - bounds.top()).max(1.0));
            Rect::from_x_y_ranges(rect.x_range(), bottom - height..=bottom)
        }
    };
    layout.bounds = bounds;
    layout.collapsed = collapsed;
    layout
}
