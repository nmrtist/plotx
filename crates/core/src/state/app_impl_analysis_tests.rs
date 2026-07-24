use super::*;
use num_complex::Complex64;
use plotx_io::{AxisSource, Dim, NmrData2D, PseudoAxis, PseudoKind, QuadMode};

#[test]
fn live_and_frozen_region_tables_record_lineage() {
    let dim = Dim {
        spectral_width_hz: 1000.0,
        observe_freq_mhz: 400.0,
        carrier_ppm: 5.0,
        nucleus: "1H".to_owned(),
        group_delay: 0.0,
    };
    let data = NmrData2D {
        data: vec![Complex64::new(1.0, 0.0); 8],
        rows: 2,
        cols: 4,
        domain: plotx_io::Domain::Frequency,
        direct: dim.clone(),
        indirect: dim,
        quad: QuadMode::Complex,
        indirect_conjugate: false,
        experiment: None,
        pseudo_axis: Some(PseudoAxis {
            name: "delay".to_owned(),
            kind: PseudoKind::Delay,
            values: vec![0.1, 0.2],
            unit: "s".to_owned(),
            source: AxisSource::EmbeddedList,
        }),
        diffusion: None,
        nus: None,
        source: "series".to_owned(),
    };
    let mut source = Nmr2DDataset::load(data);
    source.regions.push(Region {
        id: 0,
        lo: 4.0,
        hi: 6.0,
        name: "signal".to_owned(),
        color: region_color(0),
        metric: None,
    });
    let mut app = PlotxApp::new();
    app.doc.datasets.push(Dataset::Nmr2D(Box::new(source)));

    app.create_region_table(0);
    app.freeze_region_table(0);

    assert_eq!(
        app.doc.datasets[1].lineage(),
        Some(&DatasetLineage::new(
            DerivationKind::LiveRegionTable,
            [app.doc.datasets[0].resource_id()]
        ))
    );
    assert_eq!(
        app.doc.datasets[2].lineage(),
        Some(&DatasetLineage::new(
            DerivationKind::FrozenRegionTable,
            [app.doc.datasets[0].resource_id()]
        ))
    );
    assert!(app.doc.datasets[1].as_table().unwrap().provenance.is_some());
    assert!(app.doc.datasets[2].as_table().unwrap().provenance.is_none());
}

fn fit_table(meta: Option<DiffusionConstants>) -> TableDataset {
    let mut table = materialized_float_series_table(
        (
            "Gradient".into(),
            "T/m".into(),
            vec![Some(0.02), Some(0.08), Some(0.15)],
        ),
        vec![FloatSeries {
            name: "signal".into(),
            unit: String::new(),
            values: vec![Some(10.0), Some(8.0), Some(4.0)],
            uncertainty: None,
            fit: None,
        }],
        "plotx.test.fit-table.v1",
    )
    .unwrap();
    table.meta.diffusion = meta;
    table
}

#[test]
fn independent_variable_named_b_is_still_bound_to_the_x_axis() {
    use plotx_analysis::fit_model::{FitModelDefinition, ParameterDefinition, VariableDefinition};
    let mut model = FitModelDefinition::explicit(
        "12345678-1234-4234-8234-123456789abc",
        "Unrelated b",
        "y = offset + slope*b",
    );
    model.independent_variables = vec![VariableDefinition::new("b")];
    model.responses = vec![VariableDefinition::new("y")];
    model.parameters = vec![
        ParameterDefinition::free("offset", 0.0),
        ParameterDefinition::free("slope", 1.0),
    ];
    let table = fit_table(None);
    let view = table.fit_analysis_view().unwrap();
    let inputs = super::app_impl_analysis::build_table_fit_inputs(&view, model, false, 0)
        .expect("an unrelated b variable must not require diffusion metadata");
    assert_eq!(inputs.datasets[0].inputs["b"], vec![0.02, 0.08, 0.15]);
    assert!(matches!(
        inputs.bindings[0].variables["b"],
        FitDataBinding::Column { .. }
    ));
}

#[test]
fn stejskal_tanner_binds_gradient_and_diffusion_constants_without_transforming_x() {
    let diffusion = DiffusionConstants {
        gamma: 2.675_222_005e8,
        delta: 2.0e-3,
        big_delta: 80.0e-3,
        tau: 1.0e-3,
        shape_factor: 1.0 / 3.0,
    };
    let table = fit_table(Some(diffusion));
    let view = table.fit_analysis_view().unwrap();
    let model = plotx_analysis::models::builtin_model_by_name("Stejskal–Tanner").unwrap();
    let inputs = super::app_impl_analysis::build_table_fit_inputs(&view, model, false, 0)
        .expect("diffusion metadata satisfies the model's semantic constants");
    assert_eq!(inputs.input_name, "g");
    assert_eq!(inputs.datasets[0].inputs["g"], vec![0.02, 0.08, 0.15]);
    assert_eq!(inputs.datasets[0].constants["gamma"], diffusion.gamma);
    assert!(matches!(
        inputs.bindings[0].constants["gamma"],
        FitDataBinding::Metadata { .. }
    ));
}

#[test]
fn matching_constant_names_do_not_inherit_the_builtin_diffusion_profile() {
    use plotx_analysis::fit_model::{ConstantDefinition, FitModelDefinition, VariableDefinition};
    let mut model = FitModelDefinition::explicit(
        "87654321-1234-4234-8234-123456789abc",
        "Unrelated tau",
        "y = tau*x",
    );
    model.independent_variables = vec![VariableDefinition::new("x")];
    model.responses = vec![VariableDefinition::new("y")];
    model.constants = vec![ConstantDefinition {
        id: "tau".into(),
        display_name: "Unrelated tau".into(),
        unit: String::new(),
        description: String::new(),
        default_value: None,
    }];
    let table = fit_table(Some(DiffusionConstants {
        gamma: 2.675_222_005e8,
        delta: 2.0e-3,
        big_delta: 80.0e-3,
        tau: 1.0e-3,
        shape_factor: 1.0 / 3.0,
    }));
    let view = table.fit_analysis_view().unwrap();
    let error = super::app_impl_analysis::build_table_fit_inputs(&view, model, false, 0)
        .err()
        .expect("custom model constants require their own binding choice");
    assert!(error.contains("no source"));
}

#[test]
fn curve_fit_selection_records_exact_rows_and_non_finite_causes() {
    use plotx_analysis::fit_model::{FitOptions, NonFinitePolicy};

    let dataset = materialized_float_series_table(
        (
            "x".into(),
            "s".into(),
            vec![Some(0.0), Some(1.0), None, Some(3.0), Some(4.0)],
        ),
        vec![FloatSeries {
            name: "signal".into(),
            unit: String::new(),
            values: vec![
                Some(2.0),
                Some(f64::INFINITY),
                Some(6.0),
                Some(8.0),
                Some(10.0),
            ],
            uncertainty: None,
            fit: None,
        }],
        "plotx.test.fit-selection-table.v1",
    )
    .unwrap();
    let view = dataset.fit_analysis_view().unwrap();
    let model = plotx_analysis::models::builtin_model_by_name("Linear").unwrap();
    let inputs = super::app_impl_analysis::build_table_fit_inputs(&view, model, false, 0)
        .expect("table binds to the linear model");
    let options = FitOptions {
        non_finite: NonFinitePolicy::ExcludeRows,
        ..FitOptions::default()
    };
    let result = plotx_analysis::fit_model::fit_model(inputs.model, inputs.datasets, &[], options)
        .expect("three finite rows are enough for a linear fit");
    let selection = super::fit_selection::snapshot(&view, &inputs.bindings, &result)
        .expect("selection identity is valid");

    assert_eq!(selection.source_revision, view.revision_id);
    assert_eq!(selection.instances.len(), 1);
    let instance = &selection.instances[0];
    assert_eq!(instance.included_rows.len(), 3);
    assert_eq!(instance.excluded_rows.len(), 2);
    assert_eq!(instance.excluded_rows[0].quantities, ["y"]);
    assert_eq!(instance.excluded_rows[1].quantities, ["x"]);
    assert_eq!(
        instance.excluded_rows[0].reason,
        FitRowExclusionReason::NonFiniteRequiredValue
    );
    assert_eq!(
        instance.excluded_rows[1].reason,
        FitRowExclusionReason::NullRequiredValue
    );
    assert_eq!(instance.excluded_rows[0].row, view.row_ids[1]);
    assert_eq!(instance.excluded_rows[1].row, view.row_ids[2]);
}
