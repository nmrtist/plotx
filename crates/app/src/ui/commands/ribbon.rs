use plotx_core::export::ExportFormat;
use plotx_core::state::{Tool, ToolGroup, WorkflowTab};

use super::{Applicability, CommandId, RibbonPlacement};

pub(super) fn ribbon_placement(id: CommandId) -> Option<RibbonPlacement> {
    use Applicability::{Always, Homonuclear2dOnly, LineAlignmentOnly, SeriesOnly, TableOnly};
    use WorkflowTab::{Analyze, Arrange, Data, Figure, Process, View};
    let (tab, group, priority, applicability) = match id {
        CommandId::Tool(Tool::BrowseZoom) | CommandId::ZoomToFit | CommandId::ZoomToSelection => {
            (View, "Navigate", 0, Always)
        }
        CommandId::TogglePrimarySidebar
        | CommandId::ToggleSecondarySidebar
        | CommandId::ToggleGrid
        | CommandId::Present
        | CommandId::Preferences => (View, "Display", 1, Always),
        CommandId::OpenFile
        | CommandId::ImportTable
        | CommandId::ImportImage
        | CommandId::OpenFolder
        | CommandId::PasteTable => (Data, "Import", 0, Always),
        CommandId::NewTable | CommandId::StackData => (Data, "Build", 1, Always),
        CommandId::ExportData => (Data, "Export", 0, Always),
        CommandId::Tool(Tool::Peaks) | CommandId::DetectPeaks | CommandId::PeakList => (
            Analyze,
            "Peaks",
            1,
            Applicability::ToolGroup(ToolGroup::Peaks),
        ),
        CommandId::Tool(Tool::Symmetry) => (Analyze, "Review", 1, Homonuclear2dOnly),
        CommandId::AlignTraces => (Analyze, "Align", 1, LineAlignmentOnly),
        CommandId::Tool(Tool::ManualPhase) => (Process, "Correct", 0, Always),
        CommandId::SpectrumArithmetic | CommandId::AlignSpectra => {
            (Process, "Transform", 1, Always)
        }
        CommandId::ApplyProcessingTemplate | CommandId::SaveProcessingTemplate => {
            (Process, "Recipes", 2, Always)
        }
        CommandId::SelectRange | CommandId::ClearRange => (Analyze, "Range", 0, Always),
        CommandId::ExtractMassSpectrum => (
            Analyze,
            "Extract",
            0,
            Applicability::ToolGroup(ToolGroup::MassSpectrometry),
        ),
        CommandId::Regions => (Analyze, "Regions", 0, SeriesOnly),
        CommandId::SeriesTable => (Analyze, "Regions", 0, SeriesOnly),
        CommandId::LineFit | CommandId::RunPeakFit => (
            Analyze,
            "Peak Fit",
            0,
            Applicability::ToolGroup(ToolGroup::LineFit),
        ),
        CommandId::CurveFit | CommandId::RunCurveFit => (Analyze, "Curve Fit", 0, TableOnly),
        CommandId::Statistics => (Analyze, "Statistics", 0, TableOnly),
        CommandId::Integrate | CommandId::Multiplets => (
            Analyze,
            "Interpret",
            1,
            Applicability::ToolGroup(ToolGroup::Nmr1dAnalysis),
        ),
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
        // The group's own declaration decides where it lands, so a new group
        // needs no arm here.
        CommandId::PropertyGroup(section) => {
            let spot = crate::ui::properties::discovery::group(section)?.ribbon;
            (spot.tab, spot.group, spot.priority, Always)
        }
        _ => return None,
    };
    Some(RibbonPlacement {
        tab,
        group,
        priority,
        applicability,
    })
}
