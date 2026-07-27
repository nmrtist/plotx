//! The ordered processing step-list panel: one editable pipeline per axis, with
//! an FFT anchor separating time- and frequency-domain steps.

mod editors;

use egui::{Button, Ui};
use egui_phosphor::regular as icon;
use plotx_core::actions::DatasetProcessingState;
use plotx_core::automation::{ResourceRef, TargetRef};
use plotx_core::state::{Dataset, DatasetId, PhaseAxis, PlotxApp};
use plotx_processing::{
    Apodization, AxisPipeline, BaselineMethod, BinParams, NormalizeMethod, PhaseParams,
    ProcessingStep, ReferenceParams, SmoothMethod, StepDomain, StepId, StepKind, StepSource,
    ZeroFill,
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
        Dataset::Afm(_) => "AFM".to_owned(),
    };
    (name, default)
}

fn panel_menu(app: &mut PlotxApp, di: usize, ui: &mut Ui) {
    ui.label("Advanced");
    let target = TargetRef {
        resource: ResourceRef::from(app.doc.datasets[di].resource_id()),
        component: None,
    };
    crate::ui::properties::panel::processing_advanced_section(app, &target, ui);
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

    let Some(owner) = app.doc.datasets.get(di).map(Dataset::resource_id) else {
        return;
    };
    let last = steps.len().saturating_sub(1);
    let mut op: Option<(StepId, RowOp)> = None;
    for (i, step) in steps.iter().enumerate() {
        if matches!(step.kind, StepKind::Fft) {
            fft_anchor(ui);
            continue;
        }
        row(app, di, owner, axis, step, i == 0, i == last, ui, &mut op);
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
    owner: DatasetId,
    axis: PhaseAxis,
    step: &ProcessingStep,
    first: bool,
    last: bool,
    ui: &mut Ui,
    op: &mut Option<(StepId, RowOp)>,
) {
    let id = step.id;
    // Expansion is this panel's own state and nothing else's. A search hit that
    // wants a step opened sets it in `reveal_property`, once, as the direct
    // consequence of the user activating the hit; deriving it here from the
    // property focus instead let the focus's own highlight timer collapse the
    // row again ~800 ms later — a layout change with no user action behind it —
    // and, because a focus names a property rather than a step, opened every
    // step that could carry the setting at once and asked each to scroll.
    let expanded = app.session.ui.proc_expanded_step == Some((owner, id));
    ui.horizontal(|ui| {
        ui.weak(icon::DOTS_SIX_VERTICAL);
        let target = TargetRef {
            resource: ResourceRef::from(owner),
            component: Some(plotx_core::automation::ComponentRef::ProcessingStep(id)),
        };
        crate::ui::properties::panel::processing_step_section(app, &target, ui);
        ui.label(editors::kind_icon(&step.kind));
        if ui
            .selectable_label(expanded, editors::kind_label(&step.kind))
            .clicked()
        {
            app.session.ui.proc_expanded_step = if expanded { None } else { Some((owner, id)) };
        }
        if step.source == StepSource::User {
            ui.weak("•");
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.menu_button(icon::DOTS_THREE, |ui| {
                row_menu(app, owner, id, first, last, op, ui)
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

#[allow(clippy::too_many_arguments)]
fn row_menu(
    app: &mut PlotxApp,
    owner: DatasetId,
    id: StepId,
    first: bool,
    last: bool,
    op: &mut Option<(StepId, RowOp)>,
    ui: &mut Ui,
) {
    if ui.button("Edit").clicked() {
        app.session.ui.proc_expanded_step = Some((owner, id));
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
                let params = default_bin_params(app, di);
                add_step(app, di, axis, StepKind::Bin(params));
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

fn default_bin_params(app: &PlotxApp, dataset: usize) -> BinParams {
    let Some(Dataset::Nmr(dataset)) = app.doc.datasets.get(dataset) else {
        return BinParams::DEFAULT;
    };
    let effective_minimum = 1.5 * plotx_processing::cleanup::axis_step(&dataset.spectrum.ppm);
    BinParams {
        width: BinParams::DEFAULT.width.max(effective_minimum.next_up()),
        ..BinParams::DEFAULT
    }
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

fn apply_row_op(app: &mut PlotxApp, di: usize, axis: PhaseAxis, id: StepId, op: RowOp) {
    let Some(dataset) = app.doc.datasets.get(di) else {
        return;
    };
    let before = DatasetProcessingState::from_dataset(dataset);
    let mut after = before.clone();
    let duplicate_id = if matches!(op, RowOp::Duplicate) {
        match allocate_step_id(app, di) {
            Some(id) => Some(id),
            // Only spectral datasets expose processing rows; a stale index or a
            // non-spectral dataset is a no-op, not a crash.
            None => return,
        }
    } else {
        None
    };
    if let Some(pipe) = state_pipe(&mut after, axis)
        && let Some(idx) = pipe.steps.iter().position(|s| s.id == id)
    {
        match op {
            RowOp::Duplicate => {
                let Some(duplicate_id) = duplicate_id else {
                    return;
                };
                let mut clone = pipe.steps[idx].clone();
                clone.id = duplicate_id;
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

/// Reserve a step identity from the dataset that will own it. `None` for a
/// stale index or a dataset kind with no processing pipeline.
fn allocate_step_id(app: &mut PlotxApp, di: usize) -> Option<StepId> {
    match app.doc.datasets.get_mut(di)? {
        Dataset::Nmr(dataset) => Some(dataset.allocate_step_id()),
        Dataset::Nmr2D(dataset) => Some(dataset.allocate_step_id()),
        _ => None,
    }
}

fn add_step(app: &mut PlotxApp, di: usize, axis: PhaseAxis, kind: StepKind) {
    let Some(dataset) = app.doc.datasets.get(di) else {
        return;
    };
    let before = DatasetProcessingState::from_dataset(dataset);
    let Some(id) = allocate_step_id(app, di) else {
        return;
    };
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
            .insert(at, ProcessingStep::new(id, kind, StepSource::User));
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
        app.doc.datasets[di].resource_id(),
        before,
        after,
    ));
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
            DatasetProcessingState::Nmr2D {
                params: a,
                group_delay_correct: ga,
                ..
            },
            DatasetProcessingState::Nmr2D {
                params: b,
                group_delay_correct: gb,
                ..
            },
        ) => ga == gb && a.layout == b.layout && pipe_eq(&a.f2, &b.f2) && pipe_eq(&a.f1, &b.f1),
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

#[cfg(test)]
mod tests {
    use super::*;
    use num_complex::Complex64;
    use plotx_core::state::Nmr2DDataset;
    use plotx_io::{Dim, Domain, NmrData2D, QuadMode};

    #[test]
    fn two_dimensional_group_delay_participates_in_the_default_badge() {
        let dim = |nucleus: &str| Dim {
            spectral_width_hz: 2_000.0,
            observe_freq_mhz: 400.0,
            carrier_ppm: 4.7,
            nucleus: nucleus.to_owned(),
            group_delay: 2.0,
        };
        let data = NmrData2D {
            data: vec![Complex64::new(1.0, 0.2); 32],
            rows: 4,
            cols: 8,
            domain: Domain::Time,
            direct: dim("1H"),
            indirect: dim("13C"),
            quad: QuadMode::Complex,
            indirect_conjugate: false,
            experiment: Some("hsqc".to_owned()),
            pseudo_axis: None,
            diffusion: None,
            nus: None,
            source: "default badge".to_owned(),
        };
        let mut dataset = Dataset::Nmr2D(Box::new(Nmr2DDataset::load(data)));
        assert!(is_default_processing(&dataset));
        dataset.as_nmr2d_mut().unwrap().group_delay_correct = false;
        assert!(!is_default_processing(&dataset));
    }
}
