//! Display identity of every command: label, icon, toggle state, and the
//! stable IDs the palette and shortcut map key on. Split from `commands.rs`
//! to keep both halves within the repository source-size limit.

use egui_phosphor::regular as icon;
use plotx_core::actions::ZOrder;
use plotx_core::layout::{Align, Distribute, GutterPreset, SpacingMode};
use plotx_core::state::{PlotxApp, Tool};

use super::CommandId;

impl CommandId {
    pub fn stable_id(self) -> String {
        match self {
            Self::OpenRecent(i) => format!("file.open_recent.{i}"),
            Self::NewCanvas(i) => format!("file.new_canvas.{i}"),
            Self::Export(f) => format!("file.export.{}", f.extension()),
            Self::ArrangeGrid(r, c) => format!("arrange.grid.{r}x{c}"),
            Self::SetSpacingMode(mode) => format!("arrange.spacing_mode.{}", spacing_slug(mode)),
            Self::SetGutterPreset(preset) => format!("arrange.gutter.{}", gutter_slug(preset)),
            Self::Align(mode) => format!("arrange.align.{}", align_slug(mode)),
            Self::Distribute(Distribute::Horizontal) => "arrange.distribute.horizontal".into(),
            Self::Distribute(Distribute::Vertical) => "arrange.distribute.vertical".into(),
            Self::ZOrder(mode) => format!("arrange.order.{}", zorder_slug(mode)),
            Self::ApplyTheme(id) => format!("view.theme.{id}"),
            Self::SetCanvasSizePreset(id) => format!("figure.canvas_size.{id}"),
            Self::Tool(tool) => format!("tool.{}", tool_slug(tool)),
            _ => simple_stable_id(self).to_owned(),
        }
    }
}

pub(super) fn command_identity(
    app: &PlotxApp,
    id: CommandId,
) -> (String, Option<&'static str>, Option<bool>) {
    let plain = |text: &str, glyph| (text.to_owned(), glyph, None);
    match id {
        CommandId::OpenProject => plain("Open Project…", Some(icon::FOLDER_OPEN)),
        CommandId::OpenFile => plain("Open File…", Some(icon::FILE)),
        CommandId::OpenFolder => plain("Open Folder…", Some(icon::FOLDER)),
        CommandId::RunBatchWorkflow => plain("Automation…", Some(icon::PLAY)),
        CommandId::OpenRecent(i) => (
            recent_label(app, i),
            Some(icon::CLOCK_COUNTER_CLOCKWISE),
            None,
        ),
        CommandId::ClearRecentFiles => plain("Clear Recent Files", None),
        CommandId::HelpManual => plain("User Manual", Some(icon::BOOK_OPEN)),
        CommandId::ImportTable => plain("Import Table…", Some(icon::TABLE)),
        CommandId::PasteTable => plain("Paste Table from Clipboard", Some(icon::CLIPBOARD_TEXT)),
        CommandId::SaveProject => plain("Save Project", Some(icon::FLOPPY_DISK)),
        CommandId::NewTable => plain("New Empty Data Table", Some(icon::TABLE)),
        CommandId::ExportData => plain("Export Data…", Some(icon::EXPORT)),
        CommandId::NewCanvas(i) => {
            let label = plotx_core::templates::CanvasTemplate::all()
                .get(i)
                .map(|t| format!("New Canvas: {}", t.name))
                .unwrap_or_else(|| "New Canvas".to_owned());
            (label, Some(icon::FILE_PLUS), None)
        }
        CommandId::Export(format) => (
            format!("Export {}…", format.label()),
            Some(icon::EXPORT),
            None,
        ),
        // Copy works on the selected board frame as well as the active
        // canvas, so the gate must mirror the execution path's target lookup.
        CommandId::CopyFigure => ("Copy Figure".into(), Some(icon::COPY), None),
        CommandId::Quit => plain("Quit PlotX", None),
        CommandId::Undo => ("Undo".into(), Some(icon::ARROW_ARC_LEFT), None),
        CommandId::Redo => ("Redo".into(), Some(icon::ARROW_ARC_RIGHT), None),
        CommandId::SelectAll => ("Select All Objects".into(), None, None),
        CommandId::Group => ("Group Selection".into(), None, None),
        CommandId::Ungroup => ("Ungroup Selection".into(), None, None),
        CommandId::TogglePrimarySidebar => (
            "Left Sidebar".into(),
            Some(icon::SIDEBAR),
            Some(app.session.primary_sidebar_visible),
        ),
        CommandId::ToggleSecondarySidebar => (
            "Right Sidebar".into(),
            Some(icon::SIDEBAR),
            Some(app.session.secondary_sidebar_visible),
        ),
        CommandId::ZoomToFit => ("Zoom to Fit".into(), Some(icon::ARROWS_OUT), None),
        CommandId::ZoomToSelection => ("Zoom to Selection".into(), None, None),
        CommandId::UiScaleUp => (
            ui_scale_label(app, "Increase UI Scale"),
            Some(icon::MAGNIFYING_GLASS_PLUS),
            None,
        ),
        CommandId::UiScaleDown => (
            ui_scale_label(app, "Decrease UI Scale"),
            Some(icon::MAGNIFYING_GLASS_MINUS),
            None,
        ),
        CommandId::UiScaleReset => (
            "Reset UI Scale to Automatic".into(),
            Some(icon::ARROW_COUNTER_CLOCKWISE),
            None,
        ),
        CommandId::Present => (
            "Present Full Screen".into(),
            Some(icon::PROJECTOR_SCREEN),
            Some(app.session.present_mode),
        ),
        CommandId::ToggleGrid => {
            let checked = app
                .session
                .active_canvas
                .is_some_and(|ci| app.doc.canvases[ci].layout.show_grid);
            ("Layout Grid".into(), Some(icon::GRID_FOUR), Some(checked))
        }
        CommandId::ToggleSnap => (
            "Toggle Snapping".into(),
            Some(icon::MAGNET),
            Some(app.session.ui.snap_enabled),
        ),
        CommandId::Preferences => plain("Preferences…", Some(icon::GEAR_SIX)),
        CommandId::CommandPalette => plain("Command Palette…", Some(icon::MAGNIFYING_GLASS)),
        CommandId::CheckUpdates => plain("Check for Updates…", None),
        CommandId::OperationHistory => plain("Operation and Diagnostic History", None),
        CommandId::About => plain("About PlotX", None),
        CommandId::SaveProcessingTemplate => {
            plain("Save Processing Template…", Some(icon::BOOKMARK_SIMPLE))
        }
        CommandId::ApplyProcessingTemplate => {
            plain("Apply Processing Template…", Some(icon::MAGIC_WAND))
        }
        CommandId::SpectrumArithmetic => plain("Spectrum Arithmetic…", Some(icon::MATH_OPERATIONS)),
        CommandId::AlignSpectra => plain("Align Spectra…", Some(icon::ARROWS_LEFT_RIGHT)),
        CommandId::StackData => plain("Stack Selected Data", Some(icon::STACK)),
        CommandId::SelectRange => (
            "Analysis Range".into(),
            Some(icon::SELECTION),
            Some(app.session.tool == Tool::SelectRegion),
        ),
        CommandId::ClearRange => plain("Clear Range", Some(icon::X)),
        CommandId::Regions => (
            "Draw Regions".into(),
            Some(icon::RECTANGLE),
            Some(app.session.tool == Tool::Regions),
        ),
        CommandId::SeriesTable => plain("Series Table", Some(icon::TABLE)),
        CommandId::DetectPeaks => plain("Detect Peaks", Some(icon::WAVE_SINE)),
        CommandId::PeakList => (
            "Peak List".into(),
            Some(icon::LIST_BULLETS),
            Some(app.session.tool == Tool::Peaks),
        ),
        CommandId::LineFit => (
            "Peak Fit".into(),
            Some(icon::CHART_LINE),
            Some(app.session.tool == Tool::LineFit),
        ),
        CommandId::RunPeakFit => plain("Run Peak Fit", Some(icon::PLAY)),
        CommandId::CurveFit => plain("Fit Curves", Some(icon::FUNCTION)),
        CommandId::RunCurveFit => plain("Run Fit", Some(icon::PLAY)),
        CommandId::Statistics => plain("Statistics", Some(icon::CHART_BAR)),
        CommandId::ChartType => plain("Chart Type…", Some(icon::CHART_SCATTER)),
        CommandId::FigureTypography => plain("Figure Typography…", Some(icon::TEXT_AA)),
        CommandId::Integrate => (
            "Integrate".into(),
            Some(icon::SIGMA),
            Some(app.session.tool == Tool::Integrate),
        ),
        CommandId::Multiplets => plain("Multiplets", Some(icon::BRACKETS_CURLY)),
        CommandId::TidyBoard => plain("Tidy Up Frames", Some(icon::BROOM)),
        CommandId::CanvasSettings => plain("Canvas Size & Settings…", Some(icon::FRAME_CORNERS)),
        CommandId::SetCanvasSizePreset(id) => {
            let label = plotx_core::state::preset_by_id(id)
                .map(|preset| format!("Canvas Size: {}", preset.label))
                .unwrap_or_else(|| "Canvas Size".to_owned());
            (label, Some(icon::FRAME_CORNERS), None)
        }
        CommandId::ArrangeGrid(rows, cols) => (
            format!("Arrange Plots {rows} × {cols}"),
            Some(icon::SQUARES_FOUR),
            None,
        ),
        CommandId::SimplifyInnerAxes => plain("Simplify Inner Axes", Some(icon::SQUARES_FOUR)),
        CommandId::SetSpacingMode(mode) => {
            let checked = app
                .session
                .active_canvas
                .is_some_and(|ci| app.doc.canvases[ci].layout.spacing_mode == mode);
            (
                format!("Spacing: {}", spacing_label(mode)),
                Some(icon::ARROWS_LEFT_RIGHT),
                Some(checked),
            )
        }
        CommandId::SetGutterPreset(preset) => {
            let checked = app.session.active_canvas.is_some_and(|ci| {
                (app.doc.canvases[ci].layout.gutter_mm - preset.millimetres()).abs() < 0.001
            });
            (
                format!(
                    "Minimum spacing: {} ({} mm)",
                    preset.label(),
                    preset.millimetres()
                ),
                Some(icon::ARROWS_LEFT_RIGHT),
                Some(checked),
            )
        }
        CommandId::Align(mode) => (
            format!("Align {}", align_label(mode)),
            Some(align_icon(mode)),
            None,
        ),
        CommandId::Distribute(mode) => (
            format!("Distribute {}", distribute_label(mode)),
            Some(match mode {
                Distribute::Horizontal => icon::COLUMNS,
                Distribute::Vertical => icon::ROWS,
            }),
            None,
        ),
        CommandId::ZOrder(mode) => (zorder_label(mode).into(), Some(zorder_icon(mode)), None),
        CommandId::ApplyTheme(id) => {
            let label = plotx_core::theme::Theme::by_id(id)
                .map(|t| format!("Apply Theme: {}", t.name))
                .unwrap_or_else(|| "Apply Theme".into());
            (label, Some(icon::PALETTE), None)
        }
        CommandId::Tool(tool) => (
            format!("Tool: {}", tool.label()),
            tool_icon(tool),
            Some(app.session.tool == tool),
        ),
    }
}

/// "Increase UI Scale (135%)" — the live value keeps the menu/palette entries
/// self-describing without a dedicated indicator surface.
fn ui_scale_label(app: &PlotxApp, verb: &str) -> String {
    match &app.session.monitor {
        Some(monitor) => format!("{verb} ({:.0}%)", monitor.effective() * 100.0),
        None => verb.to_owned(),
    }
}

fn tool_icon(tool: Tool) -> Option<&'static str> {
    Some(match tool {
        Tool::Select => icon::CURSOR,
        Tool::BrowseZoom => icon::MAGNIFYING_GLASS_PLUS,
        Tool::ManualPhase => icon::WAVE_SINE,
        Tool::SelectRegion => icon::SELECTION,
        Tool::Integrate => icon::SIGMA,
        Tool::Peaks => icon::MAP_PIN,
        Tool::Slice => icon::SCISSORS,
        Tool::LineFit => icon::CHART_LINE,
        Tool::Annotate => icon::TEXT_AA,
        Tool::Text => icon::TEXT_T,
        Tool::PanelLabel => icon::TAG,
        Tool::Rect => icon::RECTANGLE,
        Tool::Ellipse => icon::CIRCLE,
        Tool::Line => icon::LINE_SEGMENT,
        Tool::Arrow => icon::ARROW_UP_RIGHT,
        Tool::Regions | Tool::PeakAnalysis => return None,
    })
}

/// The disambiguated display name of one recent entry, with no "Open Recent:"
/// prefix: the file name, extended with the parent directory when two entries
/// share a name (two `project.plotx` in different folders must stay tellable
/// apart). `None` when the index is past the list.
///
/// Used bare in the File > Open Recent submenu, whose parent already reads
/// "Open Recent", and prefixed by [`recent_label`] in the command palette.
pub(crate) fn recent_entry_label(app: &PlotxApp, index: usize) -> Option<String> {
    let path = app.session.recent_files.get(index)?;
    let name = match path.file_name().and_then(|name| name.to_str()) {
        Some(name) => name.to_owned(),
        None => return Some(path.display().to_string()),
    };
    let duplicated = app
        .session
        .recent_files
        .iter()
        .enumerate()
        .any(|(other, candidate)| {
            other != index && candidate.file_name().and_then(|n| n.to_str()) == Some(&name)
        });
    Some(match path.parent().filter(|_| duplicated) {
        Some(parent) => format!("{name} — {}", parent.display()),
        None => name,
    })
}

/// The palette-searchable label of one recent entry: "Open Recent:" plus the
/// disambiguated entry name, so the command stays findable by that verb.
fn recent_label(app: &PlotxApp, index: usize) -> String {
    match recent_entry_label(app, index) {
        Some(name) => format!("Open Recent: {name}"),
        None => "Open Recent".to_owned(),
    }
}

fn simple_stable_id(id: CommandId) -> &'static str {
    match id {
        CommandId::OpenProject => "file.open_project",
        CommandId::OpenFile => "file.open_file",
        CommandId::OpenFolder => "file.open_folder",
        CommandId::RunBatchWorkflow => "tools.automation",
        CommandId::ImportTable => "file.import_table",
        CommandId::PasteTable => "file.paste_table",
        CommandId::SaveProject => "file.save",
        CommandId::NewTable => "file.new_table",
        CommandId::ExportData => "file.export_data",
        CommandId::CopyFigure => "file.copy_figure",
        CommandId::Quit => "file.quit",
        CommandId::Undo => "edit.undo",
        CommandId::Redo => "edit.redo",
        CommandId::SelectAll => "edit.select_all",
        CommandId::Group => "edit.group",
        CommandId::Ungroup => "edit.ungroup",
        CommandId::TogglePrimarySidebar => "view.primary_sidebar",
        CommandId::ToggleSecondarySidebar => "view.secondary_sidebar",
        CommandId::ZoomToFit => "view.zoom_fit",
        CommandId::ZoomToSelection => "view.zoom_selection",
        CommandId::UiScaleUp => "view.ui_scale_up",
        CommandId::UiScaleDown => "view.ui_scale_down",
        CommandId::UiScaleReset => "view.ui_scale_reset",
        CommandId::Present => "view.present",
        CommandId::ToggleGrid => "view.grid",
        CommandId::ToggleSnap => "view.snap",
        CommandId::Preferences => "app.preferences",
        CommandId::CommandPalette => "app.command_palette",
        CommandId::CheckUpdates => "help.updates",
        CommandId::OperationHistory => "help.history",
        CommandId::About => "help.about",
        CommandId::ClearRecentFiles => "file.clear_recent",
        CommandId::HelpManual => "help.manual",
        CommandId::SaveProcessingTemplate => "process.save_template",
        CommandId::ApplyProcessingTemplate => "process.apply_template",
        CommandId::SpectrumArithmetic => "process.arithmetic",
        CommandId::AlignSpectra => "process.align_spectra",
        CommandId::StackData => "data.stack",
        CommandId::SelectRange => "analysis.select_range",
        CommandId::ClearRange => "analysis.clear_range",
        CommandId::Regions => "analysis.regions",
        CommandId::SeriesTable => "analysis.series_table",
        CommandId::DetectPeaks => "analysis.detect_peaks",
        CommandId::PeakList => "analysis.peak_list",
        CommandId::LineFit => "analysis.line_fit",
        CommandId::RunPeakFit => "analysis.run_peak_fit",
        CommandId::CurveFit => "analysis.curve_fit",
        CommandId::RunCurveFit => "analysis.run_curve_fit",
        CommandId::Statistics => "analysis.statistics",
        CommandId::ChartType => "figure.chart_type",
        CommandId::FigureTypography => "figure.typography",
        CommandId::Integrate => "analysis.integrate",
        CommandId::Multiplets => "analysis.multiplets",
        CommandId::TidyBoard => "arrange.tidy",
        CommandId::CanvasSettings => "figure.canvas_settings",
        CommandId::SimplifyInnerAxes => "arrange.simplify_inner_axes",
        _ => unreachable!("dynamic commands have formatted stable IDs"),
    }
}

fn spacing_slug(mode: SpacingMode) -> &'static str {
    match mode {
        SpacingMode::Frame => "frame",
        SpacingMode::Visual => "visual",
    }
}

fn spacing_label(mode: SpacingMode) -> &'static str {
    match mode {
        SpacingMode::Frame => "Frame",
        SpacingMode::Visual => "Visual",
    }
}

fn gutter_slug(preset: GutterPreset) -> &'static str {
    match preset {
        GutterPreset::Tight => "tight",
        GutterPreset::Normal => "normal",
        GutterPreset::Spacious => "spacious",
    }
}

fn align_label(mode: Align) -> &'static str {
    match mode {
        Align::Left => "Left",
        Align::HCenter => "Center",
        Align::Right => "Right",
        Align::Top => "Top",
        Align::VCenter => "Middle",
        Align::Bottom => "Bottom",
    }
}
fn align_icon(mode: Align) -> &'static str {
    match mode {
        Align::Left => icon::ALIGN_LEFT_SIMPLE,
        Align::HCenter => icon::ALIGN_CENTER_HORIZONTAL_SIMPLE,
        Align::Right => icon::ALIGN_RIGHT_SIMPLE,
        Align::Top => icon::ALIGN_TOP_SIMPLE,
        Align::VCenter => icon::ALIGN_CENTER_VERTICAL_SIMPLE,
        Align::Bottom => icon::ALIGN_BOTTOM_SIMPLE,
    }
}
fn align_slug(mode: Align) -> &'static str {
    match mode {
        Align::Left => "left",
        Align::HCenter => "center",
        Align::Right => "right",
        Align::Top => "top",
        Align::VCenter => "middle",
        Align::Bottom => "bottom",
    }
}
fn distribute_label(mode: Distribute) -> &'static str {
    match mode {
        Distribute::Horizontal => "Horizontally",
        Distribute::Vertical => "Vertically",
    }
}
fn zorder_label(mode: ZOrder) -> &'static str {
    match mode {
        ZOrder::Front => "Bring to Front",
        ZOrder::Forward => "Bring Forward",
        ZOrder::Backward => "Send Backward",
        ZOrder::Back => "Send to Back",
    }
}
fn zorder_icon(mode: ZOrder) -> &'static str {
    match mode {
        ZOrder::Front => icon::ARROW_LINE_UP,
        ZOrder::Forward => icon::ARROW_UP,
        ZOrder::Backward => icon::ARROW_DOWN,
        ZOrder::Back => icon::ARROW_LINE_DOWN,
    }
}
fn zorder_slug(mode: ZOrder) -> &'static str {
    match mode {
        ZOrder::Front => "front",
        ZOrder::Forward => "forward",
        ZOrder::Backward => "backward",
        ZOrder::Back => "back",
    }
}
fn tool_slug(tool: Tool) -> &'static str {
    match tool {
        Tool::Select => "select",
        Tool::BrowseZoom => "zoom",
        Tool::ManualPhase => "phase",
        Tool::SelectRegion => "select_range",
        Tool::Regions => "regions",
        Tool::Integrate => "integrate",
        Tool::Peaks => "peaks",
        Tool::Slice => "slice",
        Tool::LineFit => "line_fit",
        Tool::Annotate => "annotate",
        Tool::PeakAnalysis => "peak_analysis",
        Tool::Text => "text",
        Tool::PanelLabel => "panel_label",
        Tool::Rect => "rectangle",
        Tool::Ellipse => "ellipse",
        Tool::Line => "line",
        Tool::Arrow => "arrow",
    }
}
