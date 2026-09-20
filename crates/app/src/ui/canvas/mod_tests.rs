use super::*;

use plotx_core::state::{CanvasObject, CanvasObjectKind, CanvasViewport, PlotObject, TextBox};
use plotx_figure::{Axis, Figure};

#[test]
fn edge_contact_is_visible_and_non_finite_is_not() {
    let clip = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(10.0, 10.0));
    assert!(finite_rect_intersects(
        egui::Rect::from_min_max(egui::pos2(10.0, 2.0), egui::pos2(20.0, 8.0)),
        clip,
    ));
    assert!(!finite_rect_intersects(
        egui::Rect::from_min_max(egui::pos2(11.0, 2.0), egui::pos2(20.0, 8.0)),
        clip,
    ));
    assert!(!finite_rect_intersects(
        egui::Rect::from_min_max(egui::pos2(f32::NAN, 0.0), egui::pos2(1.0, 1.0)),
        clip,
    ));
}

#[test]
fn hit_object_selects_text_box() {
    let mut canvas = CanvasDocument::new("page".to_owned(), [200.0, 200.0]);
    canvas.objects.push(CanvasObject {
        id: ObjectId::new(7),
        name: "Text".to_owned(),
        frame: ObjectFrame::new(20.0, 20.0, 100.0, 30.0),
        locked: false,
        visible: true,
        kind: CanvasObjectKind::Text(TextBox::label("hi".to_owned())),
    });

    let hit = hit_object(&canvas, Pos2::new(50.0, 30.0), 1.0);

    assert_eq!(hit.map(|hit| hit.object), Some(ObjectId::new(7)));
}

#[test]
fn hit_object_finds_object_outside_page_bounds() {
    let mut canvas = CanvasDocument::new("page".to_owned(), [100.0, 100.0]);
    canvas.objects.push(CanvasObject {
        id: ObjectId::new(1),
        name: "plot".to_owned(),
        frame: ObjectFrame::new(-30.0, 20.0, 50.0, 40.0),
        locked: false,
        visible: true,
        kind: CanvasObjectKind::Plot(Box::new({
            let figure = Figure::new("plot", Axis::new("x", 0.0, 1.0), Axis::new("y", 0.0, 1.0));
            let viewport = CanvasViewport::from_figure(&figure);
            PlotObject::new(
                None,
                plotx_core::state::SeriesId::new(1),
                plotx_core::state::DataBinding { series: Vec::new() },
                plotx_core::state::ChartSpec::default(),
                plotx_core::state::StackSpec::default(),
                plotx_core::state::AxisProjections::default(),
                plotx_core::state::AxisOverrides::default(),
                figure,
                viewport,
            )
        })),
    });

    let hit = hit_object(&canvas, Pos2::new(-10.0, 30.0), 1.0);

    assert_eq!(hit.map(|hit| hit.object), Some(ObjectId::new(1)));
}

#[test]
fn data_edit_target_requires_data_tool_and_selected_plot() {
    let mut app = PlotxApp::new();
    let mut canvas = CanvasDocument::new("page".to_owned(), [200.0, 200.0]);
    canvas.objects.push(CanvasObject {
        id: ObjectId::new(3),
        name: "plot".to_owned(),
        frame: ObjectFrame::new(10.0, 10.0, 80.0, 60.0),
        locked: false,
        visible: true,
        kind: CanvasObjectKind::Plot(Box::new({
            let figure = Figure::new("plot", Axis::new("x", 0.0, 1.0), Axis::new("y", 0.0, 1.0));
            let viewport = CanvasViewport::from_figure(&figure);
            PlotObject::new(
                None,
                plotx_core::state::SeriesId::new(1),
                plotx_core::state::DataBinding { series: Vec::new() },
                plotx_core::state::ChartSpec::default(),
                plotx_core::state::StackSpec::default(),
                plotx_core::state::AxisProjections::default(),
                plotx_core::state::AxisOverrides::default(),
                figure,
                viewport,
            )
        })),
    });
    app.doc.canvases.push(canvas);
    app.session.active_canvas = Some(0);
    app.doc.canvases[0].selected_object = Some(ObjectId::new(3));

    app.session.tool = Tool::Select;
    assert_eq!(data_edit_target(&app, 0), None);

    app.session.tool = Tool::BrowseZoom;
    assert_eq!(data_edit_target(&app, 0), Some(ObjectId::new(3)));
}

#[test]
fn phase_editor_open_drives_on_plot_pivot() {
    use num_complex::Complex64;
    use plotx_core::state::{Dataset, NmrDataset, PhaseAxis};
    use plotx_io::{Domain, NmrData};
    use std::f64::consts::TAU;

    let npoints = 256;
    let (sw, obs, carrier) = (4000.0, 400.0, 5.0);
    let dt = 1.0 / sw;
    let points = (0..npoints)
        .map(|k| {
            let t = k as f64 * dt;
            let decay = (-t / 0.25f64).exp();
            let freq_hz = (2.0 - carrier) * obs;
            Complex64::from_polar(decay, TAU * freq_hz * t)
        })
        .collect();
    let data = NmrData {
        points,
        domain: Domain::Time,
        spectral_width_hz: sw,
        observe_freq_mhz: obs,
        carrier_ppm: carrier,
        nucleus: "1H".to_owned(),
        source: "synthetic".to_owned(),
        group_delay: 0.0,
    };

    let mut app = PlotxApp::new();
    app.doc
        .datasets
        .push(Dataset::Nmr(Box::new(NmrDataset::load(data).unwrap())));
    let mut canvas = CanvasDocument::new("page".to_owned(), [200.0, 200.0]);
    let id = canvas.allocate_object_id();
    let obj = app.build_plot_object(
        0,
        ObjectFrame::new(10.0, 10.0, 80.0, 60.0),
        id,
        "plot".into(),
    );
    canvas.objects.push(obj);
    app.doc.canvases.push(canvas);
    app.session.active_canvas = Some(0);
    app.focus_single(0);

    let pivot = Color32::from_rgb(0xE0, 0x6C, 0x22);
    let count = |app: &mut PlotxApp| {
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1000.0, 800.0),
            )),
            ..Default::default()
        };
        let ctx = egui::Context::default();
        // Two passes: the first lays out the board, the second paints with a
        // stable geometry.
        let _ = ctx.run_ui(input.clone(), |ui| {
            egui::CentralPanel::default().show_inside(ui, |ui| render_central(app, ui));
        });
        let out = ctx.run_ui(input, |ui| {
            egui::CentralPanel::default().show_inside(ui, |ui| render_central(app, ui));
        });
        out.shapes
            .iter()
            .filter(|cs| match &cs.shape {
                egui::epaint::Shape::LineSegment { stroke, .. } => stroke.color == pivot,
                egui::epaint::Shape::Circle(c) => c.fill == pivot,
                _ => false,
            })
            .count()
    };

    let phase_id = app.doc.datasets[0]
        .axis_pipeline(PhaseAxis::Direct)
        .unwrap()
        .steps
        .iter()
        .find(|s| matches!(s.kind, plotx_processing::StepKind::Phase(_)))
        .unwrap()
        .id;

    app.sync_phase_interaction();
    assert_eq!(count(&mut app), 0, "no pivot before the Phase editor opens");

    app.session.ui.proc_expanded_step = Some((app.doc.datasets[0].resource_id(), phase_id));
    app.sync_phase_interaction();
    assert_eq!(app.session.tool, Tool::ManualPhase);
    assert!(
        count(&mut app) > 0,
        "pivot appears while the Phase editor is open"
    );

    app.session.ui.proc_expanded_step = None;
    app.sync_phase_interaction();
    assert_ne!(app.session.tool, Tool::ManualPhase);
    assert_eq!(count(&mut app), 0, "pivot gone after the editor collapses");
}
