use super::tests::{first_plot_mut, sample_app, temp_project};
use super::*;
use crate::state::{DatasetId, DatasetLineage, DerivationKind, ProjectionSource, SeriesBinding};

fn save_error(app: &PlotxApp, name: &str) -> String {
    let path = temp_project(name);
    let _ = std::fs::remove_file(&path);
    let error = save_project(app, &path, false).unwrap_err().to_string();
    assert!(!path.exists(), "an invalid project must not be committed");
    error
}

#[test]
fn save_rejects_missing_lineage_source() {
    let mut app = sample_app();
    let missing = DatasetId::new();
    app.doc.datasets[0].set_lineage(Some(DatasetLineage::new(
        DerivationKind::Projection,
        [missing],
    )));

    let error = save_error(&app, "missing-lineage-source");
    assert!(
        error.contains("references missing lineage source"),
        "{error}"
    );
}

#[test]
fn save_rejects_missing_primary_and_series_datasets() {
    let mut missing_primary = sample_app();
    let missing = DatasetId::new();
    first_plot_mut(&mut missing_primary).binding.series[0].dataset = missing;
    let error = save_error(&missing_primary, "missing-primary-dataset");
    assert!(
        error.contains(&format!("missing primary dataset {missing}")),
        "{error}"
    );

    let mut missing_series = sample_app();
    let missing = DatasetId::new();
    first_plot_mut(&mut missing_series)
        .binding
        .series
        .push(SeriesBinding::new(missing));
    let error = save_error(&missing_series, "missing-series-dataset");
    assert!(
        error.contains(&format!("missing series dataset {missing}")),
        "{error}"
    );
}

#[test]
fn save_rejects_missing_attached_projection_dataset() {
    let mut app = sample_app();
    let missing = DatasetId::new();
    first_plot_mut(&mut app).projections.top.source = ProjectionSource::Attached(missing);

    let error = save_error(&app, "missing-projection-dataset");
    assert!(
        error.contains(&format!(
            "axis projection references missing dataset {missing}"
        )),
        "{error}"
    );
}

#[test]
fn object_allocator_roundtrip_preserves_deleted_high_water_mark() {
    let mut app = sample_app();
    let canvas = &mut app.doc.canvases[0];
    let first_unused = canvas.allocate_object_id();
    let high_water = canvas.next_object_id;
    assert!(canvas.object(first_unused).is_none());

    let path = temp_project("object_allocator");
    let _ = std::fs::remove_file(&path);
    save_project(&app, &path, false).unwrap();
    let loaded = load_project(&path).unwrap();
    let _ = std::fs::remove_file(&path);

    assert_eq!(loaded.doc.canvases[0].next_object_id, high_water);
}

#[test]
fn loading_a_maximum_object_id_reports_exhaustion() {
    let app = sample_app();
    let path = temp_project("maximum-object-id");
    let _ = std::fs::remove_file(&path);
    save_project(&app, &path, false).unwrap();
    let file = std::fs::File::open(&path).unwrap();
    let mut zip = zip::ZipArchive::new(file).unwrap();
    let view: ViewObject = serde_json::from_value(serde_json::json!({
        "id": "view-max-object-id",
        "role": "canvas",
        "classification": {
            "domain": "visualization",
            "object": "page"
        },
        "name": "Maximum object id",
        "next_object_id": 1,
        "layout": { "size_mm": [120.0, 80.0] },
        "objects": [{
            "id": u64::MAX.to_string(),
            "name": "Label",
            "kind": "text",
            "frame": { "x": 0.0, "y": 0.0, "width": 10.0, "height": 10.0 },
            "locked": false,
            "visible": true
        }]
    }))
    .unwrap();
    let mut loading_app = PlotxApp::new();

    let error = match view_to_canvas(
        &mut loading_app,
        &mut zip,
        "view-max-object-id",
        &view,
        0,
        &HashMap::new(),
    ) {
        Ok(_) => panic!("an exhausted object id space must be rejected"),
        Err(error) => error,
    };
    let _ = std::fs::remove_file(&path);

    assert!(
        matches!(error, ProjectError::Invalid(ref message) if message.contains("object id space exhausted")),
        "{error}"
    );
}
