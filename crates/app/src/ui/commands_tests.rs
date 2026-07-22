use super::*;
use num_complex::Complex64;
use plotx_core::actions::Action;
use plotx_core::state::{
    AnalysisSelection, DEFAULT_CANVAS_SIZE_MM, FloatSeries, LineShapeKind, NmrDataset,
    ProcessingTemplateDialogState, Selection, materialized_float_series_table,
};
use plotx_io::{Domain, NmrData};

fn app() -> PlotxApp {
    PlotxApp::new_with_settings(plotx_core::settings::Settings::default())
}

/// A plotted data table with `curves` y-columns, which is what Curve Fit gates on.
fn app_with_table(curves: usize) -> PlotxApp {
    let mut app = app();
    let series = (0..curves)
        .map(|index| FloatSeries {
            name: format!("curve {index}"),
            unit: String::new(),
            values: (0..8).map(|i| Some((-(i as f64) / 3.0).exp())).collect(),
            uncertainty: None,
            fit: None,
        })
        .collect();
    let table = materialized_float_series_table(
        (
            "delay".into(),
            "s".into(),
            (0..8).map(|i| Some(i as f64)).collect(),
        ),
        series,
        "plotx.test.command-table.v1",
    )
    .unwrap();
    let action = Action::insert_dataset_with_default_canvas(
        &app,
        Dataset::Table(Box::new(table)),
        "Canvas — table".to_owned(),
        DEFAULT_CANVAS_SIZE_MM,
    );
    app.execute_action(action);
    app
}

fn app_with_nmr() -> PlotxApp {
    let mut app = app();
    let npoints = 256;
    let spectral_width_hz = 4_000.0;
    let observe_freq_mhz = 400.0;
    let carrier_ppm = 5.0;
    let points = (0..npoints)
        .map(|index| {
            let time = index as f64 / spectral_width_hz;
            let decay = (-time / 0.25).exp();
            let frequency_hz = (2.0 - carrier_ppm) * observe_freq_mhz;
            Complex64::from_polar(decay, std::f64::consts::TAU * frequency_hz * time)
        })
        .collect();
    let data = NmrData {
        points,
        domain: Domain::Time,
        spectral_width_hz,
        observe_freq_mhz,
        carrier_ppm,
        nucleus: "1H".to_owned(),
        source: "synthetic command-catalog test".to_owned(),
        group_delay: 0.0,
    };
    let action = Action::insert_dataset_with_default_canvas(
        &app,
        Dataset::Nmr(Box::new(NmrDataset::load(data))),
        "Canvas — 1D NMR".to_owned(),
        DEFAULT_CANVAS_SIZE_MM,
    );
    app.execute_action(action);
    app
}

fn ribbon_groups(app: &PlotxApp) -> Vec<(WorkflowTab, Vec<&'static str>)> {
    let catalog = catalog(app);
    WorkflowTab::ALL
        .into_iter()
        .map(|tab| {
            let mut groups: Vec<_> = catalog
                .iter()
                .filter_map(|command| command.ribbon)
                .filter(|placement| placement.tab == tab)
                .map(|placement| placement.group)
                .collect();
            groups.sort_unstable();
            groups.dedup();
            (tab, groups)
        })
        .collect()
}

#[test]
fn stable_ids_cover_static_and_dynamic_commands() {
    assert_eq!(CommandId::SaveProject.stable_id(), "file.save");
    assert_eq!(CommandId::ArrangeGrid(2, 3).stable_id(), "arrange.grid.2x3");
    assert_eq!(CommandId::Tool(Tool::LineFit).stable_id(), "tool.line_fit");
    assert_eq!(CommandId::CurveFit.stable_id(), "analysis.curve_fit");
    assert_eq!(CommandId::ExportData.stable_id(), "file.export_data");
    assert_eq!(CommandId::OpenRecent(3).stable_id(), "file.open_recent.3");
    assert_eq!(CommandId::ClearRecentFiles.stable_id(), "file.clear_recent");
    assert_eq!(CommandId::HelpManual.stable_id(), "help.manual");
    assert_eq!(CommandId::RunBatchWorkflow.stable_id(), "tools.automation");
}

#[test]
fn origin_import_reuses_import_table_command_identity() {
    let app = app();
    assert_eq!(CommandId::ImportTable.stable_id(), "file.import_table");
    assert_eq!(
        describe(&app, CommandId::ImportTable).label,
        "Import Table…"
    );
}

#[test]
fn automation_is_a_global_menu_and_palette_command() {
    let app = app();
    let command = describe(&app, CommandId::RunBatchWorkflow);
    assert!(command.enabled);
    assert_eq!(command.label, "Automation…");
    assert!(command.ribbon.is_none());
    assert!(catalog(&app).iter().any(|entry| {
        entry.id == CommandId::RunBatchWorkflow && entry.id.stable_id() == "tools.automation"
    }));
}

#[test]
fn automation_command_is_declared_as_a_tool_editor() {
    assert_eq!(
        CommandId::RunBatchWorkflow.execution_class(),
        CommandExecutionClass::ToolEditor
    );
}

/// A populated recent list registers one palette-searchable command per entry,
/// with labels that stay tellable apart when two entries share a file name.
#[test]
fn recent_files_surface_through_the_catalog() {
    let mut app = app();
    assert!(
        !catalog(&app)
            .iter()
            .any(|command| matches!(command.id, CommandId::OpenRecent(_)))
    );
    let clear = describe(&app, CommandId::ClearRecentFiles);
    assert!(!clear.enabled);
    assert_eq!(
        clear.disabled_reason,
        Some("Open a file or project to build the recent list.")
    );

    app.session.recent_files = vec![
        std::path::PathBuf::from("C:/alpha/project.plotx"),
        std::path::PathBuf::from("C:/beta/project.plotx"),
        std::path::PathBuf::from("C:/gamma/run.abf"),
    ];
    let commands = catalog(&app);
    let recent: Vec<_> = commands
        .iter()
        .filter(|command| matches!(command.id, CommandId::OpenRecent(_)))
        .collect();
    assert_eq!(recent.len(), 3);
    assert!(recent.iter().all(|command| command.enabled));
    // Ribbon-free by design: recent entries live in menus and the palette.
    assert!(recent.iter().all(|command| command.ribbon.is_none()));
    assert_eq!(
        describe(&app, CommandId::OpenRecent(0)).label,
        "Open Recent: project.plotx — C:/alpha"
    );
    assert_eq!(
        describe(&app, CommandId::OpenRecent(2)).label,
        "Open Recent: run.abf"
    );
    assert!(describe(&app, CommandId::ClearRecentFiles).enabled);
    assert!(describe(&app, CommandId::HelpManual).enabled);
}

/// The File > Open Recent submenu drops the "Open Recent:" prefix its title
/// already carries, while keeping the same disambiguation as the palette label.
#[test]
fn recent_entry_labels_are_prefix_free_but_still_disambiguated() {
    let mut app = app();
    assert_eq!(recent_entry_label(&app, 0), None);

    app.session.recent_files = vec![
        std::path::PathBuf::from("C:/alpha/project.plotx"),
        std::path::PathBuf::from("C:/beta/project.plotx"),
        std::path::PathBuf::from("C:/gamma/run.abf"),
    ];
    assert_eq!(
        recent_entry_label(&app, 0).as_deref(),
        Some("project.plotx — C:/alpha")
    );
    assert_eq!(recent_entry_label(&app, 2).as_deref(), Some("run.abf"));
    // The palette label is exactly the entry label behind the shared prefix.
    assert_eq!(
        describe(&app, CommandId::OpenRecent(0)).label,
        format!("Open Recent: {}", recent_entry_label(&app, 0).unwrap())
    );
    assert_eq!(recent_entry_label(&app, 3), None);
}

#[test]
fn catalog_stable_ids_are_present_and_unique() {
    let app = app();
    let mut seen = std::collections::BTreeMap::new();
    for command in catalog(&app) {
        let stable_id = command.id.stable_id();
        assert!(
            !stable_id.trim().is_empty(),
            "catalog command {:?} has an empty stable ID",
            command.id
        );
        assert!(
            seen.insert(stable_id.clone(), command.id).is_none(),
            "catalog contains duplicate stable ID {stable_id}"
        );
    }
}

#[test]
fn every_disabled_catalog_command_explains_how_to_enable_it() {
    let mut palette_blocked = app();
    palette_blocked.session.ui.processing_template_dialog =
        Some(ProcessingTemplateDialogState::SaveAs {
            dataset: 0,
            name: String::new(),
        });
    let states = [
        ("empty document", app()),
        ("table without curves", app_with_table(0)),
        ("table with curves", app_with_table(2)),
        ("1D NMR", app_with_nmr()),
        ("modal dialog", palette_blocked),
    ];

    for (state_name, app) in states {
        for command in catalog(&app) {
            if command.enabled || command.disabled_reason.is_some() {
                continue;
            }
            // Explicit whitelist: while a modal/gesture blocks Command Palette,
            // no surface that could show its tooltip is visible or interactive.
            assert_eq!(
                command.id,
                CommandId::CommandPalette,
                "disabled command {:?} has no reason in {state_name}",
                command.id
            );
        }
    }
}

#[test]
fn transient_state_never_changes_ribbon_group_visibility() {
    let mut app = app_with_nmr();
    let canvas = app.session.active_canvas.expect("NMR canvas");
    let object = app.doc.canvases[canvas]
        .active_plot_object_id()
        .expect("NMR plot object");
    let range = app.analysis_range_for(0).expect("visible NMR range");
    app.add_manual_peak(0, (range.min + range.max) / 2.0, None);
    app.session.ui.selection = Selection::single(object);
    let expected = ribbon_groups(&app);

    app.session.ui.selection = Selection::None;
    app.session.tool = Tool::Integrate;
    app.session.ui.peak_column = Some(plotx_core::data::ColumnId::new());
    app.session.ui.analysis_selection = Some(AnalysisSelection {
        dataset: 0,
        canvas,
        object,
        x_range: range,
        y_range: None,
    });
    assert_eq!(ribbon_groups(&app), expected);

    app.start_line_fit(0, range.min, range.max, LineShapeKind::Lorentzian)
        .expect("synthetic NMR peak fit should start");
    assert!(app.session.line_fit_job.is_some());
    assert_eq!(ribbon_groups(&app), expected);
    app.cancel_line_fit();
}

#[test]
fn data_export_requires_exportable_current_data() {
    let empty = app();
    let command = describe(&empty, CommandId::ExportData);
    assert!(!command.enabled);
    assert_eq!(
        command.disabled_reason,
        Some("Select a dataset with processed data or analysis results to export.")
    );

    let table = app_with_table(1);
    assert!(describe(&table, CommandId::ExportData).enabled);
}

#[test]
fn live_checked_and_enabled_state_comes_from_one_descriptor() {
    let mut app = app();
    assert_eq!(
        describe(&app, CommandId::TogglePrimarySidebar).checked,
        Some(true)
    );
    assert!(!describe(&app, CommandId::ZoomToFit).enabled);
    app.session.primary_sidebar_visible = false;
    assert_eq!(
        describe(&app, CommandId::TogglePrimarySidebar).checked,
        Some(false)
    );
    // Plain actions carry no toggle state, so no surface renders them as
    // check items.
    assert_eq!(describe(&app, CommandId::SaveProject).checked, None);
    assert_eq!(
        describe(&app, CommandId::SelectRange).label,
        "Analysis Range"
    );
    assert_eq!(describe(&app, CommandId::LineFit).label, "Peak Fit");
    assert_eq!(describe(&app, CommandId::CurveFit).label, "Fit Curves");
}

/// The disabled tooltip must name the requirement that actually blocked the
/// command, not one fixed sentence per command.
#[test]
fn disabled_reason_reports_the_first_unmet_requirement() {
    let app = app();
    // No dataset at all: the type requirement fails before the region one.
    let series_table = describe(&app, CommandId::SeriesTable);
    assert!(!series_table.enabled);
    assert_eq!(
        series_table.disabled_reason,
        Some("Select a series dataset before building a series table.")
    );
    let regions = describe(&app, CommandId::Regions);
    assert!(!regions.enabled);
    assert_eq!(
        regions.disabled_reason,
        Some("Select a series dataset before drawing regions.")
    );

    // A table with no curves clears the type requirement but not the next one.
    let empty = app_with_table(0);
    let run = describe(&empty, CommandId::RunCurveFit);
    assert!(!run.enabled);
    assert_eq!(
        run.disabled_reason,
        Some("Add at least one curve column before running Curve Fit.")
    );

    // A table is not a 1D trace, so Peak Fit stops at the type requirement
    // rather than telling the user to draw a range they cannot draw.
    let run_peak = describe(&empty, CommandId::RunPeakFit);
    assert!(!run_peak.enabled);
    assert_eq!(
        run_peak.disabled_reason,
        Some("Plot 1D data before running Peak Fit.")
    );
}

#[test]
fn enabled_commands_carry_no_disabled_reason() {
    let app = app_with_table(2);
    let curve_fit = describe(&app, CommandId::CurveFit);
    assert!(curve_fit.enabled);
    assert_eq!(curve_fit.disabled_reason, None);
    assert!(describe(&app, CommandId::RunCurveFit).enabled);
}

/// A group that cannot apply is dropped from the Ribbon rather than shown dead:
/// `groups_for_tab` spends the width budget on whatever is placed, so a dead
/// group would push a usable one into the overflow menu.
#[test]
fn ribbon_hides_contextual_groups_that_cannot_apply() {
    let empty = app();
    assert_eq!(describe(&empty, CommandId::CurveFit).ribbon, None);
    assert_eq!(describe(&empty, CommandId::RunCurveFit).ribbon, None);
    assert_eq!(describe(&empty, CommandId::Regions).ribbon, None);
    assert_eq!(describe(&empty, CommandId::SeriesTable).ribbon, None);

    // A table restores the Curve Fit group but still has no series to region.
    let table = app_with_table(2);
    assert_eq!(
        describe(&table, CommandId::CurveFit).ribbon,
        Some(RibbonPlacement {
            tab: WorkflowTab::Analyze,
            group: "Curve Fit",
            priority: 0,
            applicability: Applicability::TableOnly,
        })
    );
    assert_eq!(describe(&table, CommandId::Regions).ribbon, None);
}

#[test]
fn ribbon_separates_peak_and_curve_fit_tasks() {
    assert_eq!(
        ribbon_placement(CommandId::TogglePrimarySidebar),
        Some(RibbonPlacement {
            tab: WorkflowTab::View,
            group: "Display",
            priority: 1,
            applicability: Applicability::Always,
        })
    );
    assert_eq!(
        ribbon_placement(CommandId::Regions),
        Some(RibbonPlacement {
            tab: WorkflowTab::Analyze,
            group: "Regions",
            priority: 0,
            applicability: Applicability::SeriesOnly,
        })
    );
    assert_eq!(
        ribbon_placement(CommandId::LineFit),
        Some(RibbonPlacement {
            tab: WorkflowTab::Analyze,
            group: "Peak Fit",
            priority: 0,
            applicability: Applicability::Always,
        })
    );
    assert_eq!(
        ribbon_placement(CommandId::CurveFit),
        Some(RibbonPlacement {
            tab: WorkflowTab::Analyze,
            group: "Curve Fit",
            priority: 0,
            applicability: Applicability::TableOnly,
        })
    );
}

/// The Figure tab: canvas creation, the chart-gallery entry, themes, and the
/// figure endpoints (copy + the two mainstream export formats).
#[test]
fn figure_tab_groups_chart_creation_styling_and_output() {
    let empty = app();
    for (id, group) in [
        (CommandId::NewCanvas(0), "Create"),
        (
            CommandId::ApplyTheme(plotx_core::theme::Theme::all()[0].id),
            "Style",
        ),
        (CommandId::CopyFigure, "Output"),
        (CommandId::Export(ExportFormat::Png), "Output"),
        (CommandId::Export(ExportFormat::Svg), "Output"),
    ] {
        let placement = describe(&empty, id).ribbon.expect("Figure tab placement");
        assert_eq!(placement.tab, WorkflowTab::Figure, "{id:?}");
        assert_eq!(placement.group, group, "{id:?}");
    }
    // Only the two figure-endpoint formats earn Ribbon buttons; the rest stay
    // in the File menu and the palette.
    assert_eq!(
        describe(&empty, CommandId::Export(ExportFormat::Pdf)).ribbon,
        None
    );

    // The chart-gallery entry shows and enables with a plotted table, and
    // hides (kind gate) for other dataset kinds.
    let table = app_with_table(1);
    let chart = describe(&table, CommandId::ChartType);
    assert!(chart.enabled);
    assert_eq!(
        chart.ribbon,
        Some(RibbonPlacement {
            tab: WorkflowTab::Figure,
            group: "Chart",
            priority: 0,
            applicability: Applicability::TableOnly,
        })
    );
    assert_eq!(describe(&app_with_nmr(), CommandId::ChartType).ribbon, None);
    assert_eq!(
        describe(&empty, CommandId::ChartType).disabled_reason,
        Some("Select a data table before choosing a chart type.")
    );
}

/// Vertical alignment sits on the Ribbon alongside horizontal alignment.
#[test]
fn all_align_modes_share_one_ribbon_group() {
    for mode in [
        Align::Left,
        Align::HCenter,
        Align::Right,
        Align::Top,
        Align::VCenter,
        Align::Bottom,
    ] {
        assert_eq!(
            ribbon_placement(CommandId::Align(mode)),
            Some(RibbonPlacement {
                tab: WorkflowTab::Arrange,
                group: "Align",
                priority: 1,
                applicability: Applicability::Always,
            })
        );
    }
}
