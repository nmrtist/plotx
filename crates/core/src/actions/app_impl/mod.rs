use super::*;
mod apply;
mod axis_overrides;
mod meta_edits;
mod processing;
mod revert;
mod table_edit;
mod validate;
pub use validate::ActionApplyError;
use validate::{ValidationShape, validate_action};

impl PlotxApp {
    pub fn execute_action(&mut self, action: Action) {
        self.finish_axis_overrides_edit();
        if let Err(error) = self.try_execute_action(action) {
            self.session.status = error.to_string();
        }
    }

    /// Validate a whole action before applying it, preventing partial stale composites.
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
        self.mark_document_dirty();
        self.doc.automation_revision = self.doc.automation_revision.saturating_add(1);
        Ok(())
    }

    pub fn undo(&mut self) {
        self.finish_pending_wheel_zoom(f64::INFINITY, true);
        self.finish_pending_wheel_property(f64::INFINITY, true);
        self.finish_axis_overrides_edit();
        self.reset_interaction();
        let Some(action) = self.session.undo_stack.pop() else {
            return;
        };
        let label = action.undo_label();
        self.revert_action(&action);
        self.session.redo_stack.push(action);
        self.mark_document_dirty();
        self.doc.automation_revision = self.doc.automation_revision.saturating_add(1);
        self.session.status = format!("Undid {label}.");
    }

    pub fn redo(&mut self) {
        self.finish_pending_wheel_zoom(f64::INFINITY, true);
        self.finish_pending_wheel_property(f64::INFINITY, true);
        self.finish_axis_overrides_edit();
        self.reset_interaction();
        let Some(action) = self.session.redo_stack.pop() else {
            return;
        };
        let label = action.undo_label();
        self.apply_action(&action);
        self.session.undo_stack.push(action);
        self.mark_document_dirty();
        self.doc.automation_revision = self.doc.automation_revision.saturating_add(1);
        self.session.status = format!("Redid {label}.");
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
        self.session.ui.wheel_property = None;
        self.session.ui.canvas_size_edit = None;
        self.session.ui.processing_session = None;
        self.session.ui.property_gesture = None;
        self.session.ui.inspector_edit = None;
        self.session.ui.axis_overrides_before = None;
        self.session.ui.selection = Selection::None;
        self.session.ui.panel_note_inline_edit = None;
        self.session.ui.panel_note_edit = None;
        self.session.ui.text_edit = None;
        self.session.ui.processing_scheme_dialog = None;
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
            self.commit_object_viewport(
                pending.canvas,
                pending.object,
                pending.before,
                object.viewport.clone(),
            );
        }
    }

    pub fn finish_pending_wheel_property(&mut self, now: f64, force: bool) {
        let Some(pending) = self.session.ui.wheel_property.as_ref() else {
            return;
        };
        if !force && now - pending.last_input_time < 0.18 {
            return;
        }
        let gesture_started = pending.gesture_started;
        let deferred_contour =
            gesture_started && pending.property == crate::properties::contour::BASE_MAGNITUDE;
        let target = (pending.canvas, pending.object);
        self.session.ui.wheel_property = None;
        if gesture_started {
            self.end_property_gesture();
        }
        if deferred_contour {
            self.rebuild_plot_presentation(target.0, target.1);
        }
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
            canvas.board_pos = crate::state::next_board_frame_pos(self, canvas.size_pt());
        }
        self.doc.canvases.insert(index, canvas);
        self.session.active_canvas = Some(index);
        let active = self.doc.canvases[index]
            .active_dataset()
            .and_then(|id| self.doc.dataset_index(id));
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
            .and_then(|ci| self.doc.canvases[ci].active_dataset())
            .and_then(|id| self.doc.dataset_index(id));
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
            c.next_object_id = c.next_object_id.max(id.checked_advance(1));
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
        if matches!(self.session.ui.axis_overrides_before, Some((ci, object, _)) if ci == canvas && object == id)
        {
            self.session.ui.axis_overrides_before = None;
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
        self.set_object_binding_with_viewport(canvas, object, binding, false);
    }

    fn set_object_presentation(&mut self, canvas: usize, object: ObjectId, binding: &DataBinding) {
        // A contour wheel gesture can emit many catalog commits in a fraction
        // of a second. Persist every value for accurate readout/undo, but retain
        // the last complete geometry and rebuild only once when the gesture
        // closes. This bounds background work without weakening ordinary panel
        // edits or starving other plots that share the same source field.
        let defer_contour = self
            .session
            .ui
            .wheel_property
            .as_ref()
            .is_some_and(|pending| {
                pending.canvas == canvas
                    && pending.object == object
                    && pending.gesture_started
                    && pending.property == crate::properties::contour::BASE_MAGNITUDE
            });
        if defer_contour {
            if let Some(plot) = self
                .doc
                .canvases
                .get_mut(canvas)
                .and_then(|canvas| canvas.object_mut(object))
                .and_then(|object| object.plot_mut())
            {
                plot.binding = binding.clone();
            }
            return;
        }
        self.set_object_binding_with_viewport(canvas, object, binding, true);
    }

    fn rebuild_plot_presentation(&mut self, canvas: usize, object: ObjectId) {
        let Some((binding, chart, stack, projections, frame, previous_contours)) = self
            .doc
            .canvases
            .get(canvas)
            .and_then(|canvas| canvas.object(object))
            .and_then(|object| {
                let plot = object.plot()?;
                Some((
                    plot.binding.clone(),
                    plot.chart.clone(),
                    plot.stack,
                    plot.projections.clone(),
                    object.frame,
                    plot.figure().contours.clone(),
                ))
            })
        else {
            return;
        };
        let size = [
            frame.width / crate::state::MM_TO_PT,
            frame.height / crate::state::MM_TO_PT,
        ];
        let mut figure = self.build_object_figure(&binding, &chart, &stack, &projections, size);
        if figure.contours.len() < previous_contours.len() {
            figure.contours = previous_contours;
        }
        if let Some(plot) = self
            .doc
            .canvases
            .get_mut(canvas)
            .and_then(|canvas| canvas.object_mut(object))
            .and_then(|object| object.plot_mut())
        {
            plot.preserve_viewport_on_rebuild(figure);
        }
    }

    fn set_object_binding_with_viewport(
        &mut self,
        canvas: usize,
        object: ObjectId,
        binding: &DataBinding,
        preserve_viewport: bool,
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
        plot.binding = binding.clone();
        let chart = plot.chart.clone();
        let stack = plot.stack;
        let projections = plot.projections.clone();
        let frame = o.frame;
        let size = [
            frame.width / crate::state::MM_TO_PT,
            frame.height / crate::state::MM_TO_PT,
        ];
        let fig = self.build_object_figure(binding, &chart, &stack, &projections, size);
        if let Some(plot) = self
            .doc
            .canvases
            .get_mut(canvas)
            .and_then(|c| c.object_mut(object))
            .and_then(|o| o.plot_mut())
        {
            if preserve_viewport {
                plot.preserve_viewport_on_rebuild(fig);
            } else {
                plot.reset_viewport_on_rebuild(fig);
            }
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
        let fig = self.build_object_figure(&binding, chart, &stack, &projections, size);
        if let Some(plot) = self
            .doc
            .canvases
            .get_mut(canvas)
            .and_then(|c| c.object_mut(object))
            .and_then(|o| o.plot_mut())
        {
            plot.reset_viewport_on_rebuild(fig);
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
        let fig = self.build_object_figure(&binding, &chart, stack, &projections, size);
        if let Some(plot) = self
            .doc
            .canvases
            .get_mut(canvas)
            .and_then(|c| c.object_mut(object))
            .and_then(|o| o.plot_mut())
        {
            plot.reset_viewport_on_rebuild(fig);
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
        let fig = self.build_object_figure(&binding, &chart, &stack, projections, size);
        if let Some(plot) = self
            .doc
            .canvases
            .get_mut(canvas)
            .and_then(|c| c.object_mut(object))
            .and_then(|o| o.plot_mut())
        {
            plot.preserve_viewport_on_rebuild(fig);
        }
    }
}
