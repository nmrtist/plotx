use egui::DragValue;
use plotx_core::state::{
    AlignOutcome, AlignSpectraDialogState, AlignTargetMode, Dataset, PlotxApp,
};

pub(crate) fn open_align_spectra_dialog(app: &mut PlotxApp) {
    if !app.can_align_spectra() {
        app.session.status = "Alignment needs at least two 1D NMR spectra.".into();
        return;
    }
    let Some(di) = app.align_reference() else {
        return;
    };
    let (lo, hi) = app
        .analysis_range_for(di)
        .map(|r| (r.min, r.max))
        .or_else(|| {
            app.doc
                .datasets
                .get(di)
                .and_then(Dataset::as_nmr)
                .map(|n| n.spectrum.ppm_bounds())
        })
        .unwrap_or((0.0, 1.0));
    app.session.ui.align_spectra_dialog = Some(AlignSpectraDialogState {
        lo,
        hi,
        custom_target: false,
        target_ppm: 0.0,
        plan: None,
        history_mark: (0, 0),
    });
}

pub(crate) fn align_spectra_window(app: &mut PlotxApp, ctx: &egui::Context) {
    let Some(mut state) = app.session.ui.align_spectra_dialog.take() else {
        return;
    };
    if !app.can_align_spectra() {
        return;
    }

    let mut run = false;
    let mut cancel = false;
    let modal =
        super::modal(ctx, "align_spectra_modal", super::ModalKind::Dialog).show(ctx, |ui| {
            ui.heading("Align spectra");
            ui.separator();
            ui.label(
                "Shift each spectrum so its tallest peak in the window lands on the target \
                 position, through its referencing step.",
            );
            ui.add_space(4.0);
            let mut changed = false;
            ui.horizontal(|ui| {
                ui.label("Window (ppm)");
                changed |= ui
                    .add(DragValue::new(&mut state.lo).speed(0.01).max_decimals(4))
                    .changed();
                ui.label("to");
                changed |= ui
                    .add(DragValue::new(&mut state.hi).speed(0.01).max_decimals(4))
                    .changed();
            });
            let reference = app.align_reference();
            let reference_name = reference
                .map(|di| app.doc.datasets[di].display_name())
                .unwrap_or_default();
            ui.horizontal(|ui| {
                changed |= ui
                    .radio_value(
                        &mut state.custom_target,
                        false,
                        format!("Peak of {reference_name}"),
                    )
                    .changed();
                changed |= ui
                    .radio_value(&mut state.custom_target, true, "Target (ppm)")
                    .changed();
                if state.custom_target {
                    changed |= ui
                        .add(
                            DragValue::new(&mut state.target_ppm)
                                .speed(0.01)
                                .max_decimals(4),
                        )
                        .changed();
                }
            });

            let mark = (app.session.undo_stack.len(), app.session.redo_stack.len());
            let scope = app.align_scope();
            let stale = state.plan.as_ref().is_none_or(|p| {
                p.reference != reference
                    || !p.rows.iter().map(|r| r.dataset).eq(scope.iter().copied())
            });
            if changed || stale || state.history_mark != mark {
                let mode = if state.custom_target {
                    AlignTargetMode::Custom(state.target_ppm)
                } else {
                    AlignTargetMode::ReferencePeak
                };
                state.plan = Some(app.plan_spectrum_alignment(state.lo, state.hi, mode));
                state.history_mark = mark;
            }
            let plan = state.plan.as_ref().expect("plan computed above");

            ui.separator();
            egui::ScrollArea::vertical()
                .max_height(240.0)
                .show(ui, |ui| {
                    egui::Grid::new("align_rows").striped(true).show(ui, |ui| {
                        ui.strong("Dataset");
                        ui.strong("Peak (ppm)");
                        ui.strong("Shift (ppm)");
                        ui.end_row();
                        for row in &plan.rows {
                            let name = app
                                .doc
                                .datasets
                                .get(row.dataset)
                                .map(Dataset::display_name)
                                .unwrap_or_default();
                            ui.label(name);
                            match &row.outcome {
                                AlignOutcome::Peak { ppm, shift } => {
                                    ui.label(format!("{ppm:.4}"));
                                    match shift {
                                        Some(s) => ui.label(format!("{s:+.4}")),
                                        None => ui.weak("—"),
                                    };
                                }
                                AlignOutcome::Skip(reason) => {
                                    ui.colored_label(ui.visuals().warn_fg_color, "Skipped");
                                    ui.weak(reason);
                                }
                            }
                            ui.end_row();
                        }
                    });
                });
            if plan.target_ppm.is_none() {
                ui.colored_label(
                    ui.visuals().warn_fg_color,
                    "The reference spectrum has no peak in the window; pick a target ppm.",
                );
            }
            let pending = app.has_pending_processing();
            if pending {
                ui.colored_label(
                    ui.visuals().warn_fg_color,
                    "Resolve the paused processing edit in the panel before aligning.",
                );
            }
            ui.separator();
            let can_run = plan.target_ppm.is_some() && plan.shift_count() >= 1 && !pending;
            ui.horizontal(|ui| {
                if ui
                    .add_enabled(can_run, egui::Button::new("Align"))
                    .clicked()
                {
                    run = true;
                }
                if ui.button("Cancel").clicked() {
                    cancel = true;
                }
            });
        });

    if run {
        let mode = if state.custom_target {
            AlignTargetMode::Custom(state.target_ppm)
        } else {
            AlignTargetMode::ReferencePeak
        };
        let plan = app.plan_spectrum_alignment(state.lo, state.hi, mode);
        app.apply_spectrum_alignment(&plan);
        return;
    }
    if !cancel && !modal.should_close() && app.session.ui.align_spectra_dialog.is_none() {
        app.session.ui.align_spectra_dialog = Some(state);
    }
}
