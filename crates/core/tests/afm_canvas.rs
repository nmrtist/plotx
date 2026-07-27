use plotx_core::actions::Action;
use plotx_core::automation::{KIND_FIELD, ProjectResourceProvider, ResourceProvider};
use plotx_core::state::{
    CanvasObjectKind, DEFAULT_CANVAS_SIZE_MM, Dataset, DerivedAxes, NATURE_DOUBLE_COLUMN, PlotxApp,
    StackSpec,
};
use plotx_figure::{ContourSpec, SeriesEncoding};
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
    assert_eq!(plot.figure().series.len(), 2);
    assert_eq!([plot.figure().x.min, plot.figure().x.max], [-100.0, 100.0]);
    assert_eq!(plot.figure().y.label, "Force (nN)");
    assert!((plot.figure().series[0].points[0][1] - 0.1).abs() < 1.0e-12);
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
    let CanvasObjectKind::Plot(map) = &objects[0].kind else {
        panic!("expected AFM map plot");
    };
    assert_eq!(
        map.binding.series[0].source.field,
        app.doc.datasets[0]
            .field_descriptors()
            .into_iter()
            .find(|field| field.local_id.starts_with("afm.channel."))
            .unwrap()
            .id
    );
    assert!(matches!(
        map.binding.series[0].encoding,
        SeriesEncoding::Heatmap(_)
    ));
    let CanvasObjectKind::Plot(force) = &objects[1].kind else {
        panic!("expected AFM force plot");
    };
    assert_eq!(
        force.binding.series[0].source.field,
        app.doc.datasets[0]
            .field_descriptors()
            .into_iter()
            .find(|field| field.local_id == "afm.force_curve")
            .unwrap()
            .id
    );
    assert!(matches!(
        force.binding.series[0].encoding,
        SeriesEncoding::Line(_)
    ));
}

#[test]
fn afm_double_view_force_curve_keeps_its_own_derived_axes() {
    let dataset = afm_dataset(true);
    let canvas = plotx_core::workflow::build_default_canvas_for_dataset(
        &dataset,
        0,
        "AFM".to_owned(),
        DEFAULT_CANVAS_SIZE_MM,
    );
    let CanvasObjectKind::Plot(map) = &canvas.objects[0].kind else {
        panic!("expected AFM map plot");
    };
    let CanvasObjectKind::Plot(force) = &canvas.objects[1].kind else {
        panic!("expected AFM force plot");
    };

    assert_eq!(
        force.derived_axes(),
        &DerivedAxes::from_figure(force.figure()),
        "Force Curve derived axes must describe its own rebuilt figure"
    );
    assert_ne!(
        force.derived_axes().x_label,
        map.derived_axes().x_label,
        "AFM map and Force Curve should expose distinct derived x-axis labels"
    );
}

#[test]
fn afm_scalar_field_can_render_a_contour_without_a_domain_chart_branch() {
    let mut app = insert(afm_dataset(true));
    let (mut binding, chart, size) = {
        let CanvasObjectKind::Plot(map) = &app.doc.canvases[0].objects[0].kind else {
            panic!("expected AFM map plot");
        };
        (
            map.binding.clone(),
            map.chart.clone(),
            app.doc.canvases[0].size_mm,
        )
    };
    binding.series[0].encoding = SeriesEncoding::Contour(
        ContourSpec::absolute(1.5, false).expect("positive literal contour base"),
    );
    let initial = app.build_binding_figure(&binding, &chart, &StackSpec::default(), size);
    assert!(
        initial.contours.is_empty(),
        "geometry is queued, never built inline"
    );

    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    while app.compute_busy() && std::time::Instant::now() < deadline {
        app.poll_compute();
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    app.poll_compute();
    let figure = app.build_binding_figure(&binding, &chart, &StackSpec::default(), size);
    assert!(!figure.contours.is_empty());
    assert!(!figure.contours[0].segments.is_empty());
}

#[test]
fn afm_fields_expose_independent_image_and_force_capabilities() {
    let dataset = afm_dataset(true);
    let fields = dataset.field_descriptors();
    assert_eq!(fields.len(), 2);
    assert!(
        fields[0]
            .capabilities
            .contains(plotx_core::automation::CAP_FIELD_SCALAR_GRID_2D_REGULAR)
    );
    assert!(
        fields[0]
            .capabilities
            .contains(plotx_core::automation::CAP_FIELD_LOCATION_SCALE)
    );
    assert!(
        !fields[0]
            .capabilities
            .contains(plotx_core::automation::CAP_FIELD_CURVE_1D)
    );
    assert!(
        fields[1]
            .capabilities
            .contains(plotx_core::automation::CAP_FIELD_CURVE_1D)
    );
    assert!(
        !fields[1]
            .capabilities
            .contains(plotx_core::automation::CAP_FIELD_SCALAR_GRID_2D_REGULAR)
    );
}

#[test]
fn field_descriptors_are_dataset_child_resources() {
    let app = insert(afm_dataset(true));
    let dataset = &app.doc.datasets[0];
    let descriptors = ProjectResourceProvider::new(&app).descriptors();
    let fields = descriptors
        .iter()
        .filter(|descriptor| descriptor.resource.kind.0 == KIND_FIELD)
        .collect::<Vec<_>>();
    assert_eq!(fields.len(), 2);
    assert!(fields.iter().all(|field| {
        field.resource.parent_id.as_deref() == Some(&dataset.resource_id().to_string())
    }));
    let map_key = dataset
        .field_descriptors()
        .into_iter()
        .find(|field| field.local_id.starts_with("afm.channel."))
        .unwrap()
        .local_id;
    assert!(fields.iter().any(|field| {
        field.resource.local_id.as_deref() == Some(map_key.as_str()) && map_key != "afm.channel.0"
    }));
    assert!(
        fields
            .iter()
            .any(|field| field.resource.local_id.as_deref() == Some("afm.force_curve"))
    );
}
