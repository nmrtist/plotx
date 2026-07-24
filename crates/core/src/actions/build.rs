//! Constructor helpers for [`Action`], split from `mod.rs` to keep both
//! sources under the repository size limit.

use super::*;
use crate::layout::PageLayout;
use crate::state::{
    AxisOverrides, CanvasObject, CanvasViewport, ChartSpec, CurveFitReference, DataBinding,
    Dataset, NamedView, ObjectFrame, ObjectId, ObjectStyle, PanelMeta, PlotxApp, Region, Selection,
    StackSpec, StoredCurveFitAnalysis, StoredLineFit, StoredMultiplet, TableEditDelta, TextBox,
};
use crate::theme::ThemeSnapshot;
use crate::{Integral2D, IntegralResult};

impl Action {
    pub fn update_dataset_processing(
        dataset: usize,
        before: DatasetProcessingState,
        after: DatasetProcessingState,
    ) -> Self {
        Self::UpdateDatasetProcessing {
            dataset,
            before,
            after,
        }
    }

    pub fn set_object_viewport(
        canvas: usize,
        object: ObjectId,
        before: CanvasViewport,
        after: CanvasViewport,
    ) -> Self {
        Self::SetObjectViewport {
            canvas,
            object,
            before,
            after,
        }
    }

    pub fn set_axis_overrides(
        canvas: usize,
        object: ObjectId,
        before: AxisOverrides,
        after: AxisOverrides,
    ) -> Self {
        Self::SetAxisOverrides {
            canvas,
            object,
            before,
            after,
        }
    }

    pub fn move_resize_object(
        canvas: usize,
        object: ObjectId,
        before: ObjectFrame,
        after: ObjectFrame,
    ) -> Self {
        Self::MoveResizeObject {
            canvas,
            object,
            before,
            after,
        }
    }

    pub fn set_object_frames(
        canvas: usize,
        before: Vec<(ObjectId, ObjectFrame)>,
        after: Vec<(ObjectId, ObjectFrame)>,
    ) -> Self {
        Self::SetObjectFrames {
            canvas,
            before,
            after,
        }
    }

    pub fn set_object_groups(
        canvas: usize,
        before: Vec<(ObjectId, Option<crate::state::GroupId>)>,
        after: Vec<(ObjectId, Option<crate::state::GroupId>)>,
    ) -> Self {
        Self::SetObjectGroups {
            canvas,
            before,
            after,
        }
    }

    pub fn reorder_objects(canvas: usize, before: Vec<ObjectId>, after: Vec<ObjectId>) -> Self {
        Self::ReorderObjects {
            canvas,
            before,
            after,
        }
    }

    pub fn set_canvas_size(canvas: usize, before: PageSizeState, after: PageSizeState) -> Self {
        Self::SetCanvasSize {
            canvas,
            before,
            after,
        }
    }

    /// A canvas resize that also scales every object frame uniformly by the
    /// width ratio, folded into one undo step. Falls back to a plain size
    /// change when the page is empty or the width does not change.
    pub fn set_canvas_size_scaling_content(
        app: &PlotxApp,
        canvas: usize,
        after: PageSizeState,
    ) -> Self {
        let Some(doc) = app.doc.canvases.get(canvas) else {
            return Self::set_canvas_size(canvas, after.clone(), after);
        };
        let before = PageSizeState::of(doc);
        match crate::state::scaled_frames(doc, before.size_mm, after.size_mm) {
            Some((frames_before, frames_after)) => Self::Composite(vec![
                Self::set_object_frames(canvas, frames_before, frames_after),
                Self::set_canvas_size(canvas, before, after),
            ]),
            None => Self::set_canvas_size(canvas, before, after),
        }
    }

    pub fn move_canvas_on_board(canvas: usize, before: [f32; 2], after: [f32; 2]) -> Self {
        Self::MoveCanvasOnBoard {
            canvas,
            before,
            after,
        }
    }

    pub fn move_sheet_on_board(dataset: usize, before: [f32; 2], after: [f32; 2]) -> Self {
        Self::MoveSheetOnBoard {
            dataset,
            before,
            after,
        }
    }

    pub fn set_page_layout(canvas: usize, before: PageLayout, after: PageLayout) -> Self {
        Self::SetPageLayout {
            canvas,
            before,
            after,
        }
    }

    pub fn set_panel_meta(
        canvas: usize,
        object: ObjectId,
        before: PanelMeta,
        after: PanelMeta,
    ) -> Self {
        Self::SetPanelMeta {
            canvas,
            object,
            before,
            after,
        }
    }

    pub fn set_object_flags(
        canvas: usize,
        object: ObjectId,
        before: (bool, bool),
        after: (bool, bool),
    ) -> Self {
        Self::SetObjectFlags {
            canvas,
            object,
            before,
            after,
        }
    }

    pub fn board_view_insert(index: usize, view: NamedView) -> Self {
        Self::BoardViewInsert { index, view }
    }

    pub fn board_view_remove(index: usize, view: NamedView) -> Self {
        Self::BoardViewRemove { index, view }
    }

    pub fn set_data_binding(
        canvas: usize,
        object: ObjectId,
        before: DataBinding,
        after: DataBinding,
    ) -> Self {
        Self::SetDataBinding {
            canvas,
            object,
            before,
            after,
        }
    }

    pub fn set_chart_type(
        canvas: usize,
        object: ObjectId,
        before: ChartSpec,
        after: ChartSpec,
    ) -> Self {
        Self::SetChartType {
            canvas,
            object,
            before,
            after,
        }
    }

    pub fn set_stack_spec(
        canvas: usize,
        object: ObjectId,
        before: StackSpec,
        after: StackSpec,
    ) -> Self {
        Self::SetStackSpec {
            canvas,
            object,
            before,
            after,
        }
    }

    pub fn rename_canvas(canvas: usize, before: String, after: String) -> Self {
        Self::RenameCanvas {
            canvas,
            before,
            after,
        }
    }

    pub fn rename_object(canvas: usize, object: ObjectId, before: String, after: String) -> Self {
        Self::RenameObject {
            canvas,
            object,
            before,
            after,
        }
    }

    pub fn set_canvas_caption(
        canvas: usize,
        before: (String, bool),
        after: (String, bool),
    ) -> Self {
        Self::SetCanvasCaption {
            canvas,
            before,
            after,
        }
    }

    pub fn rename_dataset(dataset: usize, before: Option<String>, after: Option<String>) -> Self {
        Self::RenameDataset {
            dataset,
            before,
            after,
        }
    }

    pub fn set_curve_fit_analyses(
        dataset: usize,
        before: (Vec<Option<CurveFitReference>>, Vec<StoredCurveFitAnalysis>),
        after: (Vec<Option<CurveFitReference>>, Vec<StoredCurveFitAnalysis>),
    ) -> Self {
        Self::SetCurveFitAnalyses {
            dataset,
            before,
            after,
        }
    }

    pub fn edit_table(dataset: usize, delta: TableEditDelta) -> Self {
        Self::EditTable {
            dataset,
            delta: Box::new(delta),
        }
    }

    pub fn set_regions(dataset: usize, before: Vec<Region>, after: Vec<Region>) -> Self {
        Self::SetRegions {
            dataset,
            before,
            after,
        }
    }

    pub fn set_integrals(
        dataset: usize,
        before: Vec<IntegralResult>,
        after: Vec<IntegralResult>,
    ) -> Self {
        Self::SetIntegrals {
            dataset,
            before,
            after,
        }
    }

    pub fn set_integrals_2d(
        dataset: usize,
        before: Vec<Integral2D>,
        after: Vec<Integral2D>,
    ) -> Self {
        Self::SetIntegrals2D {
            dataset,
            before,
            after,
        }
    }

    pub fn set_peaks(
        dataset: usize,
        before: crate::state::PeakSet,
        after: crate::state::PeakSet,
    ) -> Self {
        Self::SetPeaks {
            dataset,
            before,
            after,
        }
    }

    pub fn set_line_fits(
        dataset: usize,
        before: Vec<StoredLineFit>,
        after: Vec<StoredLineFit>,
    ) -> Self {
        Self::SetLineFits {
            dataset,
            before,
            after,
        }
    }

    pub fn set_multiplets(
        dataset: usize,
        before: Vec<StoredMultiplet>,
        after: Vec<StoredMultiplet>,
    ) -> Self {
        Self::SetMultiplets {
            dataset,
            before,
            after,
        }
    }

    pub fn insert_object(canvas: usize, object: CanvasObject, selection_before: Selection) -> Self {
        Self::InsertObject {
            canvas,
            object: Box::new(object),
            selection_before,
        }
    }

    pub fn delete_object(app: &PlotxApp, canvas: usize, id: ObjectId) -> Option<Self> {
        let c = app.doc.canvases.get(canvas)?;
        let index = c.objects.iter().position(|o| o.id == id)?;
        Some(Self::DeleteObject {
            canvas,
            index,
            object: Box::new(c.objects[index].clone()),
            selection_before: app.session.ui.selection.clone(),
        })
    }

    pub fn set_object_text(
        canvas: usize,
        object: ObjectId,
        before: TextBox,
        after: TextBox,
    ) -> Self {
        Self::SetObjectText {
            canvas,
            object,
            before,
            after,
        }
    }

    pub fn set_object_style(
        canvas: usize,
        before: Vec<(ObjectId, ObjectStyle)>,
        after: Vec<(ObjectId, ObjectStyle)>,
    ) -> Self {
        Self::SetObjectStyle {
            canvas,
            before,
            after,
        }
    }

    pub fn insert_canvas(
        index: usize,
        canvas: CanvasDocument,
        active_before: Option<usize>,
    ) -> Self {
        Self::InsertCanvas {
            index,
            canvas: Box::new(canvas),
            active_before,
        }
    }

    pub fn set_figure_typography(
        before: plotx_figure::FigureTypography,
        after: plotx_figure::FigureTypography,
    ) -> Self {
        Self::SetFigureTypography { before, after }
    }

    pub fn apply_theme(canvas: usize, before: ThemeSnapshot, after: ThemeSnapshot) -> Self {
        Self::ApplyTheme {
            canvas,
            before: Box::new(before),
            after: Box::new(after),
        }
    }

    pub fn delete_canvas(app: &PlotxApp, index: usize) -> Option<Self> {
        let canvas = app.doc.canvases.get(index)?.clone();
        let active_after = active_canvas_after_delete(app.doc.canvases.len(), index);
        Some(Self::DeleteCanvas {
            index,
            canvas,
            active_before: app.session.active_canvas,
            active_after,
        })
    }

    pub fn insert_dataset_with_default_canvas(
        app: &PlotxApp,
        dataset: Dataset,
        canvas_name: String,
        size_mm: [f32; 2],
    ) -> Self {
        Self::InsertDatasetWithCanvas {
            dataset_index: app.doc.datasets.len(),
            canvas_index: app.doc.canvases.len(),
            canvas_resource_id: crate::state::CanvasId::new(),
            dataset: Box::new(dataset),
            canvas_name,
            size_mm,
            active_canvas_before: app.session.active_canvas,
            active_dataset_before: app.active_dataset(),
            inserted_into_existing_canvas: None,
            inserted_object_id: None,
        }
    }

    /// Insert a new dataset and place it as a plot object on an existing canvas
    /// (rather than minting a fresh canvas), as one undoable step.
    pub fn insert_dataset_into_canvas(app: &PlotxApp, dataset: Dataset, canvas: usize) -> Self {
        Self::InsertDatasetWithCanvas {
            dataset_index: app.doc.datasets.len(),
            canvas_index: app.doc.canvases.len(),
            canvas_resource_id: crate::state::CanvasId::new(),
            dataset: Box::new(dataset),
            canvas_name: String::new(),
            size_mm: crate::state::DEFAULT_CANVAS_SIZE_MM,
            active_canvas_before: app.session.active_canvas,
            active_dataset_before: app.active_dataset(),
            inserted_into_existing_canvas: Some(canvas),
            inserted_object_id: app.doc.canvases.get(canvas).map(|c| c.next_object_id),
        }
    }
}
