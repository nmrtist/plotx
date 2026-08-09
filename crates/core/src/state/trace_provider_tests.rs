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

fn multichannel_recording(
    response_units: [&str; 2],
    selected_channel: usize,
    source: &str,
) -> Dataset {
    let mut recording = ElectrophysiologyDataset::load(plotx_io::ElectrophysiologyData {
        abf_version: "2.9".into(),
        sample_rate_hz: 10_000.0,
        channels: response_units
            .into_iter()
            .enumerate()
            .map(|(index, unit)| plotx_io::RecordedChannel {
                name: format!("Channel {}", index + 1),
                unit: plotx_io::ElectricalUnit::from_symbol(unit),
            })
            .collect(),
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
        source: source.into(),
        import_warnings: Vec::new(),
    });
    recording.selected_channel = selected_channel;
    Dataset::Electrophysiology(Box::new(recording))
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
    app.session.ui.selection_scope = SelectionScope::DataList;
    app.session.ui.selection_anchors.dataset = Some(app.doc.datasets[0].resource_id());
    app.session.ui.selection_anchors.dataset_lead = Some(app.doc.datasets[1].resource_id());
    app.session.ui.selection_anchors.layer = Some(ObjectId::new(91));
    app.session.ui.selection_anchors.layer_lead = Some(ObjectId::new(92));
    assert_eq!(app.stackable_selection(), Some(vec![0, 1]));
    app.stack_selected_data();

    let composer = app.session.ui.trace_composer.as_mut().unwrap();
    assert_eq!(composer.selected_count(), 4);
    assert_eq!(composer.items[0].dataset_name, "pA.abf (1)");
    assert_eq!(composer.items[2].dataset_name, "pA.abf (2)");
    composer.set_all(false);
    composer.items[1].selected = true;
    composer.items[3].selected = true;
    let expected_sources = vec![
        composer.items[1].series.source,
        composer.items[3].series.source,
    ];
    app.create_trace_composer_stack();

    let canvas = app.doc.canvases.len() - 1;
    let object = app.doc.canvases[canvas].objects[0].id;
    let before = app.doc.canvases[canvas].objects[0]
        .plot()
        .unwrap()
        .binding
        .clone();
    assert_eq!(before.series.len(), 2);
    assert_eq!(
        before
            .series
            .iter()
            .map(|series| series.source)
            .collect::<Vec<_>>(),
        expected_sources
    );
    assert!(
        before
            .series
            .iter()
            .all(|series| app.series_label(series) == "Sweep 2")
    );
    assert_ne!(before.series[0].id, before.series[1].id);
    assert!(
        app.doc.canvases[canvas].objects[0]
            .plot()
            .unwrap()
            .next_series_id
            .get()
            > before
                .series
                .iter()
                .map(|series| series.id.get())
                .max()
                .unwrap()
    );
    assert_ne!(
        before.series[0].primary_color(),
        before.series[1].primary_color()
    );
    assert!(
        app.doc.canvases[canvas].objects[0]
            .plot()
            .unwrap()
            .display_owner
            .is_none()
    );
    assert_eq!(app.session.active_canvas, Some(canvas));
    assert_eq!(app.session.ui.selection, Selection::single(object));
    assert_eq!(app.doc.canvases[canvas].selected_object, Some(object));
    let frame = BoardFrameId::Page(app.doc.canvases[canvas].resource_id);
    assert_eq!(app.session.ui.frame_selection, vec![frame]);
    assert_eq!(app.session.board_reveal, Some(frame));
    assert_eq!(
        app.session.ui.selection_scope,
        SelectionScope::CanvasObjects
    );
    assert!(app.session.ui.selection_anchors.dataset.is_none());
    assert!(app.session.ui.selection_anchors.dataset_lead.is_none());
    assert!(app.session.ui.selection_anchors.layer.is_none());
    assert!(app.session.ui.selection_anchors.layer_lead.is_none());
    assert_eq!(
        app.session.ui.requested_inspector_section.as_deref(),
        Some("inspector.data")
    );
    assert_eq!(
        app.doc.canvases[canvas].objects[0]
            .plot()
            .unwrap()
            .figure()
            .series
            .len(),
        2
    );
    app.session.ui.selection = Selection::None;
    app.sync_selection_to_active_canvas();
    assert_eq!(app.session.ui.selection, Selection::single(object));

    app.undo();
    assert_eq!(app.doc.canvases.len(), canvas);
    app.redo();
    app.sync_selection_to_active_canvas();
    assert_eq!(app.session.ui.selection, Selection::single(object));
    assert_eq!(
        app.doc.canvases[canvas].objects[0].plot().unwrap().binding,
        before
    );

    let path = std::env::temp_dir().join(format!(
        "plotx-selected-trace-item-{}.plotx",
        uuid::Uuid::new_v4()
    ));
    crate::project::save_project(&app, &path, false).unwrap();
    let loaded = crate::project::load_project(&path).unwrap();
    let _ = std::fs::remove_file(path);
    let loaded_plot = loaded.doc.canvases[canvas].objects[0].plot().unwrap();
    assert_eq!(loaded_plot.binding, before);
    assert_eq!(loaded_plot.figure().series.len(), 2);
}

#[test]
fn trace_composer_uses_each_recordings_selected_channel() {
    let mut app = PlotxApp::new();
    app.doc
        .datasets
        .push(multichannel_recording(["mV", "pA"], 1, "before.abf"));
    app.doc
        .datasets
        .push(multichannel_recording(["mV", "pA"], 1, "after.abf"));
    let expected_fields = app
        .doc
        .datasets
        .iter()
        .map(|dataset| dataset.active_trace_collection_field().unwrap())
        .collect::<Vec<_>>();
    app.focus_datasets(&[0, 1], None);
    app.stack_selected_data();

    let composer = app.session.ui.trace_composer.as_ref().unwrap();
    assert_eq!(composer.items.len(), 4);
    assert!(
        composer.items[..2]
            .iter()
            .all(|item| item.dataset_name == "before.abf")
    );
    assert!(
        composer.items[2..]
            .iter()
            .all(|item| item.dataset_name == "after.abf")
    );
    let query = "before.abf";
    assert_eq!(composer.visible_count(query), 2);
    assert!(
        composer.items[..2]
            .iter()
            .all(|item| item.series.source.field == expected_fields[0])
    );
    assert!(
        composer.items[2..]
            .iter()
            .all(|item| item.series.source.field == expected_fields[1])
    );
}

#[test]
fn pseudo_map_display_composes_the_stable_stack_collection() {
    let mut app = PlotxApp::new();
    for _ in 0..2 {
        let mut dataset = Nmr2DDataset::load(pseudo_data());
        dataset.display = PseudoDisplay::DosyMap;
        app.doc.datasets.push(Dataset::Nmr2D(Box::new(dataset)));
    }
    let stack_fields = app
        .doc
        .datasets
        .iter()
        .map(|dataset| dataset.field_catalog().id_for_key("nmr.stack").unwrap())
        .collect::<Vec<_>>();
    app.focus_datasets(&[0, 1], None);
    assert_eq!(app.stackable_selection(), Some(vec![0, 1]));
    app.stack_selected_data();

    let composer = app.session.ui.trace_composer.as_ref().unwrap();
    assert_eq!(composer.items.len(), 8);
    assert!(
        composer.items[..4]
            .iter()
            .all(|item| item.series.source.field == stack_fields[0])
    );
    assert!(
        composer.items[4..]
            .iter()
            .all(|item| item.series.source.field == stack_fields[1])
    );
}

#[test]
fn cancelling_trace_composer_leaves_document_and_selection_unchanged() {
    let mut app = PlotxApp::new();
    app.doc.datasets.push(recording("pA", Some("mV")));
    app.doc.datasets.push(recording("pA", Some("mV")));
    app.focus_datasets(&[0, 1], None);
    let before = (
        app.doc.canvases.len(),
        app.doc.dirty,
        app.doc.edit_generation,
        app.doc.project_revision.clone(),
        app.session.undo_stack.len(),
        app.session.ui.data_selection.clone(),
    );
    app.stack_selected_data();
    assert_eq!(
        app.session
            .ui
            .trace_composer
            .as_ref()
            .unwrap()
            .selected_count(),
        4
    );
    app.session
        .ui
        .trace_composer
        .as_mut()
        .unwrap()
        .set_all(false);
    app.create_trace_composer_stack();
    assert!(app.session.ui.trace_composer.is_some());
    assert!(app.session.status.contains("Select at least one trace"));
    app.cancel_trace_composer();
    assert!(app.session.ui.trace_composer.is_none());
    assert_eq!(
        before,
        (
            app.doc.canvases.len(),
            app.doc.dirty,
            app.doc.edit_generation,
            app.doc.project_revision.clone(),
            app.session.undo_stack.len(),
            app.session.ui.data_selection.clone(),
        )
    );
}

#[test]
fn stale_trace_composer_source_fails_atomically_and_keeps_the_draft() {
    let mut app = PlotxApp::new();
    app.doc.datasets.push(recording("pA", Some("mV")));
    app.doc.datasets.push(recording("pA", Some("mV")));
    app.focus_datasets(&[0, 1], None);
    app.stack_selected_data();
    app.session.ui.trace_composer.as_mut().unwrap().items[0]
        .series
        .source
        .field = FieldId::new(u64::MAX);
    let before = (
        app.doc.canvases.len(),
        app.doc.dirty,
        app.doc.edit_generation,
        app.doc.project_revision.clone(),
        app.session.undo_stack.len(),
        app.session.ui.data_selection.clone(),
    );
    app.create_trace_composer_stack();
    assert!(app.session.ui.trace_composer.is_some());
    assert!(app.session.status.contains("no longer available"));
    assert_eq!(
        before,
        (
            app.doc.canvases.len(),
            app.doc.dirty,
            app.doc.edit_generation,
            app.doc.project_revision.clone(),
            app.session.undo_stack.len(),
            app.session.ui.data_selection.clone(),
        )
    );
}

#[test]
fn trace_composer_rejects_incompatible_field_units() {
    let mut app = PlotxApp::new();
    app.doc
        .datasets
        .push(multichannel_recording(["pA", "pA"], 0, "current.abf"));
    app.doc
        .datasets
        .push(multichannel_recording(["mV", "mV"], 0, "voltage.abf"));
    app.focus_datasets(&[0, 1], None);
    app.stack_selected_data();

    assert!(app.session.ui.trace_composer.is_none());
    assert!(app.doc.canvases.is_empty());
    assert!(app.stackable_selection().is_none());
    assert_eq!(app.session.ui.data_selection, vec![0, 1]);
}

#[test]
fn trace_contract_uses_capabilities_concrete_encoding_and_units_not_domain_policy() {
    let electrophysiology = recording("pA", Some("mV"));
    let electrophysiology_field = electrophysiology.active_trace_collection_field().unwrap();
    let electrophysiology_binding =
        SeriesBinding::from_field_all(&electrophysiology, electrophysiology_field)
            .into_iter()
            .next()
            .unwrap();
    let mut electrophysiology_descriptor = electrophysiology
        .field_descriptor(electrophysiology_field)
        .unwrap();
    electrophysiology_descriptor.metadata = FieldMetadata::default();

    let pseudo = Dataset::Nmr2D(Box::new(Nmr2DDataset::load(pseudo_data())));
    let pseudo_field = pseudo.active_trace_collection_field().unwrap();
    let pseudo_binding = SeriesBinding::from_field_all(&pseudo, pseudo_field)
        .into_iter()
        .next()
        .unwrap();
    let mut pseudo_descriptor = pseudo.field_descriptor(pseudo_field).unwrap();
    pseudo_descriptor.units = electrophysiology_descriptor.units.clone();
    pseudo_descriptor
        .metadata
        .0
        .insert("recommended_encoding".into(), "contour".into());

    let mut units = None;
    assert!(super::trace_composer::trace_field_contract_matches(
        &electrophysiology_descriptor,
        &electrophysiology_binding.encoding,
        &mut units,
    ));
    assert!(super::trace_composer::trace_field_contract_matches(
        &pseudo_descriptor,
        &pseudo_binding.encoding,
        &mut units,
    ));

    let mut app = PlotxApp::new();
    app.doc.datasets.push(electrophysiology);
    app.doc.datasets.push(pseudo);
    let binding = DataBinding {
        series: vec![electrophysiology_binding, pseudo_binding],
    };
    assert!(
        app.series_stackable(&binding),
        "item-addressed line applicability must not use enclosing domains"
    );
}

#[test]
fn trace_stack_forces_offset_even_when_the_primary_domain_is_field_stacked() {
    let mut true_2d = pseudo_data();
    true_2d.pseudo_axis = None;
    let mut app = PlotxApp::new();
    app.doc
        .datasets
        .push(Dataset::Nmr2D(Box::new(Nmr2DDataset::load(true_2d))));
    app.doc.datasets.push(recording("pA", Some("mV")));
    assert_eq!(
        app.doc.datasets[0].domain().stack_kind(),
        Some(StackKind::Field)
    );
    let field = app.doc.datasets[1].active_trace_collection_field().unwrap();
    let descriptor = app.doc.datasets[1].field_descriptor(field).unwrap();
    assert!(descriptor.capabilities.supports(&[
        crate::automation::CAP_FIELD_TRACE_COLLECTION,
        crate::automation::CAP_FIELD_CURVE_1D,
    ]));
    let series = SeriesBinding::from_field_all(&app.doc.datasets[1], field);
    assert!(
        series
            .iter()
            .all(|series| matches!(series.encoding, plotx_figure::SeriesEncoding::Line(_)))
    );

    assert!(app.insert_stack_canvas(&[0, 1], series, true));
    let plot = app.doc.canvases[0].objects[0].plot().unwrap();
    assert_eq!(plot.stack.mode, StackMode::Offset);
    assert_eq!(plot.figure().series.len(), 2);
    assert!(
        plot.figure().series[1].points[0][1] > 3.0,
        "the second raw trace starts at 3.0 and must receive a vertical offset"
    );
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

#[path = "trace_alignment_tests.rs"]
mod trace_alignment_tests;
