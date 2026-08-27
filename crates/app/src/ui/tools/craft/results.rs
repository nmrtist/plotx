use egui::Ui;
use plotx_core::state::{
    CraftAnalysisIntent, CraftComponentSort, CraftResultTab, CraftTaskPage, PlotxApp,
    StoredCraftRun,
};
use plotx_processing::craft::{CraftAmplitudeReport, CraftReportDefinition};
use plotx_processing::craft::{CraftComponent, CraftProfile, CraftRegionId, CraftRunStatus};

use crate::ui::commands::CommandId;

pub(super) fn show(app: &mut PlotxApp, index: usize, ui: &mut Ui) {
    let runs = app.doc.datasets[index]
        .as_nmr()
        .map(|nmr| {
            nmr.craft_runs
                .iter()
                .rev()
                .map(|run| (run.id, run_label(run)))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    if runs.is_empty() {
        ui.weak("Completed CRAFT runs will appear here.");
        return;
    }

    let mut selected = app
        .session
        .ui
        .craft_selected_run
        .or_else(|| runs.first().map(|(id, _)| *id));
    let selected_label = selected
        .and_then(|id| runs.iter().find(|(candidate, _)| *candidate == id))
        .map(|(_, label)| label.clone())
        .unwrap_or_else(|| "Select a run".into());
    egui::ComboBox::from_label("Saved run")
        .selected_text(selected_label)
        .show_ui(ui, |ui| {
            for (id, label) in &runs {
                ui.selectable_value(&mut selected, Some(*id), label);
            }
        });
    if selected != app.session.ui.craft_selected_run {
        app.session.ui.craft_component_region = None;
        app.session.ui.craft_spectrum_channel = Default::default();
        app.session.ui.craft_normalize_groups = false;
    }
    app.session.ui.craft_selected_run = selected;

    let Some((nmr, run)) = selected.and_then(|id| {
        let nmr = app.doc.datasets[index].as_nmr()?;
        Some((nmr.clone(), nmr.craft_run(id)?.clone()))
    }) else {
        ui.weak("Select a completed run to inspect it.");
        return;
    };

    ui.horizontal_wrapped(|ui| {
        if ui.button("Open result canvas").clicked() {
            super::open_result_canvas(app);
        }
        let before = app.session.ui.craft_spectrum_channel;
        super::spectrum::channel_control(&mut app.session.ui.craft_spectrum_channel, ui);
        if before != app.session.ui.craft_spectrum_channel
            && let Some(dataset) = app.session.ui.craft_task_dataset
            && let Err(message) =
                app.set_craft_result_channel(dataset, run.id, app.session.ui.craft_spectrum_channel)
        {
            app.session.ui.craft_spectrum_channel = before;
            app.session.status = message;
        }
        let before_normalize = app.session.ui.craft_normalize_groups;
        ui.checkbox(&mut app.session.ui.craft_normalize_groups, "Normalize rows")
            .on_hover_text(
                "Shape comparison only; normalized rows are not quantitative amplitudes.",
            );
        if before_normalize != app.session.ui.craft_normalize_groups
            && let Some(dataset) = app.session.ui.craft_task_dataset
            && let Err(message) = app.set_craft_group_normalization(
                dataset,
                run.id,
                app.session.ui.craft_normalize_groups,
            )
        {
            app.session.ui.craft_normalize_groups = before_normalize;
            app.session.status = message;
        }
    });
    if app.session.ui.craft_normalize_groups {
        ui.weak("Normalized rows are for shape comparison only; they do not show relative quantitative amplitude.");
    }

    ui.horizontal_wrapped(|ui| {
        if ui.button("Adjust & rerun…").clicked() {
            prepare_rerun(app, &run);
        }
        let rerun = crate::ui::commands::describe(app, CommandId::RunCraft);
        if ui
            .add_enabled(rerun.enabled, egui::Button::new("Rerun unchanged"))
            .on_disabled_hover_text(rerun.disabled_reason.unwrap_or("Command unavailable."))
            .clicked()
        {
            prepare_rerun(app, &run);
            crate::ui::commands::execute_without_clipboard(CommandId::RunCraft, app, ui.ctx());
        }
        if ui.button("Export components…").clicked() {
            export_components(app, index, &run);
        }
    });

    ui.horizontal_wrapped(|ui| {
        ui.selectable_value(
            &mut app.session.ui.craft_result_tab,
            CraftResultTab::Overview,
            "Overview",
        );
        ui.selectable_value(
            &mut app.session.ui.craft_result_tab,
            CraftResultTab::Components,
            "Signals",
        );
        ui.selectable_value(
            &mut app.session.ui.craft_result_tab,
            CraftResultTab::Diagnostics,
            "Diagnostics",
        );
        ui.selectable_value(
            &mut app.session.ui.craft_result_tab,
            CraftResultTab::Reports,
            "Reports",
        );
    });
    ui.separator();

    match app.session.ui.craft_result_tab {
        CraftResultTab::Overview => overview(app, &nmr, &run, ui),
        CraftResultTab::Components => components(app, &nmr, &run, ui),
        CraftResultTab::Diagnostics => diagnostics(app, &nmr, &run, ui),
        CraftResultTab::Reports => reports(app, index, &nmr, &run, ui),
    }
}

fn reports(
    app: &mut PlotxApp,
    index: usize,
    nmr: &plotx_core::state::NmrDataset,
    run: &StoredCraftRun,
    ui: &mut Ui,
) {
    let source = plotx_core::state::ReportSource {
        dataset: nmr.resource_id,
        craft_run: run.id,
    };
    let report_ids = app
        .doc
        .reports_for_source(source)
        .map(|r| r.id)
        .collect::<Vec<_>>();
    let quantitative_ready = run.diagnostics.status == CraftRunStatus::Complete
        && run.diagnostics.stability.passed
        && !run.is_stale_for(&nmr.data, nmr.craft_reference());
    ui.horizontal_wrapped(|ui| {
        if ui
            .add_enabled(quantitative_ready, egui::Button::new("New report"))
            .on_disabled_hover_text(
                "A stable, current CRAFT run is required for a quantitative amplitude report.",
            )
            .clicked()
        {
            let definition = CraftReportDefinition {
                threshold_an: run.provenance.invocation.params.minimum_amplitude_to_noise,
                segment_width_hz: 1.0,
                regions: Vec::new(),
            };
            if let Ok(snapshot) = run.amplitude_report(definition.clone())
                && let (Ok(definition), Ok(snapshot)) = (
                    serde_json::to_value(definition),
                    serde_json::to_value(snapshot),
                )
            {
                let id = app.doc.create_report(plotx_core::state::NewAnalysisReport {
                    name: format!("CRAFT report {}", report_ids.len() + 1),
                    kind: plotx_core::state::ReportKindId::new("craft_amplitude"),
                    source,
                    definition,
                    snapshot,
                    source_fingerprint: run.provenance.input_sha256.clone(),
                    schema_version: 1,
                });
                app.session.ui.craft_selected_report = Some(id);
            }
        }
        if !report_ids.is_empty() {
            let mut selected = app
                .session
                .ui
                .craft_selected_report
                .filter(|id| report_ids.contains(id))
                .or_else(|| report_ids.first().copied());
            egui::ComboBox::from_id_salt(("craft_report", run.id.0))
                .selected_text(
                    selected
                        .map(|id| format!("Report {}", id.0 + 1))
                        .unwrap_or_default(),
                )
                .show_ui(ui, |ui| {
                    for id in &report_ids {
                        ui.selectable_value(
                            &mut selected,
                            Some(*id),
                            format!("Report {}", id.0 + 1),
                        );
                    }
                });
            app.session.ui.craft_selected_report = selected;
            if let Some(id) = selected {
                if ui.button("Delete").clicked() {
                    app.doc.delete_report(id);
                    app.session.ui.craft_selected_report = None;
                    return;
                }
                if ui.button("Copy").clicked()
                    && let Ok(copy) = app.doc.copy_report(id, None)
                {
                    app.session.ui.craft_selected_report = Some(copy);
                }
                if ui.button("Rename").clicked() {
                    let _ = app
                        .doc
                        .rename_report(id, format!("CRAFT report {}", id.0 + 1));
                }
            }
        }
    });
    let Some(id) = app.session.ui.craft_selected_report else {
        ui.weak("Create a report to summarize trusted CRAFT components.");
        return;
    };
    let Some(record) = app.doc.report(id).cloned() else {
        return;
    };
    match record.status(&app.doc) {
        plotx_core::state::ReportStatus::Unavailable => {
            ui.colored_label(
                ui.visuals().error_fg_color,
                "Source CRAFT run is unavailable.",
            );
            return;
        }
        plotx_core::state::ReportStatus::NeedsReview => {
            ui.colored_label(
                ui.visuals().warn_fg_color,
                "Source CRAFT run changed. Review or recreate this report.",
            );
        }
        plotx_core::state::ReportStatus::Available => {}
    }
    let mut definition: CraftReportDefinition =
        serde_json::from_value(record.definition.clone()).unwrap_or_default();
    let mut changed = false;
    ui.horizontal(|ui| {
        ui.label("Report threshold A/N");
        changed |= ui
            .add(
                egui::DragValue::new(&mut definition.threshold_an)
                    .speed(0.1)
                    .range(0.001..=1_000.0),
            )
            .changed();
        ui.label("Segment width");
        changed |= ui
            .add(
                egui::DragValue::new(&mut definition.segment_width_hz)
                    .speed(0.1)
                    .range(0.001..=1_000_000.0),
            )
            .changed();
        ui.label(format!(
            "Hz ({:.5} ppm)",
            definition.segment_width_hz / nmr.data.observe_freq_mhz
        ));
    });
    let mut snapshot: CraftAmplitudeReport = serde_json::from_value(record.snapshot.clone())
        .unwrap_or(CraftAmplitudeReport {
            schema_version: 1,
            definition: definition.clone(),
            segments: Vec::new(),
        });
    if changed && let Ok(generated_snapshot) = run.amplitude_report(definition.clone()) {
        snapshot = generated_snapshot.clone();
        let mut updated = record.clone();
        updated.definition =
            serde_json::to_value(&definition).unwrap_or_else(|_| record.definition.clone());
        updated.snapshot =
            serde_json::to_value(generated_snapshot).unwrap_or_else(|_| record.snapshot.clone());
        let _ = app.doc.update_report(updated);
    }
    let component_count: usize = snapshot.segments.iter().map(|s| s.component_count).sum();
    let scalar: f64 = snapshot
        .segments
        .iter()
        .map(|s| s.scalar_amplitude_sum_t0)
        .sum();
    let coherent: f64 = snapshot
        .segments
        .iter()
        .map(|s| s.coherent_amplitude_t0)
        .sum();
    ui.small(format!(
        "{} segment(s) · {} component(s) · scalar {:.5} · coherent {:.5}",
        snapshot.segments.len(),
        component_count,
        scalar,
        coherent
    ));
    if ui
        .add_enabled(quantitative_ready, egui::Button::new("Export report…"))
        .on_disabled_hover_text(
            "Quantitative export is unavailable until CRAFT stability checks pass.",
        )
        .clicked()
    {
        match app.materialize_craft_report_table(index, id) {
            Ok(table) => app.open_data_export(table),
            Err(message) => app.session.status = message,
        }
    }
    egui::Grid::new(("craft_report_table", id.0))
        .striped(true)
        .show(ui, |ui| {
            ui.strong("Segment");
            ui.strong("Bounds (Hz)");
            ui.strong("Components");
            ui.strong("Scalar");
            ui.strong("Coherent");
            ui.end_row();
            for (i, segment) in snapshot.segments.iter().enumerate() {
                ui.label((i + 1).to_string());
                ui.label(format!("{:.4} .. {:.4}", segment.start_hz, segment.end_hz));
                ui.label(
                    segment
                        .component_ids
                        .iter()
                        .map(|id| id.0.to_string())
                        .collect::<Vec<_>>()
                        .join(", "),
                );
                ui.label(format!("{:.5}", segment.scalar_amplitude_sum_t0));
                ui.label(format!("{:.5}", segment.coherent_amplitude_t0));
                ui.end_row();
            }
        });
    let _ = index;
}

fn prepare_rerun(app: &mut PlotxApp, run: &StoredCraftRun) {
    app.session.ui.craft_base_run = Some(run.id);
    app.session.ui.craft_overrides = Default::default();
    app.session.ui.craft_resolution_cache = None;
    app.session.ui.craft_task_page = CraftTaskPage::Setup;
    app.session.ui.craft_analysis_intent = if run.provenance.invocation.sources.regions
        == plotx_processing::craft::CraftParamSource::InputDerived
    {
        CraftAnalysisIntent::ExploreBandwidth
    } else if run.provenance.invocation.params.regions.len() == 2 {
        CraftAnalysisIntent::CompareTwoSignals
    } else {
        CraftAnalysisIntent::SelectedSignals
    };
}

fn export_components(app: &mut PlotxApp, index: usize, run: &StoredCraftRun) {
    let table = run
        .component_table
        .and_then(|dataset| app.doc.dataset_index(dataset))
        .map(Ok)
        .unwrap_or_else(|| app.materialize_craft_component_table(index, run.id));
    match table {
        Ok(table) => app.open_data_export(table),
        Err(message) => app.session.status = message,
    }
}

fn run_label(run: &StoredCraftRun) -> String {
    let profile = match run.provenance.invocation.params.profile {
        CraftProfile::Conventional => "Conventional",
        CraftProfile::Ssfp => "SSFP",
    };
    format!(
        "Run {} · {profile} · {} components",
        run.id.0 + 1,
        run.components.len()
    )
}

fn overview(
    app: &mut PlotxApp,
    nmr: &plotx_core::state::NmrDataset,
    run: &StoredCraftRun,
    ui: &mut Ui,
) {
    let stale = run.is_stale_for(&nmr.data, nmr.craft_reference());
    let needs_review = stale
        || run.diagnostics.status == CraftRunStatus::Partial
        || !run.diagnostics.warnings.is_empty();
    let status = if stale {
        "Stale"
    } else if needs_review {
        "Needs review"
    } else {
        "Complete"
    };
    let status_color = if needs_review {
        ui.visuals().warn_fg_color
    } else {
        ui.visuals().selection.stroke.color
    };
    ui.colored_label(status_color, crate::typography::headline(status));
    ui.small(format!(
        "{} components · normalized residual {:.3e}",
        run.components.len(),
        run.diagnostics.normalized_residual
    ));
    ui.small(format!(
        "Fixed protocol: {:.0} Hz modeling bandwidth · boundary dispersion: {:.2}%",
        run.provenance
            .invocation
            .params
            .profile
            .modeling_bandwidth_hz(),
        run.diagnostics
            .stability
            .regions
            .iter()
            .map(|region| region.metric.relative_dispersion)
            .fold(0.0, f64::max)
            * 100.0,
    ));
    ui.small(format!(
        "Chemical-shift reference {:+.5} ppm · effective carrier {:.5} ppm",
        run.provenance.invocation.reference.offset_ppm,
        run.provenance.invocation.reference.effective_carrier_ppm(),
    ));
    if stale {
        ui.colored_label(
            ui.visuals().warn_fg_color,
            "The source FID or chemical-shift reference changed after this run. Rerun CRAFT before interpreting it.",
        );
    } else if !run.diagnostics.warnings.is_empty() {
        ui.colored_label(
            ui.visuals().warn_fg_color,
            format!(
                "{} diagnostic warning(s) require review.",
                run.diagnostics.warnings.len()
            ),
        );
    }
    if needs_review && ui.button("Review diagnostics").clicked() {
        app.session.ui.craft_result_tab = CraftResultTab::Diagnostics;
    }

    ui.separator();
    let exploratory = run.provenance.invocation.sources.regions
        == plotx_processing::craft::CraftParamSource::InputDerived;
    ui.strong(if exploratory {
        "Full-bandwidth exploration"
    } else {
        "Signal groups"
    });
    for (position, summary) in run.region_summaries.iter().enumerate() {
        let selected = app.session.ui.craft_component_region == Some(summary.region);
        if ui
            .selectable_label(
                selected,
                format!(
                    "{} · {:.4}–{:.4} ppm · coherent amplitude {:.4} · {} component(s)",
                    if exploratory {
                        "Full bandwidth".into()
                    } else {
                        format!("Signal {}", position + 1)
                    },
                    summary.start_ppm,
                    summary.end_ppm,
                    summary.coherent_amplitude_t0,
                    summary.component_count,
                ),
            )
            .clicked()
        {
            app.session.ui.craft_component_region = Some(summary.region);
            app.session.ui.craft_result_tab = CraftResultTab::Components;
        }
    }
    if let Some(ratio) = run.region_ratio {
        ui.label(format!(
            "Signal {} / Signal {} = {:.6}",
            region_number(run, ratio.numerator),
            region_number(run, ratio.denominator),
            ratio.value,
        ));
        ui.weak(
            "The ratio uses phase-aware coherent amplitudes, not a sum of component magnitudes.",
        );
    } else if run.region_summaries.len() == 1 {
        ui.weak("Use Adjust & rerun, choose Compare two signals, and draw two non-overlapping groups to obtain a quantitative ratio.");
    }

    ui.add_space(4.0);
    ui.strong("Observed spectrum, reconstruction, signal-group decomposition, and complex residual are shown on the standard result canvas.");
    if let Some(parent) = run.provenance.parent_run.and_then(|id| nmr.craft_run(id)) {
        ui.weak(format!(
            "Compared with Run {}: components {:+}, normalized residual {:+.3e}",
            parent.id.0 + 1,
            run.components.len() as isize - parent.components.len() as isize,
            run.diagnostics.normalized_residual - parent.diagnostics.normalized_residual,
        ));
    }
    ui.add_space(6.0);
    ui.weak("CRAFT runs are saved automatically with the PlotX project.");
}

fn components(
    app: &mut PlotxApp,
    _nmr: &plotx_core::state::NmrDataset,
    run: &StoredCraftRun,
    ui: &mut Ui,
) {
    if run.components.is_empty() {
        ui.weak("This run contains no retained components.");
        table_action(app, run, ui);
        return;
    }

    ui.strong("Component details");

    let regions = run
        .region_summaries
        .iter()
        .filter(|summary| summary.component_count > 0)
        .map(|summary| summary.region)
        .collect::<Vec<_>>();
    ui.horizontal_wrapped(|ui| {
        egui::ComboBox::from_id_salt(("craft_component_sort", run.id.0))
            .selected_text(match app.session.ui.craft_component_sort {
                CraftComponentSort::ChemicalShift => "Sort: ppm",
                CraftComponentSort::AmplitudeToNoise => "Sort: A/N",
            })
            .show_ui(ui, |ui| {
                ui.selectable_value(
                    &mut app.session.ui.craft_component_sort,
                    CraftComponentSort::ChemicalShift,
                    "Chemical shift",
                );
                ui.selectable_value(
                    &mut app.session.ui.craft_component_sort,
                    CraftComponentSort::AmplitudeToNoise,
                    "Amplitude / noise",
                );
            });
        if regions.len() > 1 {
            egui::ComboBox::from_id_salt(("craft_component_region", run.id.0))
                .selected_text(app.session.ui.craft_component_region.map_or_else(
                    || "All signals".into(),
                    |region| format!("Signal {}", region_number(run, region)),
                ))
                .show_ui(ui, |ui| {
                    ui.selectable_value(
                        &mut app.session.ui.craft_component_region,
                        None,
                        "All signals",
                    );
                    for &region in &regions {
                        ui.selectable_value(
                            &mut app.session.ui.craft_component_region,
                            Some(region),
                            format!("Signal {}", region_number(run, region)),
                        );
                    }
                });
        }
    });

    let mut visible = run
        .components
        .iter()
        .filter(|component| {
            app.session
                .ui
                .craft_component_region
                .is_none_or(|region| component.region == region)
        })
        .collect::<Vec<_>>();
    match app.session.ui.craft_component_sort {
        CraftComponentSort::ChemicalShift => {
            visible.sort_by(|a, b| b.chemical_shift_ppm.total_cmp(&a.chemical_shift_ppm))
        }
        CraftComponentSort::AmplitudeToNoise => {
            visible.sort_by(|a, b| b.amplitude_to_noise.total_cmp(&a.amplitude_to_noise))
        }
    }
    ui.small(format!("{} component(s)", visible.len()));
    egui::ScrollArea::vertical()
        .id_salt(("craft_component_list", run.id.0))
        .max_height(220.0)
        .auto_shrink([false, false])
        .show(ui, |ui| {
            for component in visible {
                if component_row(
                    component,
                    region_number(run, component.region),
                    app.session.ui.craft_selected_component == Some(component.id),
                    ui,
                ) {
                    app.session.ui.craft_selected_component = Some(component.id);
                }
            }
        });
    ui.separator();
    table_action(app, run, ui);
}

fn component_row(
    component: &CraftComponent,
    region_number: usize,
    selected: bool,
    ui: &mut Ui,
) -> bool {
    egui::CollapsingHeader::new(format!(
        "δ {:.5} ppm · A {:.4}",
        component.chemical_shift_ppm, component.amplitude_t0
    ))
    .id_salt(("craft_component", component.id.0))
    .default_open(selected)
    .show(ui, |ui| {
        ui.small(format!(
            "LW {:.3} Hz · A/N {:.2} · Signal {}",
            component.linewidth_hz, component.amplitude_to_noise, region_number
        ));
        ui.small(format!(
            "Frequency {:.4} Hz · Phase {:.4} rad · Decay {:.4} s^-1",
            component.frequency_hz, component.phase_rad, component.decay_rate_s_inv
        ));
        ui.weak(format!(
            "Uncertainty: frequency {} · amplitude {} · linewidth {} · phase {}",
            optional(component.frequency_std_hz, "Hz"),
            optional(component.amplitude_std, ""),
            optional(component.linewidth_std_hz, "Hz"),
            optional(component.phase_std_rad, "rad")
        ));
    })
    .header_response
    .clicked()
}

fn optional(value: Option<f64>, unit: &str) -> String {
    value.map_or_else(
        || "unavailable".into(),
        |value| {
            if unit.is_empty() {
                format!("{value:.4}")
            } else {
                format!("{value:.4} {unit}")
            }
        },
    )
}

fn table_action(app: &mut PlotxApp, run: &StoredCraftRun, ui: &mut Ui) {
    let table = run
        .component_table
        .and_then(|dataset| app.doc.dataset_index(dataset));
    if let Some(table) = table {
        ui.horizontal_wrapped(|ui| {
            if ui.button("View data table").clicked() {
                app.session.ui.sheet_open = Some(table);
                app.session.status = "Opened the CRAFT component table.".into();
            }
            let visible = app.doc.datasets[table]
                .as_table()
                .is_some_and(|table| table.board_sheet_visible());
            if !visible
                && ui.button("Add to board").clicked()
                && let Err(message) = app.show_craft_component_table_on_board(table)
            {
                app.session.status = message;
            }
        });
    } else {
        super::command_button(
            app,
            CommandId::CraftComponentTable,
            "Create data table",
            false,
            ui,
        );
        ui.weak("Creates a sortable, chartable table without leaving CRAFT.");
    }
}

fn diagnostics(
    app: &mut PlotxApp,
    nmr: &plotx_core::state::NmrDataset,
    run: &StoredCraftRun,
    ui: &mut Ui,
) {
    ui.strong("Quality check");
    ui.weak("Inspect the complete frequency-domain residual on the linked result canvas.");
    ui.separator();
    let condition = run
        .diagnostics
        .maximum_condition_number
        .map(|value| format!("{value:.3e}"))
        .unwrap_or_else(|| "unbounded".to_owned());
    ui.small(format!(
        "Noise σ {:.4} · residual RSS {:.4}",
        run.diagnostics.noise_sigma, run.diagnostics.residual_rss
    ));
    ui.small(format!("Maximum condition number {condition}"));
    if run.is_stale_for(&nmr.data, nmr.craft_reference()) {
        ui.colored_label(
            ui.visuals().warn_fg_color,
            "The source FID or chemical-shift reference changed after this run. Rerun CRAFT before interpreting it.",
        );
    }
    for warning in &run.diagnostics.warnings {
        ui.horizontal_wrapped(|ui| {
            ui.colored_label(ui.visuals().warn_fg_color, &warning.message);
            if let Some(region) = warning.region
                && ui
                    .small_button(format!("Show Signal {}", region_number(run, region)))
                    .clicked()
            {
                app.session.ui.craft_component_region = Some(region);
                app.session.ui.craft_result_tab = CraftResultTab::Components;
            }
            if ui.small_button("Adjust setup").clicked() {
                prepare_rerun(app, run);
            }
        });
    }
    ui.collapsing("Parameters and sources", |ui| {
        let invocation = &run.provenance.invocation;
        ui.small(format!(
            "Profile {:?} ({:?}) · regions {:?} ({:?})",
            invocation.params.profile,
            invocation.sources.profile,
            invocation.params.regions,
            invocation.sources.regions,
        ));
        ui.small(format!(
            "A/N {:.2} ({:?}) · model limit {} ({:?}) · linewidth {:.3}–{:.3} Hz ({:?})",
            invocation.params.minimum_amplitude_to_noise,
            invocation.sources.minimum_amplitude_to_noise,
            invocation.params.maximum_model_order,
            invocation.sources.maximum_model_order,
            invocation.params.component_linewidth_bounds_hz.0,
            invocation.params.component_linewidth_bounds_hz.1,
            invocation.sources.component_linewidth_bounds_hz,
        ));
        ui.small(format!(
            "Skip {} points ({:?}) · FIR {} taps · {} available · {} reconstructed · {} modeling window(s)",
            invocation.derived_plan.effective_skip_points,
            invocation.derived_plan.effective_skip_source,
            invocation.derived_plan.effective_fir_filter_taps,
            invocation.derived_plan.available_points,
            invocation.derived_plan.reconstruction_points,
            invocation.derived_plan.modeling_windows.len(),
        ));
        for issue in &invocation.assessment.issues {
            ui.colored_label(
                match issue.severity {
                    plotx_processing::craft::CraftIssueSeverity::Error => ui.visuals().error_fg_color,
                    plotx_processing::craft::CraftIssueSeverity::Warning => ui.visuals().warn_fg_color,
                },
                &issue.message,
            );
        }
    });
    super::results_diagnostics::show_modeling_windows(run, ui);
}

fn region_number(run: &StoredCraftRun, id: CraftRegionId) -> usize {
    run.region_summaries
        .iter()
        .position(|summary| summary.region == id)
        .map_or(0, |position| position + 1)
}
