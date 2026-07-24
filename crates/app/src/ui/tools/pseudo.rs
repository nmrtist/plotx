use egui::{Button, DragValue, Ui};
use plotx_core::actions::{Action, DatasetProcessingState};
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
        if let DatasetProcessingState::Nmr2D { params, preset } = &mut after {
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
            Layout2D::Stack => "Stack (pseudo-2D 1D slices)",
        };
        ui.label(format!("Layout: {layout}"));
        matches!(n.params.layout, Layout2D::Stack)
    };

    nus_group(app, di, ui);

    let is_pseudo = app.doc.datasets[di]
        .as_nmr2d()
        .map(|n| n.is_pseudo())
        .unwrap_or(false);
    if is_pseudo {
        ui.separator();
        pseudo_group(app, di, ui);
    } else if is_stack {
        ui.separator();
        ui.small(
            "This looks like a pseudo-2D array but no indirect-axis ruler \
             (gradient list / delay list) was recovered, so series analysis is unavailable.",
        );
    }

    if !is_stack {
        super::integrate_group(app, di, ui);
    }

    super::slice_group(app, di, ui);
    false
}

/// Non-uniform-sampling controls. The reader normally recovers JEOL schedules;
/// manual entry remains available for older or malformed files.
fn nus_group(app: &mut PlotxApp, di: usize, ui: &mut Ui) {
    let Some(nus) = app.doc.datasets[di]
        .as_nmr2d()
        .and_then(|n| n.data.nus.clone())
    else {
        return;
    };
    ui.separator();
    ui.strong("Non-uniform sampling");
    let scheme = if nus.echo_antiecho {
        "echo/anti-echo (P/N)"
    } else {
        "phase-modulated"
    };
    ui.small(format!(
        "{} scheduling — {} of {} F1 increments acquired ({}).",
        nus.mode, nus.acquired, nus.grid, scheme,
    ));

    if nus.schedule.is_some() {
        ui.small("Spectrum reconstructed from the available sampling list.");
    } else {
        ui.colored_label(
            ui.visuals().warn_fg_color,
            "No valid NUS schedule was found in the data file. Paste the sampling \
             list (space/comma separated) to reconstruct the spectrum.",
        );
    }

    let text_id = ui.make_persistent_id(("nus_list", di));
    let base_id = ui.make_persistent_id(("nus_base", di));
    let err_id = ui.make_persistent_id(("nus_err", di));
    let mut text = ui.data_mut(|d| d.get_temp::<String>(text_id).unwrap_or_default());
    let mut base = ui.data_mut(|d| d.get_temp::<usize>(base_id).unwrap_or(nus.idx_base));

    ui.horizontal(|ui| {
        ui.label("Index base");
        if ui.selectable_label(base == 1, "1-based").clicked() {
            base = 1;
        }
        if ui.selectable_label(base == 0, "0-based").clicked() {
            base = 0;
        }
    });
    ui.data_mut(|d| d.insert_temp(base_id, base));

    let resp = ui.add(
        egui::TextEdit::multiline(&mut text)
            .hint_text("1 2 3 5 7 9 …")
            .desired_rows(2)
            .desired_width(f32::INFINITY),
    );
    if resp.changed() {
        ui.data_mut(|d| d.insert_temp(text_id, text.clone()));
    }

    if ui
        .add(Button::new(format!(
            "Reconstruct ({} indices)",
            nus.acquired
        )))
        .clicked()
    {
        let result = match parse_indices(&text) {
            Ok(values) => app.apply_nus_schedule(di, &values, base),
            Err(e) => Err(e),
        };
        let err = result.err().unwrap_or_default();
        ui.data_mut(|d| d.insert_temp::<String>(err_id, err));
    }
    let err = ui.data_mut(|d| d.get_temp::<String>(err_id).unwrap_or_default());
    if !err.is_empty() {
        ui.colored_label(ui.visuals().error_fg_color, err);
    }
}

fn parse_indices(text: &str) -> Result<Vec<usize>, String> {
    let mut out = Vec::new();
    for tok in text
        .split(|c: char| c.is_whitespace() || c == ',' || c == ';')
        .filter(|s| !s.is_empty())
    {
        let v: usize = tok
            .parse()
            .map_err(|_| format!("'{tok}' is not a whole number."))?;
        out.push(v);
    }
    if out.is_empty() {
        return Err("Enter the sampling indices.".into());
    }
    Ok(out)
}

fn pseudo_group(app: &mut PlotxApp, di: usize, ui: &mut Ui) {
    use plotx_core::{DosyMethod, PseudoDisplay};

    let (is_dosy, is_gradient, is_ilt, axis_summary, diff_summary, cur_display) = {
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
            n.display,
        )
    };

    if let Some(s) = &axis_summary {
        ui.small(s);
    }
    if let Some(s) = &diff_summary {
        ui.small(s);
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
                if let Some(d2) = app.doc.datasets[di].as_nmr2d_mut() {
                    d2.dosy_method = DosyMethod::MonoExp;
                }
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
                let params = app.session.ui.ilt_params;
                if let Some(d2) = app.doc.datasets[di].as_nmr2d_mut() {
                    d2.dosy_method = DosyMethod::Ilt(params);
                }
                app.set_pseudo_display(di, PseudoDisplay::DosyMap);
            }
        });
        if is_ilt {
            ui.horizontal(|ui| {
                ui.label("λ");
                ui.add(
                    DragValue::new(&mut app.session.ui.ilt_params.lambda)
                        .speed(0.001)
                        .range(1e-6..=1e3),
                );
            });
            ui.horizontal(|ui| {
                ui.label("D min");
                ui.add(
                    DragValue::new(&mut app.session.ui.ilt_params.d_min)
                        .speed(1e-12)
                        .range(1e-13..=1e-7),
                );
                ui.label("D max");
                ui.add(
                    DragValue::new(&mut app.session.ui.ilt_params.d_max)
                        .speed(1e-10)
                        .range(1e-12..=1e-6),
                );
            });
            ui.horizontal(|ui| {
                ui.label("Grid points");
                ui.add(
                    DragValue::new(&mut app.session.ui.ilt_params.n_grid)
                        .speed(1.0)
                        .range(16..=512),
                );
            });
        }
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
                .add_enabled(progress.is_none(), Button::new("Build DOSY map"))
                .clicked()
        {
            app.request_dosy_map(di);
        }
        if is_dosy
            && is_ilt
            && ui
                .add_enabled(
                    is_gradient && progress.is_none(),
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
