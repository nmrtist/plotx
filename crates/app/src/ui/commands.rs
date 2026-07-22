//! Shared command descriptions and dispatch. Menus, the Ribbon, shortcuts and
//! the command palette all ask this module for the same live state and execute
//! the same stable command IDs.

use plotx_core::actions::ZOrder;
use plotx_core::export::ExportFormat;
use plotx_core::layout::{Align, Distribute, GutterPreset, SpacingMode};
use plotx_core::state::{Dataset, ObjectId, PlotxApp, Tool, WorkflowTab};

pub use super::command_exec::execute;

mod identity;
use identity::command_identity;
pub(crate) use identity::recent_entry_label;

/// The published user manual; opened by `HelpManual` and linked from About.
pub(crate) const MANUAL_URL: &str = "https://docs.plotx.nmrtist.space/";
/// The public source repository, linked from About.
pub(crate) const REPOSITORY_URL: &str = "https://github.com/nmrtist/plotx";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RibbonPlacement {
    pub tab: WorkflowTab,
    pub group: &'static str,
    /// Lower values survive longer as space becomes constrained.
    pub priority: u8,
    pub applicability: Applicability,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Applicability {
    Always,
    TableOnly,
    SeriesOnly,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CommandId {
    OpenProject,
    OpenFile,
    OpenFolder,
    RunBatchWorkflow,
    /// Reopen the recent-list entry at this index (newest first). Registered
    /// per live entry, so the index always resolves against the current list.
    OpenRecent(usize),
    ClearRecentFiles,
    HelpManual,
    ImportTable,
    PasteTable,
    SaveProject,
    NewTable,
    NewCanvas(usize),
    ExportData,
    Export(ExportFormat),
    CopyFigure,
    Quit,
    Undo,
    Redo,
    SelectAll,
    Group,
    Ungroup,
    TogglePrimarySidebar,
    ToggleSecondarySidebar,
    ZoomToFit,
    ZoomToSelection,
    UiScaleUp,
    UiScaleDown,
    UiScaleReset,
    Present,
    ToggleGrid,
    ToggleSnap,
    Preferences,
    CommandPalette,
    CheckUpdates,
    OperationHistory,
    About,
    SaveProcessingTemplate,
    ApplyProcessingTemplate,
    SpectrumArithmetic,
    AlignSpectra,
    StackData,
    SelectRange,
    ClearRange,
    Regions,
    SeriesTable,
    DetectPeaks,
    PeakList,
    LineFit,
    RunPeakFit,
    CurveFit,
    RunCurveFit,
    Statistics,
    ChartType,
    FigureTypography,
    Integrate,
    Multiplets,
    TidyBoard,
    CanvasSettings,
    /// Apply the size preset with this catalog id (`SizePreset::id`) to the
    /// active canvas. Registered per catalog entry so every preset is
    /// palette-searchable.
    SetCanvasSizePreset(&'static str),
    ArrangeGrid(u32, u32),
    SimplifyInnerAxes,
    SetSpacingMode(SpacingMode),
    SetGutterPreset(GutterPreset),
    Align(Align),
    Distribute(Distribute),
    ZOrder(ZOrder),
    ApplyTheme(&'static str),
    Tool(Tool),
}

/// Architectural ownership for commands that expose or consume results. New
/// commands must choose a class instead of bypassing the automation registry
/// with an arbitrary Action.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CommandExecutionClass {
    UiOnly,
    ToolEditor,
    ToolBacked,
}

impl CommandId {
    pub fn execution_class(self) -> CommandExecutionClass {
        match self {
            Self::RunBatchWorkflow => CommandExecutionClass::ToolEditor,
            Self::OperationHistory | Self::CommandPalette | Self::About => {
                CommandExecutionClass::UiOnly
            }
            Self::ExportData
            | Self::Export(_)
            | Self::ApplyProcessingTemplate
            | Self::ApplyTheme(_) => CommandExecutionClass::ToolBacked,
            _ => CommandExecutionClass::UiOnly,
        }
    }
}

pub struct CommandDescriptor {
    pub id: CommandId,
    pub execution_class: CommandExecutionClass,
    pub label: String,
    pub icon: Option<&'static str>,
    pub enabled: bool,
    /// `Some(state)` for toggle commands, `None` for plain actions. Every
    /// surface derives "renders as a check item" from `is_some()`, so a new
    /// toggle needs no per-surface registration.
    pub checked: Option<bool>,
    pub disabled_reason: Option<&'static str>,
    pub shortcut: Option<String>,
    pub ribbon: Option<RibbonPlacement>,
}

pub fn catalog(app: &PlotxApp) -> Vec<CommandDescriptor> {
    let mut ids = vec![
        CommandId::OpenProject,
        CommandId::OpenFile,
        CommandId::OpenFolder,
        CommandId::RunBatchWorkflow,
        CommandId::ClearRecentFiles,
        CommandId::HelpManual,
        CommandId::ImportTable,
        CommandId::PasteTable,
        CommandId::SaveProject,
        CommandId::NewTable,
        CommandId::ExportData,
        CommandId::CopyFigure,
        CommandId::Quit,
        CommandId::Undo,
        CommandId::Redo,
        CommandId::SelectAll,
        CommandId::Group,
        CommandId::Ungroup,
        CommandId::TogglePrimarySidebar,
        CommandId::ToggleSecondarySidebar,
        CommandId::ZoomToFit,
        CommandId::ZoomToSelection,
        CommandId::UiScaleUp,
        CommandId::UiScaleDown,
        CommandId::UiScaleReset,
        CommandId::Present,
        CommandId::ToggleGrid,
        CommandId::ToggleSnap,
        CommandId::Preferences,
        CommandId::CommandPalette,
        CommandId::CheckUpdates,
        CommandId::OperationHistory,
        CommandId::About,
        CommandId::SaveProcessingTemplate,
        CommandId::ApplyProcessingTemplate,
        CommandId::SpectrumArithmetic,
        CommandId::AlignSpectra,
        CommandId::StackData,
        CommandId::SelectRange,
        CommandId::ClearRange,
        CommandId::Regions,
        CommandId::SeriesTable,
        CommandId::DetectPeaks,
        CommandId::PeakList,
        CommandId::LineFit,
        CommandId::RunPeakFit,
        CommandId::CurveFit,
        CommandId::RunCurveFit,
        CommandId::Statistics,
        CommandId::ChartType,
        CommandId::FigureTypography,
        CommandId::Integrate,
        CommandId::Multiplets,
        CommandId::TidyBoard,
        CommandId::CanvasSettings,
        CommandId::SimplifyInnerAxes,
    ];
    ids.extend((0..app.session.recent_files.len()).map(CommandId::OpenRecent));
    ids.extend(
        plotx_core::templates::CanvasTemplate::all()
            .iter()
            .enumerate()
            .map(|(i, _)| CommandId::NewCanvas(i)),
    );
    ids.extend([SpacingMode::Frame, SpacingMode::Visual].map(CommandId::SetSpacingMode));
    ids.extend(GutterPreset::ALL.map(CommandId::SetGutterPreset));
    ids.extend(
        [
            ExportFormat::Svg,
            ExportFormat::Pdf,
            ExportFormat::Png,
            ExportFormat::Jpeg,
            ExportFormat::Tiff,
        ]
        .into_iter()
        .map(CommandId::Export),
    );
    ids.extend(
        plotx_core::state::size_presets()
            .iter()
            .map(|preset| CommandId::SetCanvasSizePreset(preset.id)),
    );
    ids.extend(
        plotx_core::layout::GRID_PRESETS
            .iter()
            .map(|&(_, rows, cols)| CommandId::ArrangeGrid(rows, cols)),
    );
    ids.extend([
        CommandId::Align(Align::Left),
        CommandId::Align(Align::HCenter),
        CommandId::Align(Align::Right),
        CommandId::Align(Align::Top),
        CommandId::Align(Align::VCenter),
        CommandId::Align(Align::Bottom),
        CommandId::Distribute(Distribute::Horizontal),
        CommandId::Distribute(Distribute::Vertical),
        CommandId::ZOrder(ZOrder::Front),
        CommandId::ZOrder(ZOrder::Forward),
        CommandId::ZOrder(ZOrder::Backward),
        CommandId::ZOrder(ZOrder::Back),
    ]);
    ids.extend(
        plotx_core::theme::Theme::all()
            .into_iter()
            .map(|theme| CommandId::ApplyTheme(theme.id)),
    );
    ids.extend(tool_commands().into_iter().map(CommandId::Tool));
    ids.into_iter()
        .map(|id| {
            debug_assert!(!id.stable_id().is_empty());
            describe(app, id)
        })
        .collect()
}

/// `Ok` when a requirement holds, otherwise the sentence a surface shows in the
/// disabled tooltip. Chain with `and_then` to report the first unmet one.
fn requires(ok: bool, reason: &'static str) -> Result<(), &'static str> {
    if ok { Ok(()) } else { Err(reason) }
}

/// The canvas and plot object displaying `dataset`, preferring the active
/// canvas so Chart Type lands on the plot the user is looking at.
pub(super) fn chart_plot_target(app: &PlotxApp, dataset: usize) -> Option<(usize, ObjectId)> {
    let candidates = app
        .session
        .active_canvas
        .into_iter()
        .chain(0..app.doc.canvases.len());
    for ci in candidates {
        let Some(canvas) = app.doc.canvases.get(ci) else {
            continue;
        };
        let hit = canvas.objects.iter().find(|object| {
            object
                .plot()
                .is_some_and(|plot| plot.binding.primary_dataset() == dataset)
        });
        if let Some(object) = hit {
            return Some((ci, object.id));
        }
    }
    None
}

pub fn describe(app: &PlotxApp, id: CommandId) -> CommandDescriptor {
    let has_canvas = app.session.active_canvas.is_some();
    let selected = app.session.ui.selection.objects().len();
    let active_dataset = app
        .active_dataset()
        .filter(|&di| di < app.doc.datasets.len());
    // Contextual predicates are evaluated per command arm: `describe` runs for
    // every command on every catalog pass, so predicates that touch dataset
    // internals must not run for the dozens of commands that ignore them.
    let dataset = || active_dataset.map(|di| &app.doc.datasets[di]);
    let table = || dataset().and_then(Dataset::as_table);
    let is_table = || table().is_some();
    let has_curves = || table().is_some_and(|table| !table.series_bindings.is_empty());
    let has_trace = || dataset().is_some_and(|d| d.has_displayed_trace(None));
    let range = || active_dataset.and_then(|di| app.analysis_range_for(di));

    let is_series = || dataset().is_some_and(Dataset::supports_region_analysis);

    let (label, icon, checked) = command_identity(app, id);
    // The gate decides the enabled state and the disabled tooltip together, so a
    // command can never be blocked by one requirement while explaining another.
    // `and_then` reports the first unmet requirement and skips the rest.
    let gate: Result<(), &'static str> = match id {
        CommandId::OpenRecent(index) => requires(
            index < app.session.recent_files.len(),
            "Open a file or project to fill the recent list.",
        ),
        CommandId::ClearRecentFiles => requires(
            !app.session.recent_files.is_empty(),
            "Open a file or project to build the recent list.",
        ),
        CommandId::ImportTable => requires(
            app.session.ui.table_import_preview.is_none(),
            "Finish or cancel the current table import preview before importing another table.",
        ),
        CommandId::ExportData => requires(
            dataset().is_some_and(|dataset| {
                !plotx_core::data_export::DataExportAvailability::for_dataset(dataset).is_empty()
            }),
            "Select a dataset with processed data or analysis results to export.",
        ),
        CommandId::Export(_) => requires(has_canvas, "Open a canvas before exporting a figure."),
        CommandId::CopyFigure => requires(
            super::clipboard_figure::resolve_copy_target(app).is_some(),
            "Open a canvas or select a page frame before copying a figure.",
        ),
        CommandId::Undo => requires(app.can_undo(), "Nothing to undo yet."),
        CommandId::Redo => requires(app.can_redo(), "Nothing to redo yet."),
        CommandId::SelectAll => requires(has_canvas, "Open a canvas before selecting objects."),
        CommandId::Group => requires(
            selected >= 2,
            "Select at least two objects before grouping them.",
        ),
        CommandId::Ungroup => requires(
            selected >= 1,
            "Select at least one object before ungrouping it.",
        ),
        CommandId::ZoomToFit => requires(has_canvas, "Open a canvas before zooming to fit."),
        CommandId::ZoomToSelection => {
            requires(has_canvas, "Open a canvas before zooming to the selection.")
        }
        CommandId::UiScaleUp | CommandId::UiScaleDown => requires(
            app.session.monitor.is_some(),
            "Wait for the display probe before changing the UI scale.",
        ),
        CommandId::UiScaleReset => requires(
            app.session
                .monitor
                .as_ref()
                .is_some_and(|monitor| monitor.user.is_some()),
            "Adjust the UI scale before resetting it to automatic.",
        ),
        CommandId::Present => requires(
            has_canvas,
            "Open a canvas before entering presentation mode.",
        ),
        CommandId::ToggleGrid => {
            requires(has_canvas, "Open a canvas before changing its layout grid.")
        }
        CommandId::SaveProcessingTemplate => requires(
            active_dataset
                .is_some_and(|di| super::processing_templates::can_use_templates(app, di)),
            "Select a non-table dataset before saving a processing template.",
        ),
        CommandId::ApplyProcessingTemplate => requires(
            active_dataset
                .is_some_and(|di| super::processing_templates::can_use_templates(app, di)),
            "Select a non-table dataset before applying a processing template.",
        ),
        CommandId::SpectrumArithmetic => requires(
            !app.spectrum_arithmetic_targets().is_empty(),
            "Load a non-empty 1D NMR spectrum before using Spectrum Arithmetic.",
        ),
        CommandId::AlignSpectra => requires(
            app.can_align_spectra(),
            "Select at least two non-empty 1D NMR spectra, or clear the selection to use all spectra.",
        ),
        CommandId::StackData => requires(
            app.stackable_selection().is_some(),
            "Select at least two compatible datasets before stacking them.",
        ),
        CommandId::SelectRange => requires(
            has_trace(),
            "Plot 1D data before selecting an analysis range.",
        ),
        CommandId::ClearRange => requires(
            range().is_some(),
            "Draw an analysis range before clearing it.",
        ),
        CommandId::Regions => requires(
            is_series(),
            "Select a series dataset before drawing regions.",
        ),
        CommandId::SeriesTable => requires(
            is_series(),
            "Select a series dataset before building a series table.",
        )
        .and_then(|()| {
            requires(
                dataset()
                    .and_then(Dataset::as_nmr2d)
                    .is_some_and(|series| !series.regions.is_empty()),
                "Add at least one region before building a series table.",
            )
        }),
        CommandId::DetectPeaks => requires(
            dataset().is_some_and(|dataset| {
                dataset.has_displayed_trace(app.session.ui.peak_column) && dataset.peaks().is_some()
            }),
            "Select a plotted 1D spectrum or table column before detecting peaks.",
        ),
        CommandId::PeakList => requires(has_trace(), "Plot 1D data before opening the peak list."),
        CommandId::LineFit => requires(has_trace(), "Plot 1D data before fitting peaks."),
        CommandId::RunPeakFit => requires(has_trace(), "Plot 1D data before running Peak Fit.")
            .and_then(|()| {
                requires(
                    range().is_some(),
                    "Draw an analysis range before running Peak Fit.",
                )
            })
            .and_then(|()| {
                requires(
                    app.session.line_fit_job.is_none(),
                    "Wait for the running peak fit to finish before starting another.",
                )
            }),
        CommandId::CurveFit => requires(is_table(), "Select a data table before fitting curves."),
        CommandId::RunCurveFit => {
            requires(is_table(), "Select a data table before running Curve Fit.").and_then(|()| {
                requires(
                    has_curves(),
                    "Add at least one curve column before running Curve Fit.",
                )
            })
        }
        CommandId::Statistics => requires(
            is_table(),
            "Select a data table before calculating statistics.",
        )
        .and_then(|()| {
            requires(
                has_curves(),
                "Add at least one table column before calculating statistics.",
            )
        }),
        CommandId::ChartType => requires(
            is_table(),
            "Select a data table before choosing a chart type.",
        )
        .and_then(|()| {
            requires(
                active_dataset.is_some_and(|di| chart_plot_target(app, di).is_some()),
                "Plot the table on a canvas before choosing its chart type.",
            )
        }),
        CommandId::Integrate => requires(has_trace(), "Plot 1D data before integrating it."),
        CommandId::Multiplets => requires(
            range().is_some(),
            "Draw an analysis range before analyzing multiplets.",
        ),
        CommandId::CanvasSettings => {
            requires(has_canvas, "Open a canvas before changing its settings.")
        }
        CommandId::SetCanvasSizePreset(_) => {
            requires(has_canvas, "Open a canvas before changing its size.")
        }
        CommandId::ArrangeGrid(_, _)
        | CommandId::SimplifyInnerAxes
        | CommandId::SetSpacingMode(_)
        | CommandId::SetGutterPreset(_) => {
            requires(has_canvas, "Open a canvas before arranging its plots.")
        }
        CommandId::ApplyTheme(_) => requires(has_canvas, "Open a canvas before applying a theme."),
        CommandId::FigureTypography => requires(
            has_canvas,
            "Open a canvas before adjusting figure typography.",
        ),
        CommandId::Align(_) => requires(
            selected >= 2,
            "Select at least two objects before aligning them.",
        ),
        CommandId::Distribute(_) => requires(
            selected >= 3,
            "Select at least three objects before distributing them.",
        ),
        CommandId::ZOrder(_) => requires(
            selected >= 1,
            "Select an object before changing its stacking order.",
        ),
        CommandId::Tool(tool) if tool.is_data_tool() => requires(
            dataset().is_some(),
            "Select a dataset before using this data tool.",
        ),
        CommandId::Tool(_) => requires(has_canvas, "Open a canvas before using this tool."),
        _ => Ok(()),
    };
    // The palette cannot be opened over a modal or active gesture. Those states
    // hide every surface that could display its disabled tooltip, so this is the
    // one intentional disabled-without-reason path.
    let palette_available = id != CommandId::CommandPalette
        || (app.session.ui.processing_scheme_dialog.is_none()
            && app.session.ui.processing_template_dialog.is_none()
            && app.session.ui.spectrum_arithmetic_dialog.is_none()
            && app.session.ui.align_spectra_dialog.is_none()
            && !app.session.ui.interaction.is_active());
    let enabled = gate.is_ok() && palette_available;
    let disabled_reason = if enabled { None } else { gate.err() };
    // A contextual group is dropped from the Ribbon entirely rather than shown
    // permanently dead: a dead group still consumes the width budget in
    // `groups_for_tab`, pushing usable groups into the overflow menu.
    let ribbon = ribbon_placement(id).filter(|placement| match placement.applicability {
        Applicability::Always => true,
        Applicability::TableOnly => is_table(),
        Applicability::SeriesOnly => is_series(),
    });
    CommandDescriptor {
        id,
        execution_class: id.execution_class(),
        label,
        icon,
        enabled,
        checked,
        disabled_reason,
        shortcut: super::shortcuts::shortcut_label(id),
        ribbon,
    }
}

fn ribbon_placement(id: CommandId) -> Option<RibbonPlacement> {
    use Applicability::{Always, SeriesOnly, TableOnly};
    use WorkflowTab::{Analyze, Arrange, Data, Figure, Process, View};
    let (tab, group, priority, applicability) = match id {
        CommandId::Tool(Tool::BrowseZoom) | CommandId::ZoomToFit | CommandId::ZoomToSelection => {
            (View, "Navigate", 0, Always)
        }
        CommandId::TogglePrimarySidebar
        | CommandId::ToggleSecondarySidebar
        | CommandId::ToggleGrid
        | CommandId::Present => (View, "Display", 1, Always),
        CommandId::OpenFile
        | CommandId::ImportTable
        | CommandId::OpenFolder
        | CommandId::PasteTable => (Data, "Import", 0, Always),
        CommandId::NewTable | CommandId::StackData => (Data, "Build", 1, Always),
        CommandId::ExportData => (Data, "Export", 0, Always),
        CommandId::Tool(Tool::Peaks) | CommandId::DetectPeaks | CommandId::PeakList => {
            (Analyze, "Peaks", 1, Always)
        }
        CommandId::Tool(Tool::ManualPhase) => (Process, "Correct", 0, Always),
        CommandId::SpectrumArithmetic | CommandId::AlignSpectra => {
            (Process, "Transform", 1, Always)
        }
        CommandId::ApplyProcessingTemplate | CommandId::SaveProcessingTemplate => {
            (Process, "Recipes", 2, Always)
        }
        CommandId::SelectRange | CommandId::ClearRange => (Analyze, "Range", 0, Always),
        CommandId::Regions => (Analyze, "Regions", 0, SeriesOnly),
        CommandId::SeriesTable => (Analyze, "Regions", 0, SeriesOnly),
        CommandId::LineFit | CommandId::RunPeakFit => (Analyze, "Peak Fit", 0, Always),
        CommandId::CurveFit | CommandId::RunCurveFit => (Analyze, "Curve Fit", 0, TableOnly),
        CommandId::Statistics => (Analyze, "Statistics", 0, TableOnly),
        CommandId::Integrate | CommandId::Multiplets => (Analyze, "Interpret", 1, Always),
        CommandId::NewCanvas(_) => (Figure, "Create", 0, Always),
        CommandId::ChartType => (Figure, "Chart", 0, TableOnly),
        CommandId::ApplyTheme(_) | CommandId::FigureTypography | CommandId::CanvasSettings => {
            (Figure, "Style", 1, Always)
        }
        // PNG and SVG cover the two figure endpoints (slides and publication);
        // the other formats stay in the File menu and the palette.
        CommandId::CopyFigure
        | CommandId::Export(ExportFormat::Png)
        | CommandId::Export(ExportFormat::Svg) => (Figure, "Output", 0, Always),
        CommandId::Tool(Tool::Select)
        | CommandId::ArrangeGrid(1, 2)
        | CommandId::ArrangeGrid(2, 2)
        | CommandId::SimplifyInnerAxes
        | CommandId::SetSpacingMode(_)
        | CommandId::SetGutterPreset(_)
        | CommandId::TidyBoard => (Arrange, "Layout", 0, Always),
        CommandId::Align(_) => (Arrange, "Align", 1, Always),
        CommandId::Distribute(_) => (Arrange, "Distribute", 2, Always),
        CommandId::ZOrder(_) => (Arrange, "Order", 2, Always),
        CommandId::ToggleSnap => (Arrange, "Guides", 1, Always),
        CommandId::Tool(
            Tool::Text | Tool::PanelLabel | Tool::Rect | Tool::Ellipse | Tool::Line | Tool::Arrow,
        ) => (Arrange, "Annotate", 3, Always),
        _ => return None,
    };
    Some(RibbonPlacement {
        tab,
        group,
        priority,
        applicability,
    })
}

fn tool_commands() -> [Tool; 14] {
    [
        Tool::Select,
        Tool::BrowseZoom,
        Tool::ManualPhase,
        Tool::Integrate,
        Tool::Peaks,
        Tool::Slice,
        Tool::LineFit,
        Tool::Annotate,
        Tool::Text,
        Tool::PanelLabel,
        Tool::Rect,
        Tool::Ellipse,
        Tool::Line,
        Tool::Arrow,
    ]
}

#[cfg(test)]
#[path = "commands_tests.rs"]
mod tests;
