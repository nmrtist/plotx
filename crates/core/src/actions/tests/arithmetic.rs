use super::*;
use crate::state::{DatasetLineage, DerivationKind};
use plotx_processing::arithmetic::SpectrumBinaryOp;

fn two_spectrum_app() -> PlotxApp {
    let mut app = sample_app();
    app.doc
        .datasets
        .push(Dataset::Nmr(Box::new(NmrDataset::load(synthetic_1d()))));
    app
}

fn spectrum_values(app: &PlotxApp, di: usize) -> Vec<num_complex::Complex64> {
    app.doc.datasets[di]
        .as_nmr()
        .unwrap()
        .spectrum
        .values
        .clone()
}

#[test]
fn subtract_with_coefficient_creates_named_dataset_on_new_canvas() {
    let mut app = two_spectrum_app();
    let a = spectrum_values(&app, 0);
    let b = spectrum_values(&app, 1);
    let canvases_before = app.doc.canvases.len();

    app.combine_spectra_datasets(0, 1, SpectrumBinaryOp::Subtract, 0.5);

    assert_eq!(app.doc.datasets.len(), 3);
    assert_eq!(app.doc.canvases.len(), canvases_before + 1);
    let result = app.doc.datasets[2].as_nmr().unwrap();
    assert!(result.name.as_deref().unwrap().contains("−"));
    assert!(result.name.as_deref().unwrap().contains("0.5·"));
    for ((r, x), y) in result.spectrum.values.iter().zip(&a).zip(&b) {
        assert!((r - (x - 0.5 * y)).norm() < 1e-9);
    }
    assert_eq!(result.spectrum.nucleus, "1H");
    assert_eq!(
        app.doc.datasets[2].lineage(),
        Some(&DatasetLineage::new(
            DerivationKind::SpectrumArithmetic,
            [
                app.doc.datasets[0].resource_id(),
                app.doc.datasets[1].resource_id(),
            ]
        ))
    );
}

#[test]
fn add_uses_plain_name_when_k_is_one() {
    let mut app = two_spectrum_app();
    let expected = format!(
        "{} + {}",
        app.doc.datasets[0].display_name(),
        app.doc.datasets[1].display_name()
    );
    app.combine_spectra_datasets(0, 1, SpectrumBinaryOp::Add, 1.0);
    let name = app.doc.datasets[2].as_nmr().unwrap().name.clone().unwrap();
    assert_eq!(name, expected);
}

#[test]
fn undo_removes_result_dataset_and_its_canvas_in_one_step() {
    let mut app = two_spectrum_app();
    let canvases_before = app.doc.canvases.len();
    app.combine_spectra_datasets(0, 1, SpectrumBinaryOp::Subtract, 1.0);
    assert_eq!(app.doc.datasets.len(), 3);

    app.undo();

    assert_eq!(app.doc.datasets.len(), 2);
    assert_eq!(app.doc.canvases.len(), canvases_before);

    app.redo();
    assert_eq!(app.doc.datasets.len(), 3);
    assert_eq!(app.doc.canvases.len(), canvases_before + 1);
}

#[test]
fn result_dataset_replays_exactly_after_retransform() {
    let mut app = two_spectrum_app();
    app.combine_spectra_datasets(0, 1, SpectrumBinaryOp::Subtract, 0.5);
    let mut ds = app.doc.datasets[2].as_nmr().unwrap().clone();
    let shown = ds.spectrum.clone();
    ds.retransform();
    assert_eq!(ds.spectrum.values.len(), shown.values.len());
    for (a, b) in ds.spectrum.values.iter().zip(&shown.values) {
        assert!((a - b).norm() < 1e-9);
    }
    for (a, b) in ds.spectrum.ppm.iter().zip(&shown.ppm) {
        assert!((a - b).abs() < 1e-9);
    }
}

#[test]
fn nucleus_mismatch_is_rejected_without_side_effects() {
    let mut app = sample_app();
    let mut other = synthetic_1d();
    other.nucleus = "13C".to_owned();
    app.doc
        .datasets
        .push(Dataset::Nmr(Box::new(NmrDataset::load(other))));
    let canvases_before = app.doc.canvases.len();

    app.combine_spectra_datasets(0, 1, SpectrumBinaryOp::Subtract, 1.0);

    assert_eq!(app.doc.datasets.len(), 2);
    assert_eq!(app.doc.canvases.len(), canvases_before);
    assert!(app.session.status.contains("Nuclei differ"));
    assert!(app.spectrum_arithmetic_compat(0, 1).is_err());
}

#[test]
fn same_operand_and_zero_coefficient_edge_cases() {
    let mut app = sample_app();
    let a = spectrum_values(&app, 0);

    app.combine_spectra_datasets(0, 0, SpectrumBinaryOp::Subtract, 1.0);
    assert!(spectrum_values(&app, 1).iter().all(|v| v.norm() < 1e-12));

    app.combine_spectra_datasets(0, 0, SpectrumBinaryOp::Add, 0.0);
    assert_eq!(spectrum_values(&app, 2), a);
}

#[test]
fn single_point_operands_combine_without_panicking() {
    use plotx_processing::Slice1D;
    let mut app = PlotxApp::new();
    let point = |ppm: f64, re: f64| Slice1D {
        ppm: vec![ppm],
        values: vec![num_complex::Complex64::new(re, 0.0)],
        nucleus: "1H".to_owned(),
        observe_freq_mhz: 400.0,
        position_ppm: None,
    };
    for (i, s) in [point(1.0, 2.0), point(3.0, 5.0)].into_iter().enumerate() {
        app.doc
            .datasets
            .push(Dataset::Nmr(Box::new(NmrDataset::from_slice(
                s,
                format!("p{i}"),
            ))));
    }

    app.combine_spectra_datasets(0, 1, SpectrumBinaryOp::Add, 1.0);

    let result = spectrum_values(&app, 2);
    assert_eq!(result.len(), 1);
    assert!((result[0].re - 2.0).abs() < 1e-12);
}

#[test]
fn unary_scale_and_offset_create_independent_dataset() {
    let mut app = sample_app();
    let a = spectrum_values(&app, 0);

    app.scale_spectrum_dataset(0, 2.0, 3.0);

    assert_eq!(app.doc.datasets.len(), 2);
    let result = app.doc.datasets[1].as_nmr().unwrap();
    for (r, x) in result.spectrum.values.iter().zip(&a) {
        let expected = 2.0 * x + num_complex::Complex64::new(3.0, 0.0);
        assert!((r - expected).norm() < 1e-9);
    }
    assert_eq!(spectrum_values(&app, 0), a);
    assert_eq!(
        app.doc.datasets[1].lineage(),
        Some(&DatasetLineage::new(
            DerivationKind::SpectrumArithmetic,
            [app.doc.datasets[0].resource_id()]
        ))
    );

    app.scale_spectrum_dataset(0, 1.0, 0.0);
    assert_eq!(app.doc.datasets.len(), 2);
}
