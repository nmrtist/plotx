use super::*;

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

        let mut next_group = dst.next_group_id;
        let mut group_map: Vec<(crate::state::GroupId, crate::state::GroupId)> = Vec::new();
        let mut inserted = Vec::with_capacity(picked.len());
        let mut removed = Vec::with_capacity(picked.len());
        for (offset, &(slot, object)) in picked.iter().enumerate() {
            let mut clone = object.clone();
            clone.id = dst.next_object_id + offset as ObjectId;
            if let Some(g) = clone.group {
                let mapped = match group_map.iter().find(|(old, _)| *old == g) {
                    Some(&(_, new)) => new,
                    None => {
                        let new = next_group;
                        next_group += 1;
                        group_map.push((g, new));
                        new
                    }
                };
                clone.group = Some(mapped);
            }
            inserted.push(clone);
            if is_move {
                removed.push((slot, object.clone()));
            }
        }

        Some(Self::TransferObjects {
            from,
            to,
            removed,
            inserted,
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
    ) -> Option<Self> {
        let Action::TransferObjects {
            removed,
            mut inserted,
            active_before,
            selection_before,
            ..
        } = Action::transfer_objects(app, from, &[object], to, true)?
        else {
            return None;
        };
        inserted.first_mut()?.frame = newcomer_frame;
        let dst = app.doc.canvases.get(to)?;
        let existing_before = existing_after
            .iter()
            .filter_map(|&(id, _)| dst.object(id).map(|o| (id, o.frame)))
            .collect();
        Some(Self::TileDrop {
            from,
            to,
            removed,
            inserted,
            existing_before,
            existing_after,
            active_before,
            selection_before,
        })
    }
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
                dst.next_object_id = dst.next_object_id.max(object.id + 1);
                if let Some(group) = object.group {
                    dst.next_group_id = dst.next_group_id.max(group + 1);
                }
                dst.objects.push(object.clone());
            }
            dst.selected_object = ids.first().copied();
        }
        self.session.active_canvas = Some(to);
        self.session.ui.selection = Selection::Objects(ids);
        let active = self.doc.canvases.get(to).and_then(|c| c.active_dataset());
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
            active_before,
            selection_before,
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
        }
        if let Some(src) = self.doc.canvases.get_mut(from) {
            // Ascending slot order keeps each re-inserted object at its original
            // index despite earlier insertions.
            for (slot, object) in removed {
                let at = (*slot).min(src.objects.len());
                src.next_object_id = src.next_object_id.max(object.id + 1);
                src.objects.insert(at, object.clone());
            }
        }
        self.session.active_canvas = active_before;
        let active = active_before
            .and_then(|ci| self.doc.canvases.get(ci))
            .and_then(|c| c.active_dataset());
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
            from,
            to,
            removed,
            inserted,
            existing_after,
            ..
        } = action
        else {
            return;
        };
        let (from, to) = (*from, *to);
        for &(id, frame) in existing_after {
            self.set_object_frame(to, id, frame);
        }
        for (_, object) in removed {
            self.remove_object_value(from, object.id);
        }
        let ids: Vec<ObjectId> = inserted.iter().map(|o| o.id).collect();
        if let Some(dst) = self.doc.canvases.get_mut(to) {
            for object in inserted {
                dst.next_object_id = dst.next_object_id.max(object.id + 1);
                if let Some(group) = object.group {
                    dst.next_group_id = dst.next_group_id.max(group + 1);
                }
                dst.objects.push(object.clone());
            }
            dst.selected_object = ids.first().copied();
        }
        // The clones' figures were built for the source frame; rebuild at the
        // landing size now that they sit in the target's layout.
        for &id in &ids {
            if let Some(frame) = self
                .doc
                .canvases
                .get(to)
                .and_then(|c| c.object(id))
                .map(|o| o.frame)
            {
                self.set_object_frame(to, id, frame);
            }
        }
        self.session.active_canvas = Some(to);
        self.session.ui.selection = Selection::Objects(ids);
        let active = self.doc.canvases.get(to).and_then(|c| c.active_dataset());
        self.set_active_dataset(active);
        self.session.view = PrimaryView::Canvas;
        self.clear_transfer_transients();
    }

    /// Inverse of `apply_tile_drop`: pull the clone back out of the target, restore
    /// its existing plots' original frames, re-insert the dragged plot into its
    /// source slot, and restore the pre-drop active canvas and selection.
    pub(super) fn revert_tile_drop(&mut self, action: &Action) {
        let Action::TileDrop {
            from,
            to,
            removed,
            inserted,
            existing_before,
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
        }
        for &(id, frame) in existing_before {
            self.set_object_frame(to, id, frame);
        }
        if let Some(src) = self.doc.canvases.get_mut(from) {
            for (slot, object) in removed {
                let at = (*slot).min(src.objects.len());
                src.next_object_id = src.next_object_id.max(object.id + 1);
                src.objects.insert(at, object.clone());
            }
        }
        self.session.active_canvas = active_before;
        let active = active_before
            .and_then(|ci| self.doc.canvases.get(ci))
            .and_then(|c| c.active_dataset());
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
        for id in [a, _b] {
            app.doc.canvases[0].object_mut(id).unwrap().group = Some(group);
        }
        // Give canvas 1 its own group id space so a collision would be visible.
        app.doc.canvases[1].next_group_id = 1;

        app.transfer_objects_to_canvas(0, &[a], 1, true);
        assert!(app.doc.canvases[0].objects.is_empty());
        assert_eq!(app.doc.canvases[1].objects.len(), 3);

        let moved: Vec<_> = app.doc.canvases[1].objects[1..]
            .iter()
            .map(|o| o.group)
            .collect();
        assert!(moved[0].is_some());
        assert_eq!(moved[0], moved[1]);

        app.undo();
        assert_eq!(app.doc.canvases[0].objects.len(), 2);
        assert_eq!(app.doc.canvases[0].object(a).unwrap().group, Some(group));
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
