use egui::{ComboBox, DragValue, Ui};
use plotx_analysis::electrophysiology::PeakMode;
use plotx_core::state::{
    Dataset, DatasetLineage, DerivationKind, PlotxApp, StimulusProtocol, StimulusSource,
    build_iv_table, build_window_statistics_table,
};

pub(super) fn electrophysiology_group(app: &mut PlotxApp, di: usize, ui: &mut Ui) -> bool {
    let selected_region = app.doc.datasets.get(di).and_then(|dataset| {
        app.session
            .ui
            .selected_region
            .and_then(|selection| selection.in_dataset(dataset.resource_id()))
    });
    let Some(recording) = app
        .doc
        .datasets
        .get_mut(di)
        .and_then(Dataset::as_electrophysiology_mut)
    else {
        return false;
    };
    let mut dirty = false;
    let mut invocation_changed = false;

    ui.label(crate::typography::headline("Recording"));
    ui.label(format!(
        "ABF {} · {:.3} kHz · {} sweeps",
        recording.data.abf_version,
        recording.data.sample_rate_hz / 1_000.0,
        recording.data.sweeps.len()
    ));
    if let Some(protocol) = &recording.data.protocol {
        ui.label(format!("Protocol: {protocol}"));
    }
    if !recording.data.import_warnings.is_empty() {
        for warning in &recording.data.import_warnings {
            ui.colored_label(ui.visuals().warn_fg_color, warning);
        }
    }
    let mut selected_channel = recording.selected_channel;
    ComboBox::from_label("Recorded channel")
        .selected_text(
            recording
                .data
                .channels
                .get(recording.selected_channel)
                .map(|c| format!("{} ({})", c.name, c.unit.symbol))
                .unwrap_or_default(),
        )
        .show_ui(ui, |ui| {
            for (index, channel) in recording.data.channels.iter().enumerate() {
                dirty |= ui
                    .selectable_value(
                        &mut selected_channel,
                        index,
                        format!("{} ({})", channel.name, channel.unit.symbol),
                    )
                    .changed();
            }
        });
    recording.set_selected_channel(selected_channel);

    ui.separator();
    ui.label(crate::typography::headline("Sweeps"));
    let item_ids = recording
        .trace_items()
        .iter()
        .map(|item| item.id)
        .collect::<Vec<_>>();
    let mut selected_ids = recording
        .invocation
        .analysis_selection
        .clone()
        .unwrap_or_else(|| item_ids.clone());
    ui.horizontal(|ui| {
        if ui.button("Select all").clicked() {
            selected_ids.clone_from(&item_ids);
            invocation_changed = true;
        }
        if ui.button("Clear").clicked() {
            selected_ids.clear();
            invocation_changed = true;
        }
    });
    ui.horizontal_wrapped(|ui| {
        for (index, item) in item_ids.iter().enumerate() {
            let mut selected = selected_ids.contains(item);
            if ui
                .checkbox(&mut selected, (index + 1).to_string())
                .changed()
            {
                if selected {
                    selected_ids.push(*item);
                } else {
                    selected_ids.retain(|id| id != item);
                }
                invocation_changed = true;
            }
        }
    });
    recording.invocation.analysis_selection = Some(selected_ids);

    ui.separator();
    ui.label(crate::typography::headline("Processing"));
    dirty |= ui
        .checkbox(
            &mut recording.processing.gaussian_lowpass_enabled,
            "Gaussian low-pass",
        )
        .changed();
    ui.add_enabled_ui(recording.processing.gaussian_lowpass_enabled, |ui| {
        ui.horizontal(|ui| {
            ui.label("Cutoff (Hz)");
            // The filter rejects a cutoff at or above Nyquist. Back off by a
            // relative margin: subtracting f64::EPSILON from a rate of a few kHz
            // rounds straight back to Nyquist and the filter would reject it.
            let max_cutoff = recording.data.sample_rate_hz / 2.0 * (1.0 - 1e-6);
            dirty |= ui
                .add(DragValue::new(&mut recording.processing.cutoff_hz).range(0.1..=max_cutoff))
                .changed();
        });
    });

    ui.separator();
    ui.label(crate::typography::headline("Metadata / quality control"));
    dirty |= ui
        .text_edit_singleline(&mut recording.metadata.cell_id)
        .changed();
    ui.weak("Cell ID");
    dirty |= ui
        .text_edit_singleline(&mut recording.metadata.experiment)
        .changed();
    ui.weak("Experiment");
    dirty |= ui
        .text_edit_singleline(&mut recording.metadata.label)
        .changed();
    ui.weak("Label");
    optional_number(
        ui,
        "Seal resistance (GΩ)",
        &mut recording.metadata.seal_resistance_gohm,
        &mut dirty,
    );
    optional_number(
        ui,
        "Leak current (pA)",
        &mut recording.metadata.leak_current_pa,
        &mut dirty,
    );
    optional_number(
        ui,
        "Capacitance (pF)",
        &mut recording.metadata.capacitance_pf,
        &mut dirty,
    );
    optional_number(
        ui,
        "Series resistance (MΩ)",
        &mut recording.metadata.series_resistance_mohm,
        &mut dirty,
    );

    ui.separator();
    ui.label(crate::typography::headline("Region statistics"));
    let region = selected_region
        .and_then(|id| {
            recording
                .region_analysis
                .regions
                .iter()
                .find(|region| region.id == id)
        })
        .or_else(|| recording.region_analysis.regions.first());
    if let Some(region) = region {
        ui.label(format!(
            "Window: {:.4}–{:.4} s",
            region.lo_min(),
            region.hi_max()
        ));
    } else {
        ui.weak("Draw and select a region on the trace to choose the analysis window.");
    }
    ComboBox::from_label("Peak mode")
        .selected_text(format!("{:?}", recording.peak_mode))
        .show_ui(ui, |ui| {
            dirty |= ui
                .selectable_value(&mut recording.peak_mode, PeakMode::Negative, "Negative")
                .changed();
            dirty |= ui
                .selectable_value(&mut recording.peak_mode, PeakMode::Positive, "Positive")
                .changed();
            dirty |= ui
                .selectable_value(&mut recording.peak_mode, PeakMode::Absolute, "Absolute")
                .changed();
        });

    ui.separator();
    ui.label(crate::typography::headline("Stimulus / IV"));
    if let Some(stimulus) = &mut recording.stimulus {
        ui.label(format!("Source: {:?}", stimulus.source));
        if stimulus.source != StimulusSource::Abf {
            dirty |= ui
                .checkbox(&mut stimulus.confirmed, "I confirm this stimulus template")
                .changed();
        }
        edit_stimulus(ui, &mut stimulus.protocol, &mut dirty);
    } else {
        ui.colored_label(ui.visuals().warn_fg_color, "No stimulus is available. IV calculation is disabled until a template is supplied and confirmed.");
    }

    let snapshot = recording.clone();
    let analysis_window = region.map(|region| plotx_analysis::electrophysiology::TimeWindow {
        start_s: region.lo_min(),
        end_s: region.hi_max(),
    });
    let mut create = None;
    ui.horizontal(|ui| {
        if ui
            .add_enabled(
                analysis_window.is_some(),
                egui::Button::new("Create statistics table"),
            )
            .on_disabled_hover_text("Draw a region before creating a statistics table.")
            .clicked()
        {
            create = Some(
                build_window_statistics_table(
                    &snapshot,
                    snapshot.selected_channel,
                    analysis_window.expect("the button is enabled only with a region"),
                    snapshot.peak_mode,
                )
                .map(|table| {
                    (
                        table,
                        "Window statistics",
                        DerivationKind::WindowStatisticsTable,
                    )
                }),
            );
        }
        if ui
            .add_enabled(
                analysis_window.is_some(),
                egui::Button::new("Create IV table"),
            )
            .on_disabled_hover_text("Draw a region before creating an IV table.")
            .clicked()
        {
            create = Some(
                build_iv_table(
                    &snapshot,
                    snapshot.selected_channel,
                    analysis_window.expect("the button is enabled only with a region"),
                    snapshot.peak_mode,
                )
                .map(|table| (table, "IV analysis", DerivationKind::IvTable)),
            );
        }
    });
    if let Some(result) = create {
        match result {
            Ok((table, name, kind)) => {
                let table_index = app.insert_typed_table_dataset(table, name.to_owned());
                let source_id = app.doc.datasets[di].resource_id();
                app.doc.datasets[table_index]
                    .set_lineage(Some(DatasetLineage::new(kind, [source_id])));
            }
            Err(error) => app.session.status = error.to_string(),
        }
    }
    if invocation_changed {
        app.apply_electrophysiology_invocation_edit(di);
    }
    dirty
}

fn optional_number(ui: &mut Ui, label: &str, value: &mut Option<f64>, dirty: &mut bool) {
    ui.horizontal(|ui| {
        ui.label(label);
        let mut present = value.is_some();
        if ui.checkbox(&mut present, "").changed() {
            *value = present.then_some(0.0);
            *dirty = true;
        }
        if let Some(number) = value {
            *dirty |= ui.add(DragValue::new(number)).changed();
        }
    });
}

fn edit_stimulus(ui: &mut Ui, protocol: &mut StimulusProtocol, dirty: &mut bool) {
    match protocol {
        StimulusProtocol::FromAbf => {
            ui.label("Waveform read from ABF DAC/epoch sections");
        }
        StimulusProtocol::VoltageStep {
            holding_mv,
            start_mv,
            step_mv,
            start_s,
            end_s,
        } => {
            ui.label("Voltage step (mV)");
            for (label, value) in [
                ("Holding", holding_mv),
                ("Start", start_mv),
                ("Step", step_mv),
                ("Start time", start_s),
                ("End time", end_s),
            ] {
                ui.horizontal(|ui| {
                    ui.label(label);
                    *dirty |= ui.add(DragValue::new(value)).changed();
                });
            }
        }
        StimulusProtocol::CurrentStep {
            holding_pa,
            start_pa,
            step_pa,
            start_s,
            end_s,
        } => {
            ui.label("Current step (pA)");
            for (label, value) in [
                ("Holding", holding_pa),
                ("Start", start_pa),
                ("Step", step_pa),
                ("Start time", start_s),
                ("End time", end_s),
            ] {
                ui.horizontal(|ui| {
                    ui.label(label);
                    *dirty |= ui.add(DragValue::new(value)).changed();
                });
            }
        }
        StimulusProtocol::Ramp {
            start,
            end,
            start_s,
            end_s,
            unit,
        } => {
            ui.label(format!("Ramp ({})", unit.symbol));
            for (label, value) in [
                ("Start", start),
                ("End", end),
                ("Start time", start_s),
                ("End time", end_s),
            ] {
                ui.horizontal(|ui| {
                    ui.label(label);
                    *dirty |= ui.add(DragValue::new(value)).changed();
                });
            }
        }
    }
}
