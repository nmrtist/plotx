use plotx_core::actions::ZOrder;
use plotx_core::export::ExportFormat;
use plotx_core::layout::{Align, Distribute, GutterPreset, SpacingMode};
use plotx_core::properties::PropertyStep;
use plotx_core::state::{Dataset, PlotxApp, Tool, ToolGroup, WorkflowTab};

pub use super::command_exec::{execute, execute_without_clipboard};
mod identity;
use identity::command_identity;
pub(crate) use identity::recent_entry_label;
mod helpers;
pub(super) use helpers::chart_plot_target;
use helpers::{requires, selected_paths_unlocked, tool_commands};
mod ribbon;
use ribbon::ribbon_placement;
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
    LineAlignmentOnly,
    TableOnly,
    SeriesOnly,
    Homonuclear2dOnly,
    ToolGroup(ToolGroup),
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CommandId {
    NewProject,
    OpenProject,
    CloseProject,
    OpenFile,
    OpenFolder,
    RunBatchWorkflow,
    RunScientificScript,
    OpenRecent(usize),
    ClearRecentFiles,
    HelpManual,
    ImportTable,
    ImportImage,
    ImportImageFirstFrame,
    ImportImageWithoutMetadata,
    ImportTiffPages,
    PasteImage,
    CancelImageImport,
    ReplaceImage,
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
    DeselectAll,
    Group,
    Ungroup,
    CreatePanel,
    ComposePanel,
    DissolvePanel,
    DeletePanel,
    DuplicatePanel,
    MergePanels,
    SplitPanel,
    ReorderPanelLabels,
    SetPanelLayout(plotx_core::state::PanelLayout),
    MoveContentToPanel(Option<plotx_core::state::PanelId>),
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
    AlignTraces,
    StackData,
    ExtractMassSpectrum,
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
    SetCanvasSizePreset(&'static str),
    ArrangeGrid(u32, u32),
    SimplifyInnerAxes,
    SetSpacingMode(SpacingMode),
    SetGutterPreset(GutterPreset),
    Align(Align),
    Distribute(Distribute),
    ZOrder(ZOrder),
    ApplyTheme(&'static str),
    /// Reveal a whole group of catalog properties at its canonical home (§8.5
    /// channel 2). Registered per declared group, so a group gains a Ribbon
    /// button, a menu entry and a palette hit by being declared once. It
    /// navigates and never edits: the panel it opens owns the controls.
    PropertyGroup(&'static str),
    /// Move the canvas-steppable property one rung (§8.5 channel 3). The
    /// property is derived from the catalog, so the binding does not name one.
    StepProperty(PropertyStep),
    CycleCursor,
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
        CommandId::NewProject,
        CommandId::OpenProject,
        CommandId::CloseProject,
        CommandId::OpenFile,
        CommandId::OpenFolder,
        CommandId::RunBatchWorkflow,
        CommandId::RunScientificScript,
        CommandId::ClearRecentFiles,
        CommandId::HelpManual,
        CommandId::ImportTable,
        CommandId::ImportImage,
        CommandId::ImportImageFirstFrame,
        CommandId::ImportImageWithoutMetadata,
        CommandId::ImportTiffPages,
        CommandId::PasteImage,
        CommandId::CancelImageImport,
        CommandId::ReplaceImage,
        CommandId::PasteTable,
        CommandId::SaveProject,
        CommandId::NewTable,
        CommandId::ExportData,
        CommandId::CopyFigure,
        CommandId::Quit,
        CommandId::Undo,
        CommandId::Redo,
        CommandId::SelectAll,
        CommandId::DeselectAll,
        CommandId::Group,
        CommandId::Ungroup,
        CommandId::CreatePanel,
        CommandId::ComposePanel,
        CommandId::DissolvePanel,
        CommandId::DeletePanel,
        CommandId::DuplicatePanel,
        CommandId::MergePanels,
        CommandId::SplitPanel,
        CommandId::ReorderPanelLabels,
        CommandId::SetPanelLayout(plotx_core::state::PanelLayout::Free),
        CommandId::SetPanelLayout(plotx_core::state::PanelLayout::VerticalStack),
        CommandId::SetPanelLayout(plotx_core::state::PanelLayout::HorizontalStack),
        CommandId::SetPanelLayout(plotx_core::state::PanelLayout::Grid { rows: 2, cols: 2 }),
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
        CommandId::AlignTraces,
        CommandId::StackData,
        CommandId::ExtractMassSpectrum,
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
    // Every declared property group, and the step gesture. Both are derived
    // from the property catalog: a group declared once appears here, and a
    // property that declares itself steppable is driven by the existing
    // binding without any new command.
    ids.extend(
        super::properties::GROUPS
            .iter()
            .map(|group| CommandId::PropertyGroup(group.section)),
    );
    ids.extend([PropertyStep::Lower, PropertyStep::Raise].map(CommandId::StepProperty));
    ids.push(CommandId::CycleCursor);
    ids.extend(tool_commands().into_iter().map(CommandId::Tool));
    ids.into_iter()
        .map(|id| {
            debug_assert!(!id.stable_id().is_empty());
            describe(app, id)
        })
        .collect()
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
    let is_frequency_nmr = || {
        dataset().is_some_and(|dataset| {
            dataset
                .as_nmr()
                .is_some_and(|nmr| nmr.output_domain() == plotx_io::Domain::Frequency)
        })
    };
    let is_time_nmr = || {
        dataset().is_some_and(|dataset| {
            dataset
                .as_nmr()
                .is_some_and(|nmr| nmr.output_domain() == plotx_io::Domain::Time)
        })
    };
    let has_selectable_analysis_trace = || has_trace() && !is_time_nmr();
    let has_generic_peak_fit_trace = || has_trace() && (is_frequency_nmr() || is_table());
    let range = || active_dataset.and_then(|di| app.analysis_range_for(di));

    let is_series = || dataset().is_some_and(Dataset::supports_region_analysis);

    let (label, icon, checked) = command_identity(app, id);
    // The gate decides the enabled state and the disabled tooltip together, so a
    // command can never be blocked by one requirement while explaining another.
    // `and_then` reports the first unmet requirement and skips the rest.
    let gate: Result<(), &'static str> = match id {
        CommandId::RunScientificScript => requires(
            app.doc
                .datasets
                .iter()
                .any(|dataset| dataset.as_mass_spec().is_some()),
            "Load an LC–MS dataset before running a scientific script.",
        ),
        CommandId::CloseProject => requires(
            app.session.project_present
                || app.doc.project_path.is_some()
                || !app.doc.datasets.is_empty()
                || !app.doc.canvases.is_empty()
                || app.doc.dirty,
            "There is no project to close.",
        ),
        CommandId::SaveProject => requires(
            !app.session.ui.project_save_in_progress,
            "Wait for the current project save to finish.",
        ),
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
        CommandId::ImportImage
        | CommandId::ImportImageFirstFrame
        | CommandId::ImportImageWithoutMetadata
        | CommandId::ImportTiffPages
        | CommandId::PasteImage => Ok(()),
        CommandId::CancelImageImport => requires(
            super::file_dialogs::image_import::has_active_jobs(),
            "Start an image import before cancelling it.",
        ),
        CommandId::ReplaceImage => requires(
            app.session.active_canvas.is_some_and(|ci| {
                app.session
                    .ui
                    .hierarchical_selection
                    .lead()
                    .and_then(|path| path.content)
                    .and_then(|id| app.doc.canvases[ci].object(id))
                    .is_some_and(|item| {
                        matches!(
                            item.kind,
                            plotx_core::state::CanvasObjectKind::RasterImage(_)
                        ) && !item.locked
                    })
            }),
            "Select an unlocked image before replacing it.",
        ),
        CommandId::ExportData => requires(
            dataset().is_some_and(|dataset| {
                !plotx_core::data_export::DataExportAvailability::for_dataset(dataset).is_empty()
            }),
            "Select a dataset with processed data or analysis results to export.",
        ),
        CommandId::Export(_) => requires(has_canvas, "Open a canvas before exporting a figure."),
        CommandId::CopyFigure => requires(
            super::clipboard_figure::resolve_copy_target(app).is_some_and(|canvas| {
                !app.doc.canvases[canvas].objects.iter().any(|item| {
                    matches!(
                        item.kind,
                        plotx_core::state::CanvasObjectKind::RasterImage(_)
                    )
                })
            }),
            "Open a figure without external images; Copy Figure cannot include them yet.",
        ),
        CommandId::Undo => requires(app.can_undo(), "Nothing to undo yet."),
        CommandId::Redo => requires(app.can_redo(), "Nothing to redo yet."),
        CommandId::SelectAll | CommandId::DeselectAll => requires(
            has_canvas || !app.doc.datasets.is_empty(),
            "Open a canvas or dataset before changing the selection.",
        ),
        CommandId::Group => requires(
            selected >= 2,
            "Select at least two objects before grouping them.",
        ),
        CommandId::Ungroup => requires(
            selected >= 1,
            "Select at least one object before ungrouping it.",
        ),
        CommandId::CreatePanel => {
            requires(has_canvas, "Open a figure page before creating a panel.")
        }
        CommandId::ComposePanel => requires(
            app.session
                .ui
                .hierarchical_selection
                .paths()
                .iter()
                .any(|path| path.content.is_some() && path.panel.is_none())
                && selected_paths_unlocked(app),
            "Select one or more unlocked loose content items before composing a panel.",
        ),
        CommandId::DissolvePanel
        | CommandId::DeletePanel
        | CommandId::DuplicatePanel
        | CommandId::SetPanelLayout(_) => requires(
            app.session
                .ui
                .hierarchical_selection
                .lead()
                .is_some_and(|path| path.panel.is_some())
                && selected_paths_unlocked(app),
            "Select an unlocked panel before using this command.",
        ),
        CommandId::MoveContentToPanel(target) => requires(
            selected >= 1
                && selected_paths_unlocked(app)
                && target.is_none_or(|panel| {
                    app.session.active_canvas.is_some_and(|ci| {
                        app.doc.canvases[ci]
                            .panel(panel)
                            .is_some_and(|panel| !panel.locked)
                    })
                }),
            "Select unlocked sibling content and an unlocked destination panel.",
        ),
        CommandId::MergePanels => requires(
            app.session
                .ui
                .hierarchical_selection
                .paths()
                .iter()
                .filter(|path| path.panel.is_some() && path.content.is_none())
                .count()
                >= 2
                && selected_paths_unlocked(app),
            "Select at least two unlocked sibling panels before merging them.",
        ),
        CommandId::SplitPanel => requires(
            app.session
                .ui
                .hierarchical_selection
                .paths()
                .iter()
                .any(|path| path.panel.is_some() && path.content.is_some())
                && selected_paths_unlocked(app),
            "Select unlocked content inside an unlocked panel before splitting it.",
        ),
        CommandId::ReorderPanelLabels => requires(
            has_canvas
                && app
                    .session
                    .active_canvas
                    .is_some_and(|ci| !app.doc.canvases[ci].panels.is_empty()),
            "Create a panel before renumbering panel labels.",
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
        CommandId::AlignTraces => requires(
            app.trace_alignment_target().is_some(),
            "Select a plot with at least two visible line series that use the same x-axis unit.",
        ),
        CommandId::StackData => requires(
            app.stackable_selection().is_some(),
            "Select at least two compatible datasets. Trace collections such as electrophysiology require compatible axes and units.",
        ),
        CommandId::ExtractMassSpectrum => requires(
            dataset().is_some_and(|dataset| {
                dataset.tool_groups().contains(&ToolGroup::MassSpectrometry)
            }),
            "Select an LC–MS dataset before extracting a mass spectrum.",
        ),
        CommandId::SelectRange => requires(
            has_selectable_analysis_trace()
                || dataset().is_some_and(|dataset| {
                    dataset.tool_groups().contains(&ToolGroup::MassSpectrometry)
                }),
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
                    .and_then(Dataset::region_analysis)
                    .is_some_and(|state| !state.regions.is_empty()),
                "Add at least one region before building a series table.",
            )
        }),
        CommandId::DetectPeaks => requires(
            dataset().is_some_and(|dataset| {
                !is_time_nmr()
                    && dataset.has_displayed_trace(app.session.ui.peak_column)
                    && dataset.peaks().is_some()
            }),
            "Select a plotted 1D spectrum or table column before detecting peaks.",
        ),
        CommandId::PeakList => requires(
            has_generic_peak_fit_trace(),
            "Plot frequency-domain or tabular 1D data before opening the peak list.",
        ),
        CommandId::LineFit => requires(
            has_generic_peak_fit_trace(),
            "Plot frequency-domain or tabular 1D data before fitting peaks.",
        ),
        CommandId::RunPeakFit => requires(
            has_generic_peak_fit_trace(),
            "Plot frequency-domain or tabular 1D data before running Peak Fit.",
        )
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
        CommandId::Integrate => requires(
            is_frequency_nmr(),
            "Select a frequency-domain 1D NMR spectrum before integrating it.",
        ),
        CommandId::Multiplets => requires(
            is_frequency_nmr(),
            "Select a frequency-domain 1D NMR spectrum before analyzing multiplets.",
        )
        .and_then(|()| {
            requires(
                range().is_some(),
                "Draw an analysis range before analyzing multiplets.",
            )
        }),
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
            selected >= 1
                || app
                    .session
                    .ui
                    .hierarchical_selection
                    .paths()
                    .iter()
                    .any(|path| path.panel.is_some() && path.content.is_none()),
            "Select an object or panel before changing its stacking order.",
        ),
        CommandId::PropertyGroup(section) => requires(
            super::properties::discovery::group_applies(app, section),
            super::properties::discovery::group(section)
                .map(|group| group.unavailable_reason)
                .unwrap_or("Select an object that has these settings."),
        ),
        CommandId::StepProperty(_) => requires(
            super::properties::discovery::step_target(app).is_some(),
            "Select a plot whose series draws contours before stepping its lowest level.",
        ),
        CommandId::CycleCursor
        | CommandId::Tool(Tool::InspectCursor)
        | CommandId::Tool(Tool::DeltaCursor) => requires(
            matches!(
                dataset(),
                Some(Dataset::Nmr(nmr))
                    if nmr.output_domain() == plotx_io::Domain::Frequency
            ) || matches!(
                dataset(),
                Some(Dataset::Nmr2D(nmr)) if nmr.is_true_2d()
            ),
            "Select a frequency-domain 1D or true-2D NMR spectrum.",
        ),
        CommandId::Tool(Tool::Symmetry) => requires(
            dataset()
                .and_then(Dataset::as_nmr2d)
                .is_some_and(|dataset| dataset.supports_symmetry_review()),
            "Select a homonuclear COSY, TOCSY, or NOESY / ROESY contour spectrum.",
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
            && app.session.ui.trace_alignment_dialog.is_none()
            && app.session.ui.trace_composer.is_none()
            && !app.session.ui.interaction.is_active());
    // Activation requirements must not trap an already-active tool after the
    // dataset context changes: its command remains available for deactivation.
    let active_tool = id
        .tool_target()
        .is_some_and(|tool| app.session.tool == tool);
    let enabled = (gate.is_ok() || active_tool) && palette_available;
    let disabled_reason = if enabled { None } else { gate.err() };
    // A contextual group is dropped from the Ribbon entirely rather than shown
    // permanently dead: a dead group still consumes the width budget in
    // `groups_for_tab`, pushing usable groups into the overflow menu.
    let ribbon = ribbon_placement(id).filter(|placement| match placement.applicability {
        Applicability::Always => true,
        Applicability::LineAlignmentOnly => app.trace_alignment_target().is_some(),
        Applicability::TableOnly => is_table(),
        Applicability::SeriesOnly => is_series(),
        Applicability::Homonuclear2dOnly => dataset()
            .and_then(Dataset::as_nmr2d)
            .is_some_and(|dataset| dataset.preset.homonuclear()),
        Applicability::ToolGroup(group) => {
            dataset().is_some_and(|dataset| dataset.tool_groups().contains(&group))
        }
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

#[cfg(test)]
#[path = "commands_alignment_tests.rs"]
mod alignment_tests;
#[cfg(test)]
#[path = "commands_mass_spec_tests.rs"]
mod mass_spec_tests;
#[cfg(test)]
#[path = "commands_tests.rs"]
mod tests;
#[cfg(test)]
#[path = "commands_xps_tests.rs"]
mod xps_tests;
