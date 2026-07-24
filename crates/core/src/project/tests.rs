use super::linefit_tests::sample_line_fit;
use super::*;
use crate::state::{
    AnalysisSelection, AxisOverrides, CanvasDocument, CanvasViewport, Dataset, Tool,
};
use crate::state::{PeakMark, PeakOrigin};
use crate::{DisplayModeLabel, IntegralResult};
use std::f64::consts::TAU;

pub(super) fn synthetic_1d() -> NmrData {
    let npoints = 1024;
    let sw = 4000.0;
    let obs = 400.0;
    let carrier = 5.0;
    let dt = 1.0 / sw;
    let points = (0..npoints)
        .map(|k| {
            let t = k as f64 * dt;
            let decay = (-t / 0.4).exp();
            let freq_hz = (2.0 - carrier) * obs;
            Complex64::from_polar(decay, TAU * freq_hz * t)
        })
        .collect();
    NmrData {
        points,
        domain: Domain::Time,
        spectral_width_hz: sw,
        observe_freq_mhz: obs,
        carrier_ppm: carrier,
        nucleus: "1H".to_owned(),
        source: "synthetic".to_owned(),
        group_delay: 0.0,
    }
}

// A small synthetic DOSY array: a decaying resonance whose amplitude follows a
// Stejskal–Tanner decay across 8 gradient steps, carrying a gradient ruler and
// diffusion metadata so `is_pseudo()` holds.
fn synthetic_dosy_2d() -> NmrData2D {
    let (cols, rows) = (64usize, 8usize);
    let meta = DiffusionMeta {
        gamma: 2.675_222e8,
        delta: 2e-3,
        big_delta: 0.1,
        tau: 0.0,
        shape_factor: 1.0 / 3.0,
    };
    let direct = Dim {
        spectral_width_hz: 5000.0,
        observe_freq_mhz: 400.0,
        carrier_ppm: 5.0,
        nucleus: "1H".to_owned(),
        group_delay: 0.0,
    };
    let g: Vec<f64> = (0..rows)
        .map(|i| 0.02 + i as f64 * (0.28 - 0.02) / (rows as f64 - 1.0))
        .collect();
    let dt = 1.0 / direct.spectral_width_hz;
    let f_hz = direct.observe_freq_mhz; // 1 ppm
    let mut data = Vec::with_capacity(rows * cols);
    for &gr in &g {
        let att = (-1.2e-9 * meta.b_factor(gr)).exp();
        for j in 0..cols {
            let t = j as f64 * dt;
            let decay = (-t / 0.2).exp();
            data.push(Complex64::from_polar(att * decay, TAU * f_hz * t));
        }
    }
    NmrData2D {
        data,
        rows,
        cols,
        domain: Domain::Time,
        direct: direct.clone(),
        indirect: direct,
        quad: QuadMode::Complex,
        indirect_conjugate: false,
        experiment: Some("bpp_ste_diffusion".to_owned()),
        pseudo_axis: Some(PseudoAxis {
            name: "g".to_owned(),
            kind: PseudoKind::Gradient,
            values: g,
            unit: "mT/m".to_owned(),
            source: AxisSource::EmbeddedRamp,
        }),
        diffusion: Some(meta),
        nus: None,
        source: "synthetic DOSY".to_owned(),
    }
}

pub(super) fn sample_app() -> PlotxApp {
    let mut app = PlotxApp::new();
    let mut dataset = NmrDataset::load(synthetic_1d());
    dataset.name = Some("sample data".to_owned());
    set_manual_phase(&mut dataset.pipeline, 0.25, -0.5, 0.4);
    dataset.rebuild();
    app.doc.datasets.push(Dataset::Nmr(Box::new(dataset)));

    let chart = crate::state::ChartSpec::default_for(app.doc.datasets[0].domain());
    let mut figure = app.build_full_canvas_figure(0, &chart, [120.0, 80.0]);
    let mut viewport = CanvasViewport::from_figure(&figure);
    viewport.view_x = AxisRange::new(
        viewport.full_x.min,
        viewport.full_x.min + viewport.full_x.span() * 0.5,
    );
    viewport.auto_y = false;
    viewport.apply_to(&mut figure);
    let mut canvas = CanvasDocument::new("sample canvas".to_owned(), [120.0, 80.0]);
    let [w, h] = canvas.size_pt();
    let id = canvas.allocate_object_id();
    let mut object =
        app.build_plot_object(0, ObjectFrame::new(0.0, 0.0, w, h), id, "Plot 1".to_owned());
    let plot = object.plot_mut().unwrap();
    plot.figure = figure;
    plot.viewport = viewport;
    canvas.selected_object = Some(id);
    canvas.objects.push(object);
    app.doc.canvases.push(canvas);
    app.focus_single(0);
    app.session.active_canvas = Some(0);
    app.session.tool = Tool::SelectRegion;
    app.session.ui.analysis_selection = Some(AnalysisSelection {
        dataset: app.doc.datasets[0].resource_id(),
        canvas: app.doc.canvases[0].resource_id,
        object: id,
        x_range: AxisRange::new(1.0, 2.0),
        y_range: None,
    });
    if let Dataset::Nmr(n) = &mut app.doc.datasets[0] {
        n.peaks.marks.push(PeakMark {
            id: 0,
            x: 2.0,
            y: 42.0,
            origin: PeakOrigin::Manual,
            label: Some("2.00".to_owned()),
        });
        n.peaks.next_id = 1;
        n.integrals.push(IntegralResult {
            id: 0,
            start_ppm: 1.0,
            end_ppm: 2.0,
            area: 3.0,
            normalized_area: 0.5,
            mode: DisplayModeLabel::Real,
            reference_value: None,
        });
        n.line_fits.push(sample_line_fit());
        n.next_line_fit_id = 8;
    }
    app
}

fn set_manual_phase(pipe: &mut AxisPipeline, phase0: f64, phase1: f64, pivot_frac: f64) {
    for step in &mut pipe.steps {
        if let StepKind::Phase(p) = &mut step.kind {
            *p = PhaseParams {
                phase0,
                phase1,
                pivot_frac,
                auto: None,
            };
        }
    }
}

fn first_phase(pipe: &AxisPipeline) -> Option<PhaseParams> {
    pipe.steps.iter().find_map(|s| match &s.kind {
        StepKind::Phase(p) => Some(*p),
        _ => None,
    })
}

pub(super) fn first_plot(app: &PlotxApp) -> &crate::state::PlotObject {
    app.doc.canvases[0].objects[0].plot().unwrap()
}

pub(super) fn first_plot_mut(app: &mut PlotxApp) -> &mut crate::state::PlotObject {
    app.doc.canvases[0].objects[0].plot_mut().unwrap()
}

pub(super) fn temp_project(name: &str) -> PathBuf {
    let base = std::env::var_os("CARGO_TARGET_TMPDIR")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    base.join(format!("plotx-{name}-{}.plotx", std::process::id()))
}

pub(super) fn temp_scheme(name: &str) -> PathBuf {
    let base = std::env::var_os("CARGO_TARGET_TMPDIR")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    base.join(format!("plotx-{name}-{}.plotxproc", std::process::id()))
}

#[test]
fn tool_survives_string_roundtrip() {
    for tool in [
        Tool::Select,
        Tool::BrowseZoom,
        Tool::ManualPhase,
        Tool::Arrow,
    ] {
        assert_eq!(tool_from_str(tool_to_str(tool)), tool);
    }
}

#[test]
fn view_layout_without_grid_field_defaults_to_none() {
    let view: ViewLayout = serde_json::from_str(r#"{"size_mm":[120.0,80.0]}"#).unwrap();
    assert!(view.grid.is_none());
}

#[test]
fn view_layout_without_board_pos_lands_on_grid_slot() {
    let view: ViewLayout = serde_json::from_str(r#"{"size_mm":[120.0,80.0]}"#).unwrap();
    assert!(view.board_pos.is_none());
    let placed = view
        .board_pos
        .unwrap_or_else(|| crate::state::default_board_layout(4));
    assert_eq!(placed, crate::state::default_board_layout(4));
    assert_ne!(placed, [0.0, 0.0]);
}

#[test]
fn project_roundtrip_preserves_data_recipe_and_view() {
    let mut app = sample_app();
    app.doc.canvases[0].layout = PageLayout {
        margin_mm: [11.0, 4.0, 9.0, 6.0],
        gutter_mm: 7.0,
        rows: 2,
        cols: 3,
        show_grid: true,
        spacing_mode: crate::layout::SpacingMode::Visual,
    };
    app.doc.canvases[0].board_pos = [780.0, 123.0];
    app.doc.canvases[0].caption = "Fig 1. Sample spectrum.".to_owned();
    app.doc.canvases[0].caption_visible = false;
    app.doc.canvases[0].panel_label_style = crate::state::PanelLabelStyle::UpperAlpha;
    app.session.board = crate::state::BoardViewport {
        zoom: 1.75,
        pan: [-40.0, 12.0],
        auto_fit: true,
    };
    app.session.board_views = vec![
        crate::state::NamedView {
            name: "overview".to_owned(),
            zoom: 0.5,
            pan: [10.0, 20.0],
        },
        crate::state::NamedView {
            name: "detail".to_owned(),
            zoom: 3.0,
            pan: [-5.0, -8.0],
        },
    ];
    first_plot_mut(&mut app).panel.user_note = "custom title\nHSQC summary".to_owned();
    first_plot_mut(&mut app).panel.position = [33.0, 14.0];
    first_plot_mut(&mut app).panel.visible = false;
    let axis_overrides = AxisOverrides {
        x_label: Some("Chemical shift".to_owned()),
        y_label: Some("Response".to_owned()),
        x_range: Some(AxisRange::new(1.0, 8.0)),
        y_range: Some(AxisRange::new(-2.0, 12.0)),
        ..AxisOverrides::default()
    };
    let plot_id = app.doc.canvases[0].objects[0].id;
    app.set_axis_overrides_value(0, plot_id, &axis_overrides);
    let custom_typography = plotx_figure::FigureTypography {
        tick_pt: 9.5,
        label_pt: 10.0,
        title_pt: 11.0,
    };
    app.set_figure_typography_value(custom_typography);
    let path = temp_project("roundtrip");
    let _ = std::fs::remove_file(&path);

    save_project(&app, &path, false).unwrap();
    let loaded = load_project(&path).unwrap();
    let _ = std::fs::remove_file(&path);

    assert_eq!(loaded.doc.datasets.len(), 1);
    assert_eq!(loaded.doc.canvases.len(), 1);
    assert_eq!(loaded.active_dataset(), Some(0));
    assert_eq!(loaded.session.active_canvas, Some(0));
    assert_eq!(loaded.session.tool, Tool::SelectRegion);
    assert_eq!(
        loaded.doc.canvases[0].selected_object,
        Some(loaded.doc.canvases[0].objects[0].id)
    );
    assert_eq!(
        loaded
            .session
            .ui
            .analysis_selection
            .as_ref()
            .map(|s| s.x_range),
        Some(AxisRange::new(1.0, 2.0))
    );
    assert_eq!(loaded.doc.canvases[0].name, "sample canvas");
    assert_eq!(loaded.doc.canvases[0].size_mm, [120.0, 80.0]);
    assert_eq!(loaded.doc.canvases[0].board_pos, [780.0, 123.0]);
    assert_eq!(loaded.session.board_views.len(), 2);
    assert_eq!(loaded.session.board_views[0].name, "overview");
    assert_eq!(loaded.session.board_views[1].zoom, 3.0);
    assert_eq!(loaded.session.board_views[1].pan, [-5.0, -8.0]);
    // Document typography survives the round-trip and is re-stamped onto the
    // rebuilt figures.
    assert_eq!(
        loaded.doc.style_library.figure_typography,
        custom_typography
    );
    assert_eq!(
        loaded.doc.canvases[0].objects[0]
            .plot()
            .unwrap()
            .figure
            .typography,
        custom_typography
    );
    assert_eq!(loaded.doc.canvases[0].caption, "Fig 1. Sample spectrum.");
    assert!(!loaded.doc.canvases[0].caption_visible);
    assert_eq!(
        loaded.doc.canvases[0].panel_label_style,
        crate::state::PanelLabelStyle::UpperAlpha
    );
    assert_eq!(loaded.session.board.zoom, 1.75);
    assert_eq!(loaded.session.board.pan, [-40.0, 12.0]);
    assert_eq!(
        loaded.doc.canvases[0].layout,
        PageLayout {
            margin_mm: [11.0, 4.0, 9.0, 6.0],
            gutter_mm: 7.0,
            rows: 2,
            cols: 3,
            show_grid: true,
            spacing_mode: crate::layout::SpacingMode::Visual,
        }
    );
    assert_eq!(
        first_plot(&loaded).viewport.view_x,
        first_plot(&app).viewport.view_x
    );
    assert_eq!(
        first_plot(&loaded).panel.user_note,
        "custom title\nHSQC summary"
    );
    assert_eq!(first_plot(&loaded).panel.position, [33.0, 14.0]);
    assert_eq!(first_plot(&loaded).axis_overrides, axis_overrides);
    assert_eq!(first_plot(&loaded).figure.x.label, "Chemical shift");
    assert_eq!(first_plot(&loaded).figure.y.label, "Response");
    assert_eq!(
        first_plot(&loaded).viewport.full_x,
        AxisRange::new(1.0, 8.0)
    );
    assert_eq!(
        first_plot(&loaded).viewport.full_y,
        AxisRange::new(-2.0, 12.0)
    );
    assert!(!first_plot(&loaded).viewport.auto_y);
    assert!(!first_plot(&loaded).panel.visible);

    let Dataset::Nmr(n) = &loaded.doc.datasets[0] else {
        panic!("expected 1D NMR dataset");
    };
    assert_eq!(n.name.as_deref(), Some("sample data"));
    assert_eq!(n.data.points.len(), 1024);
    assert_eq!(n.peaks.marks.len(), 1);
    assert_eq!(n.peaks.marks[0].label.as_deref(), Some("2.00"));
    assert_eq!(n.integrals.len(), 1);
    assert_eq!(n.integrals[0].area, 3.0);
    assert_eq!(n.line_fits, vec![sample_line_fit()]);
    assert_eq!(n.next_line_fit_id, 8);
    let phase = first_phase(&n.pipeline).expect("phase step survives round-trip");
    assert!(phase.auto.is_none());
    assert!((phase.phase0 - 0.25).abs() < f64::EPSILON);
    assert!((phase.phase1 + 0.5).abs() < f64::EPSILON);
    assert!((phase.pivot_frac - 0.4).abs() < f64::EPSILON);
}

fn synthetic_true_2d() -> plotx_io::NmrData2D {
    use plotx_io::{Dim, Domain, NmrData2D, QuadMode};
    let (cols, rows) = (32usize, 4usize);
    let dim = |nucleus: &str| Dim {
        spectral_width_hz: 4000.0,
        observe_freq_mhz: 400.0,
        carrier_ppm: 0.0,
        nucleus: nucleus.to_owned(),
        group_delay: 0.0,
    };
    NmrData2D {
        data: vec![num_complex::Complex64::new(0.0, 0.0); rows * cols],
        rows,
        cols,
        domain: Domain::Time,
        direct: dim("1H"),
        indirect: dim("13C"),
        quad: QuadMode::Complex,
        indirect_conjugate: false,
        experiment: None,
        pseudo_axis: None,
        diffusion: None,
        nus: None,
        source: "synthetic 2D".to_owned(),
    }
}

#[test]
fn project_roundtrip_preserves_axis_projections() {
    use crate::state::{AxisProjection, AxisProjections, ObjectFrame, ProjectionSource};

    // dataset 0 = the 1D spectrum a projection attaches to; dataset 1 = the contour.
    let mut app = sample_app();
    let ds = Nmr2DDataset::load(synthetic_true_2d());
    assert!(ds.is_true_2d());
    app.doc.datasets.push(Dataset::Nmr2D(Box::new(ds)));
    let mut canvas = CanvasDocument::new("2d".to_owned(), [120.0, 80.0]);
    let [w, h] = canvas.size_pt();
    let id = canvas.allocate_object_id();
    let mut object =
        app.build_plot_object(1, ObjectFrame::new(0.0, 0.0, w, h), id, "Plot 2".to_owned());
    object.plot_mut().unwrap().projections = AxisProjections {
        top: AxisProjection {
            source: ProjectionSource::Attached(app.doc.datasets[0].resource_id()),
            visible: true,
        },
        left: AxisProjection {
            source: ProjectionSource::Skyline,
            visible: false,
        },
    };
    canvas.objects.push(object);
    app.doc.canvases.push(canvas);

    let path = temp_project("projections");
    let _ = std::fs::remove_file(&path);
    save_project(&app, &path, false).unwrap();
    let loaded = load_project(&path).unwrap();
    let _ = std::fs::remove_file(&path);

    let proj = &loaded.doc.canvases[1].objects[0]
        .plot()
        .unwrap()
        .projections;
    assert_eq!(
        proj.top.source,
        ProjectionSource::Attached(loaded.doc.datasets[0].resource_id())
    );
    assert!(proj.top.visible);
    assert_eq!(proj.left.source, ProjectionSource::Skyline);
    assert!(!proj.left.visible);
}

#[test]
fn project_roundtrip_preserves_pseudo2d_metadata() {
    let mut app = PlotxApp::new();
    let ds = Nmr2DDataset::load(synthetic_dosy_2d());
    assert!(ds.is_pseudo(), "fixture should be a pseudo-2D dataset");
    app.doc.datasets.push(Dataset::Nmr2D(Box::new(ds)));

    let path = temp_project("pseudo2d");
    let _ = std::fs::remove_file(&path);
    save_project(&app, &path, false).unwrap();
    let loaded = load_project(&path).unwrap();
    let _ = std::fs::remove_file(&path);

    let Dataset::Nmr2D(n) = &loaded.doc.datasets[0] else {
        panic!("expected a 2D NMR dataset");
    };
    assert!(n.is_pseudo(), "round-trip must keep it a pseudo-2D dataset");
    let axis = n.data.pseudo_axis.as_ref().expect("pseudo axis preserved");
    assert_eq!(axis.name, "g");
    assert_eq!(axis.kind, PseudoKind::Gradient);
    assert_eq!(axis.unit, "mT/m");
    assert_eq!(axis.source, AxisSource::EmbeddedRamp);
    assert_eq!(axis.values.len(), 8);
    let meta = n.data.diffusion.as_ref().expect("diffusion meta preserved");
    assert!((meta.delta - 2e-3).abs() < 1e-12);
    assert!((meta.big_delta - 0.1).abs() < 1e-12);
    assert!((meta.shape_factor - 1.0 / 3.0).abs() < 1e-12);
}

#[test]
fn project_roundtrip_preserves_authoring_objects() {
    use crate::state::{CanvasObject, CanvasObjectKind, ShapeKind, ShapeObject, TextBox};

    let mut app = sample_app();
    let id_text = app.doc.canvases[0].allocate_object_id();
    app.doc.canvases[0].objects.push(CanvasObject {
        id: id_text,
        name: "Text".to_owned(),
        frame: ObjectFrame::new(10.0, 12.0, 160.0, 36.0),
        locked: false,
        visible: true,
        group: None,
        kind: CanvasObjectKind::Text(TextBox::label("Caption".to_owned())),
    });
    let id_shape = app.doc.canvases[0].allocate_object_id();
    let mut shape = ShapeObject::new(ShapeKind::Arrow);
    shape.stroke_width = 3.0;
    shape.fill = Some(Color::rgb(10, 20, 30));
    app.doc.canvases[0].objects.push(CanvasObject {
        id: id_shape,
        name: "Arrow".to_owned(),
        frame: ObjectFrame::new(40.0, 50.0, 80.0, 20.0),
        locked: false,
        visible: false,
        group: None,
        kind: CanvasObjectKind::Shape(shape),
    });

    let path = temp_project("authoring");
    let _ = std::fs::remove_file(&path);
    save_project(&app, &path, false).unwrap();
    let loaded = load_project(&path).unwrap();
    let _ = std::fs::remove_file(&path);

    let canvas = &loaded.doc.canvases[0];
    let text = canvas.objects.iter().find(|o| o.id == id_text).unwrap();
    assert_eq!(text.text().unwrap().text, "Caption");
    let shape = canvas.objects.iter().find(|o| o.id == id_shape).unwrap();
    assert!(!shape.visible);
    match &shape.kind {
        CanvasObjectKind::Shape(s) => {
            assert!(matches!(s.shape, ShapeKind::Arrow));
            assert_eq!(s.stroke_width, 3.0);
            assert_eq!(s.fill, Some(Color::rgb(10, 20, 30)));
        }
        _ => panic!("expected a shape object"),
    }
}

#[test]
fn project_roundtrip_preserves_zorder() {
    use crate::state::{CanvasObject, CanvasObjectKind, TextBox};

    let mut app = sample_app();
    for name in ["Text A", "Text B"] {
        let id = app.doc.canvases[0].allocate_object_id();
        app.doc.canvases[0].objects.push(CanvasObject {
            id,
            name: name.to_owned(),
            frame: ObjectFrame::new(10.0, 10.0, 40.0, 20.0),
            locked: false,
            visible: true,
            group: None,
            kind: CanvasObjectKind::Text(TextBox::label(name.to_owned())),
        });
    }
    let plot_id = app.doc.canvases[0].objects[0].id;
    app.apply_z_order(0, &[plot_id], crate::actions::ZOrder::Front);
    let expected: Vec<_> = app.doc.canvases[0].objects.iter().map(|o| o.id).collect();

    let path = temp_project("zorder");
    let _ = std::fs::remove_file(&path);
    save_project(&app, &path, false).unwrap();
    let loaded = load_project(&path).unwrap();
    let _ = std::fs::remove_file(&path);

    let got: Vec<_> = loaded.doc.canvases[0]
        .objects
        .iter()
        .map(|o| o.id)
        .collect();
    assert_eq!(got, expected);
    assert_eq!(*got.last().unwrap(), plot_id);
}

#[test]
fn project_roundtrip_preserves_overlay_binding() {
    let mut app = PlotxApp::new();
    for _ in 0..2 {
        app.doc
            .datasets
            .push(Dataset::Nmr(Box::new(NmrDataset::load(synthetic_1d()))));
    }
    app.doc.datasets[1].set_name(Some("treatment".to_owned()));
    let mut canvas = CanvasDocument::new("overlay".to_owned(), [120.0, 80.0]);
    let [w, h] = canvas.size_pt();
    let id = canvas.allocate_object_id();
    let mut object =
        app.build_plot_object(0, ObjectFrame::new(0.0, 0.0, w, h), id, "Plot 1".to_owned());
    object.plot_mut().unwrap().binding = crate::state::DataBinding {
        series: vec![
            crate::state::SeriesBinding::new(app.doc.datasets[0].resource_id()),
            crate::state::SeriesBinding {
                dataset: app.doc.datasets[1].resource_id(),
                color: Some(Color::rgb(10, 20, 30)),
                label: Some("treated".to_owned()),
                scale: 1.0,
                visible: true,
            },
        ],
    };
    canvas.objects.push(object);
    app.doc.canvases.push(canvas);
    app.focus_single(0);
    app.session.active_canvas = Some(0);

    let path = temp_project("overlay");
    let _ = std::fs::remove_file(&path);
    save_project(&app, &path, false).unwrap();
    let loaded = load_project(&path).unwrap();
    let _ = std::fs::remove_file(&path);

    let binding = &first_plot(&loaded).binding;
    assert_eq!(binding.series.len(), 2);
    assert_eq!(
        binding.series[0].dataset,
        loaded.doc.datasets[0].resource_id()
    );
    assert_eq!(
        binding.series[1].dataset,
        loaded.doc.datasets[1].resource_id()
    );
    assert_eq!(binding.series[1].color, Some(Color::rgb(10, 20, 30)));
    assert_eq!(binding.series[1].label.as_deref(), Some("treated"));
    assert!(first_plot(&loaded).figure.show_legend);
}

#[test]
fn project_roundtrip_preserves_stack_spec_and_series_fields() {
    let mut app = PlotxApp::new();
    for _ in 0..2 {
        app.doc
            .datasets
            .push(Dataset::Nmr(Box::new(NmrDataset::load(synthetic_1d()))));
    }
    let mut canvas = CanvasDocument::new("stack".to_owned(), [120.0, 80.0]);
    let [w, h] = canvas.size_pt();
    let id = canvas.allocate_object_id();
    let mut object =
        app.build_plot_object(0, ObjectFrame::new(0.0, 0.0, w, h), id, "Plot 1".to_owned());
    {
        let plot = object.plot_mut().unwrap();
        plot.binding = DataBinding {
            series: vec![
                SeriesBinding::new(app.doc.datasets[0].resource_id()),
                SeriesBinding {
                    dataset: app.doc.datasets[1].resource_id(),
                    color: None,
                    label: None,
                    scale: 2.5,
                    visible: false,
                },
            ],
        };
        plot.stack = StackSpec {
            mode: StackMode::Offset,
            spacing_y: 0.3,
            shear_x: 0.1,
            normalize: true,
            active: Some(1),
        };
    }
    canvas.objects.push(object);
    app.doc.canvases.push(canvas);
    app.focus_single(0);
    app.session.active_canvas = Some(0);

    let path = temp_project("stackspec");
    let _ = std::fs::remove_file(&path);
    save_project(&app, &path, false).unwrap();
    let loaded = load_project(&path).unwrap();
    let _ = std::fs::remove_file(&path);

    let plot = first_plot(&loaded);
    assert_eq!(plot.stack.mode, StackMode::Offset);
    assert_eq!(plot.stack.spacing_y, 0.3);
    assert_eq!(plot.stack.shear_x, 0.1);
    assert!(plot.stack.normalize);
    assert_eq!(plot.stack.active, Some(1));
    assert_eq!(plot.binding.series[1].scale, 2.5);
    assert!(!plot.binding.series[1].visible);
}

#[test]
fn legacy_single_input_loads_as_one_series_binding() {
    let view: ViewCanvasObject = serde_json::from_str(
        r#"{"id":"1","name":"Plot","kind":"line_plot","input":"recipe_000000",
                "frame":{"x":0.0,"y":0.0,"width":100.0,"height":80.0},
                "title":null,"snapshot":null,"locked":false,"visible":true}"#,
    )
    .unwrap();
    assert!(view.series.is_empty(), "old files carry no series list");
    assert!(view.axis_overrides.is_none());
}

#[test]
fn project_roundtrip_preserves_custom_pipeline_steps() {
    let mut app = PlotxApp::new();
    let mut dataset = NmrDataset::load(synthetic_1d());
    // One time-side step (an exponential window before the FFT) and one
    // frequency-side step (a referencing shift) that must both survive.
    let fft_pos = dataset
        .pipeline
        .steps
        .iter()
        .position(|s| matches!(s.kind, StepKind::Fft))
        .unwrap();
    dataset.pipeline.steps.insert(
        fft_pos,
        ProcessingStep::new(
            StepKind::Apodize(Apodization::Exponential { lb_hz: 8.0 }),
            StepSource::User,
        ),
    );
    dataset.pipeline.steps.push(ProcessingStep::new(
        StepKind::Reference(ReferenceParams {
            at_ppm: 2.0,
            target_ppm: 2.5,
        }),
        StepSource::User,
    ));
    dataset.retransform();
    app.doc.datasets.push(Dataset::Nmr(Box::new(dataset)));

    let path = temp_project("pipeline");
    let _ = std::fs::remove_file(&path);
    save_project(&app, &path, false).unwrap();
    let loaded = load_project(&path).unwrap();
    let _ = std::fs::remove_file(&path);

    let Dataset::Nmr(n) = &loaded.doc.datasets[0] else {
        panic!("expected a 1D NMR dataset");
    };
    assert!(n.pipeline.steps.iter().any(|s| matches!(
        &s.kind,
        StepKind::Apodize(Apodization::Exponential { lb_hz }) if (*lb_hz - 8.0).abs() < 1e-9
    )));
    assert!(n.pipeline.steps.iter().any(|s| matches!(
        &s.kind,
        StepKind::Reference(r) if (r.target_ppm - 2.5).abs() < 1e-9 && (r.at_ppm - 2.0).abs() < 1e-9
    )));
}

#[test]
fn scheme_save_load_apply_roundtrips() {
    use crate::actions::DatasetProcessingState;
    let mut source = NmrDataset::load(synthetic_1d());
    set_manual_phase(&mut source.pipeline, 0.3, 0.1, 0.6);
    let source_ds = Dataset::Nmr(Box::new(source));

    let path = temp_scheme("scheme");
    let _ = std::fs::remove_file(&path);
    save_scheme(&path, &source_ds).unwrap();
    let scheme = load_scheme(&path).unwrap();
    let _ = std::fs::remove_file(&path);
    assert_eq!(scheme.dimension_count, 1);

    let target = Dataset::Nmr(Box::new(NmrDataset::load(synthetic_1d())));
    let DatasetProcessingState::Nmr { pipeline, .. } = apply_scheme(&scheme, &target).unwrap()
    else {
        panic!("expected a 1D processing state");
    };
    let phase = first_phase(&pipeline).expect("phase step in applied scheme");
    assert!(phase.auto.is_none());
    assert!((phase.phase0 - 0.3).abs() < 1e-9);
    assert!((phase.pivot_frac - 0.6).abs() < 1e-9);

    let two_d = Dataset::Nmr2D(Box::new(Nmr2DDataset::load(synthetic_true_2d())));
    assert!(apply_scheme(&scheme, &two_d).is_err());

    let DatasetProcessingState::Nmr { pipeline, .. } = reset_processing(&source_ds).unwrap() else {
        panic!("expected a 1D processing state");
    };
    assert!(
        first_phase(&pipeline)
            .expect("default phase step")
            .auto
            .is_some()
    );
}

#[test]
fn snapshot_roundtrip_restores_materialized_figure() {
    let mut app = sample_app();
    let plot = first_plot_mut(&mut app);
    plot.figure.x.label = "snapshot-only x label".to_owned();
    plot.figure.x.min = 2.25;
    plot.figure.x.max = 3.75;
    plot.viewport.full_x = AxisRange::new(0.0, 10.0);
    plot.viewport.view_x = AxisRange::new(5.0, 6.0);
    let path = temp_project("snapshot");
    let _ = std::fs::remove_file(&path);

    save_project(&app, &path, true).unwrap();
    let loaded = load_project(&path).unwrap();
    let _ = std::fs::remove_file(&path);

    assert!(loaded.doc.save_include_view_snapshots);
    assert_eq!(first_plot(&loaded).figure.x.label, "snapshot-only x label");
    assert_eq!(first_plot(&loaded).figure.x.min, 2.25);
    assert_eq!(first_plot(&loaded).figure.x.max, 3.75);
    assert_eq!(
        first_plot(&loaded).viewport.view_x,
        AxisRange::new(5.0, 6.0)
    );
}
