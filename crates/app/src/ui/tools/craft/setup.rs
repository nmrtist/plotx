use egui::{DragValue, Ui};
use egui_phosphor::regular as icon;
use plotx_core::state::{ComputeKind, CraftAnalysisIntent, CraftRunFeedback, PlotxApp};
use plotx_processing::craft::{
    CraftInvocation, CraftParamSource, CraftProfile, CraftRegion, CraftRegionId,
    resolve_craft_invocation,
};

use crate::ui::commands::CommandId;

pub(super) fn show(app: &mut PlotxApp, index: usize, ui: &mut Ui) {
    let invocation = resolved(app, index);
    settings(app, index, &invocation, ui);
    ui.separator();
    readiness(app.session.ui.craft_analysis_intent, &invocation, ui);
    ui.separator();
    run_controls(app, index, ui);
}

pub(super) fn resolved(app: &mut PlotxApp, index: usize) -> CraftInvocation {
    let nmr = app.doc.datasets[index].as_nmr().unwrap();
    let dataset = nmr.resource_id;
    let reference = nmr.craft_reference();
    let parent_run = app.session.ui.craft_base_run;
    if let Some(cache) = &app.session.ui.craft_resolution_cache
        && cache.dataset == dataset
        && cache.dataset_epoch == app.session.dataset_epoch
        && cache.reference == reference
        && cache.overrides == app.session.ui.craft_overrides
        && cache.parent_run == parent_run
    {
        return cache.invocation.clone();
    }
    let invocation = resolve_craft_invocation(
        &nmr.data,
        reference,
        &app.session.ui.craft_overrides,
        parent_run.and_then(|id| nmr.craft_run(id).map(|run| &run.provenance.invocation)),
    );
    app.session.ui.craft_resolution_cache = Some(plotx_core::state::CraftResolutionCache {
        dataset,
        dataset_epoch: app.session.dataset_epoch,
        reference,
        overrides: app.session.ui.craft_overrides.clone(),
        parent_run,
        invocation: invocation.clone(),
    });
    invocation
}

fn readiness(intent: CraftAnalysisIntent, invocation: &CraftInvocation, ui: &mut Ui) {
    let assessment = &invocation.assessment;
    let selected = selected_regions(invocation).len();
    let intent_ready = match intent {
        CraftAnalysisIntent::ExploreBandwidth => true,
        CraftAnalysisIntent::SelectedSignals => selected > 0,
        CraftAnalysisIntent::CompareTwoSignals => selected == 2,
    };
    let duration = assessment.acquisition_duration_s.map_or_else(
        || "unknown duration".into(),
        |value| format!("{value:.3} s"),
    );
    let (label, color) = if !assessment.can_run() || !intent_ready {
        ("Cannot run", ui.visuals().error_fg_color)
    } else if assessment.has_warnings() {
        ("Ready with warnings", ui.visuals().warn_fg_color)
    } else {
        ("Ready", ui.visuals().selection.stroke.color)
    };
    ui.colored_label(color, crate::typography::headline(label));
    ui.small(format!(
        "{} points ({} usable) · {duration} · {} fit window(s) · {} clear signal(s)",
        assessment.point_count,
        assessment.effective_point_count,
        assessment.fit_window_count,
        assessment.clear_signals.len(),
    ));
    for issue in &assessment.issues {
        ui.colored_label(
            match issue.severity {
                plotx_processing::craft::CraftIssueSeverity::Error => ui.visuals().error_fg_color,
                plotx_processing::craft::CraftIssueSeverity::Warning => ui.visuals().warn_fg_color,
            },
            &issue.message,
        );
    }
    if !intent_ready {
        ui.colored_label(
            ui.visuals().error_fg_color,
            match intent {
                CraftAnalysisIntent::SelectedSignals => {
                    "Draw at least one signal group on the spectrum."
                }
                CraftAnalysisIntent::CompareTwoSignals => {
                    "Draw exactly two signal groups for a quantitative ratio."
                }
                CraftAnalysisIntent::ExploreBandwidth => unreachable!(),
            },
        );
    }
}

fn settings(app: &mut PlotxApp, index: usize, invocation: &CraftInvocation, ui: &mut Ui) {
    let nmr = app.doc.datasets[index].as_nmr().unwrap().clone();
    let reference = nmr.craft_reference();
    ui.label(crate::typography::headline("1. Choose the analysis goal"));
    ui.horizontal_wrapped(|ui| {
        ui.selectable_value(
            &mut app.session.ui.craft_analysis_intent,
            CraftAnalysisIntent::ExploreBandwidth,
            "Explore full bandwidth",
        );
        ui.selectable_value(
            &mut app.session.ui.craft_analysis_intent,
            CraftAnalysisIntent::SelectedSignals,
            "Measure selected signals",
        );
        ui.selectable_value(
            &mut app.session.ui.craft_analysis_intent,
            CraftAnalysisIntent::CompareTwoSignals,
            "Compare two signals",
        );
    });
    ui.small(match app.session.ui.craft_analysis_intent {
        CraftAnalysisIntent::ExploreBandwidth => {
            "Discover retained components across the acquired bandwidth; treat the result as exploratory."
        }
        CraftAnalysisIntent::SelectedSignals => {
            "Draw one or more signal groups. Components are reported inside those groups."
        }
        CraftAnalysisIntent::CompareTwoSignals => {
            "Draw exactly two non-overlapping signal groups to calculate a coherent-amplitude ratio."
        }
    });
    if app.session.ui.craft_analysis_intent == CraftAnalysisIntent::ExploreBandwidth {
        app.session.ui.craft_overrides.regions = None;
    }

    ui.add_space(8.0);
    ui.label(crate::typography::headline(
        "2. Select signals on the spectrum",
    ));
    let selecting = app.session.tool == plotx_core::state::Tool::CraftRegions;
    if ui
        .add(egui::Button::new(if selecting {
            "Selecting on spectrum…"
        } else {
            "Select on spectrum"
        }))
        .clicked()
    {
        super::select_regions_on_canvas(app, index);
    }
    ui.weak("PlotX opens or focuses the standard spectrum canvas. Drag to create a group; drag its body or 8 px edges to move or resize it.");

    ui.weak(format!(
        "Chemical-shift axis: acquisition {:.5} ppm · reference {:+.5} ppm · effective {:.5} ppm",
        nmr.data.carrier_ppm,
        reference.offset_ppm,
        reference.effective_carrier_ppm(),
    ));
    let mut overrides = app.session.ui.craft_overrides.clone();
    let mut profile = invocation.params.profile;
    egui::ComboBox::from_label("Acquisition profile")
        .selected_text(match profile {
            CraftProfile::Conventional => "Conventional FID",
            CraftProfile::Ssfp => "SSFP / interrupted FID",
        })
        .show_ui(ui, |ui| {
            ui.selectable_value(&mut profile, CraftProfile::Conventional, "Conventional FID");
            ui.selectable_value(&mut profile, CraftProfile::Ssfp, "SSFP / interrupted FID");
        });
    if profile != invocation.params.profile {
        overrides.select_profile(profile);
    }
    ui.small(source_text(invocation.sources.profile, &nmr));

    regions(&mut overrides, invocation, &nmr, ui);

    ui.add_space(8.0);
    ui.label(crate::typography::headline(
        "3. Confirm acquisition and fit settings",
    ));
    ui.collapsing("Advanced fit settings", |ui| {
        let mut value = invocation.params.min_amplitude_to_noise;
        if setting_row(
            ui,
            "Minimum A/N",
            invocation.sources.min_amplitude_to_noise,
            &nmr,
            &mut overrides.min_amplitude_to_noise,
            |ui| {
                ui.add(DragValue::new(&mut value).range(0.1..=100.0).speed(0.1))
                    .changed()
            },
        ) {
            overrides.min_amplitude_to_noise = Some(value);
        }

        let mut value = invocation.params.max_components_per_fit_window;
        if setting_row(
            ui,
            "Max components / fit window",
            invocation.sources.max_components_per_fit_window,
            &nmr,
            &mut overrides.max_components_per_fit_window,
            |ui| ui.add(DragValue::new(&mut value).range(1..=64)).changed(),
        ) {
            overrides.max_components_per_fit_window = Some(value);
        }

        let mut value = invocation.params.linewidth_hz;
        if setting_row(
            ui,
            "Linewidth range (Hz)",
            invocation.sources.linewidth_hz,
            &nmr,
            &mut overrides.linewidth_hz,
            |ui| {
                let first = ui
                    .add(DragValue::new(&mut value.0).range(0.001..=1_000.0))
                    .changed();
                ui.label("to");
                first
                    | ui.add(DragValue::new(&mut value.1).range(0.002..=2_000.0))
                        .changed()
            },
        ) {
            overrides.linewidth_hz = Some(value);
        }

        let mut value = invocation.params.max_fit_window_width_hz;
        if setting_row(
            ui,
            "Fit window width (Hz)",
            invocation.sources.max_fit_window_width_hz,
            &nmr,
            &mut overrides.max_fit_window_width_hz,
            |ui| {
                ui.add(DragValue::new(&mut value).range(10.0..=10_000.0))
                    .changed()
            },
        ) {
            overrides.max_fit_window_width_hz = Some(value);
        }

        let mut value = invocation.params.filter_taps;
        if setting_row(
            ui,
            "FIR taps",
            invocation.sources.filter_taps,
            &nmr,
            &mut overrides.filter_taps,
            |ui| {
                ui.add(DragValue::new(&mut value).range(3..=4_095))
                    .changed()
            },
        ) {
            overrides.filter_taps = Some(value | 1);
        }

        let mut value = invocation.params.max_downsampled_points;
        if setting_row(
            ui,
            "Max downsampled points",
            invocation.sources.max_downsampled_points,
            &nmr,
            &mut overrides.max_downsampled_points,
            |ui| {
                ui.add(DragValue::new(&mut value).range(64..=65_536))
                    .changed()
            },
        ) {
            overrides.max_downsampled_points = Some(value);
        }

        if invocation.params.profile == CraftProfile::Ssfp {
            let mut skip_ms = invocation.params.skip_duration_s * 1_000.0;
            ui.horizontal_wrapped(|ui| {
                ui.label("Skip initial");
                if ui
                    .add(
                        DragValue::new(&mut skip_ms)
                            .range(0.0..=100.0)
                            .suffix(" ms"),
                    )
                    .changed()
                {
                    overrides.skip_duration_s = Some(skip_ms / 1_000.0);
                }
                ui.weak(source_text(invocation.sources.skip_duration_s, &nmr));
                reset_button(&mut overrides.skip_duration_s, ui);
            });
            let mut reconstruct = invocation.params.reconstruction_duration_s.is_some();
            if ui
                .checkbox(&mut reconstruct, "Extend reconstructed FID")
                .changed()
            {
                overrides.reconstruction_duration_s = Some(reconstruct.then_some(1.2));
            }
            if let Some(mut duration) = invocation.params.reconstruction_duration_s
                && ui
                    .add(
                        DragValue::new(&mut duration)
                            .range(0.01..=10.0)
                            .suffix(" s"),
                    )
                    .changed()
            {
                overrides.reconstruction_duration_s = Some(Some(duration));
            }
        }
        ui.weak(format!(
            "Derived plan: skip {} points · {} actual taps · {} reconstructed points",
            invocation.derived_plan.effective_skip_points,
            invocation.derived_plan.actual_filter_taps,
            invocation.derived_plan.reconstruction_points,
        ));
    });

    app.session.ui.craft_overrides = overrides;
}

fn selected_regions(invocation: &CraftInvocation) -> &[CraftRegion] {
    if invocation.sources.regions == CraftParamSource::InputDerived {
        &[]
    } else {
        &invocation.params.regions
    }
}

fn regions(
    overrides: &mut plotx_processing::craft::CraftParamOverrides,
    invocation: &CraftInvocation,
    nmr: &plotx_core::state::NmrDataset,
    ui: &mut Ui,
) {
    ui.collapsing("Signal groups", |ui| {
        let inherited = invocation.sources.regions == CraftParamSource::ResultProvenance;
        let mut regions = overrides.regions.clone().unwrap_or_else(|| {
            if inherited {
                invocation.params.regions.clone()
            } else {
                Vec::new()
            }
        });
        if regions.is_empty() {
            let region = invocation.params.regions.first().copied();
            ui.weak(region.map_or_else(
                || "No explicit signal groups; analyzing the complete acquired bandwidth.".into(),
                |region| {
                    format!(
                        "Inherited/full bandwidth: {:.4} to {:.4} ppm · {}",
                        region.start_ppm,
                        region.end_ppm,
                        source_text(invocation.sources.regions, nmr)
                    )
                },
            ));
        }
        let mut changed = false;
        let mut remove = None;
        for (position, region) in regions.iter_mut().enumerate() {
            ui.horizontal_wrapped(|ui| {
                changed |= ui
                    .add(DragValue::new(&mut region.start_ppm).suffix(" ppm"))
                    .changed();
                ui.label("to");
                changed |= ui
                    .add(DragValue::new(&mut region.end_ppm).suffix(" ppm"))
                    .changed();
                if ui
                    .small_button(icon::X)
                    .on_hover_text("Remove region")
                    .clicked()
                {
                    remove = Some(position);
                }
            });
        }
        if let Some(position) = remove {
            regions.remove(position);
            changed = true;
        }
        let half_width = 45.0 / nmr.data.observe_freq_mhz.max(f64::MIN_POSITIVE);
        let suggestions = invocation
            .assessment
            .clear_signals
            .iter()
            .filter(|signal| {
                regions.iter().all(|region| {
                    let region = region.normalized();
                    signal.chemical_shift_ppm + half_width <= region.start_ppm
                        || signal.chemical_shift_ppm - half_width >= region.end_ppm
                })
            })
            .take(12)
            .copied()
            .collect::<Vec<_>>();
        if suggestions.is_empty() {
            ui.weak("No additional clear signals meet the shared preflight threshold.");
        } else {
            ui.menu_button("Add detected signal", |ui| {
                for signal in suggestions {
                    if ui
                        .button(format!("{:.4} ppm", signal.chemical_shift_ppm))
                        .clicked()
                    {
                        regions.push(CraftRegion::new(
                            next_region_id(&regions),
                            signal.chemical_shift_ppm - half_width,
                            signal.chemical_shift_ppm + half_width,
                        ));
                        changed = true;
                        ui.close();
                    }
                }
            });
        }
        if ui.small_button("Add custom region").clicked() {
            let center = nmr.craft_reference().effective_carrier_ppm();
            regions.push(CraftRegion::new(
                next_region_id(&regions),
                center - half_width,
                center + half_width,
            ));
            changed = true;
        }
        if changed {
            overrides.regions = (!regions.is_empty()).then_some(regions);
        }
        if overrides.regions.is_some()
            && ui
                .small_button("Reset to inherited/full bandwidth")
                .clicked()
        {
            overrides.regions = None;
        }
    });
}

fn run_controls(app: &mut PlotxApp, index: usize, ui: &mut Ui) {
    let dataset = app.doc.datasets[index].resource_id();
    if let Some(elapsed) = app.session.compute.progress(dataset, ComputeKind::Craft) {
        ui.horizontal_wrapped(|ui| {
            ui.spinner();
            ui.label(format!("Running… {:.1} s", elapsed.as_secs_f32()));
        });
        if let Some(CraftRunFeedback::Running(invocation)) =
            app.session.ui.craft_feedback.get(&dataset)
        {
            ui.weak(format!(
                "Frozen run: {:?} · {} region(s)",
                invocation.params.profile,
                invocation.params.regions.len()
            ));
        }
        if ui.button("Cancel CRAFT").clicked() {
            app.cancel_compute(index, ComputeKind::Craft);
        }
        return;
    }
    match app.session.ui.craft_feedback.get(&dataset) {
        Some(CraftRunFeedback::Cancelled) => {
            ui.weak("The previous run was cancelled. You can run it again.")
        }
        Some(CraftRunFeedback::Failed { message }) => ui.colored_label(
            ui.visuals().error_fg_color,
            format!("Previous run failed: {message}"),
        ),
        _ => ui.label("Run"),
    };
    super::command_button(app, CommandId::RunCraft, "Run CRAFT", true, ui);
    let invocation = resolved(app, index);
    if let Some(message) = invocation.assessment.first_blocking_message() {
        ui.colored_label(ui.visuals().error_fg_color, message);
    }
}

fn setting_row<T>(
    ui: &mut Ui,
    label: &str,
    source: CraftParamSource,
    nmr: &plotx_core::state::NmrDataset,
    reset: &mut Option<T>,
    add_control: impl FnOnce(&mut Ui) -> bool,
) -> bool {
    ui.horizontal_wrapped(|ui| {
        ui.label(label);
        let changed = add_control(ui);
        ui.weak(source_text(source, nmr));
        reset_button(reset, ui);
        changed
    })
    .inner
}

fn source_text(source: CraftParamSource, _nmr: &plotx_core::state::NmrDataset) -> String {
    match source {
        CraftParamSource::ExplicitInput => "You set".into(),
        CraftParamSource::ResultProvenance => "Selected base run".into(),
        CraftParamSource::StableDefault => "PlotX default".into(),
        CraftParamSource::InputDerived => "Derived from FID".into(),
    }
}

fn reset_button<T>(value: &mut Option<T>, ui: &mut Ui) {
    if value.is_some() && ui.small_button("Reset").clicked() {
        *value = None;
    } else if value.is_none() {
        ui.label("");
    }
}

fn next_region_id(regions: &[CraftRegion]) -> CraftRegionId {
    CraftRegionId(
        regions
            .iter()
            .map(|region| region.id.0)
            .max()
            .unwrap_or(0)
            .saturating_add(1),
    )
}
