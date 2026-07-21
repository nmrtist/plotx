//! The ordered processing step-list panel: one editable pipeline per axis, with
//! an FFT anchor separating time- and frequency-domain steps.

mod cleanup_editors;
mod editors;

use egui::{Button, Ui};
use egui_phosphor::regular as icon;
use plotx_core::actions::DatasetProcessingState;
use plotx_core::state::{Dataset, PhaseAxis, PlotxApp};
use plotx_processing::{
    Apodization, AutoPhaseMethod, AxisPipeline, BaselineMethod, BinParams, NormalizeMethod,
    PhaseParams, ProcessingStep, ReferenceParams, SmoothMethod, StepDomain, StepId, StepKind,
    StepSource, ZeroFill,
};

/// A structural change to a step, deferred until after the row loop so the list
/// is not mutated mid-render.
#[derive(Clone, Copy)]
enum RowOp {
    Duplicate,
    Delete,
    MoveUp,
    MoveDown,
}

pub(super) fn processing_group(app: &mut PlotxApp, di: usize, ui: &mut Ui) -> bool {
    if matches!(app.doc.datasets[di], Dataset::Table(_)) {
        return false;
    }

    header_row(app, di, ui);
    let axis = axis_selector(app, di, ui);
    ui.separator();
    step_list(app, di, axis, ui);
    action_bar(app, di, ui);
    analysis_card(app, di, ui);
    false
}

fn header_row(app: &mut PlotxApp, di: usize, ui: &mut Ui) {
    let (name, default) = badge(&app.doc.datasets[di]);
    ui.horizontal(|ui| {
        ui.strong(name);
        ui.weak(if default { "· default" } else { "· modified" });
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.menu_button(icon::DOTS_THREE_VERTICAL, |ui| panel_menu(app, di, ui));
        });
    });
}

fn badge(dataset: &Dataset) -> (String, bool) {
    let default = is_default_processing(dataset);
    let name = match dataset {
        Dataset::Nmr(n) => n.data.nucleus.clone(),
        Dataset::Nmr2D(n) => n.preset.label().to_owned(),
        Dataset::Table(_) => String::new(),
        Dataset::Electrophysiology(_) => "Patch clamp".to_owned(),
    };
    (name, default)
}

fn panel_menu(app: &mut PlotxApp, di: usize, ui: &mut Ui) {
    ui.label("Advanced");
    let mut gd = group_delay(&app.doc.datasets[di]);
    if ui.checkbox(&mut gd, "Group-delay correction").changed() {
        set_group_delay(app, di, gd);
    }
    let mut paused = app.session.ui.proc_paused;
    if ui.checkbox(&mut paused, "Pause auto-recompute").changed() {
        app.session.ui.proc_paused = paused;
        if !paused {
            app.apply_paused_processing();
        }
    }
}

fn axis_selector(app: &mut PlotxApp, di: usize, ui: &mut Ui) -> PhaseAxis {
    let axes = app.doc.datasets[di].phase_axes();
    let mut sel = app.doc.datasets[di].active_phase_axis(app.session.ui.phase_axis);
    if axes.len() > 1 {
        ui.horizontal(|ui| {
            for &a in axes {
                if ui.selectable_label(sel == a, a.label()).clicked() {
                    sel = a;
                }
            }
        });
    }
    app.session.ui.phase_axis = sel;
    sel
}

fn step_list(app: &mut PlotxApp, di: usize, axis: PhaseAxis, ui: &mut Ui) {
    let Some(steps) = app.doc.datasets[di]
        .axis_pipeline(axis)
        .map(|p| p.steps.clone())
    else {
        ui.small("This axis has no processing pipeline.");
        return;
    };

    let last = steps.len().saturating_sub(1);
    let mut op: Option<(StepId, RowOp)> = None;
    for (i, step) in steps.iter().enumerate() {
        if matches!(step.kind, StepKind::Fft) {
            fft_anchor(ui);
            continue;
        }
        row(app, di, axis, step, i == 0, i == last, ui, &mut op);
    }
    if let Some((id, o)) = op {
        apply_row_op(app, di, axis, id, o);
    }

    ui.add_space(2.0);
    add_step_menu(app, di, axis, ui);
}

fn fft_anchor(ui: &mut Ui) {
    ui.horizontal(|ui| {
        ui.add_space(2.0);
        ui.weak(icon::WAVEFORM);
        ui.strong("FFT");
        ui.weak("anchor");
    });
    ui.separator();
}

#[allow(clippy::too_many_arguments)]
fn row(
    app: &mut PlotxApp,
    di: usize,
    axis: PhaseAxis,
    step: &ProcessingStep,
    first: bool,
    last: bool,
    ui: &mut Ui,
    op: &mut Option<(StepId, RowOp)>,
) {
    let id = step.id;
    let expanded = app.session.ui.proc_expanded_step == Some(id);
    ui.horizontal(|ui| {
        ui.weak(icon::DOTS_SIX_VERTICAL);
        let mut enabled = step.enabled;
        if ui.checkbox(&mut enabled, "").changed() {
            set_enabled(app, di, axis, id, enabled);
        }
        ui.label(editors::kind_icon(&step.kind));
        if ui
            .selectable_label(expanded, editors::kind_label(&step.kind))
            .clicked()
        {
            app.session.ui.proc_expanded_step = if expanded { None } else { Some(id) };
        }
        if step.source == StepSource::User {
            ui.weak("•");
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.menu_button(icon::DOTS_THREE, |ui| {
                row_menu(app, id, first, last, op, ui)
            });
            ui.weak(editors::kind_summary(&step.kind));
        });
    });

    if expanded {
        ui.indent(("step_editor", id), |ui| {
            editors::editor(app, di, axis, step, ui);
        });
    }
}

fn row_menu(
    app: &mut PlotxApp,
    id: StepId,
    first: bool,
    last: bool,
    op: &mut Option<(StepId, RowOp)>,
    ui: &mut Ui,
) {
    if ui.button("Edit").clicked() {
        app.session.ui.proc_expanded_step = Some(id);
        ui.close();
    }
    if ui.button(format!("{}  Duplicate", icon::COPY)).clicked() {
        *op = Some((id, RowOp::Duplicate));
        ui.close();
    }
    if ui
        .add_enabled(!first, Button::new(format!("{}  Move up", icon::ARROW_UP)))
        .clicked()
    {
        *op = Some((id, RowOp::MoveUp));
        ui.close();
    }
    if ui
        .add_enabled(
            !last,
            Button::new(format!("{}  Move down", icon::ARROW_DOWN)),
        )
        .clicked()
    {
        *op = Some((id, RowOp::MoveDown));
        ui.close();
    }
    if ui.button(format!("{}  Delete", icon::TRASH)).clicked() {
        *op = Some((id, RowOp::Delete));
        ui.close();
    }
}

fn add_step_menu(app: &mut PlotxApp, di: usize, axis: PhaseAxis, ui: &mut Ui) {
    ui.menu_button(format!("{}  Add step", icon::PLUS), |ui| {
        ui.label("Time domain");
        if ui.button("Apodize").clicked() {
            add_step(
                app,
                di,
                axis,
                StepKind::Apodize(Apodization::Exponential { lb_hz: 1.0 }),
            );
            ui.close();
        }
        if ui.button("Zero fill").clicked() {
            add_step(app, di, axis, StepKind::ZeroFill(ZeroFill::Factor(2)));
            ui.close();
        }
        ui.separator();
        ui.label("Frequency domain");
        if ui.button("Phase").clicked() {
            add_step(app, di, axis, StepKind::Phase(PhaseParams::AUTO));
            ui.close();
        }
        if ui.button("Baseline").clicked() {
            add_step(app, di, axis, StepKind::Baseline(BaselineMethod::AUTO));
            ui.close();
        }
        if ui.button("Reference").clicked() {
            add_step(
                app,
                di,
                axis,
                StepKind::Reference(ReferenceParams {
                    at_ppm: 0.0,
                    target_ppm: 0.0,
                }),
            );
            ui.close();
        }
        if ui.button("Magnitude").clicked() {
            add_step(app, di, axis, StepKind::Magnitude);
            ui.close();
        }
        if matches!(app.doc.datasets[di], Dataset::Nmr(_)) {
            ui.separator();
            ui.label("Cleanup");
            if ui.button("Smoothing").clicked() {
                add_step(app, di, axis, StepKind::Smooth(SmoothMethod::DEFAULT));
                ui.close();
            }
            if ui.button("Normalize").clicked() {
                add_step(app, di, axis, StepKind::Normalize(NormalizeMethod::MaxPeak));
                ui.close();
            }
            if ui.button("Binning").clicked() {
                add_step(app, di, axis, StepKind::Bin(BinParams::DEFAULT));
                ui.close();
            }
            if ui.button("Reverse").clicked() {
                add_step(app, di, axis, StepKind::Reverse);
                ui.close();
            }
            if ui.button("Invert").clicked() {
                add_step(app, di, axis, StepKind::Invert);
                ui.close();
            }
        }
    });
}

fn action_bar(app: &mut PlotxApp, di: usize, ui: &mut Ui) {
    ui.separator();
    if app.session.ui.proc_paused && app.has_pending_processing() {
        ui.horizontal(|ui| {
            ui.colored_label(ui.visuals().warn_fg_color, "Changes pending");
            if ui.button("Apply").clicked() {
                app.apply_paused_processing();
            }
        });
    }
    ui.horizontal(|ui| {
        if ui
            .button(format!("{}  Reset to default", icon::ARROW_ARC_LEFT))
            .clicked()
        {
            reset_to_default(app, di);
        }
        if ui.button("Load scheme…").clicked() {
            crate::ui::file_dialogs::load_processing_scheme(app, di);
        }
        if ui.button("Save scheme…").clicked() {
            crate::ui::file_dialogs::save_processing_scheme(app, di);
        }
    });
    ui.horizontal(|ui| {
        if ui.button("Save as template…").clicked() {
            crate::ui::processing_templates::open_save_template_dialog(app, di);
        }
        if ui.button("Apply template…").clicked() {
            crate::ui::processing_templates::open_template_browser(app, di);
        }
    });
}

fn analysis_card(app: &mut PlotxApp, di: usize, ui: &mut Ui) {
    let is_pseudo = app.doc.datasets[di]
        .as_nmr2d()
        .map(|n| n.is_pseudo())
        .unwrap_or(false);
    if !is_pseudo {
        return;
    }
    ui.add_space(4.0);
    egui::Frame::group(ui.style()).show(ui, |ui| {
        ui.strong(format!("{}  Analysis (DOSY / T1 / T2)", icon::ARROW_RIGHT));
        ui.small("Use Region analysis to track peaks into a series table.");
    });
}

fn state_pipe(state: &mut DatasetProcessingState, axis: PhaseAxis) -> Option<&mut AxisPipeline> {
    match state {
        DatasetProcessingState::Nmr { pipeline, .. } if axis == PhaseAxis::Direct => Some(pipeline),
        DatasetProcessingState::Nmr2D { params, .. } => match axis {
            PhaseAxis::F2 => Some(&mut params.f2),
            PhaseAxis::F1 => Some(&mut params.f1),
            PhaseAxis::Direct => None,
        },
        _ => None,
    }
}

fn edit_step(
    state: &mut DatasetProcessingState,
    axis: PhaseAxis,
    id: StepId,
    mutate: impl FnOnce(&mut StepKind),
) {
    if let Some(pipe) = state_pipe(state, axis)
        && let Some(s) = pipe.steps.iter_mut().find(|s| s.id == id)
    {
        mutate(&mut s.kind);
    }
}

fn commit_kind(app: &mut PlotxApp, di: usize, axis: PhaseAxis, id: StepId, kind: StepKind) {
    let before = DatasetProcessingState::from_dataset(&app.doc.datasets[di]);
    let mut after = before.clone();
    edit_step(&mut after, axis, id, |k| *k = kind);
    app.commit_processing_edit(di, before, after);
}

fn set_enabled(app: &mut PlotxApp, di: usize, axis: PhaseAxis, id: StepId, enabled: bool) {
    let before = DatasetProcessingState::from_dataset(&app.doc.datasets[di]);
    let mut after = before.clone();
    if let Some(pipe) = state_pipe(&mut after, axis)
        && let Some(s) = pipe.steps.iter_mut().find(|s| s.id == id)
    {
        s.enabled = enabled;
    }
    app.commit_processing_edit(di, before, after);
}

/// Switch a Phase step between manual and one of the auto methods. Switching to
/// manual seeds the exact automatic terms so the trace does not jump.
fn set_phase_method(
    app: &mut PlotxApp,
    di: usize,
    axis: PhaseAxis,
    id: StepId,
    method: Option<AutoPhaseMethod>,
) {
    let before = DatasetProcessingState::from_dataset(&app.doc.datasets[di]);
    let mut after = before.clone();
    match method {
        Some(m) => {
            edit_step(&mut after, axis, id, |k| {
                if let StepKind::Phase(p) = k {
                    p.auto = Some(m);
                }
            });
        }
        None => {
            let seed = app.doc.datasets[di].automatic_phase_params(axis);
            edit_step(&mut after, axis, id, |k| {
                if let StepKind::Phase(p) = k {
                    if let Some((p0, p1, piv)) = seed {
                        p.phase0 = p0;
                        p.phase1 = p1;
                        p.pivot_frac = piv;
                    }
                    p.auto = None;
                }
            });
        }
    }
    app.commit_processing_edit(di, before, after);
}

fn apply_row_op(app: &mut PlotxApp, di: usize, axis: PhaseAxis, id: StepId, op: RowOp) {
    let before = DatasetProcessingState::from_dataset(&app.doc.datasets[di]);
    let mut after = before.clone();
    if let Some(pipe) = state_pipe(&mut after, axis)
        && let Some(idx) = pipe.steps.iter().position(|s| s.id == id)
    {
        match op {
            RowOp::Duplicate => {
                let mut clone = pipe.steps[idx].clone();
                clone.id = StepId::fresh();
                clone.source = StepSource::User;
                pipe.steps.insert(idx + 1, clone);
            }
            RowOp::Delete => {
                pipe.steps.remove(idx);
            }
            RowOp::MoveUp => {
                if idx > 0
                    && !matches!(pipe.steps[idx - 1].kind, StepKind::Fft)
                    && !matches!(pipe.steps[idx].kind, StepKind::Fft)
                {
                    pipe.steps.swap(idx, idx - 1);
                }
            }
            RowOp::MoveDown => {
                if idx + 1 < pipe.steps.len()
                    && !matches!(pipe.steps[idx + 1].kind, StepKind::Fft)
                    && !matches!(pipe.steps[idx].kind, StepKind::Fft)
                {
                    pipe.steps.swap(idx, idx + 1);
                }
            }
        }
    }
    app.commit_processing_edit(di, before, after);
}

fn add_step(app: &mut PlotxApp, di: usize, axis: PhaseAxis, kind: StepKind) {
    let before = DatasetProcessingState::from_dataset(&app.doc.datasets[di]);
    let mut after = before.clone();
    if let Some(pipe) = state_pipe(&mut after, axis) {
        let fft = pipe
            .steps
            .iter()
            .position(|s| matches!(s.kind, StepKind::Fft));
        let at = match (kind.domain(), fft) {
            (StepDomain::Time, Some(i)) => i,
            _ => pipe.steps.len(),
        };
        pipe.steps
            .insert(at, ProcessingStep::new(kind, StepSource::User));
    }
    app.commit_processing_edit(di, before, after);
}

fn reset_to_default(app: &mut PlotxApp, di: usize) {
    let Some(after) = plotx_core::project::reset_processing(&app.doc.datasets[di]) else {
        return;
    };
    app.session.ui.proc_pending = None;
    let before = DatasetProcessingState::from_dataset(&app.doc.datasets[di]);
    app.execute_action(plotx_core::actions::Action::update_dataset_processing(
        di, before, after,
    ));
}

fn set_group_delay(app: &mut PlotxApp, di: usize, on: bool) {
    match &app.doc.datasets[di] {
        Dataset::Nmr(_) => {
            let before = DatasetProcessingState::from_dataset(&app.doc.datasets[di]);
            let mut after = before.clone();
            if let DatasetProcessingState::Nmr {
                group_delay_correct,
                ..
            } = &mut after
            {
                *group_delay_correct = on;
            }
            app.commit_processing_edit(di, before, after);
        }
        Dataset::Nmr2D(_) => {
            if let Some(n) = app.doc.datasets[di].as_nmr2d_mut() {
                n.group_delay_correct = on;
            }
            app.apply_dataset_retransform(di);
            app.recompute_integrals_2d_after_processing(di);
        }
        Dataset::Table(_) => {}
        Dataset::Electrophysiology(_) => {}
    }
}

fn group_delay(dataset: &Dataset) -> bool {
    match dataset {
        Dataset::Nmr(n) => n.group_delay_correct,
        Dataset::Nmr2D(n) => n.group_delay_correct,
        Dataset::Table(_) => true,
        Dataset::Electrophysiology(_) => true,
    }
}

fn is_default_processing(dataset: &Dataset) -> bool {
    let Some(def) = plotx_core::project::reset_processing(dataset) else {
        return true;
    };
    let cur = DatasetProcessingState::from_dataset(dataset);
    match (&cur, &def) {
        (
            DatasetProcessingState::Nmr {
                pipeline: a,
                group_delay_correct: ga,
            },
            DatasetProcessingState::Nmr {
                pipeline: b,
                group_delay_correct: gb,
            },
        ) => ga == gb && pipe_eq(a, b),
        (
            DatasetProcessingState::Nmr2D { params: a, .. },
            DatasetProcessingState::Nmr2D { params: b, .. },
        ) => a.layout == b.layout && pipe_eq(&a.f2, &b.f2) && pipe_eq(&a.f1, &b.f1),
        _ => false,
    }
}

/// Structural equality of two pipelines, ignoring step ids and source tags.
fn pipe_eq(a: &AxisPipeline, b: &AxisPipeline) -> bool {
    a.steps.len() == b.steps.len()
        && a.steps
            .iter()
            .zip(&b.steps)
            .all(|(x, y)| x.kind == y.kind && x.enabled == y.enabled)
}
