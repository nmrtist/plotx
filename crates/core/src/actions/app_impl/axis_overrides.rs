use super::*;

impl PlotxApp {
    pub(super) fn set_object_viewport(
        &mut self,
        canvas: usize,
        object: ObjectId,
        viewport: &CanvasViewport,
    ) {
        let Some(plot) = self
            .doc
            .canvases
            .get_mut(canvas)
            .and_then(|canvas| canvas.object_mut(object))
            .and_then(|object| object.plot_mut())
        else {
            return;
        };
        plot.viewport = viewport.clone();
        plot.apply_viewport();
    }

    /// Finish a live Inspector edit before another command can change its target
    /// or history position.
    pub fn finish_axis_overrides_edit(&mut self) {
        let Some((canvas, object, before)) = self.session.ui.axis_overrides_before.take() else {
            return;
        };
        let Some(after) = self
            .doc
            .canvases
            .get(canvas)
            .and_then(|canvas| canvas.object(object))
            .and_then(|object| object.plot())
            .map(|plot| plot.axis_overrides.clone())
        else {
            return;
        };
        self.execute_action(Action::set_axis_overrides(canvas, object, before, after));
    }

    /// Record a viewport command only after applying plot-specific invariants,
    /// so the action payload exactly matches the state that apply/undo stores.
    pub fn commit_object_viewport(
        &mut self,
        canvas: usize,
        object: ObjectId,
        mut before: CanvasViewport,
        mut after: CanvasViewport,
    ) {
        if let Some(plot) = self
            .doc
            .canvases
            .get(canvas)
            .and_then(|canvas| canvas.object(object))
            .and_then(|object| object.plot())
        {
            plot.normalize_viewport(&mut before);
            plot.normalize_viewport(&mut after);
        }
        self.execute_action(Action::set_object_viewport(canvas, object, before, after));
    }

    /// Apply a live Inspector value. Transitions back to `None` rebuild once to
    /// recover data-derived labels/ranges; edits among manual values only touch
    /// the presentation model and viewport, avoiding expensive data rebuilds.
    pub fn set_axis_overrides_value(
        &mut self,
        canvas: usize,
        object: ObjectId,
        after: &AxisOverrides,
    ) {
        let after = after.clone().normalized();
        let Some((before, binding, chart, stack, projections, frame)) = self
            .doc
            .canvases
            .get(canvas)
            .and_then(|canvas| canvas.object(object))
            .and_then(|object| {
                object.plot().map(|plot| {
                    (
                        plot.axis_overrides.clone(),
                        plot.binding.clone(),
                        plot.chart.clone(),
                        plot.stack,
                        plot.projections.clone(),
                        object.frame,
                    )
                })
            })
        else {
            return;
        };
        if before == after {
            return;
        }

        let x_range_changed = before.x_range != after.x_range;
        let y_range_changed = before.y_range != after.y_range;
        let needs_automatic_rebuild = cleared(&before.x_label, &after.x_label)
            || cleared(&before.y_label, &after.y_label)
            || cleared(&before.x_range, &after.x_range)
            || cleared(&before.y_range, &after.y_range)
            || cleared(&before.lock_aspect, &after.lock_aspect)
            || cleared(&before.x_show_tick_labels, &after.x_show_tick_labels)
            || cleared(&before.x_show_label, &after.x_show_label)
            || cleared(&before.y_show_tick_labels, &after.y_show_tick_labels)
            || cleared(&before.y_show_label, &after.y_show_label);

        let rebuilt = needs_automatic_rebuild.then(|| {
            let size = [
                frame.width / crate::state::MM_TO_PT,
                frame.height / crate::state::MM_TO_PT,
            ];
            self.build_object_figure(&binding, &chart, &stack, &projections, size)
        });

        let Some(plot) = self
            .doc
            .canvases
            .get_mut(canvas)
            .and_then(|canvas| canvas.object_mut(object))
            .and_then(|object| object.plot_mut())
        else {
            return;
        };
        plot.axis_overrides = after;

        if let Some(figure) = rebuilt {
            plot.rebuild_for_axis_overrides(figure, x_range_changed, y_range_changed);
            return;
        }

        plot.apply_axis_overrides();
        if x_range_changed
            && plot.figure().x.categories.is_none()
            && let Some(range) = plot.axis_overrides.x_range
        {
            plot.viewport.full_x = range;
            plot.viewport.view_x = range;
            if plot.viewport.auto_y {
                let figure = plot.figure().clone();
                plot.viewport.reset_x(&figure);
            }
        }
        if y_range_changed
            && plot.figure().y.categories.is_none()
            && let Some(range) = plot.axis_overrides.y_range
        {
            plot.viewport.full_y = range;
            plot.viewport.view_y = range;
            plot.viewport.auto_y = false;
        }
        plot.apply_viewport();
    }
}

fn cleared<T>(before: &Option<T>, after: &Option<T>) -> bool {
    before.is_some() && after.is_none()
}
