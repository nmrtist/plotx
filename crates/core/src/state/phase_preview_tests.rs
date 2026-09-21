use super::*;
use num_complex::Complex64;
use std::time::{Duration, Instant};

fn spectrum(rows: usize, cols: usize) -> Nmr2DDataset {
    let dim = plotx_io::Dim {
        spectral_width_hz: 4000.0,
        observe_freq_mhz: 400.0,
        carrier_ppm: 5.0,
        nucleus: "1H".into(),
        group_delay: 0.0,
    };
    let data = (0..rows * cols)
        .map(|i| {
            let x = (i % cols) as f64 / cols as f64;
            let y = (i / cols) as f64 / rows as f64;
            let peak = (-((x - 0.37) / 0.025).powi(2) - ((y - 0.61) / 0.04).powi(2)).exp();
            let noise = ((i.wrapping_mul(1_664_525).wrapping_add(1_013_904_223) % 65536) as f64
                / 65536.0
                - 0.5)
                * 0.001;
            Complex64::new(peak + noise, peak * (x - 0.37) * 40.0)
        })
        .collect();
    crate::nmr_test_support::load_2d(plotx_io::NmrData2D {
        data,
        rows,
        cols,
        domain: plotx_io::Domain::Frequency,
        direct: dim.clone(),
        indirect: dim,
        quad: plotx_io::QuadMode::Complex,
        indirect_conjugate: false,
        experiment: None,
        pseudo_axis: None,
        diffusion: None,
        nus: None,
        source: "Synthetic phase benchmark".into(),
    })
    .unwrap()
}

fn settle(app: &mut PlotxApp) {
    let start = Instant::now();
    while app.poll_compute() {
        assert!(
            start.elapsed() < Duration::from_secs(120),
            "{}",
            app.session.status
        );
        std::thread::sleep(Duration::from_millis(1));
    }
}

#[test]
#[ignore = "release timing benchmark; synthetic data, no external files"]
fn bench_phase_pipeline() {
    for (rows, cols) in [(512, 1024), (1024, 2048)] {
        let mut app = app_with_spectrum(rows, cols);
        app.begin_property_gesture(crate::properties::phase::PHASE0);
        let mut times = Vec::new();
        crate::contour_probe::reset();
        for i in 0..5 {
            app.doc.datasets[0]
                .phase_params_mut(PhaseAxis::F2)
                .unwrap()
                .phase0 = 0.05 * (i + 1) as f64;
            let start = Instant::now();
            app.apply_dataset_edit(0);
            settle(&mut app);
            times.push(start.elapsed().as_secs_f64() * 1000.0);
        }
        println!(
            "{rows}x{cols}: end_to_end_ms={times:?} estimates={} contours={} UI_payloads={}",
            crate::contour_probe::queued_estimates(),
            crate::contour_probe::queued_contour_builds(),
            crate::contour_probe::field_payload_materializations()
        );
        let release = Instant::now();
        app.end_property_gesture();
        settle(&mut app);
        println!(
            "release_final_ms={:.3}",
            release.elapsed().as_secs_f64() * 1000.0
        );
        benchmark_sampling_candidate(&mut app);
    }
}

fn benchmark_sampling_candidate(app: &mut PlotxApp) {
    let series = app.doc.canvases[0].objects[0]
        .plot()
        .unwrap()
        .binding
        .series[0]
        .clone();
    let plotx_figure::SeriesEncoding::Contour(spec) = series.encoding else {
        unreachable!();
    };
    let field = FieldRef {
        resource: series.source.resource,
        field: series.source.field,
    };
    let source = VersionedFieldRef {
        field,
        version: app.session.compute.current_field_version(field).unwrap(),
    };
    let grid = app.session.compute.cached_field_grid(source).unwrap();
    let summary = grid.summary().unwrap();
    let ContourResolution::Ready { levels, .. } =
        resolve_contour_levels(source, &spec, summary, |key| {
            app.session.compute.estimate_for(key).cloned()
        })
    else {
        unreachable!();
    };
    let levels = levels
        .positive
        .iter()
        .chain(levels.negative.iter())
        .map(|level| level.get())
        .collect::<Vec<_>>();
    let [x0, x1, y0, y1] = grid.linear_bounds().unwrap();
    for (rows, cols) in [
        (grid.rows, grid.cols),
        (grid.rows.min(512), grid.cols.min(512)),
    ] {
        let sampled = (0..rows)
            .flat_map(|row| {
                let grid = &grid;
                (0..cols).map(move |col| {
                    grid.values[(row * (grid.rows - 1) / (rows - 1)) * grid.cols
                        + col * (grid.cols - 1) / (cols - 1)]
                })
            })
            .collect::<Vec<_>>();
        let start = Instant::now();
        let mut segments = 0;
        for _ in 0..3 {
            segments = std::hint::black_box(plotx_render::contour::segments(
                &sampled, rows, cols, x0, x1, y0, y1, &levels,
            ))
            .len();
        }
        println!(
            "contour_sampling_candidate {rows}x{cols}: mean_ms={:.3} segments={segments}",
            start.elapsed().as_secs_f64() * 1000.0 / 3.0
        );
    }
}

fn app_with_spectrum(rows: usize, cols: usize) -> PlotxApp {
    let mut app = PlotxApp::new_with_settings(crate::settings::Settings::default());
    app.doc
        .datasets
        .push(Dataset::Nmr2D(Box::new(spectrum(rows, cols))));
    let mut canvas = CanvasDocument::new("Phase".into(), [120.0, 80.0]);
    let id = canvas.allocate_object_id();
    canvas.objects.push(app.build_plot_object(
        0,
        ObjectFrame::new(0.0, 0.0, 340.0, 220.0),
        id,
        "Spectrum".into(),
    ));
    app.doc.canvases.push(canvas);
    app.rebuild_canvas(0);
    settle(&mut app);
    app
}

fn phase(app: &mut PlotxApp, value: f64) {
    app.doc.datasets[0]
        .phase_params_mut(PhaseAxis::F2)
        .unwrap()
        .phase0 = value;
    app.apply_dataset_edit(0);
}

fn segments(app: &PlotxApp) -> Vec<ContourSegment> {
    app.doc.canvases[0].objects[0]
        .plot()
        .unwrap()
        .figure()
        .contours
        .iter()
        .flat_map(|contour| contour.segments.iter().copied())
        .collect()
}

fn assert_final_matches_fresh_build(app: &mut PlotxApp) {
    let actual = segments(app);
    let mut fresh = PlotxApp::new_with_settings(crate::settings::Settings::default());
    fresh.doc.datasets.push(app.doc.datasets[0].clone());
    fresh.doc.canvases.push(app.doc.canvases[0].clone());
    fresh.rebuild_canvas(0);
    settle(&mut fresh);
    assert_eq!(actual, segments(&fresh));
}

#[test]
fn phase_preview_freezes_levels_without_freezing_the_contour_shape() {
    let mut app = app_with_spectrum(32, 64);
    let original = segments(&app);
    assert!(!original.is_empty());
    app.begin_property_gesture(crate::properties::phase::PHASE0);
    crate::contour_probe::reset();
    phase(&mut app, 0.4);
    settle(&mut app);
    assert_ne!(segments(&app), original);
    assert_eq!(crate::contour_probe::queued_estimates(), 0);
    assert_eq!(crate::contour_probe::queued_contour_builds(), 1);
    assert_eq!(crate::contour_probe::field_payload_materializations(), 0);
    // No final pointer movement: release alone must restore measured thresholds.
    app.end_property_gesture();
    settle(&mut app);
    assert_eq!(crate::contour_probe::queued_estimates(), 1);
    assert_final_matches_fresh_build(&mut app);
}

#[test]
fn phase_burst_keeps_latest_input_and_final_full_precision_result() {
    let mut app = app_with_spectrum(32, 64);
    app.begin_property_gesture(crate::properties::phase::PHASE0);
    crate::contour_probe::reset();
    for i in 1..=40 {
        phase(&mut app, i as f64 * 0.02);
    }
    app.end_property_gesture();
    settle(&mut app);
    let data = app.doc.datasets[0].as_nmr2d().unwrap();
    let expected = plotx_processing::nmr_execution::execute_2d(
        &data.native_base,
        &data.params,
        plotx_processing::nmr_bridge::DelayPolicy::Disabled,
        plotx_processing::nmr_bridge::RecipeRange::Frequency,
        None,
        &mut nmr::ExecutionContext::default(),
    )
    .unwrap();
    let (Processed2D::Ft(actual), Processed2D::Ft(expected)) = (&data.processed, expected.view)
    else {
        panic!("expected a plane");
    };
    assert_eq!((actual.f1_size, actual.f2_size), (32, 64));
    assert_eq!(actual.data, expected.data);
    assert_eq!(crate::contour_probe::queued_estimates(), 1);
    assert!(
        crate::contour_probe::queued_contour_builds() <= 2,
        "burst should compute only active and latest frames"
    );
    assert_final_matches_fresh_build(&mut app);
}

#[test]
fn phase_failure_keeps_display_and_reports_the_error_after_release() {
    let mut app = app_with_spectrum(16, 32);
    let original = segments(&app);
    app.begin_property_gesture(crate::properties::phase::PHASE0);
    phase(&mut app, f64::NAN);
    settle(&mut app);
    app.end_property_gesture();
    settle(&mut app);
    app.poll_compute();
    assert!(
        app.session.status.contains("2D processing failed"),
        "{}",
        app.session.status
    );
    assert_eq!(segments(&app), original);
}

#[test]
fn phase_cancel_follows_dataset_identity_after_reordering() {
    let mut app = app_with_spectrum(16, 32);
    let target = app.doc.datasets[0].resource_id();
    let before = crate::actions::DatasetProcessingState::from_dataset(&app.doc.datasets[0]);
    app.set_interaction(Interaction::Phase(PhaseDrag {
        kind: PhaseDragKind::Ph0,
        dataset: target,
        axis: PhaseAxis::F2,
        preview_pivot_ppm: None,
        gesture_before: before,
    }));
    phase(&mut app, 0.6);
    app.doc
        .datasets
        .push(Dataset::Nmr2D(Box::new(spectrum(8, 16))));
    app.doc.datasets.swap(0, 1);
    app.cancel_interaction();
    settle(&mut app);
    assert_eq!(app.doc.datasets[1].resource_id(), target);
    assert_eq!(
        app.doc.datasets[1]
            .phase_params_mut(PhaseAxis::F2)
            .unwrap()
            .phase0,
        0.0
    );
}

#[test]
fn phase_preview_draws_negative_lobes_that_were_absent_at_drag_start() {
    let mut app = app_with_spectrum(32, 64);
    app.begin_property_gesture(crate::properties::phase::PHASE0);
    crate::contour_probe::reset();
    phase(&mut app, 2.5);
    settle(&mut app);
    let plot = app.doc.canvases[0].objects[0].plot().unwrap();
    let plotx_figure::SeriesEncoding::Contour(spec) = &plot.binding.series[0].encoding else {
        panic!("expected contours");
    };
    assert!(plot.figure().contours.iter().any(|contour| contour.color
        == spec.style.negative_color.resolve()
        && !contour.segments.is_empty()));
    assert_eq!(crate::contour_probe::queued_estimates(), 0);
}

#[test]
fn contour_completion_does_not_rebuild_another_field_view() {
    let mut app = app_with_spectrum(16, 32);
    let resource = app.doc.datasets[0].resource_id();
    let magnitude = app.doc.datasets[0]
        .as_nmr2d()
        .unwrap()
        .field_catalog
        .id_for_key("nmr.magnitude")
        .unwrap();
    let id = app.doc.canvases[0].allocate_object_id();
    let mut object = app.build_plot_object(
        0,
        ObjectFrame::new(0.0, 0.0, 340.0, 220.0),
        id,
        "Magnitude".into(),
    );
    let plot = object.plot_mut().unwrap();
    plot.binding.series[0].source.field = magnitude;
    plot.binding.series[0].encoding = plotx_figure::SeriesEncoding::Heatmap(Default::default());
    app.doc.canvases[0].objects.push(object);
    app.rebuild_canvas(0);
    settle(&mut app);
    let generation = app.doc.canvases[0]
        .object(id)
        .unwrap()
        .plot()
        .unwrap()
        .figure_geometry_generation();
    let real = app.doc.datasets[0]
        .as_nmr2d()
        .unwrap()
        .field_catalog
        .id_for_key("nmr.real")
        .unwrap();
    let source = VersionedFieldRef {
        field: FieldRef {
            resource,
            field: real,
        },
        version: app.session.compute.reserve_field_version().unwrap(),
    };
    app.session.compute.promote_field_version(source, None);
    app.rebuild_canvases_for_field(0, source.field);
    settle(&mut app);
    assert_eq!(
        app.doc.canvases[0]
            .object(id)
            .unwrap()
            .plot()
            .unwrap()
            .figure_geometry_generation(),
        generation
    );
}

#[test]
#[ignore = "release continuous-input benchmark; synthetic data"]
fn bench_phase_continuous_input() {
    for (rows, cols) in [(512, 1024), (1024, 2048)] {
        let mut app = app_with_spectrum(rows, cols);
        app.begin_property_gesture(crate::properties::phase::PHASE0);
        crate::contour_probe::reset();
        let start = Instant::now();
        let mut next_input = Duration::ZERO;
        let mut last_frame = start;
        let mut intervals = Vec::new();
        let mut inputs = 0;
        let mut generation = app.doc.canvases[0].objects[0]
            .plot()
            .unwrap()
            .figure_geometry_generation();
        while start.elapsed() < Duration::from_secs(3) {
            if start.elapsed() >= next_input {
                inputs += 1;
                phase(&mut app, inputs as f64 * 0.001);
                next_input += Duration::from_micros(16_667);
            }
            app.poll_compute();
            let next = app.doc.canvases[0].objects[0]
                .plot()
                .unwrap()
                .figure_geometry_generation();
            if next != generation {
                intervals.push(last_frame.elapsed().as_secs_f64() * 1000.0);
                last_frame = Instant::now();
                generation = next;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        let release = Instant::now();
        app.end_property_gesture();
        settle(&mut app);
        intervals.sort_by(f64::total_cmp);
        println!(
            "continuous {rows}x{cols}: inputs={inputs} frames={} median_interval_ms={:.3} max_interval_ms={:.3} release_final_ms={:.3} estimates={} contours={}",
            intervals.len(),
            intervals[intervals.len() / 2],
            intervals.last().unwrap(),
            release.elapsed().as_secs_f64() * 1000.0,
            crate::contour_probe::queued_estimates(),
            crate::contour_probe::queued_contour_builds()
        );
        assert_eq!(
            app.doc.datasets[0]
                .phase_params_mut(PhaseAxis::F2)
                .unwrap()
                .phase0,
            inputs as f64 * 0.001
        );
    }
}
