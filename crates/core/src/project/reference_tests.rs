use super::tests::{first_plot_mut, sample_app, temp_project};
use super::*;
use crate::state::{DatasetLineage, DerivationKind, ProjectionSource, SeriesBinding};

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
    app.doc.datasets[0].set_lineage(Some(DatasetLineage::new(DerivationKind::Projection, [99])));

    let error = save_error(&app, "missing-lineage-source");
    assert!(error.contains("missing lineage source 99"), "{error}");
}

#[test]
fn save_rejects_missing_primary_and_series_datasets() {
    let mut missing_primary = sample_app();
    first_plot_mut(&mut missing_primary).binding.series[0].dataset = 99;
    let error = save_error(&missing_primary, "missing-primary-dataset");
    assert!(error.contains("missing primary dataset 99"), "{error}");

    let mut missing_series = sample_app();
    first_plot_mut(&mut missing_series)
        .binding
        .series
        .push(SeriesBinding::new(99));
    let error = save_error(&missing_series, "missing-series-dataset");
    assert!(error.contains("missing series dataset 99"), "{error}");
}

#[test]
fn save_rejects_missing_attached_projection_dataset() {
    let mut app = sample_app();
    first_plot_mut(&mut app).projections.top.source = ProjectionSource::Attached(99);

    let error = save_error(&app, "missing-projection-dataset");
    assert!(
        error.contains("axis projection references missing dataset 99"),
        "{error}"
    );
}
