use super::*;

fn pseudo_data() -> plotx_io::NmrData2D {
    let dimension = plotx_io::Dim {
        spectral_width_hz: 4_000.0,
        observe_freq_mhz: 400.0,
        carrier_ppm: 0.0,
        nucleus: "1H".into(),
        group_delay: 0.0,
    };
    plotx_io::NmrData2D {
        data: vec![num_complex::Complex64::new(1.0, 0.5); 16],
        rows: 4,
        cols: 4,
        domain: plotx_io::Domain::Frequency,
        direct: dimension.clone(),
        indirect: dimension,
        quad: plotx_io::QuadMode::Complex,
        indirect_conjugate: false,
        experiment: None,
        pseudo_axis: Some(plotx_io::PseudoAxis {
            name: "Gradient".into(),
            kind: plotx_io::PseudoKind::Gradient,
            values: vec![0.02, 0.04, 0.06, 0.08],
            unit: "mT/m".into(),
            source: plotx_io::AxisSource::EmbeddedList,
        }),
        diffusion: None,
        nus: None,
        source: "stable pseudo".into(),
    }
}

#[test]
fn pseudo_trace_items_keep_identity_and_format_display_units() {
    let mut dataset = Nmr2DDataset::load(pseudo_data());
    let field = dataset.field_catalog.id_for_key("nmr.stack").unwrap();
    let before = dataset
        .field_catalog
        .trace_collection(field)
        .unwrap()
        .items
        .iter()
        .map(|item| item.id)
        .collect::<Vec<_>>();
    assert_eq!(
        dataset.field_catalog.trace_collection(field).unwrap().items[0]
            .automatic_label()
            .as_deref(),
        Some("20 mT/m")
    );
    dataset.rebuild();
    assert_eq!(
        before,
        dataset
            .field_catalog
            .trace_collection(field)
            .unwrap()
            .items
            .iter()
            .map(|item| item.id)
            .collect::<Vec<_>>()
    );
    let object = crate::workflow::build_plot_object(
        &Dataset::Nmr2D(Box::new(dataset)),
        0,
        ObjectFrame::new(0.0, 0.0, 400.0, 300.0),
        ObjectId::new(1),
        "Pseudo".into(),
    );
    assert_eq!(
        object.plot().unwrap().axis_overrides.guide_visibility,
        Some(plotx_figure::GuideVisibility::Hide)
    );
}

fn recording(response_unit: &str, command_unit: Option<&str>) -> Dataset {
    let commands = command_unit
        .map(|unit| {
            vec![plotx_io::CommandWaveform {
                name: "Command".into(),
                unit: plotx_io::ElectricalUnit::from_symbol(unit),
                holding_level: if unit == "mV" { -80.0 } else { 0.0 },
                samples: if unit == "mV" {
                    vec![-80.0, -60.0]
                } else {
                    vec![0.0, 40.0]
                },
            }]
        })
        .unwrap_or_default();
    Dataset::Electrophysiology(Box::new(ElectrophysiologyDataset::load(
        plotx_io::ElectrophysiologyData {
            abf_version: "2.9".into(),
            sample_rate_hz: 10_000.0,
            channels: vec![plotx_io::RecordedChannel {
                name: "Response".into(),
                unit: plotx_io::ElectricalUnit::from_symbol(response_unit),
            }],
            sweeps: vec![
                plotx_io::Sweep {
                    start_time_s: 0.0,
                    channels: vec![vec![1.0, 2.0]],
                    commands,
                },
                plotx_io::Sweep {
                    start_time_s: 1.0,
                    channels: vec![vec![3.0]],
                    commands: Vec::new(),
                },
            ],
            protocol: None,
            source: format!("{response_unit}.abf"),
            import_warnings: Vec::new(),
        },
    )))
}

#[test]
fn electrophysiology_trace_labels_prefer_dac_and_fall_back_to_sweep() {
    let dataset = recording("pA", Some("mV"));
    let field = dataset.default_field_id().unwrap();
    let collection = dataset.trace_collection(field).unwrap();
    assert_eq!(
        collection.items[0].automatic_label().as_deref(),
        Some("-60 mV")
    );
    assert_eq!(
        collection.items[1].automatic_label().as_deref(),
        Some("Sweep 2")
    );
    let binding = DataBinding::single(&dataset);
    assert_eq!(binding.series.len(), 2);
    assert!(binding.series.iter().all(|series| {
        dataset
            .trace_item_figure(field, series.source.item.unwrap())
            .unwrap()
            .series
            .len()
            == 1
    }));
    let object = crate::workflow::build_plot_object(
        &dataset,
        0,
        ObjectFrame::new(0.0, 0.0, 400.0, 300.0),
        ObjectId::new(1),
        "Recording".into(),
    );
    assert_eq!(
        object.plot().unwrap().axis_overrides.guide_visibility,
        Some(plotx_figure::GuideVisibility::Hide)
    );
    let mut app = PlotxApp::new();
    app.doc.datasets.push(dataset);
    let mut binding = binding;
    binding.series[0].visible = false;
    let figure = app.build_binding_figure(
        &binding,
        &ChartSpec::default_for(DataDomain::Electrophysiology),
        &StackSpec::default(),
        [120.0, 80.0],
    );
    assert_eq!(
        figure.series.len(),
        1,
        "hiding one binding must hide only that sweep"
    );
    let current = recording("mV", Some("pA"));
    let current_field = current.default_field_id().unwrap();
    assert_eq!(
        current.trace_collection(current_field).unwrap().items[0]
            .automatic_label()
            .as_deref(),
        Some("40 pA")
    );
}

#[test]
fn initial_multichannel_recording_figure_uses_only_the_selected_channel() {
    let data = plotx_io::ElectrophysiologyData {
        abf_version: "2.9".into(),
        sample_rate_hz: 10_000.0,
        channels: vec![
            plotx_io::RecordedChannel {
                name: "A".into(),
                unit: plotx_io::ElectricalUnit::from_symbol("pA"),
            },
            plotx_io::RecordedChannel {
                name: "B".into(),
                unit: plotx_io::ElectricalUnit::from_symbol("pA"),
            },
        ],
        sweeps: vec![
            plotx_io::Sweep {
                start_time_s: 0.0,
                channels: vec![vec![1.0, 2.0], vec![10.0, 20.0]],
                commands: Vec::new(),
            },
            plotx_io::Sweep {
                start_time_s: 1.0,
                channels: vec![vec![3.0, 4.0], vec![30.0, 40.0]],
                commands: Vec::new(),
            },
        ],
        protocol: None,
        source: "multichannel.abf".into(),
        import_warnings: Vec::new(),
    };
    let mut recording = ElectrophysiologyDataset::load(data);
    recording.selected_channel = 1;
    let object = crate::workflow::build_plot_object(
        &Dataset::Electrophysiology(Box::new(recording)),
        0,
        ObjectFrame::new(0.0, 0.0, 400.0, 300.0),
        ObjectId::new(1),
        "Recording".into(),
    );
    let plot = object.plot().unwrap();
    assert_eq!(plot.binding.series.len(), 4);
    assert_eq!(plot.figure().series.len(), 2);
    assert_eq!(plot.figure().y.label, "B (pA)");
    assert!(plot.figure().series[0].points[0][1] > 5.0);
}

#[test]
fn stacked_trace_collections_expand_all_items_and_round_trip_visibility() {
    let mut app = PlotxApp::new();
    app.doc.datasets.push(recording("pA", Some("mV")));
    app.doc.datasets.push(recording("pA", Some("mV")));
    app.focus_datasets(&[0, 1], None);
    app.stack_selected_data();

    let canvas = app.doc.canvases.len() - 1;
    let object = app.doc.canvases[canvas].objects[0].id;
    let before = app.doc.canvases[canvas].objects[0]
        .plot()
        .unwrap()
        .binding
        .clone();
    assert_eq!(before.series.len(), 4);
    assert_eq!(app.series_label(&before.series[0]), "-60 mV");
    assert_eq!(app.series_label(&before.series[1]), "Sweep 2");
    assert_eq!(app.series_label(&before.series[2]), "-60 mV");
    assert_eq!(app.series_label(&before.series[3]), "Sweep 2");
    assert_eq!(
        app.doc.canvases[canvas].objects[0]
            .plot()
            .unwrap()
            .figure()
            .series
            .len(),
        4
    );

    let options = app.stack_candidate_series_options(&before, 1);
    assert_eq!(options.len(), 2);
    assert_eq!(app.series_label(&options[1]), "Sweep 2");
    assert_eq!(app.series_item_options(&before.series[2]).len(), 2);
    let mut after = before.clone();
    for series in &mut after.series {
        series.visible = app.series_label(series) == "Sweep 2";
    }
    app.execute_action(crate::actions::Action::set_data_binding(
        canvas,
        object,
        before.clone(),
        after.clone(),
    ));

    let plot = app.doc.canvases[canvas].objects[0].plot().unwrap();
    assert_eq!(plot.figure().series.len(), 2);
    assert!(
        plot.figure()
            .series
            .iter()
            .all(|series| series.name == "Sweep 2")
    );

    app.undo();
    assert_eq!(
        app.doc.canvases[canvas].objects[0].plot().unwrap().binding,
        before
    );
    app.redo();
    assert_eq!(
        app.doc.canvases[canvas].objects[0].plot().unwrap().binding,
        after
    );

    let path = std::env::temp_dir().join(format!(
        "plotx-selected-trace-item-{}.plotx",
        uuid::Uuid::new_v4()
    ));
    crate::project::save_project(&app, &path, false).unwrap();
    let loaded = crate::project::load_project(&path).unwrap();
    let _ = std::fs::remove_file(path);
    let loaded_plot = loaded.doc.canvases[canvas].objects[0].plot().unwrap();
    assert_eq!(loaded_plot.binding, after);
    assert_eq!(loaded_plot.figure().series.len(), 2);
}

#[test]
fn fixed_prepulse_is_skipped_for_the_varying_abf_test_pulse() {
    let levels = [-20.0, 0.0, 20.0];
    let data = plotx_io::ElectrophysiologyData {
        abf_version: "2.9".into(),
        sample_rate_hz: 10_000.0,
        channels: vec![plotx_io::RecordedChannel {
            name: "Response".into(),
            unit: plotx_io::ElectricalUnit::from_symbol("pA"),
        }],
        sweeps: levels
            .iter()
            .enumerate()
            .map(|(index, level)| plotx_io::Sweep {
                start_time_s: index as f64,
                channels: vec![vec![0.0; 6]],
                commands: vec![plotx_io::CommandWaveform {
                    name: "Command".into(),
                    unit: plotx_io::ElectricalUnit::from_symbol("mV"),
                    holding_level: -80.0,
                    samples: vec![-80.0, -40.0, -40.0, *level, *level, -80.0],
                }],
            })
            .collect(),
        protocol: None,
        source: "prepulse.abf".into(),
        import_warnings: Vec::new(),
    };
    let recording = ElectrophysiologyDataset::load(data);
    let labels = recording
        .trace_items()
        .iter()
        .map(|item| item.automatic_label().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(labels, ["-20 mV", "0 mV", "20 mV"]);
    let (stimulus, quantity) = recording.stimulus_values().unwrap();
    assert_eq!(stimulus, levels);
    assert_eq!(quantity, plotx_io::ElectricalQuantity::Voltage);
}

#[test]
fn single_and_multi_item_materialization_apply_identical_line_style() {
    let dataset = Dataset::Nmr2D(Box::new(Nmr2DDataset::load(pseudo_data())));
    let field = dataset.field_catalog().id_for_key("nmr.stack").unwrap();
    let mut bindings = SeriesBinding::from_field_all(&dataset, field);
    for binding in bindings.iter_mut().take(2) {
        binding.label = Some("styled".into());
        if let plotx_figure::SeriesEncoding::Line(line) = &mut binding.encoding {
            line.scale = 2.0;
            line.width = plotx_figure::PositiveFiniteF32::new(3.25).unwrap();
            line.color = plotx_figure::ColorSource::Explicit(plotx_figure::Color::rgb(7, 19, 31));
        }
    }
    let mut app = PlotxApp::new();
    app.doc.datasets.push(dataset);
    let chart = ChartSpec::default_for(DataDomain::Nmr2d);
    let single = app.build_binding_figure(
        &DataBinding {
            series: vec![bindings[0].clone()],
        },
        &chart,
        &StackSpec::default(),
        [120.0, 80.0],
    );
    let multi = app.build_binding_figure(
        &DataBinding {
            series: bindings[..2].to_vec(),
        },
        &chart,
        &StackSpec::default(),
        [120.0, 80.0],
    );
    for figure in [&single, &multi] {
        assert!(figure.series.iter().all(|series| {
            series.name == "styled"
                && series.width == 3.25
                && series.color == plotx_figure::Color::rgb(7, 19, 31)
        }));
    }
    assert_ne!(
        SeriesBinding::from_field_all(&app.doc.datasets[0], field)[0].primary_color(),
        SeriesBinding::from_field_all(&app.doc.datasets[0], field)[1].primary_color(),
        "initial palette colors belong to the bindings"
    );
}

#[test]
fn confirming_a_stimulus_template_refreshes_trace_labels() {
    let mut dataset = recording("pA", None);
    let field = dataset.default_field_id().unwrap();
    let recording = dataset.as_electrophysiology_mut().unwrap();
    assert_eq!(
        recording.trace_items()[0].automatic_label().as_deref(),
        Some("Sweep 1")
    );
    recording.stimulus = Some(StimulusDefinition {
        protocol: StimulusProtocol::VoltageStep {
            holding_mv: -80.0,
            start_mv: -60.0,
            step_mv: 10.0,
            start_s: 0.1,
            end_s: 0.2,
        },
        source: StimulusSource::User,
        confirmed: true,
    });
    recording.refresh_trace_collections();
    assert_eq!(
        recording
            .field_catalog
            .trace_collection(field)
            .unwrap()
            .items[0]
            .automatic_label()
            .as_deref(),
        Some("-60 mV")
    );
}

#[test]
fn electrophysiology_trace_figures_drop_non_finite_points() {
    let mut dataset = recording("pA", None);
    let recording = dataset.as_electrophysiology_mut().unwrap();
    recording.processing.gaussian_lowpass_enabled = false;
    recording.data.sweeps[0].channels[0] = vec![1.0, f64::NAN, f64::INFINITY, 2.0];
    let field = dataset.default_field_id().unwrap();
    let item = dataset.trace_collection(field).unwrap().items[0].id;
    let figure = dataset.trace_item_figure(field, item).unwrap();
    assert_eq!(figure.series[0].points.len(), 2);
    assert!(
        figure.series[0]
            .points
            .iter()
            .flatten()
            .all(|value| value.is_finite())
    );
}
