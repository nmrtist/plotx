use egui::{Color32, Mesh, Pos2, Ui};

use super::task_card::ResizeEdges;

const HIGHLIGHT_RADIUS: f32 = 90.0;
const HIGHLIGHT_STEP: f32 = 6.0;

pub(super) fn paint_feedback(
    ui: &Ui,
    card: egui::Rect,
    hit_rect: egui::Rect,
    edges: ResizeEdges,
    response: &egui::Response,
) {
    if !response.hovered() && !response.dragged() {
        return;
    }
    let pointer = if response.dragged() {
        response
            .interact_pointer_pos()
            .or_else(|| ui.ctx().pointer_interact_pos())
    } else {
        ui.ctx().pointer_hover_pos()
    };
    let Some(pointer) = pointer.filter(|point| response.dragged() || hit_rect.contains(*point))
    else {
        return;
    };
    let color = if ui.visuals().dark_mode {
        Color32::from_gray(if response.dragged() { 112 } else { 88 })
    } else {
        Color32::from_gray(if response.dragged() { 145 } else { 174 })
    };
    if edges.left {
        paint_line(ui, card.left(), pointer.y, card.y_range(), true, color);
    }
    if edges.right {
        paint_line(ui, card.right(), pointer.y, card.y_range(), true, color);
    }
    if edges.top {
        paint_line(ui, card.top(), pointer.x, card.x_range(), false, color);
    }
    if edges.bottom {
        paint_line(ui, card.bottom(), pointer.x, card.x_range(), false, color);
    }
}

fn paint_line(
    ui: &Ui,
    fixed: f32,
    pointer: f32,
    range: egui::Rangef,
    vertical: bool,
    color: Color32,
) {
    let min = range.min.max(pointer - HIGHLIGHT_RADIUS);
    let max = range.max.min(pointer + HIGHLIGHT_RADIUS);
    if min >= max {
        return;
    }
    let segments = ((max - min) / HIGHLIGHT_STEP).ceil().max(1.0) as usize;
    let mut mesh = Mesh::default();
    for index in 0..=segments {
        let along = egui::lerp(min..=max, index as f32 / segments as f32);
        let t = ((along - pointer).abs() / HIGHLIGHT_RADIUS).clamp(0.0, 1.0);
        let faded = color.linear_multiply(1.0 - t * t * (3.0 - 2.0 * t));
        let vertex = mesh.vertices.len() as u32;
        let (a, b) = if vertical {
            (Pos2::new(fixed - 0.5, along), Pos2::new(fixed + 0.5, along))
        } else {
            (Pos2::new(along, fixed - 0.5), Pos2::new(along, fixed + 0.5))
        };
        mesh.colored_vertex(a, faded);
        mesh.colored_vertex(b, faded);
        if index > 0 {
            mesh.add_triangle(vertex - 2, vertex - 1, vertex);
            mesh.add_triangle(vertex, vertex - 1, vertex + 1);
        }
    }
    ui.painter().add(mesh);
}
