use egui::{Color32, CornerRadius, FontId, Id, Pos2, Rect, Sense, Stroke, Ui, Vec2};
use plotx_core::state::PrimaryView;

const PILL_SPRING_RESPONSE: f32 = 0.34;
const PILL_SPRING_DAMPING: f32 = 0.82;
const TRACK_HEIGHT: f32 = 34.0;
const SEGMENT_INSET: f32 = 3.0;

#[derive(Clone, Copy)]
struct Spring {
    pos: f32,
    vel: f32,
}

/// A critically-ish damped spring integrated once per frame, keyed in egui's
/// frame-local store by `id`. Re-targeting mid-flight carries the momentum, so a
/// quick re-click redirects the glide instead of snapping.
pub(crate) fn animate_spring(ctx: &egui::Context, id: Id, target: f32, dt: f32) -> f32 {
    let omega = std::f32::consts::TAU / PILL_SPRING_RESPONSE.max(1e-4);
    let k = omega * omega;
    let c = 2.0 * PILL_SPRING_DAMPING * omega;
    let mut s = ctx
        .data_mut(|d| d.get_temp::<Spring>(id))
        .unwrap_or(Spring {
            pos: target,
            vel: 0.0,
        });
    let dt = dt.clamp(0.0, 1.0 / 30.0);
    let accel = -k * (s.pos - target) - c * s.vel;
    s.vel += accel * dt;
    s.pos += s.vel * dt;
    if (s.pos - target).abs() < 5e-4 && s.vel.abs() < 5e-4 {
        s.pos = target;
        s.vel = 0.0;
    } else {
        ctx.request_repaint();
    }
    ctx.data_mut(|d| d.insert_temp(id, s));
    s.pos
}

/// Seed a spring's stored position without disturbing its velocity, so the next
/// `animate_spring` glides from `pos` (the current value) rather than snapping to
/// a fresh target. Preserving velocity lets a re-seed mid-flight keep momentum.
pub(crate) fn seed_spring(ctx: &egui::Context, id: Id, pos: f32) {
    let vel = ctx
        .data_mut(|d| d.get_temp::<Spring>(id))
        .map_or(0.0, |s| s.vel);
    ctx.data_mut(|d| d.insert_temp(id, Spring { pos, vel }));
}

fn lerp_color(a: Color32, b: Color32, t: f32) -> Color32 {
    let t = t.clamp(0.0, 1.0);
    let l = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color32::from_rgba_unmultiplied(
        l(a.r(), b.r()),
        l(a.g(), b.g()),
        l(a.b(), b.b()),
        l(a.a(), b.a()),
    )
}

/// A segmented control with a sliding, spring-animated selection pill. Returns
/// `true` if the selection changed this frame.
pub fn segmented(ui: &mut Ui, current: &mut PrimaryView) -> bool {
    let dark = ui.visuals().dark_mode;
    let track_fill = if dark {
        Color32::from_white_alpha(20)
    } else {
        Color32::from_black_alpha(14)
    };
    let card_fill = if dark {
        ui.visuals().widgets.active.bg_fill
    } else {
        Color32::WHITE
    };
    let text_strong = ui.visuals().strong_text_color();
    let text_muted = ui.visuals().weak_text_color();

    let width = ui.available_width();
    let (track_rect, _) = ui.allocate_exact_size(Vec2::new(width, TRACK_HEIGHT), Sense::hover());
    let track_radius = CornerRadius::same(9);
    let segment_radius = CornerRadius::same(6);

    let painter = ui.painter().clone();
    painter.rect_filled(track_rect, track_radius, track_fill);

    let inner = track_rect.shrink(SEGMENT_INSET);
    let views = PrimaryView::all();
    let count = views.len() as f32;
    let slot_w = inner.width() / count;

    let active_index = views.iter().position(|v| v == current).unwrap_or(0);
    let dt = ui.input(|i| i.stable_dt);
    let pill_pos = animate_spring(
        ui.ctx(),
        Id::new("primary_view_pill"),
        active_index as f32,
        dt,
    );
    let card_rect = Rect::from_min_size(
        Pos2::new(inner.left() + pill_pos * slot_w, inner.top()),
        Vec2::new(slot_w, inner.height()),
    );
    // The pill floats like the chrome cards do: no hairline box, just a soft
    // drop shadow lifting it off the track.
    let pill_shadow = egui::epaint::Shadow {
        offset: [0, 1],
        blur: 4,
        spread: 0,
        color: Color32::from_black_alpha(if dark { 96 } else { 30 }),
    };
    painter.add(pill_shadow.as_shape(card_rect, segment_radius));
    painter.rect(
        card_rect,
        segment_radius,
        card_fill,
        Stroke::NONE,
        egui::StrokeKind::Inside,
    );

    let mut changed = false;
    for (index, view) in views.iter().enumerate() {
        let left = egui::lerp(inner.left()..=inner.right(), index as f32 / count);
        let right = egui::lerp(inner.left()..=inner.right(), (index + 1) as f32 / count);
        let rect = Rect::from_min_max(
            Pos2::new(left, inner.top()),
            Pos2::new(right, inner.bottom()),
        );
        let response = ui.interact(
            rect,
            Id::new(("primary_view_segment", index)),
            Sense::click(),
        );
        let selected = *current == *view;

        let label_t = (1.0 - (pill_pos - index as f32).abs()).clamp(0.0, 1.0);
        if !selected && label_t < 0.5 && response.hovered() {
            painter.rect_filled(rect, segment_radius, Color32::from_white_alpha(10));
        }

        let galley = painter.layout_no_wrap(
            view.label().to_owned(),
            FontId::proportional(13.0),
            lerp_color(text_muted, text_strong, label_t),
        );
        painter.galley(
            Pos2::new(
                rect.center().x - galley.size().x / 2.0,
                rect.center().y - galley.size().y / 2.0,
            ),
            galley,
            Color32::PLACEHOLDER,
        );

        if response.clicked() && !selected {
            *current = *view;
            changed = true;
        }
    }
    changed
}
