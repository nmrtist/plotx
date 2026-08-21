use egui::{Button, Ui};
use egui_phosphor::regular as icon;
use plotx_core::state::{
    CraftAnalysisIntent, CraftTaskPage, Dataset, FrameRef, PlotxApp, Selection, TaskDockTab, Tool,
};

use super::task_card::{self, TaskCardGeometry};
use crate::ui::commands::{self, CommandId};

mod results;
mod setup;
mod spectrum;

pub(crate) fn open_for_active(app: &mut PlotxApp) {
    let Some(index) = app.active_dataset() else {
        return;
    };
    let Some(nmr) = app.doc.datasets.get(index).and_then(Dataset::as_nmr) else {
        return;
    };
    if nmr.data.domain != plotx_io::Domain::Time {
        return;
    }
    let dataset = nmr.resource_id;
    let selected = nmr.craft_runs.last().map(|run| run.id);
    if app.session.ui.craft_task_dataset != Some(dataset) {
        app.session.ui.craft_selected_run = selected;
        app.session.ui.craft_overrides_dataset = Some(dataset);
        app.session.ui.craft_overrides = Default::default();
        app.session.ui.craft_resolution_cache = None;
        app.session.ui.craft_base_run = selected;
        app.session.ui.craft_task_page = if selected.is_some() {
            CraftTaskPage::Results
        } else {
            CraftTaskPage::Setup
        };
        app.session.ui.craft_component_region = None;
        app.session.ui.craft_selected_component = None;
        app.session.ui.craft_normalize_groups = false;
    }
    app.session.ui.craft_task_dataset = Some(dataset);
    app.session.ui.open_task_tab(TaskDockTab::Craft);
}

pub(crate) fn open_result_canvas(app: &mut PlotxApp) {
    let Some((dataset, run)) = app
        .session
        .ui
        .craft_task_dataset
        .zip(app.session.ui.craft_selected_run)
    else {
        return;
    };
    if let Err(message) = app.open_craft_result_canvas(dataset, run) {
        app.session.status = message;
    }
}

pub(crate) fn run(app: &mut PlotxApp) {
    let Some(index) = app
        .session
        .ui
        .craft_task_dataset
        .and_then(|id| app.doc.dataset_index(id))
    else {
        return;
    };
    app.request_craft_analysis(
        index,
        app.session.ui.craft_overrides.clone(),
        app.session.ui.craft_base_run,
    );
}

pub(crate) fn create_component_table(app: &mut PlotxApp) {
    let Some((index, run)) = app
        .session
        .ui
        .craft_task_dataset
        .and_then(|id| app.doc.dataset_index(id))
        .zip(app.session.ui.craft_selected_run)
    else {
        return;
    };
    match app.materialize_craft_component_table(index, run) {
        Ok(_) => {
            app.session.status =
                "Created the CRAFT component table. Use View data table when you need it.".into()
        }
        Err(message) => app.session.status = message,
    }
}

pub(crate) fn select_regions_on_canvas(app: &mut PlotxApp, index: usize) {
    let Some(dataset_id) = app.doc.datasets.get(index).map(Dataset::resource_id) else {
        return;
    };
    let Some(source_field) = app.doc.datasets[index]
        .as_nmr()
        .filter(|nmr| nmr.spectrum().is_some())
        .and_then(|nmr| nmr.field_catalog.id_for_key("nmr.real"))
    else {
        app.session.status =
            "Process the FID to a frequency-domain spectrum before selecting CRAFT signal groups."
                .to_owned();
        return;
    };
    let invocation = setup::resolved(app, index);
    if app.session.ui.craft_overrides.regions.is_none() {
        app.session.ui.craft_overrides.regions = Some(
            if invocation.sources.regions
                == plotx_processing::craft::CraftParamSource::ResultProvenance
            {
                invocation.params.regions
            } else {
                Vec::new()
            },
        );
    }
    if app.session.ui.craft_analysis_intent == CraftAnalysisIntent::ExploreBandwidth {
        app.session.ui.craft_analysis_intent = CraftAnalysisIntent::SelectedSignals;
    }
    app.session.ui.craft_resolution_cache = None;

    let eligible = |app: &PlotxApp, ci: usize| {
        app.doc.canvases.get(ci).and_then(|canvas| {
            canvas.objects.iter().find_map(|object| {
                object
                    .plot()
                    .filter(|plot| {
                        plot.binding.series.iter().any(|series| {
                            series.source.resource == dataset_id
                                && series.source.field == source_field
                                && series.source.item.is_none()
                        })
                    })
                    .map(|_| object.id)
            })
        })
    };
    let mut target = app
        .session
        .active_canvas
        .and_then(|ci| eligible(app, ci).map(|object| (ci, object)))
        .or_else(|| {
            (0..app.doc.canvases.len())
                .rev()
                .find_map(|ci| eligible(app, ci).map(|object| (ci, object)))
        });
    if target.is_none() && app.insert_dataset_canvas(index) {
        let ci = app.doc.canvases.len() - 1;
        target = eligible(app, ci).map(|object| (ci, object));
    }
    let Some((canvas, object)) = target else {
        app.session.status =
            "Could not create a frequency-domain spectrum for CRAFT selection.".to_owned();
        return;
    };
    app.reveal_board_frame(FrameRef::Page(canvas));
    app.set_selection(Selection::single(object));
    app.set_tool(Tool::CraftRegions);
    app.session.status =
        "Draw, move, or resize CRAFT signal groups on the spectrum; press Esc to cancel a drag."
            .to_owned();
}

pub(crate) fn render_task(app: &mut PlotxApp, host: &mut Ui) {
    if !task_card::is_active(app, TaskDockTab::Craft) {
        return;
    }
    let Some(index) = app
        .session
        .ui
        .craft_task_dataset
        .and_then(|id| app.doc.dataset_index(id))
    else {
        return;
    };
    if app.active_dataset() != Some(index)
        || !app.doc.datasets.get(index).is_some_and(|dataset| {
            dataset
                .as_nmr()
                .is_some_and(|nmr| nmr.data.domain == plotx_io::Domain::Time)
        })
    {
        return;
    }

    let TaskCardGeometry {
        pos,
        width,
        min_body_height,
        max_body_height,
    } = task_card::geometry_with_width(host, 420.0, 820.0);
    let collapsed = app.session.ui.craft_task_collapsed;
    let dark = host.visuals().dark_mode;
    let mut close = false;
    let mut toggle_collapse = false;
    let area_id = egui::Id::new("craft_task_card");
    task_card::area(host, area_id, pos).show(host.ctx(), |ui| {
        ui.set_width(width);
        crate::ui::card_frame(dark, egui::Margin::ZERO).show(ui, |ui| {
            if task_card::tab_bar(app, TaskDockTab::Craft, ui) {
                ui.separator();
            }
            let nmr = app.doc.datasets[index].as_nmr().unwrap();
            let runs = nmr.craft_runs.len();
            task_card::header(ui, area_id, |ui| {
                ui.label(crate::typography::headline("CRAFT"));
                ui.weak(format!("{} points · {runs} run(s)", nmr.data.points.len()));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui
                        .small_button(icon::X)
                        .on_hover_text("Close CRAFT")
                        .clicked()
                    {
                        close = true;
                    }
                    let glyph = if collapsed {
                        icon::CARET_DOWN
                    } else {
                        icon::CARET_UP
                    };
                    if ui
                        .small_button(glyph)
                        .on_hover_text(if collapsed {
                            "Expand CRAFT"
                        } else {
                            "Collapse CRAFT"
                        })
                        .clicked()
                    {
                        toggle_collapse = true;
                    }
                });
            });
            if !collapsed {
                ui.separator();
                task_card::resizable_body(
                    ui,
                    "craft_task_body_resize",
                    650.0,
                    min_body_height,
                    max_body_height,
                    |ui| body(app, index, ui),
                );
            }
        });
    });
    if toggle_collapse {
        app.session.ui.craft_task_collapsed = !collapsed;
    }
    if close {
        app.session.ui.close_task_tab(TaskDockTab::Craft);
    }
}

fn body(app: &mut PlotxApp, index: usize, ui: &mut Ui) {
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            let has_results = app.doc.datasets[index]
                .as_nmr()
                .is_some_and(|nmr| !nmr.craft_runs.is_empty());
            ui.horizontal(|ui| {
                ui.selectable_value(
                    &mut app.session.ui.craft_task_page,
                    CraftTaskPage::Setup,
                    "Setup",
                );
                ui.add_enabled_ui(has_results, |ui| {
                    ui.selectable_value(
                        &mut app.session.ui.craft_task_page,
                        CraftTaskPage::Results,
                        "Results",
                    );
                });
            });
            ui.separator();
            match app.session.ui.craft_task_page {
                CraftTaskPage::Setup => {
                    ui.small(
                        "Decompose the original complex FID into damped sinusoidal components.",
                    );
                    setup::show(app, index, ui);
                }
                CraftTaskPage::Results => results::show(app, index, ui),
            }
            ui.add_space(8.0);
        });
}

fn command_button(app: &mut PlotxApp, command: CommandId, label: &str, primary: bool, ui: &mut Ui) {
    let descriptor = commands::describe(app, command);
    let mut button = Button::new(label);
    if primary {
        button = button
            .fill(ui.visuals().selection.bg_fill)
            .stroke(egui::Stroke::NONE);
    }
    let response = ui
        .add_enabled(descriptor.enabled, button)
        .on_disabled_hover_text(descriptor.disabled_reason.unwrap_or("Command unavailable."));
    if response.clicked() {
        commands::execute_without_clipboard(command, app, ui.ctx());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use num_complex::Complex64;
    use plotx_core::state::{CraftRunId, NmrDataset, StoredCraftRun};
    use plotx_io::{Domain, NmrData};
    use plotx_processing::craft::{
        CraftDiagnostics, CraftInvocation, CraftParams, CraftResult, CraftRunStatus,
    };

    fn time_domain_data(source: &str) -> NmrData {
        NmrData {
            points: vec![Complex64::new(1.0, 0.0); 64],
            domain: Domain::Time,
            spectral_width_hz: 1_000.0,
            observe_freq_mhz: 500.0,
            carrier_ppm: 4.7,
            nucleus: "1H".to_owned(),
            source: source.to_owned(),
            group_delay: 0.0,
        }
    }

    fn stored_run(data: &NmrData, params: CraftParams) -> StoredCraftRun {
        StoredCraftRun::from_result(
            CraftRunId(0),
            data,
            CraftInvocation::acquisition(data, params),
            None,
            CraftResult {
                components: Vec::new(),
                region_summaries: Vec::new(),
                region_ratio: None,
                diagnostics: CraftDiagnostics {
                    status: CraftRunStatus::Complete,
                    noise_sigma: 1.0,
                    residual_rss: 1.0,
                    normalized_residual: 1.0,
                    maximum_condition_number: Some(1.0),
                    fit_windows: Vec::new(),
                    warnings: Vec::new(),
                },
                synthetic_fid: Vec::new(),
                residual_fid: Vec::new(),
            },
        )
    }

    #[test]
    fn changing_target_rebuilds_draft_from_target_provenance() {
        let first = NmrDataset::load(time_domain_data("first"));
        let mut second = NmrDataset::load(time_domain_data("second"));
        let mut provenance_params = CraftParams::ssfp();
        provenance_params.min_amplitude_to_noise = 8.5;
        second
            .craft_runs
            .push(stored_run(&second.data, provenance_params.clone()));
        let mut app = PlotxApp::new_with_settings(plotx_core::settings::Settings::default());
        app.doc.datasets.push(Dataset::Nmr(Box::new(first)));
        app.doc.datasets.push(Dataset::Nmr(Box::new(second)));

        app.set_active_dataset(Some(0));
        open_for_active(&mut app);
        app.session.ui.craft_overrides.min_amplitude_to_noise = Some(6.0);
        open_for_active(&mut app);
        assert_eq!(
            app.session.ui.craft_overrides.min_amplitude_to_noise,
            Some(6.0)
        );

        app.set_active_dataset(Some(1));
        open_for_active(&mut app);

        assert_eq!(app.session.ui.craft_overrides, Default::default());
        assert_eq!(setup::resolved(&mut app, 1).params, provenance_params);
        assert_eq!(app.session.ui.craft_selected_run, Some(CraftRunId(0)));
        assert_eq!(app.session.ui.craft_task_page, CraftTaskPage::Results);
    }

    #[test]
    fn preview_indices_cover_endpoints_with_a_bounded_sample_count() {
        let indices = results::preview_sample_indices(65_536, 310);

        assert_eq!(indices.len(), 310);
        assert_eq!(indices.first(), Some(&0));
        assert_eq!(indices.last(), Some(&65_535));
        assert!(indices.windows(2).all(|pair| pair[0] < pair[1]));
    }

    #[test]
    fn detected_signal_suggestions_advance_past_an_existing_selection() {
        let sw = 1_000.0;
        let mut data = time_domain_data("signal picker");
        data.carrier_ppm = 0.0;
        data.points = (0..256)
            .map(|index| {
                let time = index as f64 / sw;
                Complex64::from_polar(10.0, std::f64::consts::TAU * 125.0 * time)
                    + Complex64::from_polar(3.0, -std::f64::consts::TAU * 250.0 * time)
            })
            .collect();
        let dataset = NmrDataset::load(data);

        let invocation = plotx_processing::craft::resolve_craft_invocation(
            &dataset.data,
            dataset.craft_reference(),
            &plotx_processing::craft::CraftParamOverrides {
                filter_taps: Some(31),
                ..Default::default()
            },
            None,
        );
        let suggestions = invocation.assessment.clear_signals;
        assert!(
            suggestions
                .iter()
                .any(|signal| (signal.chemical_shift_ppm - 0.25).abs() < 1e-9)
        );
        assert!(
            suggestions
                .iter()
                .any(|signal| (signal.chemical_shift_ppm + 0.5).abs() < 1e-9)
        );
    }

    #[test]
    fn detected_signal_width_is_independent_of_internal_fit_window_width() {
        assert!((45.0 / 600.0_f64 - 0.075).abs() < f64::EPSILON);
    }
}
