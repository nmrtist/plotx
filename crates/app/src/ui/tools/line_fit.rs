use egui::{Button, Ui};
use egui_phosphor::regular as icon;
use plotx_core::state::{Dataset, LineShapeKind, PlotxApp, StoredLineFit, Tool};

use super::curve_fit::fmt_val_sigma;

pub(crate) fn line_fit_shape_id(dataset: usize) -> egui::Id {
    egui::Id::new((dataset, "line_fit_shape"))
}

pub(super) fn line_fit_group(app: &mut PlotxApp, di: usize, ui: &mut Ui) -> bool {
    if !app
        .doc
        .datasets
        .get(di)
        .is_some_and(|d| d.has_displayed_trace(None))
    {
        ui.small("Peak fitting needs a dataset with a 1D trace.");
        return false;
    }

    let shape_id = line_fit_shape_id(di);
    let mut shape: LineShapeKind = ui
        .ctx()
        .data(|d| d.get_temp(shape_id))
        .unwrap_or(LineShapeKind::Lorentzian);

    let active = app.session.tool == Tool::LineFit;
    if ui
        .selectable_label(active, format!("{}  Peak fit", icon::CHART_LINE))
        .on_hover_text("Deconvolve a region into overlapping lineshape components.")
        .clicked()
    {
        app.set_tool(if active {
            Tool::BrowseZoom
        } else {
            Tool::LineFit
        });
    }
    if active {
        ui.small(
            "Drag across a region to set the fit range · peak marks inside seed the \
             components (auto-detected when there are none).",
        );
    }

    egui::ComboBox::from_label("Shape")
        .selected_text(shape.label())
        .show_ui(ui, |ui| {
            for &kind in LineShapeKind::all() {
                ui.selectable_value(&mut shape, kind, kind.label());
            }
        });
    ui.ctx().data_mut(|d| d.insert_temp(shape_id, shape));

    let unit = app.doc.datasets[di].trace_x_unit();
    let range = app.analysis_range_for(di);
    if let Some(range) = range {
        ui.label(format!("Range: {:.3}-{:.3} {unit}", range.min, range.max));
    } else {
        ui.weak("No active plot range.");
    }

    let progress = app.line_fit_progress();
    let pending = progress.is_some();
    if ui
        .add_enabled(
            range.is_some() && !pending,
            // Same wording as the Ribbon's `RunPeakFit`: one action, one name.
            Button::new(if pending {
                "Fitting…"
            } else {
                "Run Peak Fit"
            }),
        )
        .on_disabled_hover_text(if pending {
            "A fit is running; results appear when it finishes"
        } else {
            "Select a range on a plotted spectrum first"
        })
        .clicked()
        && let Some(range) = range
        && let Err(e) = app.start_line_fit(di, range.min, range.max, shape)
    {
        app.session.status = e;
    }

    if let Some((job_dataset, elapsed)) = progress {
        let seconds = elapsed.as_secs();
        ui.horizontal(|ui| {
            ui.spinner();
            ui.label(format!("Fitting… {seconds}s"));
            if job_dataset != di
                && let Some(dataset) = app.doc.datasets.get(job_dataset)
            {
                ui.weak(format!("Running for {}", dataset.display_name()));
            }
            if ui.button("Cancel").clicked() {
                app.cancel_line_fit();
            }
        });
    }

    let fits = app.doc.datasets[di].line_fits().to_vec();
    if fits.is_empty() {
        ui.weak("No fits yet — set a range and press Run Peak Fit.");
    } else {
        ui.separator();
        let mut add_to_board: Option<u64> = None;
        let mut delete: Option<u64> = None;
        for fit in &fits {
            let title = format!(
                "{} · {:.2}-{:.2} {unit} · {} peak(s) · R² {:.4}",
                fit.shape.label(),
                fit.lo,
                fit.hi,
                fit.peaks.len(),
                fit.r2
            );
            egui::CollapsingHeader::new(title)
                .id_salt((di, "line_fit", fit.id))
                .default_open(true)
                .show(ui, |ui| {
                    fit_peak_grid(fit, di, ui);
                    ui.horizontal(|ui| {
                        if ui.button("Add result to board").clicked() {
                            add_to_board = Some(fit.id);
                        }
                        if ui
                            .small_button(format!("{}  Delete fit", icon::X))
                            .clicked()
                        {
                            delete = Some(fit.id);
                        }
                    });
                });
        }
        if let Some(id) = add_to_board
            && let Err(error) = app.add_line_fit_result_to_board(di, id)
        {
            app.session.status = error;
        }
        if let Some(id) = delete {
            app.remove_line_fit(di, id);
        }
    }

    multiplet_section(app, di, range, ui);

    false
}

fn multiplet_section(
    app: &mut PlotxApp,
    di: usize,
    range: Option<plotx_core::state::AxisRange>,
    ui: &mut Ui,
) {
    if !matches!(app.doc.datasets.get(di), Some(Dataset::Nmr(_))) {
        return;
    }
    ui.separator();
    ui.strong("Multiplets");
    if ui
        .add_enabled(range.is_some(), Button::new("Analyze multiplets"))
        .on_hover_text(
            "Group the fitted components (or peak marks) in the range and classify \
             them as s/d/t/q/dd/m with J values.",
        )
        .on_disabled_hover_text("Select a range on a plotted spectrum first")
        .clicked()
        && let Some(range) = range
    {
        match app.analyze_multiplets(di, range.min, range.max) {
            Ok(ms) => app.apply_multiplet_analysis(di, ms),
            Err(e) => app.session.status = e,
        }
    }

    let multiplets = app.doc.datasets[di].multiplets().to_vec();
    if multiplets.is_empty() {
        ui.weak("No multiplets yet — set a range and press Analyze multiplets.");
        return;
    }
    let mut delete: Option<u64> = None;
    for m in &multiplets {
        let text = m.descriptor();
        ui.horizontal(|ui| {
            ui.add(
                egui::Label::new(egui::RichText::new(&text).monospace())
                    .selectable(true)
                    .wrap(),
            );
            if ui
                .small_button(icon::COPY)
                .on_hover_text("Copy this descriptor")
                .clicked()
            {
                ui.ctx().copy_text(text.clone());
            }
            if ui.small_button(icon::X).clicked() {
                delete = Some(m.id);
            }
        });
    }
    if let Some(id) = delete {
        app.remove_multiplet(di, id);
    }
    if ui
        .small_button(format!("{}  Copy all", icon::COPY))
        .on_hover_text("Copy every descriptor as one journal-style listing")
        .clicked()
    {
        let all: Vec<String> = multiplets.iter().map(|m| m.descriptor()).collect();
        ui.ctx().copy_text(all.join(", "));
    }
}

fn fit_peak_grid(fit: &StoredLineFit, di: usize, ui: &mut Ui) {
    let has_eta = fit.peaks.iter().any(|p| p.eta.is_some());
    egui::ScrollArea::horizontal()
        .id_salt((di, "line_fit_scroll", fit.id))
        .show(ui, |ui| {
            egui::Grid::new((di, "line_fit_peaks", fit.id))
                .striped(true)
                .show(ui, |ui| {
                    ui.label("#");
                    ui.label("Position");
                    ui.label("Height");
                    ui.label("FWHM");
                    ui.label("Area");
                    if has_eta {
                        ui.label("η");
                    }
                    ui.end_row();
                    for (i, p) in fit.peaks.iter().enumerate() {
                        ui.label(format!("{}", i + 1));
                        ui.label(fmt_opt_sigma(p.position, p.position_sigma));
                        ui.label(fmt_opt_sigma(p.height, p.height_sigma));
                        ui.label(fmt_opt_sigma(p.fwhm, p.fwhm_sigma));
                        ui.label(fmt_opt_sigma(p.area, p.area_sigma));
                        if has_eta && let Some(eta) = p.eta {
                            ui.label(fmt_opt_sigma(eta, p.eta_sigma));
                        }
                        ui.end_row();
                    }
                });
        });
}

fn fmt_opt_sigma(v: f64, sigma: Option<f64>) -> String {
    match sigma {
        Some(s) => fmt_val_sigma(v, s),
        None => {
            let mag = v.abs();
            if mag != 0.0 && !(1e-2..1e4).contains(&mag) {
                format!("{v:.3e}")
            } else {
                format!("{v:.4}")
            }
        }
    }
}
