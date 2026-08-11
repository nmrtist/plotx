use super::*;
use crate::state::{ContentId, ObjectId, PanelId};

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
        let page_siblings = self.page_sibling_frames(ci);
        if page_siblings.len() >= 2 {
            let ids: Vec<_> = page_siblings.iter().map(|(_, id, _)| *id).collect();
            let after = crate::layout::assign_grid(canvas.size_pt(), &after_layout, &ids);
            self.execute_action(Action::Composite(vec![
                Action::set_page_layout(ci, before_layout, after_layout),
                self.page_sibling_frame_action(ci, &page_siblings, &after),
            ]));
            self.session.status = format!(
                "Arranged {} page item(s) into a {rows}×{cols} grid.",
                after.len()
            );
            return;
        }
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
            .filter_map(|(id, _)| canvas.layout_frame(*id).map(|frame| (*id, frame)))
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
            .filter_map(|object| {
                canvas
                    .layout_frame(object.id)
                    .map(|frame| (object.id, frame))
            })
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
        self.execute_action(Action::set_spacing_mode(ci, before, mode));
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
            .filter_map(|o| c.layout_frame(o.id).map(|frame| (o.id, frame)))
            .collect()
    }

    fn page_sibling_frames(&self, ci: usize) -> Vec<(PageSibling, ObjectId, ObjectFrame)> {
        let Some(page) = self.doc.canvases.get(ci) else {
            return Vec::new();
        };
        self.session
            .ui
            .hierarchical_selection
            .paths()
            .iter()
            .filter_map(|path| match (path.panel, path.content) {
                (Some(panel), None) => page
                    .panel(panel)
                    .filter(|panel| !panel.locked)
                    .map(|panel| (PageSibling::Panel(panel.id), panel.frame)),
                (None, Some(content)) => page
                    .object(content)
                    .filter(|item| !item.locked && page.parent_panel(content).is_none())
                    .map(|item| (PageSibling::Content(content), item.frame)),
                _ => None,
            })
            .enumerate()
            .map(|(index, (target, frame))| (target, ObjectId::new(index as u64 + 1), frame))
            .collect()
    }

    fn page_sibling_frame_action(
        &self,
        ci: usize,
        before: &[(PageSibling, ObjectId, ObjectFrame)],
        after: &[(ObjectId, ObjectFrame)],
    ) -> Action {
        let state_before = PanelState::of(&self.doc.canvases[ci]);
        let mut page = self.doc.canvases[ci].clone();
        for ((target, _, _), (_, frame)) in before.iter().zip(after) {
            match target {
                PageSibling::Panel(id) => page.panel_mut(*id).unwrap().frame = *frame,
                PageSibling::Content(id) => page.object_mut(*id).unwrap().frame = *frame,
            }
        }
        Action::ReplacePanelState {
            canvas: ci,
            before: state_before,
            after: PanelState::of(&page),
        }
    }

    /// Align the current multi-selection to a shared edge/centre (≥2 objects).
    pub fn align_selected(&mut self, mode: crate::layout::Align) {
        let Some(ci) = self.session.active_canvas else {
            return;
        };
        let page_siblings = self.page_sibling_frames(ci);
        if page_siblings.len() >= 2 {
            let frames: Vec<_> = page_siblings
                .iter()
                .map(|(_, id, frame)| (*id, *frame))
                .collect();
            let after = crate::layout::align(&frames, mode);
            let action = self.page_sibling_frame_action(ci, &page_siblings, &after);
            self.execute_action(action);
            self.session.status = "Aligned page items.".to_owned();
            return;
        }
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
        let page_siblings = self.page_sibling_frames(ci);
        if page_siblings.len() >= 3 {
            let frames: Vec<_> = page_siblings
                .iter()
                .map(|(_, id, frame)| (*id, *frame))
                .collect();
            let after = crate::layout::distribute(&frames, axis);
            let action = self.page_sibling_frame_action(ci, &page_siblings, &after);
            self.execute_action(action);
            self.session.status = "Distributed page items.".to_owned();
            return;
        }
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
            .filter_map(|&id| {
                self.doc.canvases[ci]
                    .object(id)
                    .map(|_| (id, self.doc.canvases[ci].content_group(id)))
            })
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
            .filter_map(|&id| {
                self.doc.canvases[ci]
                    .object(id)
                    .map(|_| (id, self.doc.canvases[ci].content_group(id)))
            })
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
        let panel_targets: Vec<_> = self
            .session
            .ui
            .hierarchical_selection
            .paths()
            .iter()
            .filter(|path| path.content.is_none())
            .filter_map(|path| path.panel)
            .collect();
        if !panel_targets.is_empty() {
            let before = PanelState::of(&self.doc.canvases[ci]);
            let mut page = self.doc.canvases[ci].clone();
            let order: Vec<_> = page.panels.iter().map(|panel| panel.id).collect();
            let reordered = crate::actions::reorder_z(&order, &panel_targets, op);
            page.panels
                .sort_by_key(|panel| reordered.iter().position(|id| *id == panel.id));
            reorder_panel_content_blocks(&mut page, &panel_targets, op);
            self.execute_action(Action::ReplacePanelState {
                canvas: ci,
                before,
                after: PanelState::of(&page),
            });
            self.session.status = "Reordered panels.".to_owned();
            return;
        }
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
            self.mark_document_dirty();
        }
    }

    /// Toggle content-driven page height without creating an undo step.
    ///
    /// Auto height was historically a live page preference rather than an
    /// action. Keeping the mutation beside `set_show_grid` makes the catalog
    /// path preserve that boundary.
    pub fn set_canvas_auto_height(&mut self, canvas: usize, enabled: bool) {
        if let Some(c) = self.doc.canvases.get_mut(canvas)
            && c.auto_height != enabled
        {
            c.auto_height = enabled;
            self.mark_document_dirty();
        }
    }

    pub fn set_snap_enabled(&mut self, enabled: bool) {
        let target = self.app_target();
        match self.plan_property_write(
            crate::properties::app_preferences::SNAP_ENABLED,
            std::slice::from_ref(&target),
            &crate::properties::PropertyValue::Bool(enabled),
        ) {
            Ok(commit) => {
                self.commit_property(commit);
            }
            Err(error) => {
                self.session.status = format!("Could not change object snapping: {error}");
            }
        }
    }
}

/// A Panel is one stacking unit even though its children are stored in the
/// canvas-wide content collection. Reordering only `page.panels` changes tree
/// order but not paint order, so derive page-level blocks and move every child
/// in the selected Panel together. Loose content retains its own layer slot.
fn reorder_panel_content_blocks(
    page: &mut crate::state::CanvasDocument,
    panels: &[PanelId],
    op: crate::actions::ZOrder,
) {
    let mut blocks: Vec<Vec<ObjectId>> = Vec::new();
    for object in &page.objects {
        let parent = page.parent_panel(object.id);
        if let Some(panel) = parent
            && let Some(block) = blocks.iter_mut().find(|block| {
                block
                    .first()
                    .is_some_and(|first| page.parent_panel(*first) == Some(panel))
            })
        {
            block.push(object.id);
        } else {
            blocks.push(vec![object.id]);
        }
    }
    let order: Vec<_> = (0..blocks.len()).collect();
    let targets: Vec<_> = order
        .iter()
        .copied()
        .filter(|index| {
            blocks[*index]
                .first()
                .and_then(|id| page.parent_panel(*id))
                .is_some_and(|panel| panels.contains(&panel))
        })
        .collect();
    let reordered = crate::actions::reorder_z(&order, &targets, op);
    let object_order: Vec<_> = reordered
        .into_iter()
        .flat_map(|index| blocks[index].iter().copied())
        .collect();
    page.objects
        .sort_by_key(|object| object_order.iter().position(|id| *id == object.id));
}

#[derive(Clone, Copy)]
enum PageSibling {
    Panel(PanelId),
    Content(ContentId),
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
                .or_else(|| canvas.layout_frame(id))?;
            if let Some(change) = axis_changes.iter().find(|change| change.id == id) {
                let mut figure = plot.figure().clone();
                change.after.apply_to(&mut figure);
                Some(crate::layout::layout_item(id, &figure, frame))
            } else {
                Some(crate::layout::layout_item(id, plot.figure(), frame))
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
            after.x_show_tick_labels = (!keep_x).then_some(false);
            after.x_show_label = (!keep_x).then_some(false);
            after.y_show_tick_labels = (!keep_y).then_some(false);
            after.y_show_label = (!keep_y).then_some(false);
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
