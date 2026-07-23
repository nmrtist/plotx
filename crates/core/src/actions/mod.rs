use crate::layout::PageLayout;
use crate::state::{
    AxisOverrides, AxisProjections, CanvasDocument, CanvasObject, CanvasViewport, ChartSpec,
    CurveFitReference, DataBinding, Dataset, NamedView, ObjectFrame, ObjectId, ObjectStyle,
    PanelLabelStyle, PanelMeta, PlotxApp, PrimaryView, Region, Selection, StackSpec, StatAnalysis,
    StoredCurveFitAnalysis, StoredLineFit, StoredMultiplet, TableEditDelta, TextBox,
    TypedTableState,
};
use crate::theme::ThemeSnapshot;
use crate::{Integral2D, IntegralResult};
use plotx_processing::{AxisPipeline, Params2D, Preset2D};

mod app_impl;
mod arrange;
mod processing_state;
mod transfer;
mod zorder;

pub use processing_state::{ProcessingRebuild, ProcessingStateError};
pub use zorder::*;

#[cfg(test)]
mod tests;

#[derive(Clone, Debug, PartialEq)]
pub enum DatasetProcessingState {
    Nmr {
        pipeline: AxisPipeline,
        group_delay_correct: bool,
    },
    Nmr2D {
        params: Params2D,
        preset: Preset2D,
    },
    /// A table has no reversible processing recipe; its curve fits are edited
    /// through their own actions.
    Table,
    Electrophysiology(crate::state::ElectrophysiologyProcessing),
    Afm,
}

#[derive(Clone)]
pub struct PendingViewportEdit {
    pub canvas: usize,
    pub object: ObjectId,
    pub before: CanvasViewport,
    pub last_input_time: f64,
}

/// A page's size selection: the physical dimensions together with the preset
/// identity the user picked. Kept as one value so undo/redo restores both — the
/// id is what disambiguates journal widths shared by two publishers.
#[derive(Clone, Debug, PartialEq)]
pub struct PageSizeState {
    pub size_mm: [f32; 2],
    pub preset_id: Option<String>,
}

impl PageSizeState {
    pub fn of(canvas: &CanvasDocument) -> Self {
        Self {
            size_mm: canvas.size_mm,
            preset_id: canvas.size_preset_id.clone(),
        }
    }
}

#[derive(Clone)]
pub struct PendingCanvasSizeEdit {
    pub canvas: usize,
    pub before: PageSizeState,
}

#[derive(Clone)]
pub struct PendingProcessingEdit {
    pub dataset: usize,
    pub before: DatasetProcessingState,
}

#[derive(Clone)]
pub struct PendingPageLayoutEdit {
    pub canvas: usize,
    pub before: PageLayout,
}

/// Coalesces a single object-inspector interaction (a DragValue drag, a colour
/// pick, a text edit) into one undo step: the pre-edit frames and styles of the
/// touched objects, committed once the interaction ends.
#[derive(Clone)]
pub struct PendingInspectorEdit {
    pub canvas: usize,
    pub frames: Vec<(ObjectId, ObjectFrame)>,
    pub styles: Vec<(ObjectId, ObjectStyle)>,
}

#[derive(Clone)]
pub enum Action {
    Composite(Vec<Action>),
    UpdateDatasetProcessing {
        dataset: usize,
        before: DatasetProcessingState,
        after: DatasetProcessingState,
    },
    SetObjectViewport {
        canvas: usize,
        object: ObjectId,
        before: CanvasViewport,
        after: CanvasViewport,
    },
    SetAxisOverrides {
        canvas: usize,
        object: ObjectId,
        before: AxisOverrides,
        after: AxisOverrides,
    },
    MoveResizeObject {
        canvas: usize,
        object: ObjectId,
        before: ObjectFrame,
        after: ObjectFrame,
    },
    SetObjectFrames {
        canvas: usize,
        before: Vec<(ObjectId, ObjectFrame)>,
        after: Vec<(ObjectId, ObjectFrame)>,
    },
    SetObjectGroups {
        canvas: usize,
        before: Vec<(ObjectId, Option<crate::state::GroupId>)>,
        after: Vec<(ObjectId, Option<crate::state::GroupId>)>,
    },
    /// Reorder a canvas's `objects` (z-order): each `Vec` is the full id order.
    ReorderObjects {
        canvas: usize,
        before: Vec<ObjectId>,
        after: Vec<ObjectId>,
    },
    SetCanvasSize {
        canvas: usize,
        before: PageSizeState,
        after: PageSizeState,
    },
    /// Move a whole page-frame on the board (its `board_pos`, pt) as one step.
    MoveCanvasOnBoard {
        canvas: usize,
        before: [f32; 2],
        after: [f32; 2],
    },
    MoveSheetOnBoard {
        dataset: usize,
        before: [f32; 2],
        after: [f32; 2],
    },
    TidyBoard {
        before: Vec<(crate::state::FrameRef, [f32; 2])>,
        after: Vec<(crate::state::FrameRef, [f32; 2])>,
    },
    SetPageLayout {
        canvas: usize,
        before: PageLayout,
        after: PageLayout,
    },
    ArrangeObjects {
        canvas: usize,
        before_layout: PageLayout,
        after_layout: PageLayout,
        before: Vec<(ObjectId, ObjectFrame)>,
        after: Vec<(ObjectId, ObjectFrame)>,
    },
    SetPanelMeta {
        canvas: usize,
        object: ObjectId,
        before: PanelMeta,
        after: PanelMeta,
    },
    /// The Layers-list flag checkboxes (visibility, lock). Both flags travel
    /// together as `(visible, locked)` so one variant covers either checkbox
    /// without snapshotting the object.
    SetObjectFlags {
        canvas: usize,
        object: ObjectId,
        before: (bool, bool),
        after: (bool, bool),
    },
    /// Insert a named board view at `index` of the session bookmark list;
    /// revert removes it again.
    BoardViewInsert {
        index: usize,
        view: NamedView,
    },
    /// Remove the named board view at `index`; revert re-inserts `view`.
    BoardViewRemove {
        index: usize,
        view: NamedView,
    },
    /// Replace a plot's data binding (add/remove/reorder overlaid series),
    /// rebuilding its figure and refitting the viewport.
    SetDataBinding {
        canvas: usize,
        object: ObjectId,
        before: DataBinding,
        after: DataBinding,
    },
    /// Switch a plot's chart type (and its column selection), rebuilding the
    /// figure through the chart registry and re-fitting the viewport.
    SetChartType {
        canvas: usize,
        object: ObjectId,
        before: ChartSpec,
        after: ChartSpec,
    },
    /// Change a plot's stacking layout (mode, spacing, shear, normalize, active
    /// trace), rebuilding its figure and re-fitting the viewport.
    SetStackSpec {
        canvas: usize,
        object: ObjectId,
        before: StackSpec,
        after: StackSpec,
    },
    /// Set a 2D contour's marginal axis projections (top/left source + visibility),
    /// rebuilding its figure.
    SetAxisProjections {
        canvas: usize,
        object: ObjectId,
        before: AxisProjections,
        after: AxisProjections,
    },
    RenameCanvas {
        canvas: usize,
        before: String,
        after: String,
    },
    RenameObject {
        canvas: usize,
        object: ObjectId,
        before: String,
        after: String,
    },
    /// Set a page's board-only caption text and its visibility as one step.
    SetCanvasCaption {
        canvas: usize,
        before: (String, bool),
        after: (String, bool),
    },
    /// Set a page's panel-letter numbering style, re-lettering all its panels.
    SetPanelLabelStyle {
        canvas: usize,
        before: PanelLabelStyle,
        after: PanelLabelStyle,
    },
    RenameDataset {
        dataset: usize,
        before: Option<String>,
        after: Option<String>,
    },
    /// Replace a table's analysis snapshots and per-column references atomically.
    SetCurveFitAnalyses {
        dataset: usize,
        before: (Vec<Option<CurveFitReference>>, Vec<StoredCurveFitAnalysis>),
        after: (Vec<Option<CurveFitReference>>, Vec<StoredCurveFitAnalysis>),
    },
    /// Apply a stable-identity incremental table transaction.
    EditTable {
        dataset: usize,
        delta: Box<TableEditDelta>,
    },
    SetTypedTableState {
        dataset: usize,
        before: Box<TypedTableState>,
        after: Box<TypedTableState>,
    },
    /// Replace a series dataset's measurement windows (Regions tool: add / move /
    /// resize / rename / delete). The linked series table is re-derived on apply
    /// and undo, so it stays consistent without a second action.
    SetRegions {
        dataset: usize,
        before: Vec<Region>,
        after: Vec<Region>,
    },
    SetIntegrals {
        dataset: usize,
        before: Vec<IntegralResult>,
        after: Vec<IntegralResult>,
    },
    /// Replace a true-2D dataset's rectangular volume measurements as one
    /// undoable edit (create, geometry, metadata, reference, or deletion).
    SetIntegrals2D {
        dataset: usize,
        before: Vec<Integral2D>,
        after: Vec<Integral2D>,
    },
    /// Replace a dataset's peak set (detector recipe, hand-placed marks, and
    /// suppressed detections) as one undoable step.
    SetPeaks {
        dataset: usize,
        before: crate::state::PeakSet,
        after: crate::state::PeakSet,
    },
    /// Replace a 1D dataset's stored lineshape deconvolutions as one undoable step.
    SetLineFits {
        dataset: usize,
        before: Vec<StoredLineFit>,
        after: Vec<StoredLineFit>,
    },
    /// Replace a 1D NMR dataset's stored multiplet analyses as one undoable step.
    SetMultiplets {
        dataset: usize,
        before: Vec<StoredMultiplet>,
        after: Vec<StoredMultiplet>,
    },
    /// Replace a table dataset's stored statistics analyses as one undoable step.
    SetTableStatistics {
        dataset: usize,
        before: Vec<StatAnalysis>,
        after: Vec<StatAnalysis>,
    },
    DeleteCanvas {
        index: usize,
        canvas: CanvasDocument,
        active_before: Option<usize>,
        active_after: Option<usize>,
    },
    InsertCanvas {
        index: usize,
        canvas: Box<CanvasDocument>,
        active_before: Option<usize>,
    },
    /// Apply a document-level style theme to a canvas: its background, the app's
    /// new-object style defaults, and its existing objects' colours, as one step.
    ApplyTheme {
        canvas: usize,
        before: Box<ThemeSnapshot>,
        after: Box<ThemeSnapshot>,
    },
    /// Set the document's figure typography (axis tick/label/title point sizes);
    /// every plot re-stamps it on rebuild.
    SetFigureTypography {
        before: plotx_figure::FigureTypography,
        after: plotx_figure::FigureTypography,
    },
    InsertObject {
        canvas: usize,
        object: Box<CanvasObject>,
        selection_before: Selection,
    },
    DeleteObject {
        canvas: usize,
        index: usize,
        object: Box<CanvasObject>,
        selection_before: Selection,
    },
    SetObjectText {
        canvas: usize,
        object: ObjectId,
        before: TextBox,
        after: TextBox,
    },
    /// Set the visual style of one or more objects at once (inspector edits,
    /// format-once). One entry per object; kinds that don't match are ignored.
    SetObjectStyle {
        canvas: usize,
        before: Vec<(ObjectId, ObjectStyle)>,
        after: Vec<(ObjectId, ObjectStyle)>,
    },
    InsertDatasetWithCanvas {
        dataset_index: usize,
        canvas_index: usize,
        canvas_resource_id: String,
        dataset: Box<Dataset>,
        canvas_name: String,
        size_mm: [f32; 2],
        active_canvas_before: Option<usize>,
        active_dataset_before: Option<usize>,
        inserted_into_existing_canvas: Option<usize>,
        inserted_object_id: Option<ObjectId>,
    },
    /// Move or copy one or more objects (whole groups included) from `from` to
    /// `to` as one step. Because object/group ids are per-canvas, `inserted`
    /// carries fresh destination-local ids/groups baked in; `removed` keeps the
    /// source slots and original objects so a move is exactly reversible. An empty
    /// `removed` marks a copy (the source keeps its objects).
    TransferObjects {
        from: usize,
        to: usize,
        removed: Vec<(usize, CanvasObject)>,
        inserted: Vec<CanvasObject>,
        active_before: Option<usize>,
        selection_before: Selection,
    },
    /// Auto-tiling drop: move one plot from `from` to `to` and, in the same step,
    /// reframe the target's existing plots so all tile the page. Extends a
    /// `TransferObjects`-style move (`removed`/`inserted`, dest-local ids baked in,
    /// with the newcomer's landing frame baked into its clone) with the
    /// pushed-aside plots' before/after frames. Exactly reversible.
    TileDrop {
        source_index_before: usize,
        target_index_before: usize,
        target_index_after: usize,
        source_canvas_before: Option<Box<CanvasDocument>>,
        removed: Vec<(usize, CanvasObject)>,
        inserted: Vec<CanvasObject>,
        existing_before: Vec<(ObjectId, ObjectFrame)>,
        existing_after: Vec<(ObjectId, ObjectFrame)>,
        active_before: Option<usize>,
        selection_before: Selection,
    },
}

mod build;

impl Action {
    fn is_noop(&self) -> bool {
        match self {
            Self::Composite(actions) => actions.iter().all(Self::is_noop),
            Self::UpdateDatasetProcessing { before, after, .. } => before == after,
            Self::SetObjectViewport { before, after, .. } => before == after,
            Self::MoveResizeObject { before, after, .. } => before == after,
            Self::SetObjectFrames { before, after, .. } => before == after,
            Self::SetObjectGroups { before, after, .. } => before == after,
            Self::ReorderObjects { before, after, .. } => before == after,
            Self::SetCanvasSize { before, after, .. } => before == after,
            Self::MoveCanvasOnBoard { before, after, .. } => before == after,
            Self::MoveSheetOnBoard { before, after, .. } => before == after,
            Self::TidyBoard { before, after, .. } => before == after,
            Self::SetPageLayout { before, after, .. } => before == after,
            Self::ArrangeObjects {
                before_layout,
                after_layout,
                before,
                after,
                ..
            } => before_layout == after_layout && before == after,
            Self::SetPanelMeta { before, after, .. } => before == after,
            Self::SetObjectFlags { before, after, .. } => before == after,
            // Inserting or removing a bookmark always changes the list.
            Self::BoardViewInsert { .. } | Self::BoardViewRemove { .. } => false,
            Self::SetDataBinding { before, after, .. } => before == after,
            Self::SetAxisOverrides { before, after, .. } => before == after,
            Self::SetChartType { before, after, .. } => before == after,
            Self::SetStackSpec { before, after, .. } => before == after,
            Self::SetAxisProjections { before, after, .. } => before == after,
            Self::RenameCanvas { before, after, .. } => before == after,
            Self::RenameObject { before, after, .. } => before == after,
            Self::SetCanvasCaption { before, after, .. } => before == after,
            Self::SetPanelLabelStyle { before, after, .. } => before == after,
            Self::RenameDataset { before, after, .. } => before == after,
            Self::SetCurveFitAnalyses { before, after, .. } => before == after,
            Self::EditTable { delta, .. } => delta.is_empty(),
            Self::SetTypedTableState { before, after, .. } => {
                before.envelope.revision.id == after.envelope.revision.id
            }
            Self::SetRegions { before, after, .. } => before == after,
            Self::SetIntegrals { before, after, .. } => before == after,
            Self::SetIntegrals2D { before, after, .. } => before == after,
            Self::SetPeaks { before, after, .. } => before == after,
            Self::SetLineFits { before, after, .. } => before == after,
            Self::SetMultiplets { before, after, .. } => before == after,
            Self::SetTableStatistics { before, after, .. } => before == after,
            Self::SetObjectText { before, after, .. } => before == after,
            Self::SetObjectStyle { before, after, .. } => before == after,
            Self::ApplyTheme { before, after, .. } => before == after,
            Self::SetFigureTypography { before, after } => before == after,
            Self::DeleteCanvas { .. }
            | Self::InsertCanvas { .. }
            | Self::InsertDatasetWithCanvas { .. }
            | Self::InsertObject { .. }
            | Self::DeleteObject { .. }
            | Self::TransferObjects { .. }
            | Self::TileDrop { .. } => false,
        }
    }
}
