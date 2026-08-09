//! Execution half of the shared command layer. Keeping dispatch separate from
//! descriptions makes the live catalog easy to inspect and keeps source files
//! within the repository size limit.

use plotx_core::state::{
    CommandPaletteState, LineShapeKind, PlotxApp, ProjectTransition, Tool, ToolGroup,
};

use super::clipboard_table::ClipboardTablePaste;
use super::commands::{self, CommandId};

pub fn execute(
    id: CommandId,
    app: &mut PlotxApp,
    clipboard: &mut ClipboardTablePaste,
    ctx: &egui::Context,
) {
    execute_inner(id, app, Some(clipboard), ctx);
}

pub fn execute_without_clipboard(id: CommandId, app: &mut PlotxApp, ctx: &egui::Context) {
    execute_inner(id, app, None, ctx);
}

fn execute_inner(
    id: CommandId,
    app: &mut PlotxApp,
    clipboard: Option<&mut ClipboardTablePaste>,
    ctx: &egui::Context,
) {
    if matches!(id, CommandId::Undo | CommandId::Redo) {
        // Commit any debounced wheel zoom before the enabled gate, so the
        // pending zoom becomes the next undoable step and history is ordered
        // the same from every dispatch surface (keyboard, menus, the macOS
        // menu bar, the palette and the Ribbon).
        let now = ctx.input(|input| input.time);
        app.finish_pending_wheel_zoom(now, true);
        app.finish_pending_wheel_property(now, true);
    }
    if !commands::describe(app, id).enabled {
        return;
    }
    match id {
        CommandId::NewProject => {
            app.request_project_transition(ProjectTransition::New);
        }
        CommandId::OpenProject => super::file_dialogs::open_project(app),
        CommandId::CloseProject => {
            app.request_project_transition(ProjectTransition::Close);
        }
        CommandId::OpenFile => super::file_dialogs::open_file(app),
        CommandId::OpenFolder => super::file_dialogs::open_folder(app),
        CommandId::RunBatchWorkflow => super::batch_workflow::AutomationUi::request_open(ctx),
        CommandId::RunScientificScript => {
            super::batch_workflow::AutomationUi::request_run_script(ctx)
        }
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
        CommandId::PasteTable => {
            if let Some(clipboard) = clipboard {
                clipboard.request(app, ctx);
            }
        }
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
        CommandId::SelectAll => select_all_in_scope(app),
        CommandId::DeselectAll => deselect_all_in_scope(app),
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
        CommandId::ToggleSnap => app.set_snap_enabled(!app.settings.general.snap_enabled),
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
        CommandId::AlignTraces => super::trace_alignment::open_active_trace_alignment_dialog(app),
        CommandId::StackData => app.stack_selected_data(),
        CommandId::ExtractMassSpectrum => {
            app.set_tool(Tool::SelectRegion);
            reveal_group(app, ToolGroup::MassSpectrometry);
        }
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
        // Channels 2 and 4 navigate; they never edit. `reveal_property` opens
        // the home the presentation names and asks the panel to scroll and
        // highlight — the same route a palette hit takes.
        CommandId::PropertyGroup(section) => {
            let now = ctx.input(|input| input.time);
            if let Some(property) = super::properties::discovery::entry_property(
                section,
                super::properties::PRESENTATIONS,
            ) {
                super::command_palette::reveal_property(app, property, now);
                ctx.request_repaint();
            }
        }
        // Channel 3 edits, and does so through the property planner.
        CommandId::StepProperty(step) => super::properties::discovery::step_selection(app, step),
        CommandId::CycleCursor => cycle_cursor(app),
        CommandId::Tool(Tool::Symmetry) => {
            reveal_tool_group(app, Tool::Symmetry, ToolGroup::Nmr2dExperiment);
        }
        CommandId::Tool(tool) => app.toggle_tool(tool),
    }
}

fn select_all_in_scope(app: &mut PlotxApp) {
    use plotx_core::state::{Selection, SelectionScope, board_frame_id, board_frames};
    match app.session.ui.selection_scope {
        SelectionScope::Board => {
            app.session.ui.frame_selection = board_frames(app)
                .into_iter()
                .filter_map(|frame| board_frame_id(app, frame))
                .collect();
            plotx_core::state::sync_frame_selection_to_data(app);
        }
        SelectionScope::CanvasList => {
            app.session.ui.frame_selection = app
                .doc
                .canvases
                .iter()
                .map(|canvas| plotx_core::state::BoardFrameId::Page(canvas.resource_id))
                .collect();
            plotx_core::state::sync_frame_selection_to_data(app);
        }
        SelectionScope::DataList => {
            app.session.ui.frame_selection.clear();
            let indices = (0..app.doc.datasets.len()).collect::<Vec<_>>();
            app.focus_datasets(&indices, app.active_dataset());
        }
        SelectionScope::CanvasObjects => app.select_all_objects(),
        SelectionScope::Layers => {
            if let Some(ci) = app.session.active_canvas {
                let ids = app.doc.canvases[ci]
                    .objects
                    .iter()
                    .map(|object| object.id)
                    .collect::<Vec<_>>();
                app.set_selection(if ids.is_empty() {
                    Selection::None
                } else {
                    Selection::Objects(ids)
                });
            }
        }
    }
    app.session.status = "Selected all items in the current context.".to_owned();
}

fn deselect_all_in_scope(app: &mut PlotxApp) {
    use plotx_core::state::{Selection, SelectionScope};
    match app.session.ui.selection_scope {
        SelectionScope::Board | SelectionScope::CanvasList => {
            app.session.ui.frame_selection.clear();
        }
        SelectionScope::DataList => {
            app.session.ui.frame_selection.clear();
            app.focus_datasets(&[], None);
        }
        SelectionScope::CanvasObjects | SelectionScope::Layers => {
            app.set_selection(Selection::None)
        }
    }
    app.session.status = "Cleared the current selection.".to_owned();
}

fn cycle_cursor(app: &mut PlotxApp) {
    let Some(dataset) = app.active_dataset() else {
        return;
    };
    let symmetry = app.doc.datasets[dataset]
        .as_nmr2d()
        .is_some_and(|nmr| nmr.supports_symmetry_review());
    let tools: &[Tool] = if symmetry {
        &[Tool::InspectCursor, Tool::DeltaCursor, Tool::Symmetry]
    } else {
        &[Tool::InspectCursor, Tool::DeltaCursor]
    };
    let current = tools.iter().position(|&tool| tool == app.session.tool);
    let next = tools[current.map_or(0, |index| (index + 1) % tools.len())];
    app.set_tool(next);
    reveal_group(
        app,
        if app.doc.datasets[dataset].as_nmr2d().is_some() {
            ToolGroup::Nmr2dExperiment
        } else {
            ToolGroup::Nmr1dAnalysis
        },
    );
    let position = tools
        .iter()
        .position(|&tool| tool == next)
        .expect("next cursor comes from the applicable cursor list");
    let following = tools[(position + 1) % tools.len()];
    app.session.status = format!(
        "Cursor {}/{}: {}. Press C for {}; Esc exits.",
        position + 1,
        tools.len(),
        next.label(),
        following.label(),
    );
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
    app.reveal_object(ci, object);
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
    if group == ToolGroup::Processing {
        super::tools::expand_processing_surface(app);
        return;
    }
    app.session.secondary_sidebar_visible = true;
    app.session.ui.requested_tool_group = Some(group);
}

#[cfg(test)]
mod tests {
    use super::*;
    use plotx_core::actions::Action;
    use plotx_core::properties::{AggregateValue, PropertyAddress, PropertyValue, app_preferences};
    use plotx_core::settings::Settings;
    use plotx_core::state::DEFAULT_CANVAS_SIZE_MM;

    fn catalog_snap(app: &mut PlotxApp, enabled: bool) {
        let commit = app
            .plan_property_write(
                app_preferences::SNAP_ENABLED,
                std::slice::from_ref(&app.app_target()),
                &PropertyValue::Bool(enabled),
            )
            .expect("the Preferences catalog row plans");
        app.commit_property(commit);
    }

    fn resolved_snap(app: &PlotxApp) -> AggregateValue<PropertyValue> {
        app.resolve_property(&PropertyAddress::new(
            app.app_target(),
            app_preferences::SNAP_ENABLED,
        ))
        .expect("snap resolves through the catalog")
        .value
    }

    #[test]
    fn settings_toolbar_and_toggle_command_share_the_snap_catalog_value() {
        let mut app = PlotxApp::new_with_settings(Settings::default());

        // Preferences rows submit this same catalog write.
        catalog_snap(&mut app, false);
        assert_eq!(
            resolved_snap(&app),
            AggregateValue::Uniform(PropertyValue::Bool(false))
        );

        // The canvas toolbar keeps its existing setter surface, whose
        // implementation now plans and commits the catalog property.
        app.set_snap_enabled(true);
        assert_eq!(
            resolved_snap(&app),
            AggregateValue::Uniform(PropertyValue::Bool(true))
        );

        let mut clipboard = ClipboardTablePaste::default();
        execute(
            CommandId::ToggleSnap,
            &mut app,
            &mut clipboard,
            &egui::Context::default(),
        );
        assert_eq!(
            resolved_snap(&app),
            AggregateValue::Uniform(PropertyValue::Bool(false))
        );
        assert!(!app.settings.general.snap_enabled);
    }

    #[test]
    fn symmetry_command_opens_its_review_surface_and_toggles_cleanly() {
        let mut app = PlotxApp::new_with_settings(Settings::default());
        let action = Action::insert_dataset_with_default_canvas(
            &app,
            crate::ui::properties::fixture::homonuclear_frequency_2d(),
            "COSY review".to_owned(),
            DEFAULT_CANVAS_SIZE_MM,
        );
        app.execute_action(action);
        app.session.secondary_sidebar_visible = false;
        let mut clipboard = ClipboardTablePaste::default();
        let ctx = egui::Context::default();

        execute(
            CommandId::Tool(Tool::Symmetry),
            &mut app,
            &mut clipboard,
            &ctx,
        );
        assert_eq!(app.session.tool, Tool::Symmetry);
        assert!(app.session.secondary_sidebar_visible);
        assert!(app.session.ui.requested_tool_group == Some(ToolGroup::Nmr2dExperiment));

        execute(
            CommandId::Tool(Tool::Symmetry),
            &mut app,
            &mut clipboard,
            &ctx,
        );
        assert_eq!(app.session.tool, Tool::BrowseZoom);
    }

    #[test]
    fn cursor_command_has_a_stable_three_press_cycle_on_eligible_2d_data() {
        let mut app = PlotxApp::new_with_settings(Settings::default());
        let action = Action::insert_dataset_with_default_canvas(
            &app,
            crate::ui::properties::fixture::homonuclear_frequency_2d(),
            "COSY cursors".to_owned(),
            DEFAULT_CANVAS_SIZE_MM,
        );
        app.execute_action(action);
        let mut clipboard = ClipboardTablePaste::default();
        let ctx = egui::Context::default();

        for expected in [
            Tool::InspectCursor,
            Tool::DeltaCursor,
            Tool::Symmetry,
            Tool::InspectCursor,
        ] {
            execute(CommandId::CycleCursor, &mut app, &mut clipboard, &ctx);
            assert_eq!(app.session.tool, expected);
        }
        assert!(app.session.status.contains("Cursor 1/3"));
        assert!(app.session.secondary_sidebar_visible);
        assert!(app.session.ui.requested_tool_group == Some(ToolGroup::Nmr2dExperiment));
    }
}
