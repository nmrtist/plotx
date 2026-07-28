//! Ordered, typed processing pipelines in the shared canvas task dock.

mod editors;
mod surface;

use egui::{Button, Ui};
use egui_phosphor::regular as icon;
use plotx_core::actions::DatasetProcessingState;
use plotx_core::automation::{ResourceRef, TargetRef};
use plotx_core::state::{Dataset, DatasetId, PhaseAxis, PlotxApp};
use plotx_processing::{
    Apodization, AxisPipeline, BaselineMethod, BinParams, NormalizeMethod, PhaseParams,
    ProcessingStep, ReferenceParams, SmoothMethod, StepId, StepKind, StepSource, ZeroFill,
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

/// Why a step cannot move one slot in each direction — `None` on a side that is
/// free. Resolved once per row so the menu can disable the entry and state the
/// reason in the same place the user reached for.
#[derive(Clone, Copy)]
struct MoveBlocks {
    up: Option<&'static str>,
    down: Option<&'static str>,
}

/// What stops the step at `idx` from moving one slot in `op`'s direction, or
/// `None` when the move is legal.
///
/// Only the physical ends block a one-slot move. Domain reconciliation happens
/// atomically after the swap: an FFT may move like any other row, while steps
/// that no longer accept the value at their new position are visibly disabled.
fn move_block(steps: &[ProcessingStep], idx: usize, op: RowOp) -> Option<&'static str> {
    match op {
        RowOp::MoveUp if idx == 0 => return Some("This is already the first step."),
        RowOp::MoveUp => {}
        RowOp::MoveDown if idx + 1 >= steps.len() => {
            return Some("This is already the last step.");
        }
        RowOp::MoveDown => {}
        // Duplicating or deleting a step does not reorder the ones around it.
        RowOp::Duplicate | RowOp::Delete => return None,
    }
    None
}

pub(super) fn processing_group(_app: &mut PlotxApp, _di: usize, ui: &mut Ui) -> bool {
    ui.weak("Open Processing in the task dock.");
    false
}

pub(crate) fn render_task(app: &mut PlotxApp, ui: &mut Ui) {
    surface::render(app, ui);
}

fn panel_menu(app: &mut PlotxApp, di: usize, ui: &mut Ui) {
    ui.label("Recipe");
    if ui
        .button(format!("{}  Reset to default", icon::ARROW_ARC_LEFT))
        .clicked()
    {
        reset_to_default(app, di);
        ui.close();
    }
    if ui.button("Load scheme…").clicked() {
        crate::ui::file_dialogs::load_processing_scheme(app, di);
        ui.close();
    }
    if ui.button("Save scheme…").clicked() {
        crate::ui::file_dialogs::save_processing_scheme(app, di);
        ui.close();
    }
    if ui.button("Save as template…").clicked() {
        crate::ui::processing_templates::open_save_template_dialog(app, di);
        ui.close();
    }
    if ui.button("Apply template…").clicked() {
        crate::ui::processing_templates::open_template_browser(app, di);
        ui.close();
    }
    ui.separator();
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

fn row_menu(
    app: &mut PlotxApp,
    owner: DatasetId,
    id: StepId,
    is_fft: bool,
    blocks: MoveBlocks,
    op: &mut Option<(StepId, RowOp)>,
    ui: &mut Ui,
) {
    if ui.button("Edit").clicked() {
        app.session.ui.proc_expanded_step = Some((owner, id));
        ui.close();
    }
    if ui
        .add_enabled(!is_fft, Button::new(format!("{}  Duplicate", icon::COPY)))
        .on_disabled_hover_text("A pipeline has at most one active Time to Frequency transition.")
        .clicked()
    {
        *op = Some((id, RowOp::Duplicate));
        ui.close();
    }
    move_entry(
        ui,
        format!("{}  Move earlier", icon::ARROW_LEFT),
        blocks.up,
        id,
        RowOp::MoveUp,
        op,
    );
    move_entry(
        ui,
        format!("{}  Move later", icon::ARROW_RIGHT),
        blocks.down,
        id,
        RowOp::MoveDown,
        op,
    );
    if ui.button(format!("{}  Delete", icon::TRASH)).clicked() {
        *op = Some((id, RowOp::Delete));
        ui.close();
    }
}

/// One move entry, disabled with its reason attached to the entry itself.
///
/// A blocked move used to be offered and then silently do nothing, which is
/// indistinguishable from a broken menu. The reason travels with the control the
/// user pointed at rather than only reaching the status bar at the other end of
/// the window.
fn move_entry(
    ui: &mut Ui,
    label: String,
    block: Option<&'static str>,
    id: StepId,
    kind: RowOp,
    op: &mut Option<(StepId, RowOp)>,
) {
    let entry = ui.add_enabled(block.is_none(), Button::new(label));
    if let Some(reason) = block {
        entry.on_disabled_hover_text(reason);
    } else if entry.clicked() {
        *op = Some((id, kind));
        ui.close();
    }
}

fn add_step_menu(app: &mut PlotxApp, di: usize, axis: PhaseAxis, ui: &mut Ui) {
    let dataset = &app.doc.datasets[di];
    let input_domain = match dataset {
        Dataset::Nmr(dataset) => dataset.data.domain,
        Dataset::Nmr2D(dataset) => dataset.data.domain,
        Dataset::Table(_) | Dataset::Electrophysiology(_) | Dataset::Afm(_) => return,
    };
    let Some(pipeline) = dataset.axis_pipeline(axis) else {
        return;
    };
    let has_fft = pipeline
        .steps
        .iter()
        .any(|step| matches!(step.kind, StepKind::Fft));
    let output_domain = pipeline.output_domain(input_domain).unwrap_or(input_domain);
    ui.menu_button(format!("{}  Add step", icon::PLUS), |ui| {
        if input_domain == plotx_io::Domain::Time {
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
            if !has_fft && ui.button("FFT · Time to Frequency").clicked() {
                add_step(app, di, axis, StepKind::Fft);
                ui.close();
            }
            ui.separator();
        }
        if output_domain == plotx_io::Domain::Frequency {
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
        }
    });
}

fn default_bin_params(app: &PlotxApp, dataset: usize) -> BinParams {
    let Some(Dataset::Nmr(dataset)) = app.doc.datasets.get(dataset) else {
        return BinParams::DEFAULT;
    };
    let Some(spectrum) = dataset.spectrum() else {
        return BinParams::DEFAULT;
    };
    let effective_minimum = 1.5 * plotx_processing::cleanup::axis_step(&spectrum.ppm);
    BinParams {
        width: BinParams::DEFAULT.width.max(effective_minimum.next_up()),
        ..BinParams::DEFAULT
    }
}

fn action_bar(app: &mut PlotxApp, ui: &mut Ui) {
    if app.session.ui.proc_paused && app.has_pending_processing() {
        ui.horizontal(|ui| {
            ui.colored_label(ui.visuals().warn_fg_color, "Changes pending");
            if ui.button("Apply").clicked() {
                app.apply_paused_processing();
            }
        });
    }
}

fn analysis_card(app: &mut PlotxApp, di: usize, ui: &mut Ui) {
    let is_pseudo = app.doc.datasets[di]
        .as_nmr2d()
        .map(|n| n.is_pseudo())
        .unwrap_or(false);
    if !is_pseudo {
        return;
    }
    ui.small(format!(
        "{}  Pseudo-2D analysis uses Region analysis to track peaks into a series table.",
        icon::ARROW_RIGHT
    ));
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
    let owner = dataset.resource_id();
    let input_domain = match dataset {
        Dataset::Nmr(dataset) => dataset.data.domain,
        Dataset::Nmr2D(dataset) => dataset.data.domain,
        Dataset::Table(_) | Dataset::Electrophysiology(_) | Dataset::Afm(_) => return,
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
            RowOp::MoveUp | RowOp::MoveDown => {
                if let Some(reason) = move_block(&pipe.steps, idx, op) {
                    let reason = reason.to_owned();
                    app.session.status = reason.clone();
                    app.session.ui.processing_surface_feedback = Some((owner, reason));
                    return;
                }
                app.session.ui.processing_surface_feedback = None;
                let target = match op {
                    RowOp::MoveUp => idx - 1,
                    _ => idx + 1,
                };
                pipe.steps.swap(idx, target);
            }
        }
        let disabled = pipe.reconcile_domains(input_domain);
        if disabled.is_empty() {
            app.session.ui.processing_surface_feedback = None;
        } else {
            let labels = disabled
                .iter()
                .filter_map(|id| pipe.steps.iter().find(|step| step.id == *id))
                .map(|step| step.kind.label())
                .collect::<Vec<_>>()
                .join(", ");
            let message = format!(
                "Disabled {labels}: the reordered pipeline no longer provides its required input domain."
            );
            app.session.status = message.clone();
            app.session.ui.processing_surface_feedback = Some((owner, message));
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
            .position(|step| step.enabled && matches!(step.kind, StepKind::Fft));
        let at = match &kind {
            StepKind::Fft => pipe
                .steps
                .iter()
                .position(|step| step.kind.input_domain() == plotx_io::Domain::Frequency)
                .unwrap_or(pipe.steps.len()),
            kind if kind.input_domain() == plotx_io::Domain::Time => {
                fft.unwrap_or(pipe.steps.len())
            }
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

    /// `apodize, zero_fill, FFT, phase, baseline` — the factory time-domain
    /// recipe, whose shape is what the boundary rule is about.
    fn recipe() -> Vec<ProcessingStep> {
        AxisPipeline::default_1d().steps
    }

    #[test]
    fn fft_and_neighbours_can_move_like_other_steps() {
        let steps = recipe();
        let fft = steps
            .iter()
            .position(|step| matches!(step.kind, StepKind::Fft))
            .expect("the factory recipe transforms");

        assert_eq!(move_block(&steps, fft - 1, RowOp::MoveDown), None);
        assert_eq!(move_block(&steps, fft, RowOp::MoveUp), None);
        assert_eq!(move_block(&steps, fft, RowOp::MoveDown), None);
        assert_eq!(move_block(&steps, fft + 1, RowOp::MoveUp), None);
    }

    #[test]
    fn moves_within_one_domain_stay_legal() {
        let steps = recipe();
        let fft = steps
            .iter()
            .position(|step| matches!(step.kind, StepKind::Fft))
            .expect("the factory recipe transforms");

        assert_eq!(move_block(&steps, 1, RowOp::MoveUp), None);
        assert_eq!(move_block(&steps, fft + 1, RowOp::MoveDown), None);
    }

    #[test]
    fn deleting_fft_switches_canvas_data_to_time_domain() {
        let mut app = PlotxApp::new();
        app.doc
            .datasets
            .push(crate::ui::properties::fixture::time_domain_1d());

        let axis = PhaseAxis::Direct;
        let steps = app.doc.datasets[0]
            .axis_pipeline(axis)
            .expect("a time-domain acquisition has a direct-axis pipeline")
            .steps
            .clone();
        let fft = steps
            .iter()
            .find(|step| matches!(step.kind, StepKind::Fft))
            .expect("the factory recipe transforms")
            .id;
        let undo_before = app.session.undo_stack.len();

        apply_row_op(&mut app, 0, axis, fft, RowOp::Delete);

        let dataset = app.doc.datasets[0].as_nmr().unwrap();
        assert_eq!(dataset.output_domain(), plotx_io::Domain::Time);
        assert!(dataset.time_trace().is_some());
        assert!(
            dataset
                .pipeline
                .steps
                .iter()
                .filter(|step| step.enabled)
                .all(|step| step.kind.input_domain() == plotx_io::Domain::Time)
        );
        assert_eq!(app.session.undo_stack.len(), undo_before + 1);
    }

    #[test]
    fn a_legal_move_commits_exactly_one_undo_record() {
        let mut app = PlotxApp::new();
        app.doc
            .datasets
            .push(crate::ui::properties::fixture::time_domain_1d());
        let axis = PhaseAxis::Direct;
        let before = app.doc.datasets[0]
            .axis_pipeline(axis)
            .expect("pipeline")
            .steps
            .clone();
        let moved = before[1].id;
        let undo_before = app.session.undo_stack.len();

        apply_row_op(&mut app, 0, axis, moved, RowOp::MoveUp);

        let after = &app.doc.datasets[0]
            .axis_pipeline(axis)
            .expect("pipeline")
            .steps;
        assert_eq!(after[0].id, moved);
        assert_eq!(app.session.undo_stack.len(), undo_before + 1);
        assert!(app.session.ui.processing_surface_feedback.is_none());
    }

    #[test]
    fn the_ends_of_the_pipeline_say_which_end_they_are() {
        let steps = recipe();
        let last = steps.len() - 1;
        assert!(
            move_block(&steps, 0, RowOp::MoveUp).is_some_and(|reason| reason.contains("first")),
        );
        assert!(
            move_block(&steps, last, RowOp::MoveDown).is_some_and(|reason| reason.contains("last")),
        );
    }
}
