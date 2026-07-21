use egui::{Area, Button, Order, Ui};
use egui_phosphor::regular as icon;
use plotx_core::state::{Dataset, PlotxApp, TableDataset};

use super::task_card::{self, TaskCardGeometry};

/// Mirrors `region_analysis_group`: the Secondary Side Bar summarises the
/// workflow and opens the canvas card, which owns the controls. Keeping the
/// panel here too would mean two renderers for one piece of state.
pub(super) fn curve_fit_group(app: &mut PlotxApp, di: usize, ui: &mut Ui) -> bool {
    ui.strong("Curve Fit");
    let Some(table) = app.doc.datasets.get(di).and_then(Dataset::as_table) else {
        ui.small("Curve fitting is available for data tables.");
        return false;
    };
    let curves = table.series_bindings.len();
    let fitted = fitted_count(app, di);
    ui.small(format!("{curves} curves · tools open over the canvas"));
    if fitted > 0 {
        ui.small(format!(
            "{}  {fitted} of {curves} curves fitted",
            icon::CHECK
        ));
    }
    if ui.button("Show curve fit tools").clicked() {
        open_task(app, di);
    }
    false
}

/// The one way to show the Curve Fit card. Both cards share the same canvas
/// anchor, so opening either must retire the other.
pub(crate) fn open_task(app: &mut PlotxApp, di: usize) {
    if !matches!(app.doc.datasets.get(di), Some(Dataset::Table(_))) {
        return;
    }
    ensure_curve_fit_state(app, di);
    app.session.ui.close_task_cards();
    app.session.ui.curve_fit_task_dataset = Some(di);
}

pub(crate) fn render_task(app: &mut PlotxApp, host: &mut Ui) {
    let Some(di) = app.session.ui.curve_fit_task_dataset else {
        return;
    };
    if app.active_dataset() != Some(di)
        || !matches!(app.doc.datasets.get(di), Some(Dataset::Table(_)))
    {
        return;
    }

    ensure_curve_fit_state(app, di);
    let TaskCardGeometry {
        pos,
        width,
        min_body_height,
        max_body_height,
    } = task_card::geometry(host, 280.0);
    let default_body_height = 380.0;
    let collapsed = app.session.ui.curve_fit_task_collapsed;
    let dark = host.visuals().dark_mode;
    let mut close = false;
    let mut toggle_collapse = false;

    Area::new(egui::Id::new("curve_fit_task_card"))
        .order(Order::Foreground)
        .fixed_pos(pos)
        .show(host.ctx(), |ui| {
            ui.set_width(width);
            crate::ui::card_frame(dark, egui::Margin::ZERO).show(ui, |ui| {
                let table = app.doc.datasets[di].as_table().unwrap();
                let curves = table.series_bindings.len();
                let points = table.typed_state.envelope.revision.snapshot.row_count;
                ui.horizontal(|ui| {
                    ui.strong("Curve Fit");
                    let curve_count = if curves == 1 {
                        "1 curve".to_owned()
                    } else {
                        format!("{curves} curves")
                    };
                    ui.weak(format!("{curve_count} · {points} points each"));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui
                            .small_button(icon::X)
                            .on_hover_text("Close Curve Fit")
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
                                "Expand Curve Fit"
                            } else {
                                "Collapse Curve Fit"
                            })
                            .clicked()
                        {
                            toggle_collapse = true;
                        }
                    });
                });
                if !collapsed {
                    ui.separator();
                    egui::Resize::default()
                        .id_salt("curve_fit_task_body_resize")
                        .default_size([ui.available_width(), default_body_height])
                        .min_size([ui.available_width(), min_body_height])
                        .max_size([ui.available_width(), max_body_height])
                        .resizable([false, true])
                        .with_stroke(false)
                        .show(ui, |ui| curve_fit_task_body(app, di, ui));
                }
            });
        });

    if toggle_collapse {
        app.session.ui.curve_fit_task_collapsed = !collapsed;
    }
    if close {
        app.session.ui.curve_fit_task_dataset = None;
        app.session.ui.curve_fit_task_collapsed = false;
    }
}

fn curve_fit_task_body(app: &mut PlotxApp, di: usize, ui: &mut Ui) {
    ui.small("Fit one or more x-y data curves with a mathematical model.");
    let col_count = fit_settings(app, di, ui);
    model_editor(app, ui);
    let fitted = fitted_count(app, di);
    let footer_height = 82.0 + if fitted > 0 { 24.0 } else { 0.0 };
    let results_height = (ui.available_height() - footer_height).max(72.0);

    egui::ScrollArea::vertical()
        .max_height(results_height)
        .min_scrolled_height(results_height)
        .auto_shrink([false, false])
        .show(ui, |ui| {
            if fitted == 0 {
                ui.weak("Fit results will appear here.");
            } else {
                fit_results(app, di, ui);
            }
        });

    ui.separator();
    if fitted > 0 {
        ui.small(format!(
            "{}  {fitted} of {col_count} curves fitted",
            icon::CHECK
        ));
    }
    let run = ui
        .add_enabled_ui(col_count > 0, |ui| {
            let text = egui::RichText::new("Run Fit")
                .strong()
                .color(ui.visuals().selection.stroke.color);
            ui.add_sized(
                [ui.available_width(), 30.0],
                Button::new(text)
                    .fill(ui.visuals().selection.bg_fill)
                    .stroke(egui::Stroke::NONE),
            )
        })
        .inner
        .on_disabled_hover_text("This table has no curves to fit.");
    if run.clicked() {
        run_curve_fit(app, di);
    }
    ui.add_space(12.0);
}

fn fit_settings(app: &mut PlotxApp, di: usize, ui: &mut Ui) -> usize {
    use plotx_analysis::models;

    let col_count = app.doc.datasets[di]
        .as_table()
        .unwrap()
        .series_bindings
        .len();
    let mut available_models = models::builtin_models().to_vec();
    available_models.extend(app.session.ui.fit_custom_models.iter().cloned());
    let selected_name = available_models
        .iter()
        .find(|model| model.id == app.session.ui.fit_model)
        .map(|model| model.name.as_str())
        .unwrap_or("Select model");
    let mut chosen = app.session.ui.fit_model.clone();
    ui.strong("Model");
    ui.horizontal(|ui| {
        ui.label("Model");
        egui::ComboBox::from_id_salt((di, "curve_fit_model"))
            .selected_text(selected_name)
            .show_ui(ui, |ui| {
                for model in &available_models {
                    ui.selectable_value(&mut chosen, model.id.clone(), &model.name);
                }
            });
    });
    if chosen != app.session.ui.fit_model {
        app.session.ui.fit_model = chosen;
    }
    if let Some(model) = available_models
        .iter()
        .find(|model| model.id == app.session.ui.fit_model)
    {
        ui.small(&model.summary);
    }
    let mut edit_model = false;
    let mut clone_model = false;
    let mut delete_model = false;
    ui.horizontal(|ui| {
        if ui.small_button("Clone").clicked() {
            clone_model = true;
        }
        let custom = app
            .session
            .ui
            .fit_custom_models
            .iter()
            .any(|model| model.id == app.session.ui.fit_model);
        if ui
            .add_enabled(custom, Button::new("Edit custom model"))
            .clicked()
        {
            edit_model = true;
        }
        if ui.add_enabled(custom, Button::new("Delete")).clicked() {
            delete_model = true;
        }
    });
    if (clone_model || edit_model)
        && let Some(mut model) = available_models
            .iter()
            .find(|model| model.id == app.session.ui.fit_model)
            .cloned()
    {
        if clone_model {
            model.id = uuid::Uuid::new_v4().to_string();
            model.revision = 1;
            model.name = format!("{} copy", model.name);
        }
        app.session.ui.fit_model_editor = serde_json::to_string_pretty(&model).ok();
        app.session.ui.fit_model_editor_status.clear();
    }
    if delete_model {
        let id = app.session.ui.fit_model.clone();
        let result = (|| {
            let mut library = plotx_core::fit_model_library::FitModelLibrary::load()?;
            library.remove(&id)?;
            library.save()?;
            Ok::<_, plotx_core::fit_model_library::ModelLibraryError>(library)
        })();
        match result {
            Ok(library) => {
                app.session.ui.fit_custom_models = library.models;
                app.session.ui.fit_model =
                    default_fit_model(app.doc.datasets[di].as_table().unwrap()).into();
            }
            Err(error) => app.session.status = format!("Could not delete fit model: {error}"),
        }
    }

    ui.add_space(6.0);
    ui.strong("Data");
    ui.horizontal(|ui| {
        ui.label("Fit");
        let mut all = app.session.ui.fit_all_columns;
        if ui.selectable_label(all, "All curves").clicked() {
            all = true;
        }
        if ui.selectable_label(!all, "Selected curve").clicked() {
            all = false;
        }
        app.session.ui.fit_all_columns = all;
    });

    if !app.session.ui.fit_all_columns && col_count > 0 {
        let columns: Vec<(plotx_core::data::ColumnId, String)> = app.doc.datasets[di]
            .as_table()
            .unwrap()
            .series_bindings
            .iter()
            .filter_map(|binding| {
                app.doc.datasets[di]
                    .as_table()
                    .unwrap()
                    .typed_state
                    .envelope
                    .revision
                    .snapshot
                    .schema
                    .column(binding.value_column)
                    .map(|column| (binding.value_column, column.name.clone()))
            })
            .collect();
        let mut selected = app
            .session
            .ui
            .fit_column
            .filter(|selected| columns.iter().any(|(column, _)| column == selected))
            .unwrap_or(columns[0].0);
        ui.horizontal(|ui| {
            ui.label("Curve");
            egui::ComboBox::from_id_salt((di, "curve_fit_column"))
                .selected_text(
                    columns
                        .iter()
                        .find(|(column, _)| *column == selected)
                        .map_or("", |(_, name)| name.as_str()),
                )
                .show_ui(ui, |ui| {
                    for (column, name) in &columns {
                        ui.selectable_value(&mut selected, *column, name);
                    }
                });
        });
        app.session.ui.fit_column = Some(selected);
    }

    ui.add_space(6.0);
    ui.strong("Parameters");
    if app.session.ui.fit_all_columns {
        ui.checkbox(
            &mut app.session.ui.fit_global_parameters,
            "Share all free parameters across curves",
        )
        .on_hover_text("Turn this off to fit an independent parameter set for each curve.");
    } else {
        app.session.ui.fit_global_parameters = false;
    }
    if let Some(model) = available_models
        .iter()
        .find(|model| model.id == app.session.ui.fit_model)
    {
        let text = model
            .parameters
            .iter()
            .map(|parameter| {
                let bounds = match (parameter.lower_bound, parameter.upper_bound) {
                    (Some(lower), Some(upper)) => format!(" [{lower:.3e}…{upper:.3e}]"),
                    (Some(lower), None) => format!(" [≥ {lower:.3e}]"),
                    (None, Some(upper)) => format!(" [≤ {upper:.3e}]"),
                    (None, None) => String::new(),
                };
                format!("{}{}", parameter.display_name, bounds)
            })
            .collect::<Vec<_>>()
            .join(" · ");
        ui.small(text);
    }

    ui.add_space(6.0);
    ui.strong("Fit");
    ui.horizontal(|ui| {
        use plotx_analysis::fit_model::FitSolver;
        ui.label("Solver");
        egui::ComboBox::from_id_salt((di, "curve_fit_solver"))
            .selected_text(match app.session.ui.fit_options.solver {
                FitSolver::BoundedTrustRegion => "Bounded trust region",
                FitSolver::LevenbergMarquardt => "Levenberg–Marquardt",
            })
            .show_ui(ui, |ui| {
                ui.selectable_value(
                    &mut app.session.ui.fit_options.solver,
                    FitSolver::BoundedTrustRegion,
                    "Bounded trust region",
                );
                ui.selectable_value(
                    &mut app.session.ui.fit_options.solver,
                    FitSolver::LevenbergMarquardt,
                    "Levenberg–Marquardt (unbounded)",
                );
            });
    });
    if app.session.ui.fit_options.solver == plotx_analysis::fit_model::FitSolver::LevenbergMarquardt
        && available_models
            .iter()
            .find(|model| model.id == app.session.ui.fit_model)
            .is_some_and(|model| {
                model
                    .parameters
                    .iter()
                    .any(|p| p.lower_bound.is_some() || p.upper_bound.is_some())
            })
    {
        ui.colored_label(
            ui.visuals().warn_fg_color,
            "This model bounds its parameters, so Levenberg–Marquardt will refuse to run. \
             Switch to the bounded trust region solver.",
        );
    }
    ui.horizontal(|ui| {
        use plotx_analysis::fit_model::WeightMode;
        ui.label("Weights");
        let selected = match app.session.ui.fit_options.weights {
            WeightMode::Auto => "Auto",
            WeightMode::Equal => "Equal",
            WeightMode::MeasurementSigma => "Measurement σ",
            WeightMode::Relative => "Relative",
            WeightMode::Poisson => "Poisson",
            WeightMode::Expression(_) => "Expression",
        };
        egui::ComboBox::from_id_salt((di, "curve_fit_weights"))
            .selected_text(selected)
            .show_ui(ui, |ui| {
                ui.selectable_value(
                    &mut app.session.ui.fit_options.weights,
                    WeightMode::Auto,
                    "Auto",
                );
                ui.selectable_value(
                    &mut app.session.ui.fit_options.weights,
                    WeightMode::Equal,
                    "Equal",
                );
                ui.selectable_value(
                    &mut app.session.ui.fit_options.weights,
                    WeightMode::MeasurementSigma,
                    "Measurement σ",
                );
                ui.selectable_value(
                    &mut app.session.ui.fit_options.weights,
                    WeightMode::Relative,
                    "Relative",
                );
                ui.selectable_value(
                    &mut app.session.ui.fit_options.weights,
                    WeightMode::Poisson,
                    "Poisson",
                );
            });
    });
    ui.horizontal(|ui| {
        use plotx_analysis::fit_model::RobustLoss;
        ui.label("Robust loss");
        let selected = match app.session.ui.fit_options.robust_loss {
            RobustLoss::None => "None",
            RobustLoss::Huber(_) => "Huber",
            RobustLoss::SoftL1(_) => "Soft-L1",
            RobustLoss::Cauchy(_) => "Cauchy",
        };
        egui::ComboBox::from_id_salt((di, "curve_fit_loss"))
            .selected_text(selected)
            .show_ui(ui, |ui| {
                ui.selectable_value(
                    &mut app.session.ui.fit_options.robust_loss,
                    RobustLoss::None,
                    "None",
                );
                ui.selectable_value(
                    &mut app.session.ui.fit_options.robust_loss,
                    RobustLoss::Huber(1.0),
                    "Huber",
                );
                ui.selectable_value(
                    &mut app.session.ui.fit_options.robust_loss,
                    RobustLoss::SoftL1(1.0),
                    "Soft-L1",
                );
                ui.selectable_value(
                    &mut app.session.ui.fit_options.robust_loss,
                    RobustLoss::Cauchy(1.0),
                    "Cauchy",
                );
            });
    });
    ui.horizontal(|ui| {
        ui.label("Starts");
        ui.add(egui::DragValue::new(&mut app.session.ui.fit_options.multi_start).range(1..=32));
    });
    if ui.button("Preview initial curve").clicked() {
        let model = app.session.ui.fit_model.clone();
        let all = app.session.ui.fit_all_columns;
        let Some(column) = app.session.ui.fit_column else {
            return col_count;
        };
        let global = app.session.ui.fit_global_parameters;
        let options = app.session.ui.fit_options.clone();
        app.preview_table_fit(di, &model, all, column, global, options);
    }

    if let Some(model) = available_models
        .iter()
        .find(|model| model.id == app.session.ui.fit_model)
    {
        let no_meta = app.doc.datasets[di]
            .as_table()
            .unwrap()
            .meta
            .diffusion
            .is_none();
        if model.id == plotx_analysis::models::STEJSKAL_TANNER_ID && no_meta {
            ui.small("This model needs diffusion (δ/Δ/γ) parameters, which this table lacks.");
        }
    }
    col_count
}

fn fitted_count(app: &PlotxApp, di: usize) -> usize {
    app.doc.datasets[di]
        .as_table()
        .unwrap()
        .series_bindings
        .iter()
        .filter(|binding| binding.fit.is_some())
        .count()
}

fn fit_results(app: &PlotxApp, di: usize, ui: &mut Ui) {
    let table = app.doc.datasets[di].as_table().unwrap();
    if table
        .series_bindings
        .iter()
        .any(|binding| binding.fit.is_some())
    {
        ui.separator();
        ui.strong("Fit Results");
        for binding in &table.series_bindings {
            let Some(reference) = &binding.fit else {
                continue;
            };
            let Some(analysis) = table
                .curve_fit_analyses
                .iter()
                .find(|analysis| analysis.id == reference.analysis_id)
            else {
                continue;
            };
            let params_text = analysis
                .result
                .parameters
                .iter()
                .filter(|parameter| {
                    parameter
                        .dataset_id
                        .as_deref()
                        .is_none_or(|id| id == reference.instance_id)
                })
                .map(|parameter| {
                    format!(
                        "{} = {}",
                        parameter.parameter,
                        fmt_val_sigma(parameter.value, parameter.standard_error)
                    )
                })
                .collect::<Vec<_>>()
                .join(" · ");
            let r2 = analysis
                .result
                .statistics
                .responses
                .iter()
                .find(|statistic| {
                    statistic.dataset_id == reference.instance_id
                        && statistic.response == reference.response
                })
                .map_or(f64::NAN, |statistic| statistic.r_squared);
            ui.horizontal(|ui| {
                let name = table
                    .typed_state
                    .envelope
                    .revision
                    .snapshot
                    .schema
                    .column(binding.value_column)
                    .map_or("Value", |column| column.name.as_str());
                ui.strong(name);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.small(format!("R² {r2:.4}"));
                });
            });
            ui.small(params_text);
            ui.separator();
        }
        for analysis in &table.curve_fit_analyses {
            let statistics = &analysis.result.statistics;
            ui.strong(format!("{} diagnostics", analysis.name));
            let aicc = statistics
                .aicc
                .map_or_else(|| "n/a".into(), |value| format!("{value:.3}"));
            ui.small(format!(
                "χ² = {:.5} · reduced χ² = {:.5} · df = {} · AICc = {} · BIC = {:.3} · {} iterations",
                statistics.chi_squared,
                statistics.reduced_chi_squared,
                statistics.degrees_of_freedom,
                aicc,
                statistics.bic,
                analysis.result.iterations,
            ));
            for notice in &analysis.result.notices {
                ui.colored_label(ui.visuals().warn_fg_color, notice);
            }
        }
    }
}

fn ensure_curve_fit_state(app: &mut PlotxApp, di: usize) {
    let Some(table) = app.doc.datasets.get(di).and_then(Dataset::as_table) else {
        return;
    };
    let needs_default =
        app.session.ui.fit_dataset != Some(di) || app.session.ui.fit_model.is_empty();
    if app.session.ui.fit_dataset != Some(di) {
        app.session.ui.fit_dataset = Some(di);
        app.session.ui.fit_all_columns = true;
        app.session.ui.fit_column = table
            .series_bindings
            .first()
            .map(|binding| binding.value_column);
        match plotx_core::fit_model_library::FitModelLibrary::load() {
            Ok(library) => app.session.ui.fit_custom_models = library.models,
            Err(error) => {
                app.session.ui.fit_custom_models.clear();
                app.session.status = format!("Could not load custom fit models: {error}");
            }
        }
    }
    if needs_default {
        app.session.ui.fit_model = default_fit_model(table).to_owned();
    }
}

fn model_editor(app: &mut PlotxApp, ui: &mut Ui) {
    // Take the source so the validation cache on the same struct stays
    // writable; it is put back below unless save or cancel closes the editor.
    let Some(mut source) = app.session.ui.fit_model_editor.take() else {
        return;
    };
    ui.separator();
    ui.strong("Custom model editor (.plotxfit JSON)");
    ui.add(
        egui::TextEdit::multiline(&mut source)
            .code_editor()
            .desired_rows(12)
            .desired_width(f32::INFINITY),
    );
    // Parsing and compiling the DSL every frame is wasted work while the text
    // is unchanged, so the result is cached against the exact source.
    let stale = app
        .session
        .ui
        .fit_model_editor_validation
        .as_ref()
        .is_none_or(|validation| validation.source != source);
    if stale {
        let result = serde_json::from_str::<plotx_analysis::fit_model::FitModelDefinition>(&source)
            .map_err(|error| error.to_string())
            .and_then(|model| {
                plotx_analysis::fit_model::CompiledModel::compile(model.clone())
                    .map(|compiled| (model, compiled.unknown_symbols().to_vec()))
                    .map_err(|error| error.to_string())
            });
        app.session.ui.fit_model_editor_validation = Some(plotx_core::state::FitEditorValidation {
            source: source.clone(),
            result,
        });
    }
    let validation = app
        .session
        .ui
        .fit_model_editor_validation
        .as_ref()
        .map(|validation| validation.result.clone())
        .unwrap_or_else(|| Err("model has not been validated".into()));
    match &validation {
        Ok((_, unknown)) if unknown.is_empty() => {
            ui.small("Valid model · all symbols have declared roles");
        }
        Ok((_, unknown)) => {
            ui.colored_label(
                ui.visuals().warn_fg_color,
                format!("Classify symbols: {}", unknown.join(", ")),
            );
        }
        Err(error) => {
            ui.colored_label(ui.visuals().error_fg_color, error);
        }
    }
    let mut save = false;
    let mut cancel = false;
    ui.horizontal(|ui| {
        if ui
            .add_enabled(
                validation
                    .as_ref()
                    .is_ok_and(|(_, unknown)| unknown.is_empty()),
                Button::new("Save to model library"),
            )
            .clicked()
        {
            save = true;
        }
        if ui.button("Cancel").clicked() {
            cancel = true;
        }
    });
    let mut keep_open = true;
    if save && let Ok((model, _)) = validation {
        let id = model.id.clone();
        let result = (|| {
            // A load failure must abort the save: falling back to an empty
            // default library here would overwrite every other custom model.
            let mut library = plotx_core::fit_model_library::FitModelLibrary::load()?;
            if library.models.iter().any(|existing| existing.id == id) {
                library.update_as_new_revision(model)?;
            } else {
                library.add(model)?;
            }
            library.save()?;
            Ok::<_, plotx_core::fit_model_library::ModelLibraryError>(library)
        })();
        match result {
            Ok(library) => {
                app.session.ui.fit_custom_models = library.models;
                app.session.ui.fit_model = id;
                app.session.ui.fit_model_editor_status.clear();
                keep_open = false;
            }
            Err(error) => app.session.ui.fit_model_editor_status = error.to_string(),
        }
    } else if cancel {
        app.session.ui.fit_model_editor_status.clear();
        keep_open = false;
    }
    if keep_open {
        app.session.ui.fit_model_editor = Some(source);
    } else {
        app.session.ui.fit_model_editor_validation = None;
    }
    if !app.session.ui.fit_model_editor_status.is_empty() {
        ui.colored_label(
            ui.visuals().error_fg_color,
            &app.session.ui.fit_model_editor_status,
        );
    }
}

pub(super) fn run_curve_fit(app: &mut PlotxApp, di: usize) {
    ensure_curve_fit_state(app, di);
    let Some(table) = app.doc.datasets.get(di).and_then(Dataset::as_table) else {
        return;
    };
    if table.series_bindings.is_empty() {
        return;
    }
    let model = app.session.ui.fit_model.clone();
    let all = app.session.ui.fit_all_columns;
    let Some(column) = app.session.ui.fit_column else {
        return;
    };
    let global = app.session.ui.fit_global_parameters;
    let options = app.session.ui.fit_options.clone();
    app.fit_table_columns(di, &model, all, column, global, options);
}

/// A sensible default preset for a freshly opened table: DOSY meta → Stejskal–
/// Tanner; a delay ruler → inversion recovery; otherwise a mono-exponential.
fn default_fit_model(table: &TableDataset) -> &'static str {
    use plotx_analysis::models;
    if table.meta.diffusion.is_some() {
        return models::STEJSKAL_TANNER_ID;
    }
    let label = table
        .x_binding
        .and_then(|column| {
            table
                .typed_state
                .envelope
                .revision
                .snapshot
                .schema
                .column(column)
        })
        .map(|column| column.name.to_ascii_lowercase())
        .unwrap_or_default();
    if label.contains("delay") || label.contains("tau") || label.contains('τ') {
        models::INVERSION_RECOVERY_ID
    } else {
        models::MONO_EXPONENTIAL_ID
    }
}

/// `value ± sigma`, switching to scientific notation for very small or large
/// magnitudes (e.g. a diffusion coefficient) and fixed decimals otherwise.
pub(crate) fn fmt_val_sigma(v: f64, s: f64) -> String {
    let mag = v.abs();
    if mag != 0.0 && !(1e-2..1e4).contains(&mag) {
        format!("{v:.3e} ± {s:.1e}")
    } else {
        format!("{v:.4} ± {s:.4}")
    }
}
