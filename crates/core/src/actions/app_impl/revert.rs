use crate::actions::Action;
use crate::state::{Dataset, Interaction, PlotxApp, Selection};

impl PlotxApp {
    pub(super) fn revert_action(&mut self, action: &Action) {
        macro_rules! dataset_index {
            ($id:expr) => {
                match self.doc.dataset_index($id) {
                    Some(index) => index,
                    None => return,
                }
            };
        }
        match action {
            Action::Composite(actions) => {
                for action in actions.iter().rev() {
                    self.revert_action(action);
                }
            }
            Action::UpdateDatasetProcessing {
                dataset, before, ..
            } => {
                self.set_dataset_processing_state(dataset_index!(*dataset), before);
            }
            Action::SetObjectViewport {
                canvas,
                object,
                before,
                ..
            } => {
                self.set_object_viewport(*canvas, *object, before);
            }
            Action::SetAxisOverrides {
                canvas,
                object,
                before,
                ..
            } => {
                self.set_axis_overrides_value(*canvas, *object, before);
            }
            Action::MoveResizeObject {
                canvas,
                object,
                before,
                ..
            } => {
                self.set_object_frame(*canvas, *object, *before);
            }
            Action::SetObjectFrames { canvas, before, .. } => {
                for &(id, frame) in before {
                    self.set_object_frame(*canvas, id, frame);
                }
            }
            Action::SetObjectGroups { canvas, before, .. } => {
                self.set_object_groups(*canvas, before);
            }
            Action::ReorderObjects { canvas, before, .. } => {
                self.reorder_objects_value(*canvas, before);
            }
            Action::SetCanvasSize { canvas, before, .. } => {
                self.set_canvas_size(*canvas, before);
            }
            Action::MoveCanvasOnBoard { canvas, before, .. } => {
                if let Some(c) = self.doc.canvases.get_mut(*canvas) {
                    c.board_pos = *before;
                }
            }
            Action::MoveSheetOnBoard {
                dataset, before, ..
            } => {
                let dataset = dataset_index!(*dataset);
                if let Some(t) = self
                    .doc
                    .datasets
                    .get_mut(dataset)
                    .and_then(Dataset::as_table_mut)
                {
                    t.board_pos = *before;
                }
            }
            Action::TidyBoard { before, .. } => {
                for &(frame, pos) in before {
                    crate::state::set_frame_board_pos(self, frame, pos);
                }
            }
            Action::SetPageLayout { canvas, before, .. } => {
                self.set_page_layout_value(*canvas, *before);
            }
            Action::ArrangeObjects {
                canvas,
                before_layout,
                before,
                ..
            } => {
                self.apply_arrangement(*canvas, *before_layout, before);
            }
            Action::SetPanelMeta {
                canvas,
                object,
                before,
                ..
            } => {
                self.set_panel_meta(*canvas, *object, before.clone());
            }
            Action::SetObjectFlags {
                canvas,
                object,
                before,
                ..
            } => {
                self.set_object_flags(*canvas, *object, *before);
            }
            Action::BoardViewInsert { index, view } => {
                self.board_view_do_remove(*index, view);
            }
            Action::BoardViewRemove { index, view } => {
                self.board_view_do_insert(*index, view);
            }
            Action::SetDataBinding {
                canvas,
                object,
                before,
                ..
            } => {
                self.set_object_binding(*canvas, *object, before);
            }
            Action::SetChartType {
                canvas,
                object,
                before,
                ..
            } => {
                self.set_object_chart(*canvas, *object, before);
            }
            Action::SetStackSpec {
                canvas,
                object,
                before,
                ..
            } => {
                self.set_object_stack(*canvas, *object, before);
            }
            Action::SetAxisProjections {
                canvas,
                object,
                before,
                ..
            } => {
                self.set_object_projections(*canvas, *object, before);
            }
            Action::RenameCanvas { canvas, before, .. } => {
                if let Some(c) = self.doc.canvases.get_mut(*canvas) {
                    c.name = before.clone();
                }
            }
            Action::RenameObject {
                canvas,
                object,
                before,
                ..
            } => {
                if let Some(object) = self
                    .doc
                    .canvases
                    .get_mut(*canvas)
                    .and_then(|canvas| canvas.object_mut(*object))
                {
                    object.name.clone_from(before);
                }
            }
            Action::SetCanvasCaption { canvas, before, .. } => {
                self.set_canvas_caption_value(*canvas, before);
            }
            Action::SetPanelLabelStyle { canvas, before, .. } => {
                if let Some(c) = self.doc.canvases.get_mut(*canvas) {
                    c.panel_label_style = *before;
                }
            }
            Action::RenameDataset {
                dataset, before, ..
            } => {
                let dataset = dataset_index!(*dataset);
                if let Some(d) = self.doc.datasets.get_mut(dataset) {
                    d.set_name(before.clone());
                }
            }
            Action::SetCurveFitAnalyses {
                dataset, before, ..
            } => {
                self.set_curve_fit_analyses(dataset_index!(*dataset), before);
            }
            Action::EditTable { dataset, delta } => {
                self.apply_table_edit(dataset_index!(*dataset), delta, false);
            }
            Action::SetTypedTableState {
                dataset, before, ..
            } => {
                self.set_typed_table_state(dataset_index!(*dataset), before);
            }
            Action::SetRegions {
                dataset, before, ..
            } => {
                self.set_regions(dataset_index!(*dataset), before);
            }
            Action::SetIntegrals {
                dataset, before, ..
            } => {
                self.set_integrals(dataset_index!(*dataset), before);
            }
            Action::SetIntegrals2D {
                dataset, before, ..
            } => {
                self.set_integrals_2d(dataset_index!(*dataset), before);
            }
            Action::SetPeaks {
                dataset, before, ..
            } => {
                self.set_peaks(dataset_index!(*dataset), before);
            }
            Action::SetLineFits {
                dataset, before, ..
            } => {
                self.set_line_fits(dataset_index!(*dataset), before);
            }
            Action::SetMultiplets {
                dataset, before, ..
            } => {
                self.set_multiplets(dataset_index!(*dataset), before);
            }
            Action::SetTableStatistics {
                dataset, before, ..
            } => {
                self.set_table_statistics(dataset_index!(*dataset), before);
            }
            Action::InsertObject {
                canvas,
                object,
                selection_before,
                ..
            } => {
                self.remove_object_value(*canvas, object.id);
                self.set_selection(selection_before.clone());
            }
            Action::DeleteObject {
                canvas,
                index,
                object,
                selection_before,
            } => {
                if let Some(c) = self.doc.canvases.get_mut(*canvas) {
                    let at = (*index).min(c.objects.len());
                    c.objects.insert(at, object.as_ref().clone());
                    c.next_object_id = c.next_object_id.max(object.id.checked_advance(1));
                }
                self.set_selection(selection_before.clone());
            }
            Action::SetObjectText {
                canvas,
                object,
                before,
                ..
            } => {
                self.set_object_text_value(*canvas, *object, before.clone());
            }
            Action::SetObjectStyle { canvas, before, .. } => {
                self.set_object_styles(*canvas, before);
            }
            Action::DeleteCanvas {
                index,
                canvas,
                active_before,
                ..
            } => {
                if *index <= self.doc.canvases.len() {
                    self.doc.canvases.insert(*index, canvas.clone());
                    self.session.active_canvas = *active_before;
                    if let Some(ci) = self.session.active_canvas {
                        let active = self.doc.canvases[ci]
                            .active_dataset()
                            .and_then(|id| self.doc.dataset_index(id));
                        self.set_active_dataset(active);
                    }
                }
            }
            Action::InsertCanvas {
                index,
                active_before,
                ..
            } => {
                self.remove_canvas_at(*index, *active_before);
            }
            Action::ApplyTheme { canvas, before, .. } => {
                self.apply_theme_snapshot(*canvas, before);
            }
            Action::SetFigureTypography { before, .. } => {
                self.set_figure_typography_value(*before);
            }
            Action::InsertDatasetWithCanvas {
                dataset_index,
                canvas_index,
                inserted_into_existing_canvas,
                inserted_object_id,
                active_canvas_before,
                active_dataset_before,
                ..
            } => {
                if let Some(ci) = inserted_into_existing_canvas {
                    if let Some(canvas) = self.doc.canvases.get_mut(*ci)
                        && let Some(id) = inserted_object_id
                    {
                        canvas.objects.retain(|object| object.id != *id);
                        if canvas.selected_object == Some(*id) {
                            canvas.selected_object = None;
                        }
                        if self.session.active_canvas == Some(*ci)
                            && self.session.ui.selection.object() == Some(*id)
                        {
                            self.session.ui.selection = Selection::None;
                        }
                        if matches!(&self.session.ui.interaction, Interaction::PanelLabel(drag) if drag.canvas == *ci && drag.object == *id)
                        {
                            self.reset_interaction();
                        }
                        if matches!(self.session.ui.panel_note_edit, Some(ref edit) if edit.canvas == *ci && edit.object == *id)
                        {
                            self.session.ui.panel_note_edit = None;
                        }
                        if matches!(self.session.ui.panel_note_inline_edit, Some(ref edit) if edit.canvas == *ci && edit.object == *id)
                        {
                            self.session.ui.panel_note_inline_edit = None;
                        }
                    }
                } else if *canvas_index < self.doc.canvases.len() {
                    self.doc.canvases.remove(*canvas_index);
                }
                if *dataset_index < self.doc.datasets.len() {
                    self.doc.datasets.remove(*dataset_index);
                    self.session.dataset_epoch += 1;
                }
                self.session.active_canvas = *active_canvas_before;
                self.set_active_dataset(*active_dataset_before);
            }
            Action::TransferObjects { .. } => self.revert_transfer(action),
            Action::TileDrop { .. } => self.revert_tile_drop(action),
        }
    }
}
