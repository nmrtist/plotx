//! The command roster: every `CommandId` the catalog describes, in one
//! place, so `catalog()` and the exhaustive placement tests iterate the same
//! list.

use plotx_core::export::ExportFormat;
use plotx_core::layout::{Align, Distribute, GutterPreset, SpacingMode};
use plotx_core::properties::PropertyStep;

use super::helpers::tool_commands;
use super::{CommandId, ZOrder};

/// The full command roster; only the recent-files arm depends on app state.
pub(super) fn command_ids(recent_files: usize) -> Vec<CommandId> {
    let mut ids = vec![
        CommandId::NewProject,
        CommandId::OpenProject,
        CommandId::CloseProject,
        CommandId::OpenFile,
        CommandId::OpenFolder,
        CommandId::ImportNmrSampling,
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
        CommandId::FitPlotY,
        CommandId::FitPlotXY,
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
        CommandId::Craft,
        CommandId::RunCraft,
        CommandId::CraftComponentTable,
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
        CommandId::ArrangeGridCustom,
    ];
    ids.extend((0..recent_files).map(CommandId::OpenRecent));
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
        crate::ui::properties::GROUPS
            .iter()
            .map(|group| CommandId::PropertyGroup(group.section)),
    );
    ids.extend([PropertyStep::Lower, PropertyStep::Raise].map(CommandId::StepProperty));
    ids.extend(
        [
            plotx_core::state::XpsWorkbenchTab::Acquisition,
            plotx_core::state::XpsWorkbenchTab::Background,
            plotx_core::state::XpsWorkbenchTab::Components,
            plotx_core::state::XpsWorkbenchTab::Diagnostics,
        ]
        .map(CommandId::XpsWorkbench),
    );
    ids.push(CommandId::CycleCursor);
    ids.extend(tool_commands().into_iter().map(CommandId::Tool));
    ids
}
