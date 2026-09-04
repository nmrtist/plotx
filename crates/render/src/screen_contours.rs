use crate::screen::{RenderStats, ScreenRenderDetail};
use crate::screen_contour_cache::{ContourCacheKey, screen_contour_segments_cached};
use crate::screen_lod::contour_segment_budget;
use crate::{Projector, Rect};
use egui::{Color32, Pos2, Stroke};
use plotx_figure::Figure;

#[allow(clippy::too_many_arguments)]
pub(super) fn paint_contours(
    painter: &egui::Painter,
    clipped: &egui::Painter,
    fig: &Figure,
    proj: &Projector<'_>,
    plot: Rect,
    scale: f32,
    detail: ScreenRenderDetail,
    geometry_generation: Option<u64>,
    stats: &mut Option<&mut RenderStats>,
) {
    let viewport = [fig.x.min, fig.x.max, fig.y.min, fig.y.max];
    for (contour_index, contour) in fig.contours.iter().enumerate() {
        let stroke = Stroke::new(
            contour.width * scale,
            Color32::from_rgb(contour.color.r, contour.color.g, contour.color.b),
        );
        let budget = contour_segment_budget(
            detail,
            plot.width,
            plot.height,
            painter.ctx().pixels_per_point(),
        );
        let cache_key = geometry_generation.map(|generation| {
            ContourCacheKey::new(generation, contour_index, contour.segments.len(), viewport)
        });
        let segments = screen_contour_segments_cached(
            painter.ctx(),
            cache_key,
            &contour.segments,
            detail,
            viewport,
            budget,
        );
        if let Some(stats) = stats.as_deref_mut() {
            stats.record_contour(
                segments.source_segments_scanned(),
                segments.len(),
                segments.len(),
            );
        }
        for segment in segments.iter() {
            let (ax, ay) = proj.project(segment[0]);
            let (bx, by) = proj.project(segment[1]);
            clipped.line_segment([Pos2::new(ax, ay), Pos2::new(bx, by)], stroke);
        }
    }
}
