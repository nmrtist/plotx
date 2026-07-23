use plotx_core::actions::Action;
use plotx_core::state::{
    CanvasObjectKind, DEFAULT_CANVAS_SIZE_MM, Dataset, NATURE_DOUBLE_COLUMN, PlotxApp,
};
use plotx_io::{AfmData, AfmForceSet, AfmFrameDirection, AfmImageChannel, AfmScale};
use std::sync::Arc;

fn afm_dataset(with_image: bool) -> Dataset {
    let images = if with_image {
        vec![AfmImageChannel {
            name: "Height".to_owned(),
            width: 2,
            height: 2,
            scan_size_x: 1.0,
            scan_size_y: 1.0,
            lateral_unit: "nm".to_owned(),
            scale: AfmScale {
                multiplier: 1.0,
                offset: 0.0,
                unit: "nm".to_owned(),
            },
            raw: Arc::<[i32]>::from([1, 2, 3, 4]),
            frame_direction: AfmFrameDirection::Trace,
        }]
    } else {
        Vec::new()
    };
    let data = AfmData {
        images,
        forces: Some(AfmForceSet {
            grid_width: 1,
            grid_height: 1,
            samples_per_curve: 4,
            raw: Arc::<[i32]>::from([1, 2, 3, 4]),
            signal_scale: AfmScale {
                multiplier: 1.0,
                offset: 0.0,
                unit: "V".to_owned(),
            },
            sample_period_s: None,
            z_positions: Some(Arc::<[f64]>::from([-100.0, 0.0, 100.0, 0.0])),
            display_order: Arc::<[usize]>::from([0, 1, 2, 3]),
            approach_samples: 2,
            deflection_sensitivity_m_per_v: Some(1.0e-9),
            spring_constant_n_per_m: Some(0.1),
        }),
        source: "synthetic.spm".to_owned(),
        import_warnings: Vec::new(),
    };
    Dataset::Afm(Box::new(plotx_core::state::AfmDataset::load(data)))
}

fn insert(dataset: Dataset) -> PlotxApp {
    let mut app = PlotxApp::default();
    let action = Action::insert_dataset_with_default_canvas(
        &app,
        dataset,
        "AFM".to_owned(),
        DEFAULT_CANVAS_SIZE_MM,
    );
    app.execute_action(action);
    app
}

#[test]
fn force_only_gui_insertion_builds_a_nonempty_force_curve() {
    let app = insert(afm_dataset(false));
    let CanvasObjectKind::Plot(plot) = &app.doc.canvases[0].objects[0].kind else {
        panic!("expected plot");
    };
    assert_eq!(plot.chart.type_id, "afm_force_curve");
    assert_eq!(plot.figure.series.len(), 2);
    assert_eq!([plot.figure.x.min, plot.figure.x.max], [-100.0, 100.0]);
    assert_eq!(plot.figure.y.label, "Force (nN)");
    assert!((plot.figure.series[0].points[0][1] - 0.1).abs() < 1.0e-12);
}

#[test]
fn map_and_force_gui_insertion_builds_side_by_side_plots() {
    let app = insert(afm_dataset(true));
    let objects = &app.doc.canvases[0].objects;
    assert_eq!(objects.len(), 2);
    assert_eq!(
        app.doc.canvases[0].size_mm,
        [NATURE_DOUBLE_COLUMN.width_mm, DEFAULT_CANVAS_SIZE_MM[1]]
    );
    assert_eq!(
        app.doc.canvases[0].size_preset_id.as_deref(),
        Some(NATURE_DOUBLE_COLUMN.id)
    );
    let chart_ids: Vec<&str> = objects
        .iter()
        .filter_map(|object| match &object.kind {
            CanvasObjectKind::Plot(plot) => Some(plot.chart.type_id.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(chart_ids, ["afm_map", "afm_force_curve"]);
}
