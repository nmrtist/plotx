use super::*;
use crate::state::{GroupMember, LayoutGroup, PanelId, PanelLabelMode};

impl Action {
    /// Build a move/copy of `ids` (each expanded to its whole group) from canvas
    /// `from` to canvas `to`. Returns `None` when the source is missing, the
    /// canvases coincide, or nothing resolves. Destination-local ids and group
    /// ids are allocated here so the objects don't clash with the target's.
    pub fn transfer_objects(
        app: &PlotxApp,
        from: usize,
        ids: &[ObjectId],
        to: usize,
        is_move: bool,
    ) -> Option<Self> {
        if from == to {
            return None;
        }
        let src = app.doc.canvases.get(from)?;
        let dst = app.doc.canvases.get(to)?;

        // Expand every requested id to its full group, deduped, then take the
        // matching objects in source (z / slot) order.
        let mut wanted: Vec<ObjectId> = Vec::new();
        for &id in ids {
            for m in src.group_members(id) {
                if !wanted.contains(&m) {
                    wanted.push(m);
                }
            }
        }
        let picked: Vec<(usize, &CanvasObject)> = src
            .objects
            .iter()
            .enumerate()
            .filter(|(_, o)| wanted.contains(&o.id))
            .collect();
        if picked.is_empty() {
            return None;
        }

        let mut inserted = Vec::with_capacity(picked.len());
        let mut removed = Vec::with_capacity(picked.len());
        for (offset, &(slot, object)) in picked.iter().enumerate() {
            let mut clone = object.clone();
            clone.id = dst.next_object_id.checked_advance(offset as u64);
            clone.frame = src.content_page_frame(object.id)?;
            inserted.push(clone);
            if is_move {
                removed.push((slot, object.clone()));
            }
        }
        let id_map: std::collections::BTreeMap<_, _> = picked
            .iter()
            .zip(&inserted)
            .map(|((_, source), target)| (source.id, target.id))
            .collect();
        let source_groups_before = src.groups.clone();
        let mut source_groups_after = source_groups_before.clone();
        if is_move {
            source_groups_after.iter_mut().for_each(|group| {
                group.members.retain(
                    |member| !matches!(member, GroupMember::Content(id) if id_map.contains_key(id)),
                );
            });
            source_groups_after.retain(|group| group.members.len() >= 2);
        }
        let target_groups_before = dst.groups.clone();
        let mut target_groups_after = target_groups_before.clone();
        let mut next_group = dst.next_group_id;
        for group in &src.groups {
            let members: Vec<_> = group
                .members
                .iter()
                .filter_map(|member| match member {
                    GroupMember::Content(id) => id_map.get(id).copied().map(GroupMember::Content),
                    GroupMember::Panel(_) => None,
                })
                .collect();
            if members.len() >= 2 {
                target_groups_after.push(LayoutGroup {
                    id: next_group,
                    members,
                });
                next_group += 1;
            }
        }

        let source_panels_before = src.panels.clone();
        let mut source_panels_after = source_panels_before.clone();
        if is_move {
            source_panels_after.iter_mut().for_each(|panel| {
                panel.item_order.retain(|id| !id_map.contains_key(id));
            });
            source_panels_after.retain(|panel| !panel.item_order.is_empty());
            source_groups_after.iter_mut().for_each(|group| {
                group.members.retain(|member| match member {
                    GroupMember::Panel(id) => {
                        source_panels_after.iter().any(|panel| panel.id == *id)
                    }
                    GroupMember::Content(_) => true,
                });
            });
            source_groups_after.retain(|group| group.members.len() >= 2);
        }
        let target_panels_before = dst.panels.clone();
        let mut target_page = dst.clone();
        target_page.objects.extend(inserted.iter().cloned());
        target_page.next_object_id = target_page
            .next_object_id
            .max(dst.next_object_id.checked_advance(inserted.len() as u64));
        for source_panel in src.panels.iter().filter(|panel| {
            !panel.item_order.is_empty()
                && panel.item_order.iter().all(|id| id_map.contains_key(id))
        }) {
            let mut panel = source_panel.clone();
            panel.id = if is_move { panel.id } else { PanelId::new() };
            panel.item_order = source_panel
                .item_order
                .iter()
                .filter_map(|id| id_map.get(id).copied())
                .collect();
            if matches!(panel.label.mode, PanelLabelMode::Auto { .. }) {
                let slot = if is_move {
                    first_free_auto_slot(&target_page.panels)
                } else {
                    let slot = target_page.next_panel_label_slot;
                    target_page.next_panel_label_slot = slot.saturating_add(1);
                    slot
                };
                panel.label.mode = PanelLabelMode::Auto { slot };
                target_page.next_panel_label_slot = target_page
                    .next_panel_label_slot
                    .max(slot.saturating_add(1));
            }
            for &source_id in &source_panel.item_order {
                let target_id = *id_map.get(&source_id)?;
                target_page.object_mut(target_id)?.frame = src.object(source_id)?.frame;
            }
            target_page.panels.push(panel);
        }
        let panel_capable: Vec<_> = target_page
            .objects
            .iter()
            .filter(|object| {
                matches!(
                    object.kind,
                    crate::state::CanvasObjectKind::Plot(_)
                        | crate::state::CanvasObjectKind::RasterImage(_)
                )
            })
            .map(|object| object.id)
            .collect();
        for id in panel_capable {
            if target_page.parent_panel(id).is_none() {
                target_page.create_panel_for_content(id)?;
            }
        }
        for object in &mut inserted {
            *object = target_page.object(object.id)?.clone();
        }
        let target_label_slot_after = target_page.next_panel_label_slot;
        let target_panels_after = target_page.panels;

        Some(Self::TransferObjects {
            from,
            to,
            removed,
            inserted,
            source_groups_before,
            source_groups_after,
            target_groups_before,
            target_groups_after,
            source_panels_before,
            source_panels_after,
            target_panels_before,
            target_panels_after,
            source_label_slot_before: src.next_panel_label_slot,
            source_label_slot_after: src.next_panel_label_slot,
            target_label_slot_before: dst.next_panel_label_slot,
            target_label_slot_after,
            active_before: app.session.active_canvas,
            selection_before: app.session.ui.selection.clone(),
        })
    }

    /// Build an auto-tiling drop of the single plot `object` from `from` onto `to`.
    /// Reuses [`Action::transfer_objects`] to mint the destination-local clone, then
    /// bakes the newcomer's landing frame into it and carries the target's existing
    /// plots' before/after frames. `None` when the move can't be built (same canvas,
    /// stale ids). `existing_after` is the previewed reframe of the target's plots.
    pub fn tile_drop(
        app: &PlotxApp,
        from: usize,
        object: ObjectId,
        to: usize,
        newcomer_frame: crate::state::ObjectFrame,
        existing_after: Vec<(ObjectId, crate::state::ObjectFrame)>,
        remove_empty_source: bool,
    ) -> Option<Self> {
        let Action::TransferObjects {
            removed,
            mut inserted,
            active_before,
            selection_before,
            source_groups_before,
            source_groups_after,
            target_groups_before,
            target_groups_after,
            source_panels_before,
            source_panels_after,
            target_panels_before,
            mut target_panels_after,
            source_label_slot_before,
            source_label_slot_after,
            target_label_slot_before,
            mut target_label_slot_after,
            ..
        } = Action::transfer_objects(app, from, &[object], to, true)?
        else {
            return None;
        };
        inserted.first_mut()?.frame =
            crate::state::ObjectFrame::new(0.0, 0.0, newcomer_frame.width, newcomer_frame.height);
        let newcomer_id = inserted.first()?.id;
        if let Some(newcomer_panel) = target_panels_after
            .iter_mut()
            .find(|panel| panel.item_order.contains(&newcomer_id))
        {
            newcomer_panel.frame = newcomer_frame;
        }
        for &(content, frame) in &existing_after {
            if let Some(panel) = target_panels_after
                .iter_mut()
                .find(|panel| panel.item_order.contains(&content))
            {
                panel.frame = frame;
            }
        }
        let mut ordered_page = app.doc.canvases.get(to)?.clone();
        ordered_page.panels.clone_from(&target_panels_after);
        let order = ordered_page.panel_reading_order();
        let mut slot = 0_u64;
        for id in order {
            let panel = target_panels_after
                .iter_mut()
                .find(|panel| panel.id == id)?;
            if panel.label.participates_in_sequence {
                if matches!(panel.label.mode, PanelLabelMode::Auto { .. }) {
                    panel.label.mode = PanelLabelMode::Auto { slot };
                }
                slot = slot.saturating_add(1);
            }
        }
        target_label_slot_after = target_label_slot_after.max(slot);
        let src = app.doc.canvases.get(from)?;
        let dst = app.doc.canvases.get(to)?;
        let existing_before = existing_after
            .iter()
            .filter_map(|&(id, _)| dst.layout_frame(id).map(|frame| (id, frame)))
            .collect();
        let source_will_be_empty = src.objects.len() == removed.len();
        let source_canvas_before =
            (remove_empty_source && source_will_be_empty).then(|| Box::new(src.clone()));
        let target_index_after = if source_canvas_before.is_some() && from < to {
            to - 1
        } else {
            to
        };
        Some(Self::TileDrop {
            source_index_before: from,
            target_index_before: to,
            target_index_after,
            source_canvas_before,
            removed,
            inserted,
            source_groups_before,
            source_groups_after,
            target_groups_before,
            target_groups_after,
            source_panels_before,
            source_panels_after,
            target_panels_before,
            target_panels_after,
            source_label_slot_before,
            source_label_slot_after,
            target_label_slot_before,
            target_label_slot_after,
            existing_before,
            existing_after,
            active_before,
            selection_before,
        })
    }
}

fn first_free_auto_slot(panels: &[crate::state::Panel]) -> u64 {
    let used: std::collections::BTreeSet<_> = panels
        .iter()
        .filter_map(|panel| match panel.label.mode {
            PanelLabelMode::Auto { slot } => Some(slot),
            PanelLabelMode::LockedAuto { .. } | PanelLabelMode::Manual { .. } => None,
        })
        .collect();
    (0..).find(|slot| !used.contains(slot)).unwrap_or(u64::MAX)
}

impl PlotxApp {
    /// Move or copy `ids` (each expanded to its whole group) from canvas `from`
    /// to canvas `to` as one undoable step, switching focus to the destination
    /// with the transferred objects selected.
    pub fn transfer_objects_to_canvas(
        &mut self,
        from: usize,
        ids: &[ObjectId],
        to: usize,
        is_move: bool,
    ) {
        let Some(action) = Action::transfer_objects(self, from, ids, to, is_move) else {
            return;
        };
        let Action::TransferObjects { inserted, .. } = &action else {
            return;
        };
        let count = inserted.len();
        let target = self.doc.canvases[to].name.clone();
        self.execute_action(action);
        let verb = if is_move { "Moved" } else { "Copied" };
        self.session.status = format!("{verb} {count} object(s) to “{target}”.");
    }

    /// Forward (and redo) path of a cross-canvas transfer: drop the moved objects
    /// from the source, append the destination-local clones, then focus the
    /// destination with the transferred objects selected. `removed` is empty for a
    /// copy, so the source is left untouched.
    pub(super) fn apply_transfer(&mut self, action: &Action) {
        let Action::TransferObjects {
            from,
            to,
            removed,
            inserted,
            source_groups_after,
            target_groups_after,
            source_panels_after,
            target_panels_after,
            source_label_slot_after,
            target_label_slot_after,
            ..
        } = action
        else {
            return;
        };
        let (from, to) = (*from, *to);
        for (_, object) in removed {
            self.remove_object_value(from, object.id);
        }
        let ids: Vec<ObjectId> = inserted.iter().map(|o| o.id).collect();
        if let Some(dst) = self.doc.canvases.get_mut(to) {
            for object in inserted {
                dst.next_object_id = dst.next_object_id.max(object.id.checked_advance(1));
                dst.objects.push(object.clone());
            }
            dst.groups = target_groups_after.clone();
            dst.panels = target_panels_after.clone();
            dst.next_panel_label_slot = *target_label_slot_after;
            dst.next_group_id = dst.groups.iter().map(|group| group.id).max().unwrap_or(0) + 1;
            dst.selected_object = ids.first().copied();
        }
        if let Some(src) = self.doc.canvases.get_mut(from) {
            src.groups = source_groups_after.clone();
            src.panels = source_panels_after.clone();
            src.next_panel_label_slot = *source_label_slot_after;
        }
        self.session.active_canvas = Some(to);
        self.session.ui.selection = Selection::Objects(ids);
        let active = self
            .doc
            .canvases
            .get(to)
            .and_then(|c| c.active_dataset())
            .and_then(|id| self.doc.dataset_index(id));
        self.set_active_dataset(active);
        self.session.view = PrimaryView::Canvas;
        self.clear_transfer_transients();
    }

    /// Inverse of `apply_transfer`: pull the destination clones back out and, for a
    /// move, restore the originals into their source slots, then restore the
    /// pre-transfer active canvas and selection. `removed` is empty for a copy, so
    /// the source is left untouched.
    pub(super) fn revert_transfer(&mut self, action: &Action) {
        let Action::TransferObjects {
            from,
            to,
            removed,
            inserted,
            source_groups_before,
            target_groups_before,
            source_panels_before,
            target_panels_before,
            source_label_slot_before,
            target_label_slot_before,
            active_before,
            selection_before,
            ..
        } = action
        else {
            return;
        };
        let (from, to, active_before) = (*from, *to, *active_before);
        if let Some(dst) = self.doc.canvases.get_mut(to) {
            for object in inserted {
                dst.objects.retain(|o| o.id != object.id);
                if dst.selected_object == Some(object.id) {
                    dst.selected_object = None;
                }
            }
            dst.groups = target_groups_before.clone();
            dst.panels = target_panels_before.clone();
            dst.next_panel_label_slot = *target_label_slot_before;
        }
        if let Some(src) = self.doc.canvases.get_mut(from) {
            // Ascending slot order keeps each re-inserted object at its original
            // index despite earlier insertions.
            for (slot, object) in removed {
                let at = (*slot).min(src.objects.len());
                src.next_object_id = src.next_object_id.max(object.id.checked_advance(1));
                src.objects.insert(at, object.clone());
            }
            src.groups = source_groups_before.clone();
            src.panels = source_panels_before.clone();
            src.next_panel_label_slot = *source_label_slot_before;
        }
        self.session.active_canvas = active_before;
        let active = active_before
            .and_then(|ci| self.doc.canvases.get(ci))
            .and_then(|c| c.active_dataset())
            .and_then(|id| self.doc.dataset_index(id));
        self.set_active_dataset(active);
        self.set_selection(selection_before.clone());
        self.clear_transfer_transients();
    }

    /// Forward (and redo) path of an auto-tiling drop: reframe the target's
    /// existing plots, move the dragged plot in (its landing frame is baked into
    /// the clone), rebuild every reframed plot's figure to its new size, then focus
    /// the target with the newcomer selected.
    pub(super) fn apply_tile_drop(&mut self, action: &Action) {
        let Action::TileDrop {
            source_index_before,
            target_index_before,
            target_index_after,
            source_canvas_before,
            removed,
            inserted,
            source_groups_after,
            target_groups_after,
            source_panels_after,
            target_panels_after,
            source_label_slot_after,
            target_label_slot_after,
            existing_after,
            ..
        } = action
        else {
            return;
        };
        let (from, to) = (*source_index_before, *target_index_before);
        // Validate every index before mutating either canvas. A stale history
        // entry must be an all-or-nothing no-op, never a half-applied transfer.
        if from == to || from >= self.doc.canvases.len() || to >= self.doc.canvases.len() {
            self.clear_transfer_transients();
            return;
        }
        for &(id, frame) in existing_after {
            self.set_object_frame(to, id, frame);
        }
        for (_, object) in removed {
            self.remove_object_value(from, object.id);
        }
        let ids: Vec<ObjectId> = inserted.iter().map(|o| o.id).collect();
        if let Some(dst) = self.doc.canvases.get_mut(to) {
            for object in inserted {
                dst.next_object_id = dst.next_object_id.max(object.id.checked_advance(1));
                dst.objects.push(object.clone());
            }
            dst.groups = target_groups_after.clone();
            dst.panels = target_panels_after.clone();
            dst.next_panel_label_slot = *target_label_slot_after;
            dst.next_group_id = dst.groups.iter().map(|group| group.id).max().unwrap_or(0) + 1;
            dst.selected_object = ids.first().copied();
        }
        if let Some(src) = self.doc.canvases.get_mut(from) {
            src.groups = source_groups_after.clone();
            src.panels = source_panels_after.clone();
            src.next_panel_label_slot = *source_label_slot_after;
        }
        // The clones' figures were built for the source frame; rebuild at the
        // landing size now that they sit in the target's layout.
        for &id in &ids {
            if let Some(frame) = self.doc.canvases.get(to).and_then(|c| c.layout_frame(id)) {
                self.set_object_frame(to, id, frame);
            }
        }
        if source_canvas_before.is_some() {
            self.doc.canvases.remove(from);
        }
        let to = *target_index_after;
        self.session.active_canvas = Some(to);
        self.session.ui.selection = Selection::Objects(ids);
        let active = self
            .doc
            .canvases
            .get(to)
            .and_then(|c| c.active_dataset())
            .and_then(|id| self.doc.dataset_index(id));
        self.set_active_dataset(active);
        self.session.view = PrimaryView::Canvas;
        self.clear_transfer_transients();
    }

    /// Inverse of `apply_tile_drop`: pull the clone back out of the target, restore
    /// its existing plots' original frames, re-insert the dragged plot into its
    /// source slot, and restore the pre-drop active canvas and selection.
    pub(super) fn revert_tile_drop(&mut self, action: &Action) {
        let Action::TileDrop {
            source_index_before,
            target_index_before,
            target_index_after,
            source_canvas_before,
            removed,
            inserted,
            source_groups_before,
            target_groups_before,
            source_panels_before,
            target_panels_before,
            source_label_slot_before,
            target_label_slot_before,
            existing_before,
            active_before,
            selection_before,
            ..
        } = action
        else {
            return;
        };
        let (from, to, active_before) =
            (*source_index_before, *target_index_before, *active_before);
        if let Some(source) = source_canvas_before {
            if from > self.doc.canvases.len() {
                self.clear_transfer_transients();
                return;
            }
            self.doc.canvases.insert(from, (**source).clone());
        }
        let current_target = if source_canvas_before.is_some() {
            to
        } else {
            *target_index_after
        };
        if let Some(dst) = self.doc.canvases.get_mut(current_target) {
            for object in inserted {
                dst.objects.retain(|o| o.id != object.id);
                if dst.selected_object == Some(object.id) {
                    dst.selected_object = None;
                }
            }
            dst.groups = target_groups_before.clone();
        }
        for &(id, frame) in existing_before {
            self.set_object_frame(current_target, id, frame);
        }
        if let Some(dst) = self.doc.canvases.get_mut(current_target) {
            dst.panels = target_panels_before.clone();
            dst.next_panel_label_slot = *target_label_slot_before;
        }
        if source_canvas_before.is_none()
            && let Some(src) = self.doc.canvases.get_mut(from)
        {
            for (slot, object) in removed {
                let at = (*slot).min(src.objects.len());
                src.next_object_id = src.next_object_id.max(object.id.checked_advance(1));
                src.objects.insert(at, object.clone());
            }
            src.groups = source_groups_before.clone();
            src.panels = source_panels_before.clone();
            src.next_panel_label_slot = *source_label_slot_before;
        }
        self.session.active_canvas = active_before;
        let active = active_before
            .and_then(|ci| self.doc.canvases.get(ci))
            .and_then(|c| c.active_dataset())
            .and_then(|id| self.doc.dataset_index(id));
        self.set_active_dataset(active);
        self.set_selection(selection_before.clone());
        self.clear_transfer_transients();
    }

    /// Drop the transient page-space interactions that may point at objects moved
    /// off the active canvas.
    fn clear_transfer_transients(&mut self) {
        self.reset_interaction();
        self.session.ui.panel_note_inline_edit = None;
        self.session.ui.panel_note_edit = None;
        self.session.ui.text_edit = None;
    }
}

#[cfg(test)]
mod tests {
    use crate::actions::Action;
    use crate::actions::tests::{push_canvas, push_text_object, sample_app};

    #[test]
    fn move_plot_to_other_canvas_transfers_and_undoes() {
        let mut app = sample_app();
        push_canvas(&mut app, 0, "second canvas", [90.0, 60.0]);
        app.session.active_canvas = Some(0);
        let moved = app.doc.canvases[0].objects[0].id;
        let src_before = app.doc.canvases[0].objects.len();
        let dst_before = app.doc.canvases[1].objects.len();

        app.transfer_objects_to_canvas(0, &[moved], 1, true);

        assert_eq!(app.doc.canvases[0].objects.len(), src_before - 1);
        assert_eq!(app.doc.canvases[1].objects.len(), dst_before + 1);
        let new_id = app.doc.canvases[1].objects.last().unwrap().id;
        assert_eq!(
            app.doc.canvases[1]
                .objects
                .iter()
                .filter(|o| o.id == new_id)
                .count(),
            1
        );
        assert_eq!(app.session.active_canvas, Some(1));
        assert_eq!(app.session.ui.selection.object(), Some(new_id));

        app.undo();
        assert_eq!(app.doc.canvases[0].objects.len(), src_before);
        assert_eq!(app.doc.canvases[1].objects.len(), dst_before);
        assert_eq!(app.doc.canvases[0].objects[0].id, moved);
        assert_eq!(app.session.active_canvas, Some(0));

        app.redo();
        assert_eq!(app.doc.canvases[0].objects.len(), src_before - 1);
        assert_eq!(app.doc.canvases[1].objects.len(), dst_before + 1);
        assert_eq!(app.session.active_canvas, Some(1));
    }

    #[test]
    fn copy_plot_to_other_canvas_keeps_source() {
        let mut app = sample_app();
        push_canvas(&mut app, 0, "second canvas", [90.0, 60.0]);
        app.session.active_canvas = Some(0);
        let copied = app.doc.canvases[0].objects[0].id;

        app.transfer_objects_to_canvas(0, &[copied], 1, false);

        assert_eq!(app.doc.canvases[0].objects.len(), 1);
        assert_eq!(app.doc.canvases[0].objects[0].id, copied);
        assert_eq!(app.doc.canvases[1].objects.len(), 2);
        assert_eq!(app.session.active_canvas, Some(1));

        app.undo();
        assert_eq!(app.doc.canvases[1].objects.len(), 1);
        assert_eq!(app.doc.canvases[0].objects.len(), 1);
    }

    #[test]
    fn transfer_moves_whole_group_and_remaps_ids() {
        let mut app = sample_app();
        push_canvas(&mut app, 0, "second canvas", [90.0, 60.0]);
        app.session.active_canvas = Some(0);
        let a = app.doc.canvases[0].objects[0].id;
        let _b = push_text_object(&mut app, 0, "b");
        let group = app.doc.canvases[0].allocate_group_id();
        app.doc.canvases[0].groups.push(crate::state::LayoutGroup {
            id: group,
            members: vec![
                crate::state::GroupMember::Content(a),
                crate::state::GroupMember::Content(_b),
            ],
        });
        // Give canvas 1 its own group id space so a collision would be visible.
        app.doc.canvases[1].next_group_id = 1;

        app.transfer_objects_to_canvas(0, &[a], 1, true);
        assert!(app.doc.canvases[0].objects.is_empty());
        assert_eq!(app.doc.canvases[1].objects.len(), 3);

        let moved = &app.doc.canvases[1].groups[0].members;
        assert_eq!(moved.len(), 2);

        app.undo();
        assert_eq!(app.doc.canvases[0].objects.len(), 2);
        assert_eq!(app.doc.canvases[0].content_group(a), Some(group));
        assert_eq!(app.doc.canvases[1].objects.len(), 1);
    }

    #[test]
    fn transfer_to_same_canvas_is_rejected() {
        let mut app = sample_app();
        let id = app.doc.canvases[0].objects[0].id;
        assert!(Action::transfer_objects(&app, 0, &[id], 0, true).is_none());
        app.transfer_objects_to_canvas(0, &[id], 0, true);
        assert_eq!(app.doc.canvases[0].objects.len(), 1);
        assert!(!app.can_undo());
    }
}
