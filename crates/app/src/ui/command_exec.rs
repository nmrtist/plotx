//! Execution half of the shared command layer. Keeping dispatch separate from
//! descriptions makes the live catalog easy to inspect and keeps source files
//! within the repository size limit.

use plotx_core::state::{CommandPaletteState, LineShapeKind, PlotxApp, Tool, ToolGroup};

use super::clipboard_table::ClipboardTablePaste;
use super::commands::{self, CommandId};

pub fn execute(
    id: CommandId,
    app: &mut PlotxApp,
    clipboard: &mut ClipboardTablePaste,
    ctx: &egui::Context,
) {
    if matches!(id, CommandId::Undo | CommandId::Redo) {
        // Commit any debounced wheel zoom before the enabled gate, so the
        // pending zoom becomes the next undoable step and history is ordered
        // the same from every dispatch surface (keyboard, menus, the macOS
        // menu bar, the palette and the Ribbon).
        let now = ctx.input(|input| input.time);
        app.finish_pending_wheel_zoom(now, true);
    }
    if !commands::describe(app, id).enabled {
        return;
    }
    match id {
        CommandId::OpenProject => super::file_dialogs::open_project(app),
        CommandId::OpenFile => super::file_dialogs::open_file(app),
        CommandId::OpenFolder => super::file_dialogs::open_folder(app),
        CommandId::RunBatchWorkflow => super::batch_workflow::AutomationUi::request_open(ctx),
        CommandId::OpenRecent(index) => {
            if let Some(path) = app.session.recent_files.get(index).cloned() {
                super::file_dialogs::open_recent_path(app, &path);
            }
        }
        CommandId::ClearRecentFiles => app.clear_recent_files(),
        CommandId::HelpManual => {
            ctx.open_url(egui::OpenUrl::new_tab(commands::MANUAL_URL));
        }
        CommandId::ImportTable => super::file_dialogs::import_delimited_table(app),
        CommandId::PasteTable => clipboard.request(app, ctx),
        CommandId::SaveProject => app.request_save_project(),
        CommandId::NewTable => app.new_table_dataset(),
        CommandId::ExportData => {
            if let Some(dataset) = app.active_dataset() {
                app.open_data_export(dataset);
            }
        }
        CommandId::NewCanvas(index) => {
            if let Some(template) = plotx_core::templates::CanvasTemplate::all().get(index) {
                app.new_canvas_from_template(template);
            }
        }
        CommandId::Export(format) => app.request_export(format),
        CommandId::CopyFigure => super::clipboard_figure::copy_figure_to_clipboard(app, ctx),
        CommandId::Quit => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
        CommandId::Undo => app.undo(),
        CommandId::Redo => app.redo(),
        CommandId::SelectAll => app.select_all_objects(),
        CommandId::Group => app.group_selected(),
        CommandId::Ungroup => app.ungroup_selected(),
        CommandId::TogglePrimarySidebar => {
            app.session.primary_sidebar_visible = !app.session.primary_sidebar_visible;
        }
        CommandId::ToggleSecondarySidebar => {
            app.session.secondary_sidebar_visible = !app.session.secondary_sidebar_visible;
        }
        CommandId::ZoomToFit => app.zoom_active_canvas_to_fit(),
        CommandId::ZoomToSelection => super::canvas::zoom_to_selection(app, ctx),
        CommandId::UiScaleUp => crate::scale::nudge_ui_zoom(app, ctx, 1),
        CommandId::UiScaleDown => crate::scale::nudge_ui_zoom(app, ctx, -1),
        CommandId::UiScaleReset => crate::scale::reset_ui_zoom(app, ctx),
        CommandId::Present => super::present::toggle_present_mode(app),
        CommandId::ToggleGrid => {
            if let Some(canvas) = app.session.active_canvas {
                app.set_show_grid(canvas, !app.doc.canvases[canvas].layout.show_grid);
            }
        }
        CommandId::ToggleSnap => app.set_snap_enabled(!app.session.ui.snap_enabled),
        CommandId::Preferences => app.open_settings(),
        CommandId::CommandPalette => {
            app.session.ui.command_palette = match app.session.ui.command_palette.take() {
                Some(_) => None,
                None => Some(CommandPaletteState::default()),
            };
        }
        CommandId::CheckUpdates => {
            app.session.updates.check_now();
            app.open_settings();
        }
        CommandId::OperationHistory => app.session.ui.diagnostics_open = true,
        CommandId::About => app.session.ui.about_open = true,
        CommandId::SaveProcessingTemplate | CommandId::ApplyProcessingTemplate => {
            if let Some(dataset) = app.active_dataset() {
                if id == CommandId::SaveProcessingTemplate {
                    super::processing_templates::open_save_template_dialog(app, dataset);
                } else {
                    super::processing_templates::open_template_browser(app, dataset);
                }
            }
        }
        CommandId::SpectrumArithmetic => super::arithmetic::open_spectrum_arithmetic_dialog(app),
        CommandId::AlignSpectra => super::align::open_align_spectra_dialog(app),
        CommandId::StackData => app.stack_selected_data(),
        CommandId::SelectRange => app.toggle_tool(Tool::SelectRegion),
        CommandId::ClearRange => app.clear_analysis_selection(),
        CommandId::Regions => toggle_regions(app),
        CommandId::SeriesTable => open_active_region_table(app),
        CommandId::DetectPeaks => detect_peaks(app),
        CommandId::PeakList => reveal_tool_group(app, Tool::Peaks, ToolGroup::Peaks),
        CommandId::LineFit => reveal_tool_group(app, Tool::LineFit, ToolGroup::LineFit),
        CommandId::RunPeakFit => run_peak_fit(app, ctx),
        CommandId::CurveFit => open_curve_fit(app),
        CommandId::RunCurveFit => run_curve_fit(app),
        CommandId::Statistics => open_statistics(app),
        CommandId::ChartType => open_chart_type(app),
        CommandId::FigureTypography => app.session.ui.figure_typography_open = true,
        CommandId::Integrate => app.toggle_tool(Tool::Integrate),
        CommandId::Multiplets => analyze_multiplets(app),
        CommandId::TidyBoard => app.tidy_board(),
        CommandId::CanvasSettings => {
            app.session.ui.canvas_settings = app.session.active_canvas;
        }
        CommandId::SetCanvasSizePreset(preset_id) => {
            if let (Some(ci), Some(preset)) = (
                app.session.active_canvas,
                plotx_core::state::preset_by_id(preset_id),
            ) {
                super::canvas_size::apply_preset(app, ctx, ci, preset);
            }
        }
        CommandId::ArrangeGrid(rows, columns) => {
            app.arrange_active_canvas_grid(rows, columns);
        }
        CommandId::SimplifyInnerAxes => app.simplify_inner_axes(),
        CommandId::SetSpacingMode(mode) => app.set_spacing_mode(mode),
        CommandId::SetGutterPreset(preset) => app.set_gutter_preset(preset),
        CommandId::Align(mode) => app.align_selected(mode),
        CommandId::Distribute(mode) => app.distribute_selected(mode),
        CommandId::ZOrder(mode) => app.z_order_selected(mode),
        CommandId::ApplyTheme(id) => {
            if let Some(theme) = plotx_core::theme::Theme::by_id(id) {
                app.apply_theme(&theme);
            }
        }
        CommandId::Tool(tool) => app.toggle_tool(tool),
    }
}

fn detect_peaks(app: &mut PlotxApp) {
    let Some(dataset) = app.active_dataset() else {
        return;
    };
    let column = app.session.ui.peak_column;
    if let Some(peaks) = app.doc.datasets[dataset].peaks().cloned() {
        app.run_detection(dataset, peaks.detector.threshold, column);
    }
}

fn run_peak_fit(app: &mut PlotxApp, ctx: &egui::Context) {
    let Some(dataset) = app.active_dataset() else {
        return;
    };
    if let Some(range) = app.analysis_range_for(dataset) {
        let shape = ctx
            .data(|data| data.get_temp(super::tools::line_fit_shape_id(dataset)))
            .unwrap_or(LineShapeKind::Lorentzian);
        if let Err(error) = app.start_line_fit(dataset, range.min, range.max, shape) {
            app.session.status = error;
        }
    }
}

fn open_curve_fit(app: &mut PlotxApp) {
    let Some(dataset) = app.active_dataset() else {
        return;
    };
    super::tools::open_curve_fit_task(app, dataset);
}

fn run_curve_fit(app: &mut PlotxApp) {
    let Some(dataset) = app.active_dataset() else {
        return;
    };
    super::tools::open_curve_fit_task(app, dataset);
    super::tools::run_curve_fit(app, dataset);
}

fn open_statistics(app: &mut PlotxApp) {
    let Some(dataset) = app.active_dataset() else {
        return;
    };
    super::tools::open_statistics_task(app, dataset);
}

/// Routes to the chart gallery: selects the table's plot so the Object
/// inspector shows the gallery, and opens the inspector's sidebar.
fn open_chart_type(app: &mut PlotxApp) {
    let Some(dataset) = app.active_dataset() else {
        return;
    };
    let Some((ci, object)) = commands::chart_plot_target(app, dataset) else {
        return;
    };
    app.session.active_canvas = Some(ci);
    app.select_object(ci, object);
    app.session.secondary_sidebar_visible = true;
}

fn analyze_multiplets(app: &mut PlotxApp) {
    let Some(dataset) = app.active_dataset() else {
        return;
    };
    let Some(range) = app.analysis_range_for(dataset) else {
        return;
    };
    match app.analyze_multiplets(dataset, range.min, range.max) {
        Ok(values) => app.apply_multiplet_analysis(dataset, values),
        Err(error) => app.session.status = error,
    }
}

fn reveal_tool_group(app: &mut PlotxApp, tool: Tool, group: ToolGroup) {
    app.toggle_tool(tool);
    reveal_group(app, group);
}

fn toggle_regions(app: &mut PlotxApp) {
    if app.session.tool == Tool::Regions {
        app.toggle_tool(Tool::Regions);
        return;
    }
    let Some(dataset) = app
        .active_dataset()
        .filter(|&di| app.doc.datasets[di].supports_region_analysis())
    else {
        return;
    };
    app.toggle_tool(Tool::Regions);
    super::tools::open_region_task(app, dataset);
}

fn open_active_region_table(app: &mut PlotxApp) {
    let Some(dataset) = app.active_dataset() else {
        return;
    };
    super::tools::open_region_table(app, dataset);
}

fn reveal_group(app: &mut PlotxApp, group: ToolGroup) {
    app.session.secondary_sidebar_visible = true;
    app.session.ui.requested_tool_group = Some(group);
}
