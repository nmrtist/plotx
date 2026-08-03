use egui::{Area, Button, Order, Ui};
use egui_phosphor::regular as icon;
use plotx_core::actions::Action;
use plotx_core::state::{
    Dataset, PlotxApp, RegionId, RegionMetric, RegionSelection, TaskDockTab, Tool,
};

use super::task_card::{self, TaskCardGeometry};

pub(super) fn region_analysis_group(app: &mut PlotxApp, di: usize, ui: &mut Ui) -> bool {
    ui.label(crate::typography::headline("Region analysis"));
    let count = app
        .doc
        .datasets
        .get(di)
        .and_then(Dataset::region_analysis)
        .map_or(0, |state| state.regions.len());
    ui.small(format!("{count} regions · tools open over the canvas"));
    if ui.button("Show region tools").clicked() {
        open_task(app, di);
    }
    false
}

/// Opens or activates Regions without discarding sibling task state.
pub(crate) fn open_task(app: &mut PlotxApp, di: usize) {
    if !app
        .doc
        .datasets
        .get(di)
        .is_some_and(Dataset::supports_region_analysis)
    {
        return;
    }
    app.session.ui.region_task_dataset = app.doc.datasets.get(di).map(Dataset::resource_id);
    app.session.ui.open_task_tab(TaskDockTab::Regions);
}

pub(crate) fn render_task(app: &mut PlotxApp, host: &mut Ui) {
    finish_detached_label_edit(app);
    if !task_card::is_active(app, TaskDockTab::Regions) {
        return;
    }
    let Some(dataset_id) = app.session.ui.region_task_dataset else {
        return;
    };
    let Some(di) = app.doc.dataset_index(dataset_id) else {
        return;
    };
    if app.active_dataset() != Some(di)
        || !app
            .doc
            .datasets
            .get(di)
            .is_some_and(|dataset| dataset.supports_region_analysis())
    {
        return;
    }

    let TaskCardGeometry {
        pos,
        width,
        min_body_height,
        max_body_height,
    } = task_card::geometry(host, 300.0);
    let default_body_height = 460.0;
    let collapsed = app.session.ui.region_task_collapsed;
    let dark = host.visuals().dark_mode;
    let mut close = false;
    let mut toggle_collapse = false;
    let mut open_table = false;

    Area::new(egui::Id::new("region_task_card"))
        .order(Order::Foreground)
        .fixed_pos(pos)
        .show(host.ctx(), |ui| {
            ui.set_width(width);
            crate::ui::card_frame(dark, egui::Margin::ZERO).show(ui, |ui| {
                if task_card::tab_bar(app, TaskDockTab::Regions, ui) {
                    ui.separator();
                }
                let count = app.doc.datasets[di]
                    .region_analysis()
                    .map_or(0, |state| state.regions.len());
                ui.horizontal(|ui| {
                    ui.label(crate::typography::headline("Regions"));
                    let state = if app.session.tool == Tool::Regions {
                        if count == 0 {
                            "Drawing".to_owned()
                        } else {
                            format!("Drawing · {count}")
                        }
                    } else if count == 1 {
                        "1 region".to_owned()
                    } else {
                        format!("{count} regions")
                    };
                    ui.weak(state);
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui
                            .small_button(icon::X)
                            .on_hover_text("Close region tools")
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
                                "Expand region tools"
                            } else {
                                "Collapse region tools"
                            })
                            .clicked()
                        {
                            toggle_collapse = true;
                        }
                        if collapsed
                            && count > 0
                            && ui
                                .small_button(icon::TABLE)
                                .on_hover_text("View extracted curves")
                                .clicked()
                        {
                            open_table = true;
                        }
                    });
                });
                if !collapsed {
                    ui.separator();
                    egui::Resize::default()
                        .id_salt("region_task_body_resize")
                        .default_size([ui.available_width(), default_body_height])
                        .min_size([ui.available_width(), min_body_height])
                        .max_size([ui.available_width(), max_body_height])
                        .resizable([false, true])
                        .with_stroke(false)
                        .show(ui, |ui| {
                            region_task_body(app, di, ui);
                        });
                }
            });
        });

    if toggle_collapse {
        app.session.ui.region_task_collapsed = !collapsed;
    }
    if open_table {
        open_region_table(app, di);
    }
    if close {
        app.session.ui.close_task_tab(TaskDockTab::Regions);
        if app.session.tool == Tool::Regions {
            app.set_tool(Tool::BrowseZoom);
        }
    }
}

fn region_task_body(app: &mut PlotxApp, di: usize, ui: &mut Ui) {
    let dataset_id = app.doc.datasets[di].resource_id();
    let drawing = app.session.tool == Tool::Regions;
    if drawing {
        ui.label("Drag across a signal to add a region.");
        ui.small("Drag edges to resize · drag the middle to move · Esc cancels.");
    } else if ui.button("Resume drawing").clicked() {
        app.set_tool(Tool::Regions);
    }

    ui.horizontal(|ui| {
        ui.label("Measure");
        let mut metric = app.doc.datasets[di]
            .region_analysis()
            .map_or(RegionMetric::Height, |state| state.default_metric);
        let mut changed = false;
        egui::ComboBox::from_id_salt((di, "region_metric"))
            .selected_text(metric.label())
            .show_ui(ui, |ui| {
                for &m in RegionMetric::all() {
                    changed |= ui.selectable_value(&mut metric, m, m.label()).changed();
                }
            });
        if changed {
            app.set_region_default_metric(di, metric);
        }
    });
    let mut show_annotations = app.doc.datasets[di]
        .region_analysis()
        .is_some_and(|state| state.show_annotations);
    if ui
        .checkbox(&mut show_annotations, "Show regions on figure and export")
        .changed()
    {
        if let Some(state) = app.doc.datasets[di].region_analysis_mut() {
            state.show_annotations = show_annotations;
        }
        app.rebuild_canvases_for(di);
        app.mark_document_dirty();
    }

    let selected = app
        .session
        .ui
        .selected_region
        .and_then(|selection| selection.in_dataset(dataset_id));
    let mut delete_id: Option<RegionId> = None;
    let mut metric_change: Option<(RegionId, Option<RegionMetric>)> = None;
    let mut select_id: Option<RegionId> = None;
    let mut name_gained = false;
    let mut name_lost = false;
    let table_exists = app.region_table_index(di).is_some();
    // The region list gets whatever the footer leaves. Everything the footer can
    // render must be counted here, including the fit mirror, or `Resize` clips
    // the buttons below it away with no way to reach them.
    let mirror = fit_mirror_lines(app, di);
    let footer_height = 60.0
        + if drawing { 24.0 } else { 0.0 }
        + if table_exists { 44.0 } else { 0.0 }
        + if mirror.is_empty() {
            0.0
        } else {
            46.0 + mirror.len() as f32 * 16.0
        };
    let list_height = (ui.available_height() - footer_height).max(72.0);
    let axis_unit = app.doc.datasets[di].region_axis_unit().unwrap_or("");

    egui::ScrollArea::vertical()
        .max_height(list_height)
        .min_scrolled_height(list_height)
        .auto_shrink([false, false])
        .show(ui, |ui| {
            let Some(state) = app.doc.datasets[di].region_analysis_mut() else {
                return;
            };
            if state.regions.is_empty() {
                ui.weak("No regions yet — turn on Draw regions and drag across a signal.");
            }
            for region in state.regions.iter_mut() {
                ui.group(|ui| {
                    ui.set_min_width(ui.available_width());
                    ui.horizontal(|ui| {
                        let [cr, cg, cb] = region.color;
                        let (rect, _) =
                            ui.allocate_exact_size(egui::vec2(10.0, 10.0), egui::Sense::hover());
                        ui.painter()
                            .rect_filled(rect, 2.0, egui::Color32::from_rgb(cr, cg, cb));
                        let interval = if axis_unit.is_empty() {
                            format!("{:.3}–{:.3}", region.lo_min(), region.hi_max())
                        } else {
                            format!("{:.3}–{:.3} {axis_unit}", region.lo_min(), region.hi_max())
                        };
                        if ui
                            .add(Button::selectable(selected == Some(region.id), interval))
                            .clicked()
                        {
                            select_id = Some(region.id);
                        }
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui
                                .add_sized(
                                    [18.0, ui.spacing().interact_size.y],
                                    Button::new(icon::X).small(),
                                )
                                .on_hover_text("Delete region")
                                .clicked()
                            {
                                delete_id = Some(region.id);
                            }
                        });
                    });
                    ui.horizontal(|ui| {
                        ui.label("Label");
                        let response = ui.add(
                            egui::TextEdit::singleline(&mut region.name)
                                .hint_text("Use axis midpoint")
                                .desired_width(ui.available_width()),
                        );
                        if response.gained_focus() {
                            name_gained = true;
                        }
                        if response.lost_focus() {
                            name_lost = true;
                        }
                    });
                    ui.horizontal(|ui| {
                        ui.label("Measure");
                        let mut metric = region.metric;
                        egui::ComboBox::from_id_salt((region.id, "rm"))
                            .selected_text(metric.map(RegionMetric::label).unwrap_or("Default"))
                            .width(92.0)
                            .show_ui(ui, |ui| {
                                ui.selectable_value(&mut metric, None, "Default");
                                for &option in RegionMetric::all() {
                                    ui.selectable_value(&mut metric, Some(option), option.label());
                                }
                            });
                        if metric != region.metric {
                            metric_change = Some((region.id, metric));
                        }
                    });
                });
            }
        });

    if let Some(id) = select_id {
        app.session.ui.selected_region = Some(RegionSelection::new(dataset_id, id));
    }
    if name_lost {
        finish_label_edit(app, dataset_id);
    }
    if name_gained && app.session.ui.region_edit_before.is_none() {
        app.session.ui.region_edit_before = app.doc.datasets[di]
            .region_analysis()
            .map(|state| (dataset_id, state.regions.clone()));
    }
    if let Some((id, m)) = metric_change {
        app.edit_regions(di, |regions, _| {
            if let Some(r) = regions.iter_mut().find(|r| r.id == id) {
                r.metric = m;
            }
        });
    }
    if let Some(id) = delete_id {
        app.edit_regions(di, |regions, _| regions.retain(|r| r.id != id));
        if app
            .session
            .ui
            .selected_region
            .is_some_and(|selection| selection.dataset == dataset_id && selection.region == id)
        {
            app.session.ui.selected_region = None;
        }
    }

    ui.separator();
    let count = app.doc.datasets[di]
        .region_analysis()
        .map_or(0, |state| state.regions.len());
    let table = app.region_table_index(di);
    if table.is_some() {
        ui.small(format!("{} Live series table · Synced", icon::CHECK));
    }
    if drawing && ui.button("Done drawing").clicked() {
        app.set_tool(Tool::BrowseZoom);
    }
    let next = format!("View extracted curves {}", icon::ARROW_RIGHT);
    if ui
        .add_enabled_ui(count > 0, |ui| {
            let text = egui::RichText::new(next)
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
        .on_disabled_hover_text("Add at least one region to continue.")
        .clicked()
    {
        open_region_table(app, di);
    }
    if table.is_some()
        && ui
            .button("Save Snapshot")
            .on_hover_text("Save an independent copy that will not update when regions change.")
            .clicked()
    {
        app.freeze_region_table(di);
    }

    fit_mirror(app, di, &mirror, ui);
    ui.add_space(12.0);
}

fn finish_detached_label_edit(app: &mut PlotxApp) {
    let Some((dataset, _)) = app.session.ui.region_edit_before.as_ref() else {
        return;
    };
    let remains_attached = task_card::is_active(app, TaskDockTab::Regions)
        && app.session.ui.region_task_dataset == Some(*dataset)
        && app
            .doc
            .dataset_index(*dataset)
            .is_some_and(|index| app.active_dataset() == Some(index));
    if !remains_attached {
        finish_label_edit(app, *dataset);
    }
}

fn finish_label_edit(app: &mut PlotxApp, dataset: plotx_core::state::DatasetId) {
    let Some((snapshot_dataset, before)) = app.session.ui.region_edit_before.take() else {
        return;
    };
    if snapshot_dataset != dataset {
        app.session.ui.region_edit_before = Some((snapshot_dataset, before));
        return;
    }
    let Some(index) = app.doc.dataset_index(dataset) else {
        return;
    };
    let after = app.doc.datasets[index]
        .region_analysis()
        .map_or_else(Vec::new, |state| state.regions.clone());
    app.execute_action(Action::set_regions(dataset, before, after));
}

pub(crate) fn open_region_table(app: &mut PlotxApp, di: usize) {
    let created = app.region_table_index(di).is_none();
    if created {
        app.create_region_table(di);
    }
    let Some(tj) = app.region_table_index(di) else {
        return;
    };
    app.session.ui.sheet_open = None;
    if let Some(ci) = app
        .doc
        .canvases
        .iter()
        .position(|canvas| canvas.active_dataset() == Some(app.doc.datasets[tj].resource_id()))
    {
        app.reveal_board_frame(plotx_core::state::FrameRef::Page(ci));
    }
    app.focus_single(tj);
    super::curve_fit::open_task(app, tj);
    if !created {
        app.session.status = "Viewing extracted region curves.".to_owned();
    }
}

/// One summary line per fitted column of the linked series table. Computed
/// before the card lays out so the footer can reserve the height it needs.
fn fit_mirror_lines(app: &PlotxApp, di: usize) -> Vec<String> {
    let Some(tj) = app.region_table_index(di) else {
        return Vec::new();
    };
    app.doc
        .datasets
        .get(tj)
        .and_then(|d| d.as_table())
        .map(|t| {
            t.series_bindings
                .iter()
                .filter_map(|binding| {
                    let reference = binding.fit.as_ref()?;
                    let analysis = t
                        .curve_fit_analyses
                        .iter()
                        .find(|analysis| analysis.id == reference.analysis_id)?;
                    let parameter = analysis
                        .result
                        .parameters
                        .iter()
                        .filter(|parameter| {
                            parameter
                                .dataset_id
                                .as_deref()
                                .is_none_or(|id| id == reference.instance_id)
                        })
                        .find(|parameter| matches!(parameter.parameter.as_str(), "D" | "T"))?;
                    let label = if parameter.parameter == "T"
                        && matches!(
                            analysis.result.model.name.as_str(),
                            "Inversion recovery" | "Saturation recovery"
                        ) {
                        "T1"
                    } else {
                        &parameter.parameter
                    };
                    let r2 = analysis
                        .result
                        .statistics
                        .responses
                        .iter()
                        .find(|statistic| {
                            statistic.dataset_id == reference.instance_id
                                && statistic.response == reference.response
                        })?
                        .r_squared;
                    Some(format!(
                        "{}: {label} = {} · R² = {:.4}",
                        t.typed_state
                            .envelope
                            .revision
                            .snapshot
                            .schema
                            .column(binding.value_column)
                            .map_or("Value", |column| column.name.as_str()),
                        super::curve_fit::fmt_val_sigma(parameter.value, parameter.standard_error),
                        r2
                    ))
                })
                .collect()
        })
        .unwrap_or_default()
}

fn fit_mirror(app: &mut PlotxApp, di: usize, lines: &[String], ui: &mut Ui) {
    if app.region_table_index(di).is_none() {
        return;
    }
    if lines.is_empty() {
        return;
    }

    ui.separator();
    ui.label(crate::typography::headline("Fit results"));
    for line in lines {
        ui.small(line);
    }
}
