//! The Statistics task card for a data table. Mirrors the Curve Fit card: the
//! Secondary Side Bar summarises the workflow and opens a floating canvas card
//! that owns the controls. The card walks the user from a plain-language
//! question to data roles, options, an early feasibility check, and a persisted
//! result list — without requiring any statistics vocabulary to begin.

use egui::{Area, Button, Order, Ui};
use egui_phosphor::regular as icon;
use plotx_core::state::{Dataset, PlotxApp, StatDraft};

use super::statistics_config::{self, column_names};
use super::task_card::{self, TaskCardGeometry};

/// Sidebar group: a one-line summary plus a button that opens the card.
pub(super) fn statistics_group(app: &mut PlotxApp, di: usize, ui: &mut Ui) -> bool {
    ui.strong("Statistics");
    let Some(table) = app.doc.datasets.get(di).and_then(Dataset::as_table) else {
        ui.small("Statistics is available for data tables.");
        return false;
    };
    let columns = table.numeric_analysis_columns().len();
    let runs = table.statistics.len();
    ui.small(format!("{columns} columns · tools open over the canvas"));
    if runs > 0 {
        ui.small(format!("{}  {runs} saved result(s)", icon::CHECK));
    }
    if ui.button("Show statistics tools").clicked() {
        open_task(app, di);
    }
    false
}

/// The one way to show the Statistics card. Shares the canvas anchor with the
/// other task cards, so opening it retires them.
pub(crate) fn open_task(app: &mut PlotxApp, di: usize) {
    if !matches!(app.doc.datasets.get(di), Some(Dataset::Table(_))) {
        return;
    }
    ensure_draft(app, di);
    app.session.ui.close_task_cards();
    app.session.ui.stat_task_dataset = Some(di);
}

pub(crate) fn render_task(app: &mut PlotxApp, host: &mut Ui) {
    let Some(di) = app.session.ui.stat_task_dataset else {
        return;
    };
    if app.active_dataset() != Some(di)
        || !matches!(app.doc.datasets.get(di), Some(Dataset::Table(_)))
    {
        return;
    }
    ensure_draft(app, di);

    let TaskCardGeometry {
        pos,
        width,
        min_body_height,
        max_body_height,
    } = task_card::geometry(host, 320.0);
    let default_body_height = 460.0;
    let collapsed = app.session.ui.stat_task_collapsed;
    let dark = host.visuals().dark_mode;
    let mut close = false;
    let mut toggle_collapse = false;

    Area::new(egui::Id::new("statistics_task_card"))
        .order(Order::Foreground)
        .fixed_pos(pos)
        .show(host.ctx(), |ui| {
            ui.set_width(width);
            crate::ui::card_frame(dark, egui::Margin::ZERO).show(ui, |ui| {
                let table = app.doc.datasets[di].as_table().unwrap();
                let columns = table.numeric_analysis_columns().len();
                let points = table.typed_state.envelope.revision.snapshot.row_count;
                ui.horizontal(|ui| {
                    ui.strong("Statistics");
                    ui.weak(format!("{columns} columns · {points} rows"));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui
                            .small_button(icon::X)
                            .on_hover_text("Close Statistics")
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
                                "Expand Statistics"
                            } else {
                                "Collapse Statistics"
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
                        .id_salt("statistics_task_body_resize")
                        .default_size([ui.available_width(), default_body_height])
                        .min_size([ui.available_width(), min_body_height])
                        .max_size([ui.available_width(), max_body_height])
                        .resizable([false, true])
                        .with_stroke(false)
                        .show(ui, |ui| statistics_task_body(app, di, ui));
                }
            });
        });

    if toggle_collapse {
        app.session.ui.stat_task_collapsed = !collapsed;
    }
    if close {
        app.session.ui.stat_task_dataset = None;
        app.session.ui.stat_task_collapsed = false;
    }
}

fn statistics_task_body(app: &mut PlotxApp, di: usize, ui: &mut Ui) {
    let names = column_names(app, di);
    let mut draft = app.session.ui.stat_draft.clone().unwrap_or_else(|| {
        StatDraft::new(
            di,
            &names.iter().map(|(column, _)| *column).collect::<Vec<_>>(),
        )
    });

    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            let before_edit = draft.clone();
            statistics_config::question_picker(&mut draft, ui);
            ui.separator();
            statistics_config::roles_and_options(app, &mut draft, &names, ui);
            // A missing-value confirmation covers one specific selection of
            // cells; changing the question or the column roles withdraws it.
            if !draft.selects_same_data(&before_edit) {
                draft.exclusion_confirmed = false;
            }

            let preflight = app.statistics_preflight(&draft);
            statistics_config::feasibility(&mut draft, &preflight, ui);

            let can_run = preflight.role_error.is_none()
                && (!preflight.needs_confirmation() || draft.exclusion_confirmed);
            let run = ui
                .add_enabled_ui(can_run, |ui| {
                    let text = egui::RichText::new(format!("Run {}", draft.question.formal_name()))
                        .strong()
                        .color(ui.visuals().selection.stroke.color);
                    ui.add_sized(
                        [ui.available_width(), 30.0],
                        Button::new(text)
                            .fill(ui.visuals().selection.bg_fill)
                            .stroke(egui::Stroke::NONE),
                    )
                })
                .inner;
            if run.clicked()
                && let Err(error) = app.run_statistics(&draft)
            {
                app.session.status = error;
            }

            ui.add_space(8.0);
            ui.separator();
            statistics_results(app, di, ui);
        });

    app.session.ui.stat_draft = Some(draft);
}

/// The persisted results list: newest first, each with its answer-first
/// headline, the full checkable detail, and copy / add-to-board / delete.
fn statistics_results(app: &mut PlotxApp, di: usize, ui: &mut Ui) {
    let analyses = app
        .doc
        .datasets
        .get(di)
        .and_then(Dataset::as_table)
        .map(|table| table.statistics.as_slice())
        .unwrap_or_default();
    if analyses.is_empty() {
        ui.weak("Results you run are saved here and in the project.");
        return;
    }
    ui.strong("Saved results");
    let mut copy: Option<String> = None;
    let mut add_to_board: Option<u64> = None;
    let mut delete: Option<u64> = None;

    for analysis in analyses.iter().rev() {
        ui.push_id(analysis.id, |ui| {
            ui.group(|ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.strong(&analysis.title);
                });
                ui.small(plotx_core::state::headline(analysis));
                egui::CollapsingHeader::new("Details and full numbers")
                    .id_salt(("stat_detail", analysis.id))
                    .show(ui, |ui| {
                        ui.small(&analysis.configuration);
                        if let Some(note) = &analysis.data_note {
                            ui.colored_label(ui.visuals().warn_fg_color, note);
                        }
                        for line in plotx_core::state::detail_lines(analysis) {
                            ui.small(line);
                        }
                    });
                ui.horizontal(|ui| {
                    if ui
                        .small_button(format!("{}  Copy", icon::COPY))
                        .on_hover_text("Copy the full labelled result")
                        .clicked()
                    {
                        copy = Some(plotx_core::state::report_text(analysis));
                    }
                    if analysis.outcome.supports_table()
                        && ui
                            .small_button(format!("{}  Add table to board", icon::TABLE))
                            .on_hover_text("Create a data table from this result")
                            .clicked()
                    {
                        add_to_board = Some(analysis.id);
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui
                            .small_button(icon::TRASH)
                            .on_hover_text("Delete this result")
                            .clicked()
                        {
                            delete = Some(analysis.id);
                        }
                    });
                });
            });
        });
    }

    if let Some(text) = copy {
        ui.ctx().copy_text(text);
        app.session.status = "Copied the statistics result.".to_owned();
    }
    if let Some(id) = add_to_board
        && let Err(error) = app.add_statistics_result_to_board(di, id)
    {
        app.session.status = error;
    }
    if let Some(id) = delete {
        app.remove_statistics(di, id);
    }
}

/// Build a fresh draft when the card opens for a table it was not scoped to.
fn ensure_draft(app: &mut PlotxApp, di: usize) {
    let needs_new = app
        .session
        .ui
        .stat_draft
        .as_ref()
        .map(|draft| draft.dataset != di)
        .unwrap_or(true);
    if needs_new {
        let columns = column_names(app, di)
            .into_iter()
            .map(|(column, _)| column)
            .collect::<Vec<_>>();
        app.session.ui.stat_draft = Some(StatDraft::new(di, &columns));
    }
}
