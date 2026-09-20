use egui::{Button, DragValue, Ui};
use plotx_core::actions::{Action, DatasetProcessingState};
use plotx_core::settings::{MAX_ILT_LAMBDA, MIN_ILT_LAMBDA};
use plotx_core::state::PlotxApp;
use plotx_processing::{Layout2D, Preset2D};

pub(super) fn experiment_group(app: &mut PlotxApp, di: usize, ui: &mut Ui) -> bool {
    let n = app.doc.datasets[di].as_nmr2d().unwrap();
    let mut chosen = n.preset;
    egui::ComboBox::from_label("Experiment")
        .selected_text(chosen.label())
        .show_ui(ui, |ui| {
            for &p in Preset2D::all() {
                ui.selectable_value(&mut chosen, p, p.label());
            }
        });
    if chosen != n.preset {
        let before = DatasetProcessingState::from_dataset(&app.doc.datasets[di]);
        let mut after = before.clone();
        if let DatasetProcessingState::Nmr2D { params, preset, .. } = &mut after {
            *preset = chosen;
            params.layout = chosen.layout();
        }
        app.execute_action(Action::update_dataset_processing(
            app.doc.datasets[di].resource_id(),
            before,
            after,
        ));
    }

    let is_stack = {
        let n = app.doc.datasets[di].as_nmr2d().unwrap();
        let layout = match n.params.layout {
            Layout2D::Ft => "Contour (true 2D FT)",
            Layout2D::Stack
                if n.native_processed.dataset().as_raw().is_some() && n.data.nus.is_some() =>
            {
                "Acquired NUS observations (not reconstructed)"
            }
            Layout2D::Stack if n.data.nus.is_some() => "Stack (reconstructed NUS slices)",
            Layout2D::Stack => "Stack (pseudo-2D 1D slices)",
        };
        ui.label(format!("Layout: {layout}"));
        matches!(n.params.layout, Layout2D::Stack)
    };

    super::cursor_group(app, di, ui);
    nus_group(app, di, ui);

    let is_pseudo = app.doc.datasets[di]
        .as_nmr2d()
        .map(|n| n.is_pseudo())
        .unwrap_or(false);
    if is_pseudo {
        ui.separator();
        pseudo_group(app, di, ui);
    } else if is_stack && app.doc.datasets[di].as_nmr2d().unwrap().data.nus.is_none() {
        ui.separator();
        ui.small(
            "This looks like a pseudo-2D array but no indirect-axis ruler \
             (gradient list / delay list) was recovered, so series analysis is unavailable.",
        );
    }

    if !is_stack {
        super::symmetry_group(app, di, ui);
        super::integrate_group(app, di, ui);
    }

    super::slice_group(app, di, ui);
    false
}

/// Reconstruction inputs apply to the imported sampling coordinates.
fn nus_group(app: &mut PlotxApp, di: usize, ui: &mut Ui) {
    let Some(dataset) = app.doc.datasets[di].as_nmr2d() else {
        return;
    };
    let Some(nus) = &dataset.data.nus else {
        return;
    };
    ui.separator();
    ui.label(crate::typography::headline("Non-uniform sampling"));
    if let Some(warning) = &dataset.reconstruction_warning {
        ui.colored_label(ui.visuals().warn_fg_color, warning);
    }
    ui.small(format!(
        "{} observations on a {}-point indirect grid.",
        nus.acquired, nus.grid
    ));
    let mut request = dataset.nus_request.unwrap_or_default();
    let mut noise_known = request.noise_standard_deviation.is_some();
    let mut changed = ui
        .checkbox(&mut noise_known, "Override automatic noise estimate")
        .changed();
    let mut noise = request.noise_standard_deviation.unwrap_or(0.0);
    changed |= ui
        .add_enabled(
            noise_known,
            DragValue::new(&mut noise)
                .range(0.0..=f64::MAX)
                .prefix("Noise σ "),
        )
        .changed();
    ui.small("Noise is estimated automatically from acquired data. An override uses the standard deviation after the current F2 recipe; zero explicitly asserts noiseless input.");
    request.noise_standard_deviation = noise_known.then_some(noise);
    changed |= ui
        .add(
            DragValue::new(&mut request.max_iterations)
                .range(1..=2048)
                .prefix("Maximum iterations "),
        )
        .changed();
    let before = DatasetProcessingState::from_dataset(&app.doc.datasets[di]);
    let mut after = before.clone();
    if let DatasetProcessingState::Nmr2D { nus_request, .. } = &mut after {
        *nus_request = Some(request);
    }
    if changed && before != after {
        app.execute_action(Action::update_dataset_processing(
            app.doc.datasets[di].resource_id(),
            before,
            after,
        ));
    }
    ui.small("NUS reconstruction runs automatically with the F2 FFT. The F1 FFT produces the second frequency axis.");
}

fn pseudo_group(app: &mut PlotxApp, di: usize, ui: &mut Ui) {
    use plotx_core::{DosyMethod, PseudoDisplay};

    let (is_dosy, is_gradient, is_ilt, axis_summary, diff_summary, provenance_warning, cur_display) = {
        let n = app.doc.datasets[di].as_nmr2d().unwrap();
        let axis_summary = n.data.pseudo_axis.as_ref().map(|axis| {
            format!(
                "Array: {} — {} points in {} ({:?})",
                axis.name,
                axis.values.len(),
                axis.unit,
                axis.source,
            )
        });
        let diff_summary = n.data.diffusion.as_ref().map(|m| {
            format!(
                "δ = {:.3} ms, Δ = {:.1} ms, τ = {:.2} ms, Δ_eff = {:.1} ms",
                m.delta * 1e3,
                m.big_delta * 1e3,
                m.tau * 1e3,
                m.effective_delay() * 1e3,
            )
        });
        let is_gradient = n
            .data
            .pseudo_axis
            .as_ref()
            .map(|a| a.kind == plotx_io::PseudoKind::Gradient)
            .unwrap_or(false);
        (
            n.data.diffusion.is_some(),
            is_gradient,
            matches!(n.dosy_method, DosyMethod::Ilt(_)),
            axis_summary,
            diff_summary,
            n.dosy_provenance_warning
                .clone()
                .or_else(|| n.missing_selected_map_note().map(str::to_owned)),
            n.display,
        )
    };

    if let Some(s) = &axis_summary {
        ui.small(s);
    }
    if let Some(s) = &diff_summary {
        ui.small(s);
    }
    if let Some(warning) = &provenance_warning {
        ui.colored_label(ui.visuals().warn_fg_color, warning);
    }

    ui.horizontal(|ui| {
        ui.label("Show");
        let cur = cur_display;
        for (label, mode) in [
            ("Stack", PseudoDisplay::Stack),
            ("DOSY map", PseudoDisplay::DosyMap),
        ] {
            let enabled = match mode {
                PseudoDisplay::DosyMap => is_dosy,
                PseudoDisplay::Stack => true,
            };
            if ui
                .add_enabled(enabled, egui::Button::selectable(cur == mode, label))
                .clicked()
            {
                app.set_pseudo_display(di, mode);
            }
        }
    });

    if is_dosy {
        ui.separator();
        ui.horizontal(|ui| {
            ui.label("DOSY method");
            if ui.selectable_label(!is_ilt, "Per-column").clicked() && is_ilt {
                app.set_pseudo_dosy_method(di, DosyMethod::MonoExp);
                app.set_pseudo_display(di, PseudoDisplay::DosyMap);
            }
            if ui
                .add_enabled(
                    is_gradient,
                    egui::Button::selectable(is_ilt, "ILT / CONTIN"),
                )
                .on_disabled_hover_text("ILT needs a gradient-encoded ruler")
                .clicked()
                && !is_ilt
            {
                let params = app.resolve_ilt_params_for(di, app.explicit_ilt_input_for(di));
                app.set_pseudo_dosy_method(di, DosyMethod::Ilt(params));
                app.set_pseudo_display(di, PseudoDisplay::DosyMap);
            }
        });
        if is_ilt {
            // Show what this build would actually use — the explicit input if the
            // user has entered one for this dataset, otherwise what the lifecycle
            // resolves to. Only an edit here records an explicit input, so simply
            // opening the panel never turns a resolved value into an override.
            let resolved = app.resolve_ilt_params_for(di, app.explicit_ilt_input_for(di));
            let mut draft = resolved;
            let mut edited = false;
            ui.horizontal(|ui| {
                ui.label("λ");
                edited |= ui
                    .add(
                        DragValue::new(&mut draft.lambda)
                            .speed(0.001)
                            .range(MIN_ILT_LAMBDA..=MAX_ILT_LAMBDA),
                    )
                    .changed();
            });
            ui.horizontal(|ui| {
                ui.label("D min");
                edited |= ui
                    .add(
                        DragValue::new(&mut draft.d_min)
                            .speed(1e-12)
                            .range(1e-13..=1e-7),
                    )
                    .changed();
                ui.label("D max");
                edited |= ui
                    .add(
                        DragValue::new(&mut draft.d_max)
                            .speed(1e-10)
                            .range(1e-12..=1e-6),
                    )
                    .changed();
            });
            ui.horizontal(|ui| {
                ui.label("Grid points");
                edited |= ui
                    .add(DragValue::new(&mut draft.n_grid).speed(1.0).range(16..=512))
                    .changed();
            });
            if edited {
                app.set_explicit_ilt_input(di, draft);
            }
        }
    }

    let input_error = app.doc.datasets[di]
        .as_nmr2d()
        .and_then(|n| n.dosy_input_error());
    if let Some(error) = input_error {
        ui.small(error);
    }
    let progress = app
        .doc
        .datasets
        .get(di)
        .and_then(|dataset| app.session.compute.dosy_progress(dataset.resource_id()));
    ui.horizontal(|ui| {
        if is_dosy
            && !is_ilt
            && ui
                .add_enabled(
                    input_error.is_none() && progress.is_none(),
                    Button::new("Build DOSY map"),
                )
                .clicked()
        {
            app.request_dosy_map(di);
        }
        if is_dosy
            && is_ilt
            && ui
                .add_enabled(
                    is_gradient && input_error.is_none() && progress.is_none(),
                    Button::new("Build ILT DOSY map"),
                )
                .clicked()
        {
            app.request_ilt_map(di);
        }
    });
    if let Some((active_kind, elapsed)) = progress {
        ui.horizontal(|ui| {
            ui.spinner();
            ui.label(format!("Computing… {}s", elapsed.as_secs()));
            if ui.button("Cancel").clicked() {
                app.cancel_compute(di, active_kind);
            }
        });
    }
}
