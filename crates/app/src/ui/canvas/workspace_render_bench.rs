use plotx_core::state::{PlotxApp, build_render_document};
use plotx_render::screen::{RenderStats, ScreenRenderDetail};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

const SCREEN_WIDTH: f32 = 1_600.0;
const SCREEN_HEIGHT: f32 = 1_000.0;
const WARMUP_FRAMES: usize = 3;
const MEASURED_FRAMES: usize = 30;

#[test]
#[ignore = "requires PLOTX_BENCH_NMR_DIR and a release build"]
fn bench_multi_page_nmr_workspace_navigation() {
    let Ok(directory) = std::env::var("PLOTX_BENCH_NMR_DIR") else {
        println!("PLOTX_BENCH_NMR_DIR not set; skipping benchmark");
        return;
    };
    let paths = sorted_jdf_paths(Path::new(&directory));
    assert!(
        !paths.is_empty(),
        "benchmark directory contains no JDF files"
    );

    let mut app = PlotxApp::new_with_settings(plotx_core::settings::Settings::default());
    for path in &paths {
        let before = app.doc.datasets.len();
        app.load_from(path);
        assert_eq!(
            app.doc.datasets.len(),
            before + 1,
            "failed to load {}: {}",
            path.display(),
            app.session.status
        );
    }
    wait_for_compute(&mut app);

    println!(
        "loaded {} JDF file(s) into {} workspace page(s)",
        paths.len(),
        app.doc.canvases.len()
    );
    run_detail(&app, ScreenRenderDetail::Full);
    run_detail(&app, ScreenRenderDetail::Interactive);
}

fn sorted_jdf_paths(directory: &Path) -> Vec<PathBuf> {
    let mut paths = std::fs::read_dir(directory)
        .unwrap_or_else(|error| panic!("read {}: {error}", directory.display()))
        .map(|entry| entry.expect("read benchmark directory entry").path())
        .filter(|path| {
            path.extension()
                .and_then(|extension| extension.to_str())
                .is_some_and(|extension| extension.eq_ignore_ascii_case("jdf"))
        })
        .collect::<Vec<_>>();
    paths.sort();
    paths
}

fn wait_for_compute(app: &mut PlotxApp) {
    let start = Instant::now();
    while app.compute_busy() {
        assert!(
            start.elapsed() < Duration::from_secs(600),
            "background processing did not finish within ten minutes"
        );
        app.poll_compute();
        std::thread::sleep(Duration::from_millis(5));
    }
    // The completion that rebuilds a figure may enqueue its contour geometry.
    while app.poll_compute() {
        assert!(
            start.elapsed() < Duration::from_secs(600),
            "contour generation did not finish within ten minutes"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn run_detail(app: &PlotxApp, detail: ScreenRenderDetail) {
    let ctx = egui::Context::default();
    let mut samples = Vec::with_capacity(MEASURED_FRAMES);
    let mut total_stats = RenderStats::default();
    for frame in 0..(WARMUP_FRAMES + MEASURED_FRAMES) {
        let pan = [frame as f32 * 3.0, frame as f32 * -2.0];
        let mut frame_stats = RenderStats::default();
        let start = Instant::now();
        let output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(SCREEN_WIDTH, SCREEN_HEIGHT),
                )),
                ..Default::default()
            },
            |ui| {
                let screen = plotx_render::Rect::new(0.0, 0.0, SCREEN_WIDTH, SCREEN_HEIGHT);
                for canvas in &app.doc.canvases {
                    let document = build_render_document(canvas);
                    plotx_render::screen::paint_document_for_editor_with_detail_and_stats(
                        ui.painter(),
                        screen,
                        &document,
                        plotx_render::DocumentViewport {
                            zoom: 1.0,
                            pan: [canvas.board_pos[0] + pan[0], canvas.board_pos[1] + pan[1]],
                        },
                        detail,
                        Some(&mut frame_stats),
                    );
                }
            },
        );
        let primitives = ctx.tessellate(output.shapes, output.pixels_per_point);
        std::hint::black_box(primitives.len());
        let elapsed = start.elapsed();
        if frame >= WARMUP_FRAMES {
            samples.push(elapsed);
            add_stats(&mut total_stats, &frame_stats);
        }
    }
    samples.sort_unstable();
    let median = samples[samples.len() / 2];
    let p95 = samples[(samples.len() * 95).div_ceil(100).saturating_sub(1)];
    println!(
        "{detail:?}: median={median:?} p95={p95:?} documents={} line visited/submitted={}/{} contour scanned/visited/submitted={}/{}/{}",
        total_stats.documents_painted,
        total_stats.line_source_points_visited,
        total_stats.line_points_submitted,
        total_stats.contour_source_segments_scanned,
        total_stats.contour_segments_visited,
        total_stats.contour_segments_submitted,
    );
}

fn add_stats(total: &mut RenderStats, frame: &RenderStats) {
    total.documents_painted += frame.documents_painted;
    total.full_documents_painted += frame.full_documents_painted;
    total.interactive_documents_painted += frame.interactive_documents_painted;
    total.line_series_visited += frame.line_series_visited;
    total.line_source_points_scanned += frame.line_source_points_scanned;
    total.line_points_emitted += frame.line_points_emitted;
    total.line_source_points_visited += frame.line_source_points_visited;
    total.line_points_submitted += frame.line_points_submitted;
    total.contour_source_segments_scanned += frame.contour_source_segments_scanned;
    total.contour_segments_visited += frame.contour_segments_visited;
    total.contour_segments_submitted += frame.contour_segments_submitted;
}
