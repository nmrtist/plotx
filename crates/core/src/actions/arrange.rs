use super::*;

impl PlotxApp {
    /// Reposition the active canvas's plot objects into a `rows × cols` grid
    /// (row-major, current object order) as one undoable step. Objects beyond
    /// the cell count keep their frame.
    pub fn arrange_active_canvas_grid(&mut self, rows: u32, cols: u32) {
        self.arrange_active_canvas_grid_with_simplify(rows, cols, false);
    }

    pub fn arrange_active_canvas_grid_with_simplify(
        &mut self,
        rows: u32,
        cols: u32,
        simplify_inner_axes: bool,
    ) {
        let Some(ci) = self.session.active_canvas else {
            return;
        };
        let Some(canvas) = self.doc.canvases.get(ci) else {
            return;
        };
        let before_layout = canvas.layout;
        let mut after_layout = before_layout;
        after_layout.rows = rows.max(1);
        after_layout.cols = cols.max(1);
        let page = canvas.size_pt();
        let ids = canvas.plot_object_ids();
        let axis_changes = if simplify_inner_axes {
            simplified_axis_changes(canvas, &ids, rows, cols)
        } else {
            Vec::new()
        };
        let items = layout_items(canvas, &ids, &[], &axis_changes);
        let first_pass = crate::layout::arrange_grid(page, &after_layout, &items);
        // Axis tick selection depends on the resized frame. One bounded
        // refinement keeps Visual spacing object-aware without convergence
        // loops, and measures the post-simplification figure when requested.
        let refined_items = layout_items(canvas, &ids, &first_pass, &axis_changes);
        let after = crate::layout::arrange_grid(page, &after_layout, &refined_items);
        let before: Vec<(ObjectId, ObjectFrame)> = after
            .iter()
            .filter_map(|(id, _)| canvas.object(*id).map(|o| (*id, o.frame)))
            .collect();
        let placed = after.len();
        let total = ids.len();
        let arrange = Action::ArrangeObjects {
            canvas: ci,
            before_layout,
            after_layout,
            before,
            after,
        };
        if simplify_inner_axes {
            let mut actions = vec![arrange];
            actions.extend(axis_change_actions(ci, axis_changes));
            self.execute_action(Action::Composite(actions));
        } else {
            self.execute_action(arrange);
        }
        self.session.status = if placed < total {
            format!(
                "Arranged {placed} of {total} objects into {rows}×{cols}; {} kept in place.",
                total - placed
            )
        } else {
            format!("Arranged {placed} object(s) into a {rows}×{cols} grid.")
        };
    }

    /// Hide inner axis text for the current grid without changing frames.
    pub fn simplify_inner_axes(&mut self) {
        let Some(ci) = self.session.active_canvas else {
            return;
        };
        let Some(canvas) = self.doc.canvases.get(ci) else {
            return;
        };
        let frames: Vec<_> = canvas
            .objects
            .iter()
            .filter(|object| object.plot().is_some())
            .map(|object| (object.id, object.frame))
            .collect();
        if frames.len() < 2 {
            self.session.status =
                "Could not simplify axes: at least two plots are required.".to_owned();
            return;
        }
        let Some(grid) = crate::layout::infer_occupied_grid(&frames) else {
            self.session.status =
                "Could not simplify axes: arrange plots into a grid first.".to_owned();
            return;
        };
        let actions = axis_change_actions(
            ci,
            simplified_axis_changes(canvas, &grid.ids, grid.rows, grid.cols),
        );
        if actions.is_empty() {
            self.session.status = "Axes are already simplified.".to_owned();
            return;
        }
        self.execute_action(Action::Composite(actions));
        self.session.status = "Simplified inner axes.".to_owned();
    }

    pub fn set_spacing_mode(&mut self, mode: crate::layout::SpacingMode) {
        let Some(ci) = self.session.active_canvas else {
            return;
        };
        let before = self.doc.canvases[ci].layout;
        let mut after = before;
        after.spacing_mode = mode;
        self.commit_page_layout(ci, before, after);
    }

    pub fn set_gutter_preset(&mut self, preset: crate::layout::GutterPreset) {
        let Some(ci) = self.session.active_canvas else {
            return;
        };
        let before = self.doc.canvases[ci].layout;
        let mut after = before;
        after.gutter_mm = preset.millimetres();
        self.commit_page_layout(ci, before, after);
    }

    /// Re-flow every board frame (pages and sheets) into an aligned grid with a
    /// uniform gutter, as one undoable step — the board's "Tidy up". No-op when
    /// nothing would move.
    pub fn tidy_board(&mut self) {
        let after = crate::state::tidy_board_layout(self);
        let before: Vec<(crate::state::FrameRef, [f32; 2])> = after
            .iter()
            .filter_map(|&(frame, _)| {
                crate::state::frame_board_pos(self, frame).map(|pos| (frame, pos))
            })
            .collect();
        let n = after.len();
        self.execute_action(Action::TidyBoard { before, after });
        self.session.status = format!("Tidied {n} frame(s) on the board.");
    }

    /// The unlocked selected objects' `(id, frame)` on the active canvas — the
    /// input for align/distribute and group move.
    fn selected_movable_frames(&self, ci: usize) -> Vec<(ObjectId, ObjectFrame)> {
        let Some(c) = self.doc.canvases.get(ci) else {
            return Vec::new();
        };
        self.session
            .ui
            .selection
            .objects()
            .iter()
            .filter_map(|&id| c.object(id))
            .filter(|o| !o.locked)
            .map(|o| (o.id, o.frame))
            .collect()
    }

    /// Align the current multi-selection to a shared edge/centre (≥2 objects).
    pub fn align_selected(&mut self, mode: crate::layout::Align) {
        let Some(ci) = self.session.active_canvas else {
            return;
        };
        let before = self.selected_movable_frames(ci);
        if before.len() < 2 {
            return;
        }
        let after = crate::layout::align(&before, mode);
        self.execute_action(Action::set_object_frames(ci, before, after));
        self.session.status = "Aligned selection.".to_owned();
    }

    /// Equalise spacing across the current multi-selection (≥3 objects).
    pub fn distribute_selected(&mut self, axis: crate::layout::Distribute) {
        let Some(ci) = self.session.active_canvas else {
            return;
        };
        let before = self.selected_movable_frames(ci);
        if before.len() < 3 {
            return;
        }
        let after = crate::layout::distribute(&before, axis);
        self.execute_action(Action::set_object_frames(ci, before, after));
        self.session.status = "Distributed selection.".to_owned();
    }

    /// Group the current multi-selection under a fresh group id (≥2 objects).
    pub fn group_selected(&mut self) {
        let Some(ci) = self.session.active_canvas else {
            return;
        };
        let ids: Vec<ObjectId> = self.session.ui.selection.objects().to_vec();
        if ids.len() < 2 {
            return;
        }
        let group = self.doc.canvases[ci].allocate_group_id();
        let before: Vec<(ObjectId, Option<crate::state::GroupId>)> = ids
            .iter()
            .filter_map(|&id| self.doc.canvases[ci].object(id).map(|o| (id, o.group)))
            .collect();
        let after: Vec<(ObjectId, Option<crate::state::GroupId>)> =
            ids.iter().map(|&id| (id, Some(group))).collect();
        let count = after.len();
        self.execute_action(Action::set_object_groups(ci, before, after));
        self.session.status = format!("Grouped {count} objects.");
    }

    pub fn ungroup_selected(&mut self) {
        let Some(ci) = self.session.active_canvas else {
            return;
        };
        let mut ids: Vec<ObjectId> = Vec::new();
        for id in self.session.ui.selection.objects().to_vec() {
            for m in self.doc.canvases[ci].group_members(id) {
                if !ids.contains(&m) {
                    ids.push(m);
                }
            }
        }
        let before: Vec<(ObjectId, Option<crate::state::GroupId>)> = ids
            .iter()
            .filter_map(|&id| self.doc.canvases[ci].object(id).map(|o| (id, o.group)))
            .filter(|(_, g)| g.is_some())
            .collect();
        if before.is_empty() {
            return;
        }
        let after: Vec<(ObjectId, Option<crate::state::GroupId>)> =
            before.iter().map(|&(id, _)| (id, None)).collect();
        self.execute_action(Action::set_object_groups(ci, before, after));
        self.session.status = "Ungrouped selection.".to_owned();
    }

    pub fn z_order_selected(&mut self, op: crate::actions::ZOrder) {
        let Some(ci) = self.session.active_canvas else {
            return;
        };
        let targets: Vec<ObjectId> = self.session.ui.selection.objects().to_vec();
        self.apply_z_order(ci, &targets, op);
    }

    pub fn apply_z_order(&mut self, ci: usize, targets: &[ObjectId], op: crate::actions::ZOrder) {
        if targets.is_empty() {
            return;
        }
        let Some(c) = self.doc.canvases.get(ci) else {
            return;
        };
        let before: Vec<ObjectId> = c.objects.iter().map(|o| o.id).collect();
        let after = crate::actions::reorder_z(&before, targets, op);
        self.execute_action(Action::reorder_objects(ci, before, after));
        self.session.status = "Reordered objects.".to_owned();
    }

    /// Commit a page-layout change (margins/gutter/divisions) as one undoable
    /// step. `show_grid` should be toggled via `set_show_grid` instead.
    pub fn commit_page_layout(&mut self, canvas: usize, before: PageLayout, after: PageLayout) {
        self.execute_action(Action::set_page_layout(canvas, before, after));
    }

    /// Toggle the layout grid overlay for a canvas. This is a view preference,
    /// not undoable document content.
    pub fn set_show_grid(&mut self, canvas: usize, show: bool) {
        if let Some(c) = self.doc.canvases.get_mut(canvas)
            && c.layout.show_grid != show
        {
            c.layout.show_grid = show;
            self.doc.dirty = true;
        }
    }

    pub fn set_snap_enabled(&mut self, enabled: bool) {
        self.session.ui.snap_enabled = enabled;
        if !enabled {
            self.session.ui.snap_guides.clear();
        }
        crate::settings::update(|settings| {
            settings.export.include_view_snapshots = self.doc.save_include_view_snapshots;
            settings.general.snap_enabled = enabled;
        });
    }
}

fn layout_items(
    canvas: &crate::state::CanvasDocument,
    ids: &[ObjectId],
    frames: &[(ObjectId, ObjectFrame)],
    axis_changes: &[AxisOverrideChange],
) -> Vec<crate::layout::LayoutItem> {
    ids.iter()
        .filter_map(|&id| {
            let object = canvas.object(id)?;
            let plot = object.plot()?;
            let frame = frames
                .iter()
                .find_map(|(candidate, frame)| (*candidate == id).then_some(*frame))
                .unwrap_or(object.frame);
            if let Some(change) = axis_changes.iter().find(|change| change.id == id) {
                let mut figure = plot.figure.clone();
                change.after.apply_to(&mut figure);
                Some(crate::layout::layout_item(id, &figure, frame))
            } else {
                Some(crate::layout::layout_item(id, &plot.figure, frame))
            }
        })
        .collect()
}

struct AxisOverrideChange {
    id: ObjectId,
    before: crate::state::AxisOverrides,
    after: crate::state::AxisOverrides,
}

fn simplified_axis_changes(
    canvas: &crate::state::CanvasDocument,
    ids: &[ObjectId],
    rows: u32,
    cols: u32,
) -> Vec<AxisOverrideChange> {
    ids.iter()
        .zip(crate::layout::outer_axis_cells(ids.len(), rows, cols))
        .filter_map(|(&id, (keep_x, keep_y))| {
            let before = canvas.object(id)?.plot()?.axis_overrides.clone();
            let mut after = before.clone();
            if !keep_x {
                after.x_show_tick_labels = Some(false);
                after.x_show_label = Some(false);
            }
            if !keep_y {
                after.y_show_tick_labels = Some(false);
                after.y_show_label = Some(false);
            }
            (after != before).then_some(AxisOverrideChange { id, before, after })
        })
        .collect()
}

fn axis_change_actions(canvas_index: usize, changes: Vec<AxisOverrideChange>) -> Vec<Action> {
    changes
        .into_iter()
        .map(|change| {
            Action::set_axis_overrides(canvas_index, change.id, change.before, change.after)
        })
        .collect()
}
