use super::*;
use crate::automation::TargetRef;
use crate::state::{
    CanvasDocument, CanvasObject, CanvasObjectKind, Dataset, FloatSeries, ObjectFrame, ObjectId,
    PlotxApp, SeriesBinding, ShapeKind, ShapeObject, StackMode, TextBox,
    materialized_float_series_table,
};
use plotx_figure::Color;

fn object_app(kind: CanvasObjectKind) -> (PlotxApp, TargetRef, ObjectId) {
    let mut app = PlotxApp::new();
    let mut canvas = CanvasDocument::new("objects".to_owned(), [120.0, 80.0]);
    let id = canvas.allocate_object_id();
    canvas.objects.push(CanvasObject {
        id,
        name: "Object".to_owned(),
        frame: ObjectFrame::new(1.0, 2.0, 30.0, 20.0),
        locked: false,
        visible: true,
        group: None,
        kind,
    });
    app.doc.canvases.push(canvas);
    app.session.active_canvas = Some(0);
    let target = app.object_target(0, id).expect("object target");
    (app, target, id)
}

fn table_app() -> (PlotxApp, TargetRef, ObjectId) {
    let values = (0..1000).map(|value| Some(f64::from(value))).collect();
    let table = materialized_float_series_table(
        (
            "x".to_owned(),
            String::new(),
            (0..1000).map(|value| Some(f64::from(value))).collect(),
        ),
        vec![FloatSeries {
            name: "signal".to_owned(),
            unit: String::new(),
            values,
            uncertainty: None,
            fit: None,
        }],
        "plotx.test.object-properties.v1",
    )
    .expect("table fixture");
    let mut app = PlotxApp::new();
    app.doc.datasets.push(Dataset::Table(Box::new(table)));
    let mut canvas = CanvasDocument::new("table".to_owned(), [120.0, 80.0]);
    let id = canvas.allocate_object_id();
    canvas.objects.push(app.build_plot_object(
        0,
        ObjectFrame::new(0.0, 0.0, 100.0, 70.0),
        id,
        "Plot".to_owned(),
    ));
    app.doc.canvases.push(canvas);
    app.session.active_canvas = Some(0);
    let target = app.object_target(0, id).expect("plot target");
    (app, target, id)
}

fn stack_app() -> (PlotxApp, ObjectId) {
    let mut app = PlotxApp::new();
    for source in ["first", "second"] {
        let data = plotx_io::NmrData {
            points: (0..32)
                .map(|value| num_complex::Complex64::new(f64::from(value), 0.0))
                .collect(),
            domain: plotx_io::Domain::Frequency,
            spectral_width_hz: 4_000.0,
            observe_freq_mhz: 400.0,
            carrier_ppm: 0.0,
            nucleus: "1H".to_owned(),
            source: source.to_owned(),
            group_delay: 0.0,
        };
        app.doc
            .datasets
            .push(Dataset::Nmr(Box::new(crate::state::NmrDataset::load(data))));
    }
    let mut canvas = CanvasDocument::new("stack".to_owned(), [120.0, 80.0]);
    let id = canvas.allocate_object_id();
    canvas.objects.push(app.build_plot_object(
        0,
        ObjectFrame::new(0.0, 0.0, 100.0, 70.0),
        id,
        "Plot".to_owned(),
    ));
    let plot = canvas.object_mut(id).unwrap().plot_mut().unwrap();
    let mut second = SeriesBinding::from_dataset(&app.doc.datasets[1]).expect("series");
    second.id = plot.allocate_series_id();
    plot.binding.series.push(second);
    plot.stack.mode = StackMode::Offset;
    let series = plot.binding.series[0].id;
    app.doc.canvases.push(canvas);
    app.session.active_canvas = Some(0);
    let _ = series;
    (app, id)
}

fn real_stack_targets(app: &PlotxApp, id: ObjectId) -> (TargetRef, TargetRef) {
    let object = app.object_target(0, id).unwrap();
    let series = app.series_targets(0, id).remove(0);
    (object, series)
}

fn set(app: &mut PlotxApp, target: &TargetRef, property: PropertyId, value: PropertyValue) {
    let commit = app
        .plan_property_write(property, std::slice::from_ref(target), &value)
        .unwrap_or_else(|error| panic!("{property}: {error}"));
    app.commit_property(commit);
}

fn reset(app: &mut PlotxApp, target: &TargetRef, property: PropertyId) {
    let commit = app
        .plan_property_reset(property, std::slice::from_ref(target))
        .unwrap_or_else(|error| panic!("{property}: {error}"));
    assert_eq!(commit.applied.len(), 1, "{property}");
    app.commit_property(commit);
}

#[test]
fn every_stack_and_series_property_supports_reset() {
    let (mut app, id) = stack_app();
    let (target, series) = real_stack_targets(&app, id);
    set(
        &mut app,
        &target,
        object::STACK_MODE,
        PropertyValue::Enum(object::SUPERIMPOSED),
    );
    set(
        &mut app,
        &target,
        object::STACK_MODE,
        PropertyValue::Enum(object::OFFSET),
    );
    reset(&mut app, &target, object::STACK_MODE);
    set(
        &mut app,
        &target,
        object::STACK_MODE,
        PropertyValue::Enum(object::OFFSET),
    );
    for (property, changed) in [
        (object::STACK_SPACING_Y, PropertyValue::Float(0.7)),
        (object::STACK_SHEAR_X, PropertyValue::Float(0.3)),
        (object::STACK_NORMALIZE, PropertyValue::Bool(true)),
    ] {
        set(&mut app, &target, property, changed);
        reset(&mut app, &target, property);
    }
    set(
        &mut app,
        &series,
        object::SERIES_VISIBLE,
        PropertyValue::Bool(false),
    );
    reset(&mut app, &series, object::SERIES_VISIBLE);
}

#[test]
fn every_chart_property_supports_reset() {
    let (mut app, target, id) = table_app();
    set(
        &mut app,
        &target,
        object::CHART_TYPE_ID,
        PropertyValue::Enum("table_bar"),
    );
    reset(&mut app, &target, object::CHART_TYPE_ID);
    assert!(
        app.doc.canvases[0]
            .object(id)
            .unwrap()
            .plot()
            .unwrap()
            .chart
            .type_id
            .is_empty()
    );

    set(
        &mut app,
        &target,
        object::CHART_TYPE_ID,
        PropertyValue::Enum("table_histogram"),
    );
    set(
        &mut app,
        &target,
        object::CHART_BINS_AUTO,
        PropertyValue::Bool(false),
    );
    reset(&mut app, &target, object::CHART_BINS_COUNT);
    reset(&mut app, &target, object::CHART_BINS_AUTO);

    set(
        &mut app,
        &target,
        object::CHART_TYPE_ID,
        PropertyValue::Enum("table_bar_grouped"),
    );
    set(
        &mut app,
        &target,
        object::CHART_STACKED,
        PropertyValue::Bool(true),
    );
    reset(&mut app, &target, object::CHART_STACKED);

    set(
        &mut app,
        &target,
        object::CHART_TYPE_ID,
        PropertyValue::Enum("table_surface"),
    );
    set(
        &mut app,
        &target,
        object::CHART_COLORMAP,
        PropertyValue::Enum("plasma"),
    );
    reset(&mut app, &target, object::CHART_COLORMAP);
    for (property, changed) in [
        (
            object::CHART_VIEW_AZIMUTH,
            PropertyValue::Float(10_f64.to_radians()),
        ),
        (
            object::CHART_VIEW_ELEVATION,
            PropertyValue::Float(60_f64.to_radians()),
        ),
    ] {
        set(&mut app, &target, property, changed);
        reset(&mut app, &target, property);
    }
}

#[test]
fn every_panel_text_shape_and_object_property_supports_reset() {
    let (mut panel_app, panel_target, panel_id) = table_app();
    set(
        &mut panel_app,
        &panel_target,
        object::PANEL_USER_NOTE,
        PropertyValue::Text("note".to_owned()),
    );
    reset(&mut panel_app, &panel_target, object::PANEL_USER_NOTE);
    set(
        &mut panel_app,
        &panel_target,
        object::PANEL_VISIBLE,
        PropertyValue::Bool(false),
    );
    reset(&mut panel_app, &panel_target, object::PANEL_VISIBLE);
    set(
        &mut panel_app,
        &panel_target,
        object::LOCKED,
        PropertyValue::Bool(true),
    );
    reset(&mut panel_app, &panel_target, object::LOCKED);
    assert!(!panel_app.doc.canvases[0].object(panel_id).unwrap().locked);

    let (mut text_app, text_target, _) =
        object_app(CanvasObjectKind::Text(TextBox::label(String::new())));
    for (property, changed) in [
        (object::TEXT, PropertyValue::Text("caption".to_owned())),
        (object::TEXT_FONT_SIZE, PropertyValue::Float(22.0)),
        (object::TEXT_BOLD, PropertyValue::Bool(true)),
        (object::TEXT_ALIGN, PropertyValue::Enum(object::ALIGN_RIGHT)),
        (
            object::TEXT_COLOR,
            PropertyValue::Color(Color::rgb(1, 2, 3)),
        ),
    ] {
        set(&mut text_app, &text_target, property, changed);
        reset(&mut text_app, &text_target, property);
    }

    let (mut shape_app, shape_target, _) =
        object_app(CanvasObjectKind::Shape(ShapeObject::new(ShapeKind::Rect)));
    for (property, changed) in [
        (object::SHAPE_KIND, PropertyValue::Enum(object::SHAPE_ARROW)),
        (
            object::SHAPE_STROKE,
            PropertyValue::Color(Color::rgb(1, 2, 3)),
        ),
        (object::SHAPE_STROKE_WIDTH, PropertyValue::Float(8.0)),
    ] {
        set(&mut shape_app, &shape_target, property, changed);
        reset(&mut shape_app, &shape_target, property);
    }
    set(
        &mut shape_app,
        &shape_target,
        object::SHAPE_FILL_ENABLED,
        PropertyValue::Bool(true),
    );
    reset(&mut shape_app, &shape_target, object::SHAPE_FILL_ENABLED);
    set(
        &mut shape_app,
        &shape_target,
        object::SHAPE_FILL_ENABLED,
        PropertyValue::Bool(true),
    );
    set(
        &mut shape_app,
        &shape_target,
        object::SHAPE_FILL_COLOR,
        PropertyValue::Color(Color::rgb(4, 5, 6)),
    );
    reset(&mut shape_app, &shape_target, object::SHAPE_FILL_COLOR);
}

#[test]
fn automatic_bins_disable_count_and_manual_mode_seeds_the_live_auto_result() {
    let (mut app, target, id) = table_app();
    set(
        &mut app,
        &target,
        object::CHART_TYPE_ID,
        PropertyValue::Enum("table_histogram"),
    );
    assert_eq!(
        app.doc.canvases[0]
            .object(id)
            .unwrap()
            .plot()
            .unwrap()
            .chart
            .bins,
        None
    );
    let count = app
        .resolve_property(&PropertyAddress::new(
            target.clone(),
            object::CHART_BINS_COUNT,
        ))
        .unwrap();
    let Availability::Disabled(reason) = count.availability else {
        panic!("automatic bins must disable the count");
    };
    assert!(reason.contains("Turn off Automatic"), "{reason}");

    set(
        &mut app,
        &target,
        object::CHART_BINS_AUTO,
        PropertyValue::Bool(false),
    );
    assert_eq!(
        app.doc.canvases[0]
            .object(id)
            .unwrap()
            .plot()
            .unwrap()
            .chart
            .bins,
        Some(10),
        "the 0..999 sample's live Freedman–Diaconis result seeds manual mode"
    );
}

#[test]
fn fill_disabled_round_trip_discards_the_old_color_and_uses_the_existing_gray_fallback() {
    let mut shape = ShapeObject::new(ShapeKind::Rect);
    shape.fill = Some(Color::rgb(9, 8, 7));
    let (mut app, target, id) = object_app(CanvasObjectKind::Shape(shape));
    set(
        &mut app,
        &target,
        object::SHAPE_FILL_ENABLED,
        PropertyValue::Bool(false),
    );
    let fill = app
        .resolve_property(&PropertyAddress::new(
            target.clone(),
            object::SHAPE_FILL_COLOR,
        ))
        .unwrap();
    assert!(matches!(fill.availability, Availability::Disabled(_)));
    set(
        &mut app,
        &target,
        object::SHAPE_FILL_ENABLED,
        PropertyValue::Bool(true),
    );
    assert_eq!(
        app.doc.canvases[0]
            .object(id)
            .unwrap()
            .shape()
            .unwrap()
            .fill,
        Some(Color::rgb(200, 200, 200))
    );
}

#[test]
fn style_properties_write_multiple_objects_atomically() {
    let (mut text_app, first, first_id) =
        object_app(CanvasObjectKind::Text(TextBox::label(String::new())));
    let mut second = text_app.doc.canvases[0].object(first_id).unwrap().clone();
    second.id = text_app.doc.canvases[0].allocate_object_id();
    let second_id = second.id;
    text_app.doc.canvases[0].objects.push(second);
    let second_target = text_app.object_target(0, second_id).unwrap();
    let commit = text_app
        .plan_property_write(
            object::TEXT_COLOR,
            &[first, second_target],
            &PropertyValue::Color(Color::rgb(3, 4, 5)),
        )
        .unwrap();
    assert_eq!(commit.applied.len(), 2);
    text_app.commit_property(commit);
}

#[test]
fn chart_properties_write_multiple_objects_atomically() {
    let (mut chart_app, first, first_id) = table_app();
    let mut second = chart_app.doc.canvases[0].object(first_id).unwrap().clone();
    second.id = chart_app.doc.canvases[0].allocate_object_id();
    let second_id = second.id;
    chart_app.doc.canvases[0].objects.push(second);
    let second_target = chart_app.object_target(0, second_id).unwrap();
    let commit = chart_app
        .plan_property_write(
            object::CHART_TYPE_ID,
            &[first, second_target],
            &PropertyValue::Enum("table_bar"),
        )
        .unwrap();
    assert_eq!(commit.applied.len(), 2);
    chart_app.commit_property(commit);
}

#[test]
fn continuous_style_drag_records_one_undo_step() {
    let (mut app, target, id) =
        object_app(CanvasObjectKind::Shape(ShapeObject::new(ShapeKind::Rect)));
    let before = app.doc.canvases[0]
        .object(id)
        .unwrap()
        .shape()
        .unwrap()
        .stroke_width;
    app.begin_property_gesture(object::SHAPE_STROKE_WIDTH);
    for width in [2.0, 3.0, 4.0] {
        set(
            &mut app,
            &target,
            object::SHAPE_STROKE_WIDTH,
            PropertyValue::Float(width),
        );
    }
    app.end_property_gesture();
    assert_eq!(app.session.undo_stack.len(), 1);
    app.undo();
    assert_eq!(
        app.doc.canvases[0]
            .object(id)
            .unwrap()
            .shape()
            .unwrap()
            .stroke_width,
        before
    );
}

#[test]
fn continuous_chart_drag_records_one_undo_step() {
    let (mut app, target, id) = table_app();
    set(
        &mut app,
        &target,
        object::CHART_TYPE_ID,
        PropertyValue::Enum("table_surface"),
    );
    let history = app.session.undo_stack.len();
    let before = app.doc.canvases[0]
        .object(id)
        .unwrap()
        .plot()
        .unwrap()
        .chart
        .view_angles[0];
    app.begin_property_gesture(object::CHART_VIEW_AZIMUTH);
    for degrees in [-40.0_f64, -30.0, -20.0] {
        set(
            &mut app,
            &target,
            object::CHART_VIEW_AZIMUTH,
            PropertyValue::Float(degrees.to_radians()),
        );
    }
    app.end_property_gesture();
    assert_eq!(app.session.undo_stack.len(), history + 1);
    app.undo();
    assert_eq!(
        app.doc.canvases[0]
            .object(id)
            .unwrap()
            .plot()
            .unwrap()
            .chart
            .view_angles[0],
        before
    );
}
