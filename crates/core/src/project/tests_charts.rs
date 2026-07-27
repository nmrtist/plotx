//! Round-trip coverage for chart-type selection and per-chart options.

use super::tests::{first_plot, first_plot_mut, temp_project};
use super::*;
use crate::state::{CanvasDocument, Dataset, ObjectFrame, PlotxApp};

fn chart_table() -> crate::state::TableDataset {
    use crate::state::{FloatSeries, materialized_float_series_table};
    materialized_float_series_table(
        (
            "Gradient".into(),
            "mT/m".into(),
            vec![Some(0.0), Some(1.0), Some(2.0)],
        ),
        ["a", "b"]
            .into_iter()
            .map(|name| FloatSeries {
                name: name.into(),
                unit: String::new(),
                values: vec![Some(3.0), Some(2.0), Some(1.0)],
                uncertainty: None,
                fit: None,
            })
            .collect(),
        "plotx.test.project-chart-table.v1",
    )
    .unwrap()
}

#[test]
fn project_roundtrip_preserves_non_default_chart_type() {
    use crate::state::ChartSpec;
    let mut app = PlotxApp::new();
    app.doc
        .datasets
        .push(Dataset::Table(Box::new(chart_table())));
    let mut canvas = CanvasDocument::new("table".to_owned(), [120.0, 80.0]);
    let [w, h] = canvas.size_pt();
    let id = canvas.allocate_object_id();
    let object =
        app.build_plot_object(0, ObjectFrame::new(0.0, 0.0, w, h), id, "Plot 1".to_owned());
    canvas.objects.push(object);
    app.doc.canvases.push(canvas);
    app.focus_single(0);
    app.session.active_canvas = Some(0);
    let selected_column = app.doc.datasets[0].as_table().unwrap().series_bindings[1].value_column;
    first_plot_mut(&mut app).chart = ChartSpec {
        type_id: "table_bar".to_owned(),
        column: Some(selected_column),
        ..ChartSpec::default()
    };

    let path = temp_project("charttype");
    let _ = std::fs::remove_file(&path);
    save_project(&app, &path, false).unwrap();
    let loaded = load_project(&path).unwrap();
    let _ = std::fs::remove_file(&path);

    let chart = &first_plot(&loaded).chart;
    assert_eq!(chart.type_id, "table_bar");
    assert_eq!(chart.column, Some(selected_column));
    // The materialised figure is the bar chart (one rectangle per x row).
    assert_eq!(first_plot(&loaded).figure().polygons.len(), 3);
}

#[test]
fn project_roundtrip_preserves_chart_options() {
    use crate::state::ChartSpec;
    let mut app = PlotxApp::new();
    app.doc
        .datasets
        .push(Dataset::Table(Box::new(chart_table())));
    let mut canvas = CanvasDocument::new("table".to_owned(), [120.0, 80.0]);
    let [w, h] = canvas.size_pt();
    let id = canvas.allocate_object_id();
    let object =
        app.build_plot_object(0, ObjectFrame::new(0.0, 0.0, w, h), id, "Plot 1".to_owned());
    canvas.objects.push(object);
    app.doc.canvases.push(canvas);
    app.focus_single(0);
    app.session.active_canvas = Some(0);
    let selected_column = app.doc.datasets[0].as_table().unwrap().series_bindings[1].value_column;
    first_plot_mut(&mut app).chart = ChartSpec {
        type_id: "table_histogram".to_owned(),
        column: Some(selected_column),
        bins: Some(7),
        stacked: true,
        colormap: plotx_figure::ColormapId::Plasma,
        view_angles: [-30.0, 55.0],
    };

    let path = temp_project("chartopts");
    let _ = std::fs::remove_file(&path);
    save_project(&app, &path, false).unwrap();
    let loaded = load_project(&path).unwrap();
    let _ = std::fs::remove_file(&path);

    let chart = &first_plot(&loaded).chart;
    assert_eq!(chart.type_id, "table_histogram");
    assert_eq!(chart.bins, Some(7));
    assert!(chart.stacked);
    assert_eq!(chart.colormap, plotx_figure::ColormapId::Plasma);
    assert_eq!(chart.view_angles, [-30.0, 55.0]);
    assert_eq!(chart.column, Some(selected_column));
}

#[test]
fn catalog_read_preserves_the_empty_follow_default_chart_sentinel_on_resave() {
    let mut app = PlotxApp::new();
    app.doc
        .datasets
        .push(Dataset::Table(Box::new(chart_table())));
    let mut canvas = CanvasDocument::new("table".to_owned(), [120.0, 80.0]);
    let id = canvas.allocate_object_id();
    canvas.objects.push(app.build_plot_object(
        0,
        ObjectFrame::new(0.0, 0.0, 100.0, 70.0),
        id,
        "Plot".to_owned(),
    ));
    canvas
        .object_mut(id)
        .and_then(|object| object.plot_mut())
        .expect("plot")
        .chart
        .type_id
        .clear();
    app.doc.canvases.push(canvas);
    let first_path = temp_project("chart-sentinel-first");
    let second_path = temp_project("chart-sentinel-second");
    let _ = std::fs::remove_file(&first_path);
    let _ = std::fs::remove_file(&second_path);
    save_project(&app, &first_path, false).unwrap();

    let loaded = load_project(&first_path).unwrap();
    let target = loaded.object_target(0, id).expect("plot target");
    let resolved = loaded
        .resolve_property(&crate::properties::PropertyAddress::new(
            target,
            crate::properties::object::CHART_TYPE_ID,
        ))
        .expect("catalog read");
    assert_eq!(
        resolved.value,
        crate::properties::AggregateValue::Uniform(crate::properties::PropertyValue::Enum(
            "table_line"
        ))
    );
    assert!(
        first_plot(&loaded).chart.type_id.is_empty(),
        "reading resolves the sentinel for display without materializing it"
    );
    save_project(&loaded, &second_path, false).unwrap();
    let resaved = load_project(&second_path).unwrap();
    let _ = std::fs::remove_file(&first_path);
    let _ = std::fs::remove_file(&second_path);
    assert!(first_plot(&resaved).chart.type_id.is_empty());
}
