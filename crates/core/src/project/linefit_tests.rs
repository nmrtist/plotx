use super::tests::synthetic_1d;
use super::*;
use crate::state::{LineShapeKind, StoredFittedPeak, StoredLineFit};

pub(super) fn sample_line_fit() -> StoredLineFit {
    StoredLineFit {
        id: 7,
        lo: 1.5,
        hi: 2.5,
        shape: LineShapeKind::Lorentzian,
        peaks: vec![StoredFittedPeak {
            position: 2.0,
            height: 42.0,
            fwhm: 0.1,
            eta: None,
            area: 6.6,
            position_sigma: Some(0.01),
            height_sigma: Some(0.5),
            fwhm_sigma: None,
            eta_sigma: None,
            area_sigma: None,
        }],
        offset: 0.3,
        offset_sigma: Some(0.02),
        r2: 0.998,
    }
}

#[test]
fn table_line_fits_survive_project_roundtrip() {
    use crate::state::{Dataset, FloatSeries, materialized_float_series_table};

    let mut tds = materialized_float_series_table(
        ("x".into(), "".into(), vec![Some(1.0), Some(2.0), Some(3.0)]),
        vec![FloatSeries {
            name: "y".to_owned(),
            unit: String::new(),
            values: vec![Some(0.5), Some(4.0), Some(0.5)],
            uncertainty: None,
            fit: None,
        }],
        "plotx.test.project-linefit-table.v1",
    )
    .unwrap();
    tds.line_fits.push(sample_line_fit());
    tds.next_line_fit_id = 8;
    let mut app = crate::state::PlotxApp::new();
    app.doc.datasets.push(Dataset::Table(Box::new(tds)));

    let path = super::tests::temp_project("table-linefit");
    let _ = std::fs::remove_file(&path);
    save_project(&app, &path, false).unwrap();
    let loaded = load_project(&path).unwrap();
    let _ = std::fs::remove_file(&path);

    let Dataset::Table(t) = &loaded.doc.datasets[0] else {
        panic!("table dataset survives round-trip");
    };
    assert_eq!(t.line_fits, vec![sample_line_fit()]);
    assert_eq!(t.next_line_fit_id, 8);
}

#[test]
fn table_statistics_survive_project_roundtrip() {
    use crate::state::{
        Dataset, FloatSeries, StatAnalysis, StatOutcome, StatQuestion,
        materialized_float_series_table,
    };

    let mut tds = materialized_float_series_table(
        (
            "row".into(),
            "".into(),
            vec![Some(0.0), Some(1.0), Some(2.0), Some(3.0)],
        ),
        vec![FloatSeries {
            name: "control".to_owned(),
            unit: String::new(),
            values: vec![Some(4.2), Some(4.5), Some(4.1), Some(4.4)],
            uncertainty: None,
            fit: None,
        }],
        "plotx.test.project-statistics-table.v1",
    )
    .unwrap();
    tds.statistics.push(StatAnalysis {
        id: 4,
        question: StatQuestion::Summarize,
        title: "Descriptive statistics: control".to_owned(),
        configuration: "Columns: control".to_owned(),
        data_note: None,
        selection: Default::default(),
        outcome: StatOutcome::Descriptive(Vec::new()),
    });
    tds.next_stat_id = 5;
    let mut app = crate::state::PlotxApp::new();
    app.doc.datasets.push(Dataset::Table(Box::new(tds)));

    let path = super::tests::temp_project("table-statistics");
    let _ = std::fs::remove_file(&path);
    save_project(&app, &path, false).unwrap();
    let loaded = load_project(&path).unwrap();
    let _ = std::fs::remove_file(&path);

    let Dataset::Table(t) = &loaded.doc.datasets[0] else {
        panic!("table dataset survives round-trip");
    };
    assert_eq!(t.statistics.len(), 1);
    assert_eq!(t.statistics[0].id, 4);
    assert_eq!(t.statistics[0].title, "Descriptive statistics: control");
    // The runtime id source is rebuilt above the highest stored id on load.
    assert_eq!(t.next_stat_id, 5);
}

#[test]
fn recipe_without_line_fits_key_loads_with_empty_fits() {
    let mut dataset = NmrDataset::load(synthetic_1d());
    dataset.line_fits.push(sample_line_fit());
    dataset.next_line_fit_id = 8;
    let recipe = RecipeObject {
        id: "recipe_000000".to_owned(),
        role: "recipe".to_owned(),
        classification: Classification {
            domain: "spectroscopy".to_owned(),
            technique: Some("nmr".to_owned()),
            object: "recipe".to_owned(),
        },
        input: "data_000000".to_owned(),
        parameters: RecipeParameters::default(),
        extensions: serde_json::json!({
            "plotx.analysis": { "peaks": crate::state::PeakSet::default(), "integrals": [] }
        }),
    };

    apply_1d_recipe(&mut dataset, &recipe).unwrap();

    assert!(dataset.line_fits.is_empty());
    assert_eq!(dataset.next_line_fit_id, 0);
}
