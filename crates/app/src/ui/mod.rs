pub(crate) mod align;
pub(crate) mod arithmetic;
pub(crate) mod batch_workflow;
pub(crate) mod canvas;
pub(crate) mod canvas_size;
mod clipboard_figure;
#[cfg(windows)]
mod clipboard_native;
pub(crate) mod clipboard_table;
mod command_exec;
mod command_palette;
pub(crate) mod commands;
mod data_export_dialog;
mod data_sheet;
mod diagnostics;
mod export_dialog;
mod figure_typography;
pub(crate) mod file_dialogs;
mod menus;
#[cfg(target_os = "macos")]
pub(crate) mod native_menu;
mod object_inspector;
mod present;
mod primary_sidebar;
pub(crate) mod processing_templates;
mod ribbon;
mod secondary_sidebar;
mod settings_dialog;
mod shortcuts;
mod switcher;
#[cfg(not(target_os = "macos"))]
mod title_bar;
pub(crate) mod tools;
mod windows;

use data_sheet::*;
use diagnostics::*;
use egui::{Color32, Pos2, Response, Sense, Stroke, Ui, Vec2};
use export_dialog::*;
use plotx_core::actions::{Action, PendingPageLayoutEdit};
use plotx_core::export::{ExportPageScope, ExportScopeKind, ExportSettings};
use plotx_core::layout::PageLayout;
use plotx_core::operation::OperationOutcome;
use plotx_core::state::{CanvasSizeUnit, Interaction, PanelLabelStyle, PlotxApp, Selection};
pub(crate) use settings_dialog::apply_chrome_theme;
use settings_dialog::settings_window;
use shortcuts::*;
use windows::*;

pub fn render(
    app: &mut PlotxApp,
    clipboard_table_paste: &mut clipboard_table::ClipboardTablePaste,
    batch_workflow: &mut batch_workflow::AutomationUi,
    ui: &mut Ui,
    input_blocked: bool,
) {
    let ctx = ui.ctx().clone();
    clipboard_table_paste.begin_frame(app, &ctx);
    if let Some(payload) = app.poll_data_export() {
        copy_table_export(&ctx, payload);
    }
    present::sync_fullscreen(app, &ctx);
    if app.session.present_mode {
        present::render(app, ui);
        if app.session.present_mode {
            return;
        }
    }

    // Derived page-size state (auto height, stale preset ids) re-derives once
    // per frame, outside the undo stack, before anything paints.
    app.reconcile_page_fit();

    handle_palette_shortcut(app, clipboard_table_paste, &ctx);

    // Every dialog is egui::Modal-based, so its modal layer already blocks
    // background input; the chrome must NOT be disabled here (`Ui::disable`
    // repaints everything at `disabled_alpha`, washing the chrome out against
    // the window clear colour). This flag only gates the global shortcuts.
    let modal_open = input_blocked
        || app.session.ui.processing_scheme_dialog.is_some()
        || app.session.ui.processing_template_dialog.is_some()
        || app.session.ui.spectrum_arithmetic_dialog.is_some()
        || app.session.ui.align_spectra_dialog.is_some()
        || app.session.ui.command_palette.is_some()
        || app.session.ui.save_project_options
        || app.session.ui.quit_confirm
        || app.session.ui.export_options.is_some()
        || app.session.ui.data_export.is_some()
        || app.session.ui.table_import_preview.is_some()
        || app.session.ui.settings_dialog.is_some()
        || batch_workflow.is_open();
    if !modal_open {
        handle_command_shortcuts(app, clipboard_table_paste, &ctx);
        handle_escape_shortcut(app, &ctx);
        handle_rename_shortcut(app, &ctx);
        handle_fit_shortcut(app, &ctx);
        handle_focus_shortcut(app, &ctx);
        handle_delete_shortcut(app, &ctx);
    }

    let dark = ui.visuals().dark_mode;
    // The gaps between chrome cards show whatever the root panel painted, so
    // lay the workspace colour under everything first.
    ui.painter()
        .rect_filled(ui.max_rect(), 0.0, workspace_fill(dark));

    #[cfg(not(target_os = "macos"))]
    egui::Panel::top("title_bar")
        .frame(flush_frame(dark, egui::Margin::ZERO))
        .show_separator_line(false)
        .show_inside(ui, |ui| {
            title_bar::render(app, clipboard_table_paste, ui);
        });

    egui::Panel::top("ribbon")
        .frame(card_frame(
            dark,
            egui::Margin {
                left: 8,
                right: 8,
                top: 4,
                bottom: 4,
            },
        ))
        .show_separator_line(false)
        .show_inside(ui, |ui| {
            ribbon::render(app, clipboard_table_paste, ui);
        });

    let workspace_width = ui.available_width();
    render_sidebars(app, ui, dark, workspace_width);

    feedback_banner(app, ui, dark);
    render_status(app, ui, dark);

    // The sidebars above may have expanded/collapsed a Phase step this frame; put
    // the canvas into (or out of) on-plot phase mode before it paints.
    app.sync_phase_interaction();
    egui::CentralPanel::default()
        .frame(
            egui::Frame::new()
                .fill(workspace_fill(dark))
                .inner_margin(egui::Margin {
                    left: 4,
                    right: 4,
                    top: 4,
                    bottom: 2,
                }),
        )
        .show_inside(ui, |ui| {
            canvas::render_central(app, ui);
            tools::render_region_task(app, ui);
            tools::render_curve_fit_task(app, ui);
            tools::render_statistics_task(app, ui);
        });

    canvas_settings_window(app, &ctx);
    figure_typography::figure_typography_window(app, &ctx);
    panel_note_edit_window(app, &ctx);
    text_edit_window(app, &ctx);
    data_sheet_window(app, &ctx);
    save_project_window(app, &ctx);
    export_options_window(app, &ctx);
    data_export_dialog::data_export_window(app, &ctx);
    file_dialogs::table_import_preview_window(app, &ctx);
    settings_window(app, &ctx);
    command_palette::command_palette_window(app, clipboard_table_paste, &ctx);
    menus::about_window(app, &ctx);
    quit_confirm_window(app, &ctx);
    diagnostic_history_window(app, &ctx);
    file_dialogs::processing_scheme_window(app, &ctx);
    processing_templates::processing_template_window(app, &ctx);
    arithmetic::spectrum_arithmetic_window(app, &ctx);
    align::align_spectra_window(app, &ctx);
    batch_workflow.show(app, &ctx);

    handle_file_drop(app, &ctx);
    handle_close_request(app, &ctx);
    #[cfg(not(target_os = "macos"))]
    title_bar::resize_zones(&ctx);

    let now = ctx.input(|i| i.time);
    app.finish_pending_wheel_zoom(now, false);
}

fn copy_table_export(ctx: &egui::Context, payload: plotx_core::data_export::ClipboardExport) {
    #[cfg(windows)]
    {
        if clipboard_native::set_table_formats(&payload.text, payload.schema_json.as_deref())
            .is_ok()
        {
            return;
        }
    }
    ctx.copy_text(payload.text);
}

fn render_status(app: &PlotxApp, ui: &mut Ui, dark: bool) {
    egui::Panel::bottom("status")
        .frame(
            card_frame(
                dark,
                egui::Margin {
                    left: 8,
                    right: 8,
                    top: 4,
                    bottom: 8,
                },
            )
            .inner_margin(egui::Margin::symmetric(10, 4)),
        )
        .show_separator_line(false)
        .show_inside(ui, |ui| {
            ui.horizontal(|ui| {
                let show_summary = ui.available_width() > 460.0;
                ui.add(
                    egui::Label::new(&app.session.status)
                        .truncate()
                        .sense(Sense::hover()),
                )
                .on_hover_text(&app.session.status);
                if show_summary && let Some(di) = app.active_dataset() {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(
                            egui::RichText::new(app.doc.datasets[di].summary())
                                .color(ui.visuals().weak_text_color()),
                        );
                    });
                }
            });
        });
}

/// The newest *unacknowledged* warning/failure plus how many older ones are
/// still pending. Selecting by outcome — not recency of the whole history —
/// means a success landing right after a failure (the second file of a batch,
/// a background job) can never mask the failure.
fn pending_feedback(
    app: &PlotxApp,
) -> Option<(
    plotx_core::operation::OperationId,
    u64,
    OperationOutcome,
    String,
    usize,
)> {
    let acknowledged = app.session.ui.dismissed_feedback_order;
    let mut pending = app
        .session
        .operation_history
        .operations()
        .filter(|operation| {
            matches!(
                operation.outcome,
                OperationOutcome::Warning | OperationOutcome::Failure
            )
        })
        .filter(|operation| acknowledged.is_none_or(|latest| operation.completion_order > latest));
    let newest = pending.next_back()?;
    let (id, completion_order, outcome, summary) = (
        newest.id,
        newest.completion_order,
        newest.outcome,
        newest.summary.clone(),
    );
    Some((id, completion_order, outcome, summary, pending.count()))
}

fn feedback_banner(app: &mut PlotxApp, ui: &mut Ui, dark: bool) {
    let Some((_, completion_order, outcome, summary, earlier)) = pending_feedback(app) else {
        return;
    };
    let (icon, color) = match outcome {
        OperationOutcome::Failure => (
            egui_phosphor::regular::WARNING_OCTAGON,
            ui.visuals().error_fg_color,
        ),
        OperationOutcome::Warning => (
            egui_phosphor::regular::WARNING,
            if dark {
                Color32::from_rgb(245, 190, 72)
            } else {
                Color32::from_rgb(145, 86, 0)
            },
        ),
        OperationOutcome::Success => return,
    };
    egui::Panel::top("operation_feedback")
        .frame(
            egui::Frame::new()
                .fill(color.linear_multiply(if dark { 0.16 } else { 0.10 }))
                .inner_margin(egui::Margin::symmetric(12, 8))
                .stroke(Stroke::new(1.0_f32, color.linear_multiply(0.55))),
        )
        .show_separator_line(false)
        .show_inside(ui, |ui| {
            egui::containers::Sides::new()
                .shrink_left()
                .truncate()
                .show(
                    ui,
                    |ui| {
                        ui.colored_label(color, egui::RichText::new(icon).size(18.0));
                        ui.colored_label(color, &summary).on_hover_text(&summary);
                        if earlier > 0 {
                            ui.label(
                                egui::RichText::new(format!("+{earlier} earlier"))
                                    .color(ui.visuals().weak_text_color()),
                            )
                            .on_hover_text(
                                "Older unresolved reports — open Details to review them.",
                            );
                        }
                    },
                    |ui| {
                        let dismiss = ui.button("Dismiss");
                        let dismiss = if earlier > 0 {
                            dismiss.on_hover_text("Acknowledge this and the earlier reports")
                        } else {
                            dismiss
                        };
                        if dismiss.clicked() {
                            app.session.ui.dismissed_feedback_order = Some(completion_order);
                        }
                        if ui.button("Details").clicked() {
                            app.session.ui.diagnostics_open = true;
                        }
                    },
                );
        });
}

fn render_sidebars(app: &mut PlotxApp, ui: &mut Ui, dark: bool, workspace_width: f32) {
    let compact = workspace_width < 1200.0;
    if !app.session.secondary_sidebar_visible {
        app.finish_axis_overrides_edit();
    }
    if app.session.primary_sidebar_visible {
        let panel = egui::Panel::left("primary_sidebar")
            .frame(egui::Frame::NONE.inner_margin(egui::Margin {
                left: 8,
                right: 4,
                top: 4,
                bottom: 8,
            }))
            .show_separator_line(false);
        let panel = if compact {
            panel
                .resizable(true)
                .default_size(180.0)
                .size_range(150.0..=420.0)
        } else {
            panel
                .resizable(true)
                .default_size(app.session.primary_sidebar_width)
                .size_range(190.0..=420.0)
        };
        panel.show_inside(ui, |ui| {
            let size = ui.available_size();
            let frame = card_frame(dark, egui::Margin::ZERO);
            let inset = frame.total_margin().sum();
            frame.show(ui, |ui| {
                ui.set_min_size((size - inset).max(Vec2::ZERO));
                primary_sidebar::render(app, ui);
            });
        });
    }

    if app.session.secondary_sidebar_visible {
        let panel = egui::Panel::right("secondary_sidebar")
            .frame(egui::Frame::NONE.inner_margin(egui::Margin {
                left: 4,
                right: 8,
                top: 4,
                bottom: 8,
            }))
            .show_separator_line(false);
        let panel = if compact {
            panel
                .resizable(true)
                .default_size(230.0)
                .size_range(180.0..=460.0)
        } else {
            panel
                .resizable(true)
                .default_size(app.session.secondary_sidebar_width)
                .size_range(230.0..=460.0)
        };
        panel.show_inside(ui, |ui| {
            let size = ui.available_size();
            let frame = card_frame(dark, egui::Margin::ZERO);
            let inset = frame.total_margin().sum();
            frame.show(ui, |ui| {
                ui.set_min_size((size - inset).max(Vec2::ZERO));
                secondary_sidebar::render(app, ui);
            });
        });
    }
}

/// Chrome layering: the Ribbon and sidebars sit as rounded cards on a slightly
/// darker workspace instead of being fenced off by separator lines. Fills stay
/// near-neutral so the chrome never skews colour judgement of plot data.
fn workspace_fill(dark: bool) -> Color32 {
    if dark {
        Color32::from_rgb(21, 22, 24)
    } else {
        Color32::from_rgb(243, 244, 246)
    }
}

pub(super) enum ModalKind {
    /// Command-palette style: no backdrop dim and anchored near the top of the
    /// window, so the workspace stays readable while searching (the convention
    /// set by VS Code / Search Everywhere / Spotlight).
    Palette,
    /// Blocking dialog: dims the background, but lighter on light themes where
    /// egui's default black-alpha backdrop reads far heavier than on dark ones.
    Dialog,
}

/// Every modal goes through here so backdrop depth, anchoring, and future
/// theming decisions live in one place instead of at each call site.
pub(super) fn modal(ctx: &egui::Context, id: &str, kind: ModalKind) -> egui::Modal {
    let id = egui::Id::new(id);
    match kind {
        ModalKind::Palette => egui::Modal::new(id)
            .backdrop_color(Color32::TRANSPARENT)
            .area(
                egui::Modal::default_area(id)
                    .anchor(egui::Align2::CENTER_TOP, Vec2::new(0.0, 96.0)),
            ),
        ModalKind::Dialog => {
            let alpha = if ctx.theme() == egui::Theme::Dark {
                100
            } else {
                48
            };
            egui::Modal::new(id).backdrop_color(Color32::from_black_alpha(alpha))
        }
    }
}

fn card_frame(dark: bool, outer_margin: egui::Margin) -> egui::Frame {
    let (fill, stroke, shadow_alpha) = if dark {
        (
            Color32::from_rgb(34, 35, 38),
            Color32::from_white_alpha(12),
            96,
        )
    } else {
        (Color32::WHITE, Color32::from_black_alpha(15), 20)
    };
    egui::Frame::new()
        .fill(fill)
        .stroke(Stroke::new(1.0_f32, stroke))
        .corner_radius(8)
        .inner_margin(8)
        .outer_margin(outer_margin)
        .shadow(egui::epaint::Shadow {
            offset: [0, 2],
            blur: 8,
            spread: 0,
            color: Color32::from_black_alpha(shadow_alpha),
        })
}

/// The menu strip blends into the workspace rather than floating on it.
#[cfg(not(target_os = "macos"))]
fn flush_frame(dark: bool, inner_margin: egui::Margin) -> egui::Frame {
    egui::Frame::new()
        .fill(workspace_fill(dark))
        .inner_margin(inner_margin)
}

#[cfg(test)]
mod feedback_tests {
    use super::*;
    use plotx_core::operation::{
        Diagnostic, DiagnosticCode, OperationId, OperationKind, OperationReport, Severity,
    };

    fn app() -> PlotxApp {
        PlotxApp::new_with_settings(plotx_core::settings::Settings::default())
    }

    fn record_failure(app: &mut PlotxApp, summary: &str) -> OperationId {
        let id = app.session.begin_operation();
        app.session.record_operation(OperationReport::<()>::failure(
            id,
            OperationKind::DatasetLoad,
            summary,
            Diagnostic::new(Severity::Error, DiagnosticCode::DatasetLoadFailed, summary),
        ));
        id
    }

    fn record_success(app: &mut PlotxApp) {
        let id = app.session.begin_operation();
        app.session.record_operation(OperationReport::success(
            id,
            OperationKind::DatasetLoad,
            "Loaded".to_owned(),
            (),
        ));
    }

    /// A success landing after a failure (second file of a batch, background
    /// job) must not mask the failure.
    #[test]
    fn newest_unacknowledged_failure_survives_a_later_success() {
        let mut app = app();
        record_failure(&mut app, "boom");
        record_success(&mut app);
        let (_, _, outcome, summary, earlier) = pending_feedback(&app).expect("failure pending");
        assert_eq!(outcome, OperationOutcome::Failure);
        assert_eq!(summary, "boom");
        assert_eq!(earlier, 0);
    }

    #[test]
    fn dismiss_acknowledges_through_the_shown_report() {
        let mut app = app();
        record_failure(&mut app, "first");
        let newest = record_failure(&mut app, "second");
        let (shown, shown_order, _, summary, earlier) =
            pending_feedback(&app).expect("failures pending");
        assert_eq!(shown, newest);
        assert_eq!(summary, "second");
        assert_eq!(earlier, 1);

        app.session.ui.dismissed_feedback_order = Some(shown_order);
        assert!(pending_feedback(&app).is_none());

        // A report arriving after the watermark surfaces again.
        record_failure(&mut app, "third");
        let (_, _, _, summary, earlier) = pending_feedback(&app).expect("new failure pending");
        assert_eq!(summary, "third");
        assert_eq!(earlier, 0);
    }

    #[test]
    fn dismissal_uses_report_order_for_delayed_operations() {
        let mut app = app();
        let delayed_failure = app.session.begin_operation();
        let later_warning = app.session.begin_operation();
        app.session.record_operation(OperationReport::warning(
            later_warning,
            OperationKind::DataExport,
            "A later warning",
            (),
        ));

        let (_, dismissed_order, _, summary, _) = pending_feedback(&app).expect("warning pending");
        assert_eq!(summary, "A later warning");
        app.session.ui.dismissed_feedback_order = Some(dismissed_order);

        app.session.record_operation(OperationReport::<()>::failure(
            delayed_failure,
            OperationKind::DataExport,
            "The delayed export failed",
            Diagnostic::new(
                Severity::Error,
                DiagnosticCode::DataExportWriteFailed,
                "The delayed export failed.",
            ),
        ));

        let (_, _, outcome, summary, earlier) =
            pending_feedback(&app).expect("delayed failure pending");
        assert_eq!(outcome, OperationOutcome::Failure);
        assert_eq!(summary, "The delayed export failed");
        assert_eq!(earlier, 0);
    }
}
