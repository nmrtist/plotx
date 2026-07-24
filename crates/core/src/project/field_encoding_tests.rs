use super::tests::{first_plot, synthetic_true_2d, temp_project};
use super::*;
use crate::state::{AfmDataset, CanvasDocument, Dataset, Nmr2DDataset, ObjectFrame, PlotxApp};
use plotx_figure::{HeatmapSpec, SeriesEncoding};
use std::collections::BTreeSet;
use std::sync::Arc;

#[test]
fn project_roundtrip_preserves_concrete_contour_series_encoding() {
    let mut app = PlotxApp::new();
    app.doc
        .datasets
        .push(Dataset::Nmr2D(Box::new(Nmr2DDataset::load(
            synthetic_true_2d(),
        ))));
    let mut canvas = CanvasDocument::new("contour".to_owned(), [120.0, 80.0]);
    let [width, height] = canvas.size_pt();
    let object = app.build_plot_object(
        0,
        ObjectFrame::new(0.0, 0.0, width, height),
        canvas.allocate_object_id(),
        "Contour".to_owned(),
    );
    canvas.objects.push(object);
    app.doc.canvases.push(canvas);
    let before = first_plot(&app).binding.series[0].clone();
    assert!(matches!(
        before.encoding,
        plotx_figure::SeriesEncoding::Contour(_)
    ));

    let path = temp_project("contour-series-binding");
    let _ = std::fs::remove_file(&path);
    save_project(&app, &path, false).unwrap();
    let loaded = load_project(&path).unwrap();
    let _ = std::fs::remove_file(&path);

    assert_eq!(first_plot(&loaded).binding.series, vec![before]);
}

#[test]
fn nmr_two_dimensional_fields_expose_real_and_magnitude_capabilities() {
    let dataset = Dataset::Nmr2D(Box::new(Nmr2DDataset::load(synthetic_true_2d())));
    let fields = dataset.field_descriptors();
    assert_eq!(fields.len(), 2);
    assert_eq!(fields[0].local_id, "nmr.real");
    assert!(
        fields[0]
            .capabilities
            .contains(crate::automation::CAP_FIELD_SCALAR_GRID_2D_REGULAR)
    );
    assert!(
        fields[0]
            .capabilities
            .contains(crate::automation::CAP_FIELD_SIGNED)
    );
    assert!(
        fields[0]
            .capabilities
            .contains(crate::automation::CAP_FIELD_NOISE_SCALE)
    );
    assert_eq!(fields[1].local_id, "nmr.magnitude");
    assert!(
        fields[1]
            .capabilities
            .contains(crate::automation::CAP_FIELD_SCALAR_GRID_2D_REGULAR)
    );
    assert!(
        fields[1]
            .capabilities
            .contains(crate::automation::CAP_FIELD_BOUNDED)
    );
    assert!(
        !fields[1]
            .capabilities
            .contains(crate::automation::CAP_FIELD_SIGNED)
    );
}

#[test]
fn afm_calibration_and_scan_metadata_keep_field_ids_distinct_after_roundtrip() {
    let base = afm_channel(1.0, 0.0, 2.0, 3.0);
    let variants = [
        base.clone(),
        afm_channel(2.0, 0.0, 2.0, 3.0),
        afm_channel(1.0, 1.0, 2.0, 3.0),
        afm_channel(1.0, 0.0, 4.0, 3.0),
        afm_channel(1.0, 0.0, 2.0, 5.0),
    ];
    let keys: Vec<_> = variants.iter().map(crate::state::afm_channel_key).collect();
    assert_eq!(keys.iter().collect::<BTreeSet<_>>().len(), variants.len());

    let mut app = PlotxApp::new();
    app.doc
        .datasets
        .push(Dataset::Afm(Box::new(AfmDataset::load(
            plotx_io::AfmData {
                images: variants.to_vec(),
                forces: None,
                source: "calibration identity test".to_owned(),
                import_warnings: Vec::new(),
            },
        ))));
    let before = app.doc.datasets[0].field_descriptors();
    assert_eq!(before.len(), variants.len());
    assert_eq!(
        before
            .iter()
            .map(|field| field.id)
            .collect::<BTreeSet<_>>()
            .len(),
        variants.len()
    );
    for field in &before {
        assert!(
            app.doc.datasets[0]
                .encoded_field_figure(field.id, &SeriesEncoding::Heatmap(HeatmapSpec::default()))
                .is_some()
        );
    }

    let path = temp_project("afm-calibration-field-ids");
    let _ = std::fs::remove_file(&path);
    save_project(&app, &path, false).unwrap();
    let loaded = load_project(&path).unwrap();
    let _ = std::fs::remove_file(&path);
    let after = loaded.doc.datasets[0].field_descriptors();
    assert_eq!(
        after.iter().map(|field| field.id).collect::<Vec<_>>(),
        before.iter().map(|field| field.id).collect::<Vec<_>>()
    );
}

fn afm_channel(
    multiplier: f64,
    offset: f64,
    scan_size_x: f64,
    scan_size_y: f64,
) -> plotx_io::AfmImageChannel {
    plotx_io::AfmImageChannel {
        name: "Height".to_owned(),
        width: 2,
        height: 2,
        scan_size_x,
        scan_size_y,
        lateral_unit: "nm".to_owned(),
        scale: plotx_io::AfmScale {
            multiplier,
            offset,
            unit: "nm".to_owned(),
        },
        raw: Arc::from(vec![1, 2, 3, 4]),
        frame_direction: plotx_io::AfmFrameDirection::Trace,
    }
}
