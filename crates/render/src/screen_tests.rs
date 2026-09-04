use super::ScreenRenderDetail;
use crate::screen_lod::{
    FULL_MAX_LINE_COLUMNS, FULL_MIN_LINE_COLUMNS, contour_segment_budget, line_columns,
    screen_contour_segments, screen_line_points,
};

#[test]
fn editor_keeps_outside_page_items_visible_while_document_rendering_clips() {
    use plotx_figure::Color;
    let document = crate::Document {
        width: 100.0,
        height: 80.0,
        background: Color::rgb(255, 255, 255),
        items: vec![crate::DocumentItem::Overlay(crate::DocumentOverlay {
            frame: crate::Rect::new(120.0, 10.0, 30.0, 20.0),
            visible: true,
            kind: crate::OverlayKind::Shape(crate::OverlayShape {
                shape: crate::OverlayShapeKind::Rect,
                stroke: Color::BLACK,
                stroke_width: 1.0,
                fill: Some(Color::BLACK),
            }),
        })],
    };
    let render = |editor| {
        let ctx = egui::Context::default();
        let output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(300.0, 200.0),
                )),
                ..Default::default()
            },
            |ui| {
                let screen = crate::Rect::new(0.0, 0.0, 300.0, 200.0);
                let viewport = crate::DocumentViewport {
                    zoom: 1.0,
                    pan: [0.0, 0.0],
                };
                if editor {
                    super::paint_document_for_editor(ui.painter(), screen, &document, viewport);
                } else {
                    super::paint_document(ui.painter(), screen, &document, viewport);
                }
            },
        );
        output
            .shapes
            .iter()
            .filter(|shape| matches!(shape.shape, egui::Shape::Rect(_)))
            .map(|shape| shape.clip_rect.max.x)
            .next_back()
            .unwrap()
    };

    assert_eq!(render(false), 100.0);
    assert!(render(true) >= 150.0);
}

#[test]
fn hidden_axis_text_keeps_screen_axis_and_tick_shapes() {
    use plotx_figure::{Axis, Figure};
    let mut fig = Figure::new(
        "",
        Axis::new("UNIQUE_X_TITLE", 0.0, 90_000.0),
        Axis::new("UNIQUE_Y_TITLE", -90_000.0, 90_000.0),
    );
    fig.x.show_tick_labels = false;
    fig.x.show_label = false;
    fig.y.show_tick_labels = false;
    fig.y.show_label = false;
    let ctx = egui::Context::default();
    let output = ctx.run_ui(egui::RawInput::default(), |ui| {
        super::paint(
            ui.painter(),
            crate::Rect::new(0.0, 0.0, 400.0, 300.0),
            &fig,
            1.0,
        );
    });
    let text = output
        .shapes
        .iter()
        .filter(|shape| matches!(shape.shape, egui::Shape::Text(_)))
        .count();
    let lines = output
        .shapes
        .iter()
        .filter(|shape| matches!(shape.shape, egui::Shape::LineSegment { .. }))
        .count();
    assert_eq!(text, 0);
    assert!(lines > 2, "axis and tick marks remain on screen");
}

#[test]
fn long_trace_is_bounded_and_keeps_narrow_extrema() {
    let mut points: Vec<_> = (0..2_000_000).map(|index| [index as f64, 0.0]).collect();
    points[1_234_567][1] = -42.0;
    let pooled = screen_line_points(&points, 0.0, 2_000_000.0, 2_000);
    assert!(pooled.points.len() <= 4_002);
    assert!(pooled.points.iter().any(|point| point[1] == -42.0));
}

#[test]
fn spectrum_sized_trace_is_pooled_to_the_screen_budget() {
    let points: Vec<_> = (0..32_768)
        .map(|index| [index as f64, (index % 7) as f64])
        .collect();
    let drawn = screen_line_points(&points, 0.0, 32_768.0, FULL_MIN_LINE_COLUMNS);
    assert!(drawn.points.len() <= FULL_MIN_LINE_COLUMNS * 2 + 2);
    assert!(matches!(drawn.points, std::borrow::Cow::Owned(_)));
}

#[test]
fn short_trace_keeps_its_real_samples() {
    let points: Vec<_> = (0..2_000)
        .map(|index| [index as f64, (index % 7) as f64])
        .collect();
    let drawn = screen_line_points(&points, 0.0, 2_000.0, FULL_MIN_LINE_COLUMNS);
    assert!(drawn.points.as_ref() == points.as_slice());
    assert!(matches!(drawn.points, std::borrow::Cow::Borrowed(_)));
}

#[test]
fn pooling_keeps_positive_and_negative_extrema() {
    let mut points: Vec<_> = (0..20_000).map(|index| [index as f64, 0.0]).collect();
    points[4_321][1] = -17.0;
    points[12_345][1] = 23.0;
    let columns = line_columns(ScreenRenderDetail::Interactive, 320.0, 1.0);
    let drawn = screen_line_points(&points, 0.0, 20_000.0, columns);
    assert!(drawn.points.iter().any(|point| point[1] == -17.0));
    assert!(drawn.points.iter().any(|point| point[1] == 23.0));
}

#[test]
fn zoomed_view_keeps_only_visible_samples() {
    let points: Vec<_> = (0..100_000).map(|index| [index as f64, 1.0]).collect();
    let drawn = screen_line_points(&points, 40_000.0, 41_000.0, FULL_MIN_LINE_COLUMNS);
    assert!(drawn.points.len() < 1_100);
    assert!(drawn.points.first().unwrap()[0] < 40_000.0);
    assert!(drawn.points.last().unwrap()[0] > 41_000.0);
}

#[test]
fn descending_x_view_clips_like_nmr_ppm() {
    let points: Vec<_> = (0..100_000)
        .map(|index| [(100_000 - index) as f64, 1.0])
        .collect();
    let drawn = screen_line_points(&points, 40_000.0, 41_000.0, FULL_MIN_LINE_COLUMNS);
    assert!(drawn.points.len() < 1_100);
    assert!(drawn.points.first().unwrap()[0] > 41_000.0);
    assert!(drawn.points.last().unwrap()[0] < 40_000.0);
}

#[test]
fn non_monotonic_x_is_pooled_without_unsafe_clipping() {
    let mut points: Vec<_> = (0..20_000)
        .map(|index| [((index * 37) % 101) as f64, index as f64])
        .collect();
    let first_x = points[0][0];
    points.last_mut().unwrap()[0] = first_x;
    let drawn = screen_line_points(&points, 4_000.0, 5_000.0, FULL_MIN_LINE_COLUMNS);
    assert!(drawn.points.len() <= FULL_MIN_LINE_COLUMNS * 2 + 2);
    assert_eq!(drawn.points.first(), points.first());
    assert_eq!(drawn.points.last(), points.last());
}

#[test]
fn columns_track_device_pixels_within_bounds() {
    assert_eq!(
        line_columns(ScreenRenderDetail::Full, 320.0, 1.0),
        FULL_MIN_LINE_COLUMNS
    );
    assert_eq!(line_columns(ScreenRenderDetail::Full, 900.0, 2.0), 3_600);
    assert_eq!(
        line_columns(ScreenRenderDetail::Full, 9_000.0, 2.0),
        FULL_MAX_LINE_COLUMNS
    );
    assert_eq!(
        line_columns(ScreenRenderDetail::Interactive, 320.1, 1.0),
        321
    );
    assert_eq!(line_columns(ScreenRenderDetail::Interactive, 20.0, 1.0), 64);
}

#[test]
fn interactive_contour_budget_is_stable_and_prioritizes_visible_segments() {
    let source_len = 250_000;
    let max_budget = contour_segment_budget(ScreenRenderDetail::Interactive, 512.0, 256.0, 1.0);
    assert_eq!(max_budget, 16_384);
    assert_eq!(
        contour_segment_budget(ScreenRenderDetail::Full, 512.0, 256.0, 1.0),
        usize::MAX
    );
    let segments = (0..source_len)
        .map(|index| {
            let x = if (120_000..121_000).contains(&index) {
                (index - 120_000) as f64 / 1_000.0
            } else {
                10.0
            };
            [[x, 0.2], [x, 0.8]]
        })
        .collect::<Vec<_>>();
    let selected = || {
        screen_contour_segments(
            &segments,
            ScreenRenderDetail::Interactive,
            [0.0, 1.0, 0.0, 1.0],
            512,
        )
    };
    let first = selected();
    let first_x = first
        .iter()
        .map(|segment| segment[0][0])
        .collect::<Vec<_>>();
    let second = selected();
    let second_x = second
        .iter()
        .map(|segment| segment[0][0])
        .collect::<Vec<_>>();
    assert_eq!(first.source_segments_scanned(), source_len);
    assert_eq!(first.len(), 512);
    assert_eq!(first_x, second_x);
    assert!(first_x.iter().all(|x| (0.0..=1.0).contains(x)));

    let full = screen_contour_segments(
        &segments,
        ScreenRenderDetail::Full,
        [0.0, 1.0, 0.0, 1.0],
        512,
    );
    assert_eq!(full.len(), source_len);
}

#[test]
fn render_stats_separate_full_and_interactive_work() {
    use plotx_figure::{Axis, Color, Contour, Figure, Series};
    let mut fig = Figure::new("", Axis::new("x", 0.0, 10_000.0), Axis::new("y", -1.0, 1.0));
    fig.series.push(Series::line(
        "trace",
        (0..10_000).map(|i| [i as f64, (i % 3) as f64]).collect(),
    ));
    fig.contours.push(Contour {
        segments: (0..1_000)
            .map(|i| [[i as f64, 0.0], [i as f64, 1.0]])
            .collect(),
        color: Color::BLACK,
        width: 1.0,
    });
    let ctx = egui::Context::default();
    let full_stats = render_stats(&ctx, &fig, 7, ScreenRenderDetail::Full);
    assert_eq!(full_stats.full_documents_painted, 1);
    assert_eq!(full_stats.interactive_documents_painted, 0);
    assert_eq!(full_stats.contour_source_segments_scanned, 1_000);
    assert_eq!(full_stats.contour_segments_submitted, 1_000);

    let stats = render_stats(&ctx, &fig, 7, ScreenRenderDetail::Interactive);
    assert_eq!(stats.documents_painted, 1);
    assert_eq!(stats.full_documents_painted, 0);
    assert_eq!(stats.interactive_documents_painted, 1);
    assert_eq!(stats.line_series_visited, 1);
    assert_eq!(stats.line_source_points_visited, 10_000);
    assert!(stats.line_points_submitted < 1_000);
    assert_eq!(stats.contour_source_segments_scanned, 0);
    assert_eq!(stats.contour_segments_visited, 1_000);
    assert_eq!(stats.contour_segments_submitted, 1_000);

    let repeated = render_stats(&ctx, &fig, 7, ScreenRenderDetail::Interactive);
    assert_eq!(repeated.contour_source_segments_scanned, 0);

    fig.x.max = 100.0;
    let changed_viewport = render_stats(&ctx, &fig, 7, ScreenRenderDetail::Interactive);
    assert_eq!(changed_viewport.contour_source_segments_scanned, 1_000);
    assert_eq!(changed_viewport.contour_segments_submitted, 101);

    let changed_geometry = render_stats(&ctx, &fig, 8, ScreenRenderDetail::Interactive);
    assert_eq!(changed_geometry.contour_source_segments_scanned, 1_000);
}

fn render_stats(
    ctx: &egui::Context,
    figure: &plotx_figure::Figure,
    geometry_generation: u64,
    detail: ScreenRenderDetail,
) -> super::RenderStats {
    use plotx_figure::Color;
    let document = crate::Document {
        width: 400.0,
        height: 300.0,
        background: Color::rgb(255, 255, 255),
        items: vec![crate::DocumentItem::Plot(crate::DocumentObject {
            id: "plot".into(),
            frame: crate::Rect::new(0.0, 0.0, 400.0, 300.0),
            figure,
            geometry_generation: Some(geometry_generation),
            visible: true,
            title: None,
        })],
    };
    let mut stats = super::RenderStats::default();
    let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
        super::paint_document_for_editor_with_detail_and_stats(
            ui.painter(),
            crate::Rect::new(0.0, 0.0, 400.0, 300.0),
            &document,
            crate::DocumentViewport {
                zoom: 1.0,
                pan: [0.0; 2],
            },
            detail,
            Some(&mut stats),
        );
    });
    stats
}
