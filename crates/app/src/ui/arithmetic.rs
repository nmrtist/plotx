use egui::DragValue;
use plotx_core::state::{PlotxApp, SpectrumArithmeticDialogState, SpectrumArithmeticOp};
use plotx_processing::arithmetic::SpectrumBinaryOp;

pub(crate) fn open_spectrum_arithmetic_dialog(app: &mut PlotxApp) {
    let targets = app.spectrum_arithmetic_targets();
    let Some(&first) = targets.first() else {
        app.session.status = "Spectrum arithmetic needs at least one 1D NMR spectrum.".into();
        return;
    };
    let a = app
        .active_dataset()
        .filter(|di| targets.contains(di))
        .unwrap_or(first);
    let b = targets.iter().copied().find(|&i| i != a).unwrap_or(a);
    app.session.ui.spectrum_arithmetic_dialog = Some(SpectrumArithmeticDialogState {
        a,
        b,
        op: SpectrumArithmeticOp::SubtractDataset,
        k: 1.0,
        constant: 0.0,
    });
}

pub(crate) fn spectrum_arithmetic_window(app: &mut PlotxApp, ctx: &egui::Context) {
    let Some(mut state) = app.session.ui.spectrum_arithmetic_dialog.take() else {
        return;
    };
    let targets = app.spectrum_arithmetic_targets();
    if targets.is_empty() {
        return;
    }
    if !targets.contains(&state.a) {
        state.a = targets[0];
    }
    if !targets.contains(&state.b) {
        state.b = targets[0];
    }

    let mut run = false;
    let mut cancel = false;
    let modal =
        super::modal(ctx, "spectrum_arithmetic_modal", super::ModalKind::Dialog).show(ctx, |ui| {
            ui.heading("Spectrum arithmetic");
            ui.separator();
            ui.label("Combine spectra or apply a constant; the result becomes a new dataset.");
            ui.add_space(4.0);
            dataset_combo(app, ui, "Spectrum A", "arith_a", &targets, &mut state.a);
            egui::ComboBox::from_label("Operation")
                .selected_text(state.op.label())
                .show_ui(ui, |ui| {
                    for op in SpectrumArithmeticOp::ALL {
                        ui.selectable_value(&mut state.op, op, op.label());
                    }
                });
            match state.op {
                SpectrumArithmeticOp::AddDataset | SpectrumArithmeticOp::SubtractDataset => {
                    dataset_combo(app, ui, "Spectrum B", "arith_b", &targets, &mut state.b);
                    ui.horizontal(|ui| {
                        ui.label("Coefficient k");
                        ui.add(DragValue::new(&mut state.k).speed(0.01).max_decimals(4));
                    });
                    ui.small("Result = A ± k·B. Tune k to null a solvent line.");
                }
                SpectrumArithmeticOp::MultiplyConstant => {
                    ui.horizontal(|ui| {
                        ui.label("Factor k");
                        ui.add(DragValue::new(&mut state.k).speed(0.01).max_decimals(4));
                    });
                }
                SpectrumArithmeticOp::AddConstant => {
                    ui.horizontal(|ui| {
                        ui.label("Constant c");
                        ui.add(DragValue::new(&mut state.constant).speed(0.1));
                    });
                }
            }
            let compat = if state.op.is_binary() {
                app.spectrum_arithmetic_compat(state.a, state.b)
            } else {
                Ok(None)
            };
            match &compat {
                Ok(None) => {}
                Ok(Some(note)) => {
                    ui.colored_label(ui.visuals().warn_fg_color, note);
                }
                Err(reason) => {
                    ui.colored_label(ui.visuals().error_fg_color, reason);
                }
            }
            ui.separator();
            ui.horizontal(|ui| {
                if ui
                    .add_enabled(compat.is_ok(), egui::Button::new("Create dataset"))
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
        match state.op {
            SpectrumArithmeticOp::AddDataset => {
                app.combine_spectra_datasets(state.a, state.b, SpectrumBinaryOp::Add, state.k)
            }
            SpectrumArithmeticOp::SubtractDataset => {
                app.combine_spectra_datasets(state.a, state.b, SpectrumBinaryOp::Subtract, state.k)
            }
            SpectrumArithmeticOp::MultiplyConstant => {
                app.scale_spectrum_dataset(state.a, state.k, 0.0)
            }
            SpectrumArithmeticOp::AddConstant => {
                app.scale_spectrum_dataset(state.a, 1.0, state.constant)
            }
        }
        return;
    }
    if !cancel && !modal.should_close() && app.session.ui.spectrum_arithmetic_dialog.is_none() {
        app.session.ui.spectrum_arithmetic_dialog = Some(state);
    }
}

fn dataset_combo(
    app: &PlotxApp,
    ui: &mut egui::Ui,
    label: &str,
    id: &str,
    targets: &[usize],
    selected: &mut usize,
) {
    ui.horizontal(|ui| {
        ui.label(label);
        egui::ComboBox::from_id_salt(id)
            .width(280.0)
            .selected_text(app.doc.datasets[*selected].display_name())
            .show_ui(ui, |ui| {
                for &di in targets {
                    ui.selectable_value(selected, di, app.doc.datasets[di].display_name());
                }
            });
    });
}
