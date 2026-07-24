use egui::{Area, Button, Order, Ui};
use egui_phosphor::regular as icon;
use plotx_core::actions::Action;
use plotx_core::state::{Dataset, PlotxApp, RegionMetric, Tool};

use super::task_card::{self, TaskCardGeometry};

pub(super) fn region_analysis_group(app: &mut PlotxApp, di: usize, ui: &mut Ui) -> bool {
    ui.strong("Region analysis");
    let count = app
        .doc
        .datasets
        .get(di)
        .and_then(|dataset| dataset.as_nmr2d())
        .map_or(0, |series| series.regions.len());
    ui.small(format!("{count} regions · tools open over the canvas"));
    if ui.button("Show region tools").clicked() {
        open_task(app, di);
    }
    false
}

/// The one way to show the Regions card. The task cards share the same canvas
/// anchor, so opening one must retire the others.
pub(crate) fn open_task(app: &mut PlotxApp, di: usize) {
    if !app
        .doc
        .datasets
        .get(di)
        .is_some_and(Dataset::supports_region_analysis)
    {
        return;
    }
    app.session.ui.close_task_cards();
    app.session.ui.region_task_dataset = Some(di);
}

pub(crate) fn render_task(app: &mut PlotxApp, host: &mut Ui) {
    let Some(di) = app.session.ui.region_task_dataset else {
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
                let count = app.doc.datasets[di].as_nmr2d().unwrap().regions.len();
                ui.horizontal(|ui| {
                    ui.strong("Regions");
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
                                .on_hover_text("Continue to Series Table")
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
        app.session.ui.region_task_dataset = None;
        app.session.ui.region_task_collapsed = false;
        if app.session.tool == Tool::Regions {
            app.set_tool(Tool::BrowseZoom);
        }
    }
}

fn region_task_body(app: &mut PlotxApp, di: usize, ui: &mut Ui) {
    let drawing = app.session.tool == Tool::Regions;
    if drawing {
        ui.label("Drag across a signal to add a region.");
        ui.small("Drag edges to resize · drag the middle to move · Esc cancels.");
    } else if ui.button("Resume drawing").clicked() {
        app.set_tool(Tool::Regions);
    }

    ui.horizontal(|ui| {
        ui.label("Measure");
        let mut metric = app.doc.datasets[di].as_nmr2d().unwrap().region_metric;
        let mut changed = false;
        egui::ComboBox::from_id_salt((di, "region_metric"))
            .selected_text(metric.label())
            .show_ui(ui, |ui| {
                for &m in RegionMetric::all() {
                    changed |= ui.selectable_value(&mut metric, m, m.label()).changed();
                }
            });
        if changed {
            if let Some(d2) = app.doc.datasets[di].as_nmr2d_mut() {
                d2.region_metric = metric;
            }
            app.sync_region_table(di);
        }
    });

    let selected = app.session.ui.selected_region;
    let mut delete_id: Option<u64> = None;
    let mut metric_change: Option<(u64, Option<RegionMetric>)> = None;
    let mut select_id: Option<u64> = None;
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

    egui::ScrollArea::vertical()
        .max_height(list_height)
        .min_scrolled_height(list_height)
        .auto_shrink([false, false])
        .show(ui, |ui| {
            let d2 = app.doc.datasets[di].as_nmr2d_mut().unwrap();
            if d2.regions.is_empty() {
                ui.weak("No regions yet — turn on Draw regions and drag across a signal.");
            }
            for region in d2.regions.iter_mut() {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 4.0;
                    let [cr, cg, cb] = region.color;
                    let (rect, _) =
                        ui.allocate_exact_size(egui::vec2(10.0, 10.0), egui::Sense::hover());
                    ui.painter()
                        .rect_filled(rect, 2.0, egui::Color32::from_rgb(cr, cg, cb));

                    let is_sel = selected == Some(region.id);
                    if ui
                        .add_sized(
                            [104.0, ui.spacing().interact_size.y],
                            Button::selectable(
                                is_sel,
                                format!("{:.2}–{:.2}", region.lo_min(), region.hi_max()),
                            ),
                        )
                        .clicked()
                    {
                        select_id = Some(region.id);
                    }

                    let resp = ui.add(
                        egui::TextEdit::singleline(&mut region.name)
                            .hint_text("name")
                            .desired_width(60.0),
                    );
                    if resp.gained_focus() {
                        name_gained = true;
                    }
                    if resp.lost_focus() {
                        name_lost = true;
                    }

                    let mut m = region.metric;
                    egui::ComboBox::from_id_salt((region.id, "rm"))
                        .selected_text(m.map(RegionMetric::label).unwrap_or("default"))
                        .width(68.0)
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut m, None, "default");
                            for &opt in RegionMetric::all() {
                                ui.selectable_value(&mut m, Some(opt), opt.label());
                            }
                        });
                    if m != region.metric {
                        metric_change = Some((region.id, m));
                    }

                    if ui
                        .add_sized(
                            [18.0, ui.spacing().interact_size.y],
                            Button::new(icon::X).small(),
                        )
                        .clicked()
                    {
                        delete_id = Some(region.id);
                    }
                });
            }
        });

    if let Some(id) = select_id {
        app.session.ui.selected_region = Some(id);
    }
    if name_gained && app.session.ui.region_edit_before.is_none() {
        app.session.ui.region_edit_before =
            Some(app.doc.datasets[di].as_nmr2d().unwrap().regions.clone());
    }
    if name_lost && let Some(before) = app.session.ui.region_edit_before.take() {
        let after = app.doc.datasets[di].as_nmr2d().unwrap().regions.clone();
        app.execute_action(Action::set_regions(
            app.doc.datasets[di].resource_id(),
            before,
            after,
        ));
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
        if app.session.ui.selected_region == Some(id) {
            app.session.ui.selected_region = None;
        }
    }

    ui.separator();
    let count = app.doc.datasets[di].as_nmr2d().unwrap().regions.len();
    let table = app.region_table_index(di);
    if table.is_some() {
        ui.small(format!("{} Live series table · Synced", icon::CHECK));
    }
    if drawing && ui.button("Done drawing").clicked() {
        app.set_tool(Tool::BrowseZoom);
    }
    let next = if table.is_some() {
        format!("Open Series Table {}", icon::ARROW_RIGHT)
    } else {
        format!("Continue to Series Table {}", icon::ARROW_RIGHT)
    };
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

pub(crate) fn open_region_table(app: &mut PlotxApp, di: usize) {
    if app.region_table_index(di).is_none() {
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
        app.session.active_canvas = Some(ci);
        app.sync_selection_to_active_canvas();
    }
    app.focus_single(tj);
    app.session.ui.region_task_dataset = None;
    app.session.ui.region_task_collapsed = false;
    super::curve_fit::open_task(app, tj);
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
    ui.strong("Fit results");
    for line in lines {
        ui.small(line);
    }
}
