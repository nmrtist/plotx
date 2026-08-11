use plotx_core::state::{ObjectId, PlotxApp, Tool};

use super::{CommandExecutionClass, CommandId};

impl CommandId {
    pub(super) fn tool_target(self) -> Option<Tool> {
        match self {
            Self::SelectRange => Some(Tool::SelectRegion),
            Self::Regions => Some(Tool::Regions),
            Self::PeakList => Some(Tool::Peaks),
            Self::LineFit => Some(Tool::LineFit),
            Self::Integrate => Some(Tool::Integrate),
            Self::Tool(tool) => Some(tool),
            _ => None,
        }
    }

    pub fn execution_class(self) -> CommandExecutionClass {
        match self {
            Self::RunBatchWorkflow | Self::RunScientificScript => CommandExecutionClass::ToolEditor,
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

pub(super) fn requires(ok: bool, reason: &'static str) -> Result<(), &'static str> {
    if ok { Ok(()) } else { Err(reason) }
}

pub(super) fn selected_paths_unlocked(app: &PlotxApp) -> bool {
    app.session.active_canvas.is_some_and(|ci| {
        app.session
            .ui
            .hierarchical_selection
            .paths()
            .iter()
            .all(|path| {
                path.panel
                    .and_then(|id| app.doc.canvases[ci].panel(id))
                    .is_none_or(|panel| !panel.locked)
                    && path
                        .content
                        .and_then(|id| app.doc.canvases[ci].object(id))
                        .is_none_or(|item| !item.locked)
            })
    })
}

pub(crate) fn chart_plot_target(app: &PlotxApp, dataset: usize) -> Option<(usize, ObjectId)> {
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
            object.plot().is_some_and(|plot| {
                plot.binding.primary_dataset() == Some(app.doc.datasets[dataset].resource_id())
            })
        });
        if let Some(object) = hit {
            return Some((ci, object.id));
        }
    }
    None
}

pub(super) fn tool_commands() -> [Tool; 17] {
    [
        Tool::Select,
        Tool::BrowseZoom,
        Tool::ManualPhase,
        Tool::Integrate,
        Tool::Peaks,
        Tool::InspectCursor,
        Tool::DeltaCursor,
        Tool::Symmetry,
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
