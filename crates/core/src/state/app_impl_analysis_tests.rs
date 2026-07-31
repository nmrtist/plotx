use super::*;
use num_complex::Complex64;
use plotx_io::{
    AxisSource, Dim, ElectricalUnit, ElectrophysiologyData, NmrData2D, PseudoAxis, PseudoKind,
    QuadMode, RecordedChannel, Sweep,
};

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
    source.region_analysis.regions.push(Region {
        id: RegionId::new(0),
        lo: 4.0,
        hi: 6.0,
        name: "signal".to_owned(),
        label_position: Some([0.25, 0.75]),
        color: region_color(0),
        metric: None,
    });
    source.region_analysis.regions.push(Region {
        id: RegionId::new(1),
        lo: 4.5,
        hi: 5.5,
        name: "reference".to_owned(),
        label_position: None,
        color: region_color(1),
        metric: Some(RegionMetric::Area),
    });
    source.region_analysis.next_region_id = RegionId::new(2);
    let mut app = PlotxApp::new();
    app.doc.datasets.push(Dataset::Nmr2D(Box::new(source)));

    let source_figure = app.build_full_canvas_figure(
        0,
        &ChartSpec::default_for(app.doc.datasets[0].domain()),
        [120.0, 80.0],
    );
    assert_eq!(source_figure.range_annotations.len(), 2);
    assert_eq!(source_figure.range_annotations[0].label, "signal");
    assert_eq!(
        source_figure.range_annotations[0].label_position,
        Some([0.25, 0.75])
    );
    assert_eq!(
        source_figure.range_annotations[0].color,
        plotx_figure::Color::rgb(region_color(0)[0], region_color(0)[1], region_color(0)[2])
    );

    app.create_region_table(0);
    app.freeze_region_table(0);
    assert_eq!(app.doc.datasets.len(), 3, "{}", app.session.status);

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

    let table_figure = app.doc.datasets[1].as_table().unwrap().figure();
    assert!(table_figure.series_colors_are_semantic);
    assert_eq!(table_figure.series.len(), 2);
    assert_eq!(table_figure.series[0].name, "signal");
    assert_eq!(table_figure.series[1].name, "reference");
    assert_ne!(table_figure.series[0].color, table_figure.series[1].color);
    for (series, expected) in table_figure
        .series
        .iter()
        .zip([region_color(0), region_color(1)])
    {
        assert_eq!(
            series.color,
            plotx_figure::Color::rgb(expected[0], expected[1], expected[2])
        );
    }
}

#[test]
fn electrophysiology_edits_keep_the_live_region_table_synchronized() {
    let data = ElectrophysiologyData {
        abf_version: "test".to_owned(),
        sample_rate_hz: 10.0,
        channels: vec![
            RecordedChannel {
                name: "A".to_owned(),
                unit: ElectricalUnit::from_symbol("pA"),
            },
            RecordedChannel {
                name: "B".to_owned(),
                unit: ElectricalUnit::from_symbol("pA"),
            },
        ],
        sweeps: vec![
            Sweep {
                start_time_s: 0.0,
                channels: vec![vec![0.0, -10.0, 0.0, 0.0], vec![0.0, -100.0, 0.0, 0.0]],
                commands: Vec::new(),
            },
            Sweep {
                start_time_s: 1.0,
                channels: vec![vec![0.0, -20.0, 0.0, 0.0], vec![0.0, -200.0, 0.0, 0.0]],
                commands: Vec::new(),
            },
        ],
        protocol: None,
        source: "synthetic.abf".to_owned(),
        import_warnings: Vec::new(),
    };
    let mut recording = ElectrophysiologyDataset::load(data);
    recording.processing.gaussian_lowpass_enabled = false;
    recording.region_analysis.regions.push(Region {
        id: RegionId::new(0),
        lo: 0.0,
        hi: 0.4,
        name: "response".to_owned(),
        label_position: None,
        color: region_color(0),
        metric: Some(RegionMetric::Height),
    });
    recording.region_analysis.next_region_id = RegionId::new(1);
    let mut app = PlotxApp::new();
    app.doc
        .datasets
        .push(Dataset::Electrophysiology(Box::new(recording)));
    app.create_region_table(0);

    let values = |app: &PlotxApp| {
        app.doc.datasets[1].as_table().unwrap().figure().series[0]
            .points
            .iter()
            .map(|point| point[1])
            .collect::<Vec<_>>()
    };
    let channel_a = values(&app);
    app.doc.datasets[0]
        .as_electrophysiology_mut()
        .unwrap()
        .selected_channel = 1;
    app.apply_dataset_edit(0);
    let channel_b = values(&app);
    assert_ne!(channel_a, channel_b);

    app.doc.datasets[0]
        .as_electrophysiology_mut()
        .unwrap()
        .selected_sweeps[0] = false;
    app.apply_dataset_edit(0);
    assert_eq!(values(&app).len(), 1);

    let raw = values(&app);
    let recording = app.doc.datasets[0].as_electrophysiology_mut().unwrap();
    recording.processing.gaussian_lowpass_enabled = true;
    recording.processing.cutoff_hz = 1.0;
    app.apply_dataset_edit(0);
    assert_ne!(values(&app), raw);

    app.doc.datasets[0]
        .as_electrophysiology_mut()
        .unwrap()
        .selected_sweeps
        .fill(false);
    app.apply_dataset_edit(0);
    assert!(values(&app).is_empty());
}

fn electrophysiology_region_app(samples: Vec<f64>, metric: RegionMetric) -> PlotxApp {
    let sample_count = samples.len();
    let data = ElectrophysiologyData {
        abf_version: "test".to_owned(),
        sample_rate_hz: 10.0,
        channels: vec![RecordedChannel {
            name: "A".to_owned(),
            unit: ElectricalUnit::from_symbol("pA"),
        }],
        sweeps: vec![Sweep {
            start_time_s: 0.0,
            channels: vec![samples],
            commands: Vec::new(),
        }],
        protocol: None,
        source: "synthetic.abf".to_owned(),
        import_warnings: Vec::new(),
    };
    let mut recording = ElectrophysiologyDataset::load(data);
    recording.processing.gaussian_lowpass_enabled = false;
    recording.region_analysis.default_metric = metric;
    recording.region_analysis.regions.push(Region {
        id: RegionId::new(0),
        lo: 0.0,
        hi: sample_count as f64 / 10.0,
        name: "window".to_owned(),
        label_position: None,
        color: region_color(0),
        metric: None,
    });
    recording.region_analysis.next_region_id = RegionId::new(1);
    let mut app = PlotxApp::new();
    app.doc
        .datasets
        .push(Dataset::Electrophysiology(Box::new(recording)));
    app
}

fn first_region_table_value(app: &PlotxApp) -> f64 {
    app.doc.datasets[1].as_table().unwrap().figure().series[0].points[0][1]
}

#[test]
fn electrophysiology_region_metrics_reject_non_finite_windows() {
    for samples in [
        vec![f64::NAN, f64::INFINITY, f64::NEG_INFINITY],
        vec![1.0, f64::NAN, 3.0],
    ] {
        let mut app = electrophysiology_region_app(samples, RegionMetric::Area);

        app.create_region_table(0);

        assert_eq!(app.doc.datasets.len(), 1);
        assert!(app.session.status.contains("non-finite samples"));
    }
}

#[test]
fn changing_default_region_metric_dirties_and_resynchronizes_the_document() {
    let mut app = electrophysiology_region_app(vec![-1.0, -2.0, -3.0, -4.0], RegionMetric::Height);
    app.create_region_table(0);
    let height = first_region_table_value(&app);
    app.doc.dirty = false;
    let generation = app.doc.edit_generation;

    app.set_region_default_metric(0, RegionMetric::Area);

    assert!(app.doc.dirty);
    assert_eq!(app.doc.edit_generation, generation + 1);
    assert_eq!(
        app.doc.datasets[0]
            .region_analysis()
            .unwrap()
            .default_metric,
        RegionMetric::Area
    );
    assert_ne!(first_region_table_value(&app), height);
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
