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
    assert_eq!(first_plot(&loaded).figure.polygons.len(), 3);
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
