use super::*;

mod meta_edits;
mod processing;
mod revert;
mod table_edit;
mod validate;

pub use validate::ActionApplyError;
use validate::{ValidationShape, validate_action};

impl PlotxApp {
    pub fn execute_action(&mut self, action: Action) {
        if let Err(error) = self.try_execute_action(action) {
            self.session.status = error.to_string();
        }
    }

    /// Validate and atomically commit one action transaction. Validation walks
    /// composites before the first child is applied, so a stale later child can
    /// never leave a partially modified document.
    pub fn try_execute_action(&mut self, action: Action) -> Result<(), ActionApplyError> {
        if action.is_noop() {
            return Ok(());
        }
        validate_action(self, &action, &mut ValidationShape::from_app(self))?;
        self.apply_action(&action);
        self.session.undo_stack.push(action);
        if self.session.undo_stack.len() > self.session.history_limit {
            self.session.undo_stack.remove(0);
        }
        self.session.redo_stack.clear();
        self.doc.dirty = true;
        self.doc.automation_revision = self.doc.automation_revision.saturating_add(1);
        Ok(())
    }

    pub fn undo(&mut self) {
        self.reset_interaction();
        let Some(action) = self.session.undo_stack.pop() else {
            return;
        };
        self.revert_action(&action);
        self.session.redo_stack.push(action);
        self.doc.dirty = true;
        self.doc.automation_revision = self.doc.automation_revision.saturating_add(1);
        self.session.status = "Undid last edit.".to_owned();
    }

    pub fn redo(&mut self) {
        self.reset_interaction();
        let Some(action) = self.session.redo_stack.pop() else {
            return;
        };
        self.apply_action(&action);
        self.session.undo_stack.push(action);
        self.doc.dirty = true;
        self.doc.automation_revision = self.doc.automation_revision.saturating_add(1);
        self.session.status = "Redid edit.".to_owned();
    }

    pub fn can_undo(&self) -> bool {
        !self.session.undo_stack.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.session.redo_stack.is_empty()
    }

    pub fn clear_history(&mut self) {
        self.session.undo_stack.clear();
        self.session.redo_stack.clear();
        self.reset_interaction();
        self.session.ui.wheel_zoom = None;
        self.session.ui.canvas_size_edit = None;
        self.session.ui.processing_edit = None;
        self.session.ui.processing_session = None;
        self.session.ui.inspector_edit = None;
        self.session.ui.selection = Selection::None;
        self.session.ui.panel_note_inline_edit = None;
        self.session.ui.panel_note_edit = None;
        self.session.ui.text_edit = None;
        self.session.ui.processing_scheme_dialog = None;
    }

    pub fn set_dataset_processing_state(&mut self, dataset: usize, state: &DatasetProcessingState) {
        if let (Some(Dataset::Nmr2D(current)), DatasetProcessingState::Nmr2D { params, preset }) =
            (self.doc.datasets.get_mut(dataset), state)
        {
            current.params = params.clone();
            current.preset = *preset;
            self.schedule_2d_processing(dataset, false);
            return;
        }
        let Some(current) = self.doc.datasets.get_mut(dataset) else {
            return;
        };
        if let Err(error) = state.apply_to(current) {
            self.session.status = error.to_string();
            return;
        }
        self.recompute_integrals_2d_after_processing(dataset);
        self.rebuild_canvases_for(dataset);
    }

    pub fn finish_pending_wheel_zoom(&mut self, now: f64, force: bool) {
        let Some(pending) = self.session.ui.wheel_zoom.clone() else {
            return;
        };
        if !force && now - pending.last_input_time < 0.18 {
            return;
        }
        self.session.ui.wheel_zoom = None;
        if let Some(canvas) = self.doc.canvases.get(pending.canvas) {
            let Some(object) = canvas
                .object(pending.object)
                .and_then(|object| object.plot())
            else {
                return;
            };
            self.execute_action(Action::set_object_viewport(
                pending.canvas,
                pending.object,
                pending.before,
                object.viewport.clone(),
            ));
        }
    }

    fn apply_action(&mut self, action: &Action) {
        match action {
            Action::Composite(actions) => {
                for action in actions {
                    self.apply_action(action);
                }
            }
            Action::UpdateDatasetProcessing { dataset, after, .. } => {
                self.set_dataset_processing_state(*dataset, after);
            }
            Action::SetObjectViewport {
                canvas,
                object,
                after,
                ..
            } => {
                self.set_object_viewport(*canvas, *object, after);
            }
            Action::MoveResizeObject {
                canvas,
                object,
                after,
                ..
            } => {
                self.set_object_frame(*canvas, *object, *after);
            }
            Action::SetObjectFrames { canvas, after, .. } => {
                for &(id, frame) in after {
                    self.set_object_frame(*canvas, id, frame);
                }
            }
            Action::SetObjectGroups { canvas, after, .. } => {
                self.set_object_groups(*canvas, after);
            }
            Action::ReorderObjects { canvas, after, .. } => {
                self.reorder_objects_value(*canvas, after);
            }
            Action::SetCanvasSize { canvas, after, .. } => {
                self.set_canvas_size(*canvas, after);
            }
            Action::MoveCanvasOnBoard { canvas, after, .. } => {
                if let Some(c) = self.doc.canvases.get_mut(*canvas) {
                    c.board_pos = *after;
                }
            }
            Action::MoveSheetOnBoard { dataset, after, .. } => {
                if let Some(t) = self
                    .doc
                    .datasets
                    .get_mut(*dataset)
                    .and_then(Dataset::as_table_mut)
                {
                    t.board_pos = *after;
                }
            }
            Action::TidyBoard { after, .. } => {
                for &(frame, pos) in after {
                    crate::state::set_frame_board_pos(self, frame, pos);
                }
            }
            Action::SetPageLayout { canvas, after, .. } => {
                self.set_page_layout_value(*canvas, *after);
            }
            Action::ArrangeObjects {
                canvas,
                after_layout,
                after,
                ..
            } => {
                self.apply_arrangement(*canvas, *after_layout, after);
            }
            Action::SetPanelMeta {
                canvas,
                object,
                after,
                ..
            } => {
                self.set_panel_meta(*canvas, *object, after.clone());
            }
            Action::SetObjectFlags {
                canvas,
                object,
                after,
                ..
            } => {
                self.set_object_flags(*canvas, *object, *after);
            }
            Action::BoardViewInsert { index, view } => {
                self.board_view_do_insert(*index, view);
            }
            Action::BoardViewRemove { index, view } => {
                self.board_view_do_remove(*index, view);
            }
            Action::SetDataBinding {
                canvas,
                object,
                after,
                ..
            } => {
                self.set_object_binding(*canvas, *object, after);
            }
            Action::SetChartType {
                canvas,
                object,
                after,
                ..
            } => {
                self.set_object_chart(*canvas, *object, after);
            }
            Action::SetStackSpec {
                canvas,
                object,
                after,
                ..
            } => {
                self.set_object_stack(*canvas, *object, after);
            }
            Action::SetAxisProjections {
                canvas,
                object,
                after,
                ..
            } => {
                self.set_object_projections(*canvas, *object, after);
            }
            Action::RenameCanvas { canvas, after, .. } => {
                if let Some(c) = self.doc.canvases.get_mut(*canvas) {
                    c.name = after.clone();
                }
            }
            Action::RenameObject {
                canvas,
                object,
                after,
                ..
            } => {
                if let Some(object) = self
                    .doc
                    .canvases
                    .get_mut(*canvas)
                    .and_then(|canvas| canvas.object_mut(*object))
                {
                    object.name.clone_from(after);
                }
            }
            Action::SetCanvasCaption { canvas, after, .. } => {
                self.set_canvas_caption_value(*canvas, after);
            }
            Action::SetPanelLabelStyle { canvas, after, .. } => {
                if let Some(c) = self.doc.canvases.get_mut(*canvas) {
                    c.panel_label_style = *after;
                }
            }
            Action::RenameDataset { dataset, after, .. } => {
                if let Some(d) = self.doc.datasets.get_mut(*dataset) {
                    d.set_name(after.clone());
                }
            }
            Action::SetCurveFitAnalyses { dataset, after, .. } => {
                self.set_curve_fit_analyses(*dataset, after);
            }
            Action::EditTable { dataset, delta } => {
                self.apply_table_edit(*dataset, delta, true);
            }
            Action::SetTypedTableState { dataset, after, .. } => {
                self.set_typed_table_state(*dataset, after);
            }
            Action::SetRegions { dataset, after, .. } => {
                self.set_regions(*dataset, after);
            }
            Action::SetIntegrals { dataset, after, .. } => {
                self.set_integrals(*dataset, after);
            }
            Action::SetIntegrals2D { dataset, after, .. } => {
                self.set_integrals_2d(*dataset, after);
            }
            Action::SetPeaks { dataset, after, .. } => {
                self.set_peaks(*dataset, after);
            }
            Action::SetLineFits { dataset, after, .. } => {
                self.set_line_fits(*dataset, after);
            }
            Action::SetMultiplets { dataset, after, .. } => {
                self.set_multiplets(*dataset, after);
            }
            Action::SetTableStatistics { dataset, after, .. } => {
                self.set_table_statistics(*dataset, after);
            }
            Action::InsertObject { canvas, object, .. } => {
                self.insert_object_value(*canvas, object.as_ref().clone());
            }
            Action::DeleteObject { canvas, object, .. } => {
                self.remove_object_value(*canvas, object.id);
            }
            Action::SetObjectText {
                canvas,
                object,
                after,
                ..
            } => {
                self.set_object_text_value(*canvas, *object, after.clone());
            }
            Action::SetObjectStyle { canvas, after, .. } => {
                self.set_object_styles(*canvas, after);
            }
            Action::DeleteCanvas {
                index,
                active_after,
                ..
            } => {
                if *index < self.doc.canvases.len() {
                    self.doc.canvases.remove(*index);
                    self.session.active_canvas = *active_after;
                    if let Some(ci) = self.session.active_canvas {
                        let active = self.doc.canvases[ci].active_dataset();
                        self.set_active_dataset(active);
                    }
                    self.reset_interaction();
                    self.session.ui.wheel_zoom = None;
                    self.session.ui.selection = Selection::None;
                    self.session.ui.panel_note_inline_edit = None;
                    self.session.ui.panel_note_edit = None;
                    self.session.ui.canvas_settings = None;
                    self.session.ui.rename = None;
                }
            }
            Action::InsertCanvas { index, canvas, .. } => {
                self.insert_canvas_value(*index, canvas.as_ref().clone());
            }
            Action::ApplyTheme { canvas, after, .. } => {
                self.apply_theme_snapshot(*canvas, after);
            }
            Action::SetFigureTypography { after, .. } => {
                self.set_figure_typography_value(*after);
            }
            Action::InsertDatasetWithCanvas {
                dataset_index,
                canvas_index,
                canvas_resource_id,
                dataset,
                canvas_name,
                size_mm,
                inserted_into_existing_canvas,
                inserted_object_id,
                ..
            } => {
                if *dataset_index != self.doc.datasets.len() {
                    return;
                }
                self.doc.datasets.push(dataset.as_ref().clone());
                if let Some(ci) = inserted_into_existing_canvas {
                    let Some(canvas) = self.doc.canvases.get(*ci) else {
                        return;
                    };
                    let page = canvas.size_pt();
                    let offset = 18.0 * canvas.objects.len() as f32;
                    let object_name = format!("Plot {}", canvas.objects.len() + 1);
                    let frame = ObjectFrame::new(
                        24.0 + offset,
                        24.0 + offset,
                        (page[0] * 0.58).max(120.0),
                        (page[1] * 0.45).max(90.0),
                    );
                    let id = inserted_object_id.unwrap_or(canvas.next_object_id);
                    let object = self.build_plot_object(*dataset_index, frame, id, object_name);
                    let canvas = self.doc.canvases.get_mut(*ci).unwrap();
                    canvas.next_object_id = canvas.next_object_id.max(id + 1);
                    canvas.objects.push(object);
                    self.session.active_canvas = Some(*ci);
                } else {
                    if *canvas_index != self.doc.canvases.len() {
                        return;
                    }
                    let mut canvas = CanvasDocument::new(canvas_name.clone(), *size_mm);
                    canvas.resource_id.clone_from(canvas_resource_id);
                    canvas.board_pos = crate::state::next_page_board_pos(self);
                    let page = canvas.size_pt();
                    let id = canvas.allocate_object_id();
                    let object = self.build_plot_object(
                        *dataset_index,
                        ObjectFrame::new(0.0, 0.0, page[0], page[1]),
                        id,
                        "Plot 1".to_owned(),
                    );
                    canvas.objects.push(object);
                    self.doc.canvases.push(canvas);
                    self.session.active_canvas = Some(*canvas_index);
                }
                self.focus_single(*dataset_index);
                self.session.view = PrimaryView::Canvas;
            }
            Action::TransferObjects { .. } => self.apply_transfer(action),
            Action::TileDrop { .. } => self.apply_tile_drop(action),
        }
    }

    fn set_object_viewport(&mut self, canvas: usize, object: ObjectId, viewport: &CanvasViewport) {
        let Some(object) = self
            .doc
            .canvases
            .get_mut(canvas)
            .and_then(|canvas| canvas.object_mut(object))
        else {
            return;
        };
        let Some(plot) = object.plot_mut() else {
            return;
        };
        plot.viewport = viewport.clone();
        plot.viewport.apply_to(&mut plot.figure);
    }

    pub fn set_object_frame(&mut self, canvas: usize, object: ObjectId, frame: ObjectFrame) {
        let Some(o) = self
            .doc
            .canvases
            .get_mut(canvas)
            .and_then(|canvas| canvas.object_mut(object))
        else {
            return;
        };
        o.frame = frame;
        if let Some(plot) = o.plot() {
            let binding = plot.binding.clone();
            let chart = plot.chart.clone();
            let stack = plot.stack;
            let projections = plot.projections.clone();
            let size = [
                frame.width / crate::state::MM_TO_PT,
                frame.height / crate::state::MM_TO_PT,
            ];
            let fig = self.build_object_figure(&binding, &chart, &stack, &projections, size);
            self.apply_viewport_to_plot_object(canvas, object, fig);
        }
    }

    fn set_object_groups(
        &mut self,
        canvas: usize,
        groups: &[(ObjectId, Option<crate::state::GroupId>)],
    ) {
        let Some(c) = self.doc.canvases.get_mut(canvas) else {
            return;
        };
        for &(id, group) in groups {
            if let Some(object) = c.object_mut(id) {
                object.group = group;
            }
        }
    }

    fn reorder_objects_value(&mut self, canvas: usize, order: &[ObjectId]) {
        if let Some(c) = self.doc.canvases.get_mut(canvas) {
            let mut objects = std::mem::take(&mut c.objects);
            objects.sort_by_key(|o| {
                order
                    .iter()
                    .position(|id| *id == o.id)
                    .unwrap_or(usize::MAX)
            });
            c.objects = objects;
        }
    }

    fn insert_canvas_value(&mut self, index: usize, mut canvas: CanvasDocument) {
        if index > self.doc.canvases.len() {
            return;
        }
        if canvas.board_pos == [0.0, 0.0] {
            canvas.board_pos = crate::state::next_page_board_pos(self);
        }
        self.doc.canvases.insert(index, canvas);
        self.session.active_canvas = Some(index);
        let active = self.doc.canvases[index].active_dataset();
        self.set_active_dataset(active);
        self.session.view = PrimaryView::Canvas;
        self.set_selection(Selection::None);
    }

    fn remove_canvas_at(&mut self, index: usize, active_before: Option<usize>) {
        if index >= self.doc.canvases.len() {
            return;
        }
        self.doc.canvases.remove(index);
        self.session.active_canvas = active_before.filter(|&i| i < self.doc.canvases.len());
        let active = self
            .session
            .active_canvas
            .and_then(|ci| self.doc.canvases[ci].active_dataset());
        self.set_active_dataset(active);
        self.set_selection(Selection::None);
    }

    fn set_canvas_size(&mut self, canvas: usize, state: &PageSizeState) {
        let Some(c) = self.doc.canvases.get_mut(canvas) else {
            return;
        };
        c.size_mm = state.size_mm;
        c.size_preset_id = state.preset_id.clone();
        self.rebuild_canvas(canvas);
    }

    fn set_canvas_caption_value(&mut self, canvas: usize, caption: &(String, bool)) {
        if let Some(c) = self.doc.canvases.get_mut(canvas) {
            c.caption = caption.0.clone();
            c.caption_visible = caption.1;
        }
    }

    fn set_page_layout_value(&mut self, canvas: usize, layout: PageLayout) {
        if let Some(c) = self.doc.canvases.get_mut(canvas) {
            c.layout = layout;
        }
    }

    fn apply_arrangement(
        &mut self,
        canvas: usize,
        layout: PageLayout,
        frames: &[(ObjectId, ObjectFrame)],
    ) {
        self.set_page_layout_value(canvas, layout);
        for &(id, frame) in frames {
            self.set_object_frame(canvas, id, frame);
        }
    }

    fn set_curve_fit_analyses(
        &mut self,
        dataset: usize,
        state: &(Vec<Option<CurveFitReference>>, Vec<StoredCurveFitAnalysis>),
    ) {
        if let Some(t) = self
            .doc
            .datasets
            .get_mut(dataset)
            .and_then(Dataset::as_table_mut)
        {
            for (binding, fit) in t.series_bindings.iter_mut().zip(&state.0) {
                binding.fit = fit.clone();
            }
            t.curve_fit_analyses.clone_from(&state.1);
        }
        self.rebuild_canvases_for(dataset);
    }

    fn insert_object_value(&mut self, canvas: usize, object: CanvasObject) {
        let id = object.id;
        if let Some(c) = self.doc.canvases.get_mut(canvas) {
            c.next_object_id = c.next_object_id.max(id + 1);
            c.objects.push(object);
        }
        self.select_object(canvas, id);
    }

    pub(super) fn remove_object_value(&mut self, canvas: usize, id: ObjectId) {
        if let Some(c) = self.doc.canvases.get_mut(canvas) {
            c.objects.retain(|o| o.id != id);
            if c.selected_object == Some(id) {
                c.selected_object = None;
            }
        }
        if self.session.ui.selection.object() == Some(id) {
            self.session.ui.selection = Selection::None;
        }
        if matches!(self.session.ui.text_edit, Some(ref e) if e.canvas == canvas && e.object == id)
        {
            self.session.ui.text_edit = None;
        }
        if matches!(self.session.ui.panel_note_edit, Some(ref e) if e.canvas == canvas && e.object == id)
        {
            self.session.ui.panel_note_edit = None;
        }
        if matches!(self.session.ui.panel_note_inline_edit, Some(ref e) if e.canvas == canvas && e.object == id)
        {
            self.session.ui.panel_note_inline_edit = None;
        }
    }

    pub fn set_object_styles(&mut self, canvas: usize, styles: &[(ObjectId, ObjectStyle)]) {
        let Some(c) = self.doc.canvases.get_mut(canvas) else {
            return;
        };
        for (id, style) in styles {
            if let Some(o) = c.object_mut(*id) {
                o.set_style(style);
            }
        }
    }

    fn set_object_text_value(&mut self, canvas: usize, object: ObjectId, text: TextBox) {
        if let Some(t) = self
            .doc
            .canvases
            .get_mut(canvas)
            .and_then(|c| c.object_mut(object))
            .and_then(|o| o.text_mut())
        {
            *t = text;
        }
    }

    fn set_object_binding(&mut self, canvas: usize, object: ObjectId, binding: &DataBinding) {
        let Some(o) = self
            .doc
            .canvases
            .get_mut(canvas)
            .and_then(|c| c.object_mut(object))
        else {
            return;
        };
        let Some(plot) = o.plot_mut() else {
            return;
        };
        plot.binding = binding.clone();
        let chart = plot.chart.clone();
        let stack = plot.stack;
        let projections = plot.projections.clone();
        let frame = o.frame;
        let size = [
            frame.width / crate::state::MM_TO_PT,
            frame.height / crate::state::MM_TO_PT,
        ];
        let mut fig = self.build_object_figure(binding, &chart, &stack, &projections, size);
        if let Some(plot) = self
            .doc
            .canvases
            .get_mut(canvas)
            .and_then(|c| c.object_mut(object))
            .and_then(|o| o.plot_mut())
        {
            plot.viewport = CanvasViewport::from_figure(&fig);
            plot.viewport.apply_to(&mut fig);
            plot.figure = fig;
        }
    }

    /// Apply a plot's chart-type selection, rebuilding its figure through the
    /// registry and re-fitting the viewport (chart axes change between types).
    fn set_object_chart(
        &mut self,
        canvas: usize,
        object: ObjectId,
        chart: &crate::state::ChartSpec,
    ) {
        let Some(o) = self
            .doc
            .canvases
            .get_mut(canvas)
            .and_then(|c| c.object_mut(object))
        else {
            return;
        };
        let Some(plot) = o.plot_mut() else {
            return;
        };
        plot.chart = chart.clone();
        let binding = plot.binding.clone();
        let stack = plot.stack;
        let projections = plot.projections.clone();
        let frame = o.frame;
        let size = [
            frame.width / crate::state::MM_TO_PT,
            frame.height / crate::state::MM_TO_PT,
        ];
        let mut fig = self.build_object_figure(&binding, chart, &stack, &projections, size);
        if let Some(plot) = self
            .doc
            .canvases
            .get_mut(canvas)
            .and_then(|c| c.object_mut(object))
            .and_then(|o| o.plot_mut())
        {
            plot.viewport = CanvasViewport::from_figure(&fig);
            plot.viewport.apply_to(&mut fig);
            plot.figure = fig;
        }
    }

    /// Apply a plot's stacking layout, rebuilding its figure and re-fitting the
    /// viewport (the vertical offsets change the figure's extents).
    fn set_object_stack(
        &mut self,
        canvas: usize,
        object: ObjectId,
        stack: &crate::state::StackSpec,
    ) {
        let Some(o) = self
            .doc
            .canvases
            .get_mut(canvas)
            .and_then(|c| c.object_mut(object))
        else {
            return;
        };
        let Some(plot) = o.plot_mut() else {
            return;
        };
        plot.stack = *stack;
        let binding = plot.binding.clone();
        let chart = plot.chart.clone();
        let projections = plot.projections.clone();
        let frame = o.frame;
        let size = [
            frame.width / crate::state::MM_TO_PT,
            frame.height / crate::state::MM_TO_PT,
        ];
        let mut fig = self.build_object_figure(&binding, &chart, stack, &projections, size);
        if let Some(plot) = self
            .doc
            .canvases
            .get_mut(canvas)
            .and_then(|c| c.object_mut(object))
            .and_then(|o| o.plot_mut())
        {
            plot.viewport = CanvasViewport::from_figure(&fig);
            plot.viewport.apply_to(&mut fig);
            plot.figure = fig;
        }
    }

    /// Apply a plot's marginal axis projections, rebuilding its figure. The data
    /// ranges are unchanged, so the viewport is preserved rather than refit.
    fn set_object_projections(
        &mut self,
        canvas: usize,
        object: ObjectId,
        projections: &crate::state::AxisProjections,
    ) {
        let Some(o) = self
            .doc
            .canvases
            .get_mut(canvas)
            .and_then(|c| c.object_mut(object))
        else {
            return;
        };
        let Some(plot) = o.plot_mut() else {
            return;
        };
        plot.projections = projections.clone();
        let binding = plot.binding.clone();
        let chart = plot.chart.clone();
        let stack = plot.stack;
        let frame = o.frame;
        let size = [
            frame.width / crate::state::MM_TO_PT,
            frame.height / crate::state::MM_TO_PT,
        ];
        let mut fig = self.build_object_figure(&binding, &chart, &stack, projections, size);
        if let Some(plot) = self
            .doc
            .canvases
            .get_mut(canvas)
            .and_then(|c| c.object_mut(object))
            .and_then(|o| o.plot_mut())
        {
            plot.viewport.apply_to(&mut fig);
            plot.figure = fig;
        }
    }
}
