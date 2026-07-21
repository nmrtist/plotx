use super::*;
use crate::state::{FloatSeries, materialized_float_series_table};
use plotx_analysis::fit_model::{FitDataset, FitOptions};
use std::collections::BTreeMap;

#[test]
fn curve_fit_snapshot_redraws_without_model_library() {
    let x: Vec<f64> = (0..12).map(|value| value as f64).collect();
    let y: Vec<f64> = x.iter().map(|value| 2.0 + 0.5 * value).collect();
    let model = plotx_analysis::models::builtin_model_by_name("Linear").unwrap();
    let result = plotx_analysis::fit_model::fit_model(
        model,
        vec![FitDataset {
            id: "column-0".into(),
            inputs: BTreeMap::from([("x".into(), x.clone())]),
            responses: BTreeMap::from([("y".into(), y.clone())]),
            sigmas: BTreeMap::new(),
            constants: BTreeMap::new(),
        }],
        &[],
        FitOptions::default(),
    )
    .unwrap();
    let mut loaded = materialized_float_series_table(
        ("x".into(), "".into(), x.into_iter().map(Some).collect()),
        vec![FloatSeries {
            name: "line".into(),
            unit: String::new(),
            values: y.into_iter().map(Some).collect(),
            uncertainty: None,
            fit: Some(CurveFitReference {
                analysis_id: 7,
                instance_id: "column-0".into(),
                response: "y".into(),
            }),
        }],
        "plotx.test.curve-fit-table.v1",
    )
    .unwrap();
    loaded.curve_fit_analyses.push(StoredCurveFitAnalysis {
        id: 7,
        name: "Linear".into(),
        bindings: Vec::new(),
        result,
        selection: None,
        plot_samples: BTreeMap::from([(
            "column-0".into(),
            BTreeMap::from([(
                "y".into(),
                (0..=200)
                    .map(|index| {
                        let px = 11.0 * index as f64 / 200.0;
                        [px, 2.0 + 0.5 * px]
                    })
                    .collect(),
            )]),
        )]),
    });
    assert_eq!(loaded.curve_fit_analyses[0].result.model.name, "Linear");
    assert_eq!(loaded.figure().series.len(), 2);
    assert_eq!(loaded.figure().series[1].points.len(), 201);
}
