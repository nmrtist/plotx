//! Small state-edit helpers shared by apply and revert, split from
//! `app_impl/mod.rs` to keep it under the repository size limit.

use super::*;

impl PlotxApp {
    /// Insert half of the board-view pair (`apply(Insert)` ≡ `revert(Remove)`):
    /// a stale index clamps to the end so the view is never lost.
    pub(super) fn board_view_do_insert(&mut self, index: usize, view: &crate::state::NamedView) {
        let index = index.min(self.session.board_views.len());
        self.session.board_views.insert(index, view.clone());
    }

    /// Remove half of the pair (`apply(Remove)` ≡ `revert(Insert)`): when the
    /// index no longer holds `view`, fall back to removing the view by value,
    /// so both directions degrade the same way on a stale list.
    pub(super) fn board_view_do_remove(&mut self, index: usize, view: &crate::state::NamedView) {
        if self.session.board_views.get(index) == Some(view) {
            self.session.board_views.remove(index);
        } else if let Some(position) = self
            .session
            .board_views
            .iter()
            .rposition(|candidate| candidate == view)
        {
            self.session.board_views.remove(position);
        }
    }

    /// Set `(visible, locked)` on one canvas object; stale undo entries whose
    /// canvas or object is gone degrade to a no-op instead of panicking.
    pub(super) fn set_object_flags(
        &mut self,
        canvas: usize,
        object: ObjectId,
        flags: (bool, bool),
    ) {
        let Some(object) = self
            .doc
            .canvases
            .get_mut(canvas)
            .and_then(|canvas| canvas.object_mut(object))
        else {
            return;
        };
        (object.visible, object.locked) = flags;
    }

    pub(super) fn set_panel_meta(&mut self, canvas: usize, object: ObjectId, panel: PanelMeta) {
        let Some(plot) = self
            .doc
            .canvases
            .get_mut(canvas)
            .and_then(|canvas| canvas.object_mut(object))
            .and_then(|object| object.plot_mut())
        else {
            return;
        };
        plot.panel = panel;
    }
}
